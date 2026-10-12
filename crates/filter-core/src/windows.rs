use crate::{
    icc,
    models::{Monitor, Profile, Snapshot},
    session::Backend,
    storage, Result,
};
use std::{
    ffi::{c_char, c_void, CString},
    mem::{size_of, zeroed},
    path::{Path, PathBuf},
    ptr,
};
use winreg::{enums::*, RegKey};
type Handle = *mut c_void;
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}
fn text(b: &[u16]) -> String {
    String::from_utf16_lossy(&b[..b.iter().position(|x| *x == 0).unwrap_or(b.len())])
}
fn check(ok: i32, op: &str) -> Result<()> {
    if ok == 0 {
        Err(format!("{op}失败：{}", std::io::Error::last_os_error()))
    } else {
        Ok(())
    }
}
fn code(value: i32, op: &str) -> Result<()> {
    if value == 0 {
        Ok(())
    } else {
        Err(format!(
            "{op}失败：{}",
            std::io::Error::from_raw_os_error(value)
        ))
    }
}
#[repr(C)]
struct DisplayDevice {
    size: u32,
    name: [u16; 32],
    description: [u16; 128],
    flags: u32,
    id: [u16; 128],
    key: [u16; 128],
}
impl DisplayDevice {
    fn new() -> Self {
        let mut s: Self = unsafe { zeroed() };
        s.size = size_of::<Self>() as u32;
        s
    }
}
#[repr(C)]
#[derive(Copy, Clone)]
struct Luid {
    low: u32,
    high: i32,
}
#[repr(C)]
struct PathSource {
    adapter: Luid,
    id: u32,
    mode: u32,
    flags: u32,
}
#[repr(C)]
struct PathTarget {
    adapter: Luid,
    id: u32,
    mode: u32,
    technology: u32,
    rotation: u32,
    scaling: u32,
    refresh_num: u32,
    refresh_den: u32,
    scanline: u32,
    available: i32,
    flags: u32,
}
#[repr(C)]
struct DisplayPath {
    source: PathSource,
    target: PathTarget,
    flags: u32,
}
#[repr(C, align(8))]
struct DisplayMode {
    bytes: [u8; 64],
}
#[repr(C)]
struct InfoHeader {
    kind: u32,
    size: u32,
    adapter: Luid,
    id: u32,
}
#[repr(C)]
struct SourceName {
    header: InfoHeader,
    name: [u16; 32],
}
#[repr(C)]
struct AdvancedColor {
    header: InfoHeader,
    value: u32,
    encoding: u32,
    bits: u32,
}
#[repr(C)]
struct ProcessEntry {
    size: u32,
    usage: u32,
    id: u32,
    heap: usize,
    module: u32,
    threads: u32,
    parent: u32,
    priority: i32,
    flags: u32,
    name: [u16; 260],
}
#[link(name = "user32")]
extern "system" {
    fn MessageBoxW(window: Handle, text: *const u16, caption: *const u16, flags: u32) -> i32;
    fn EnumDisplayDevicesW(
        device: *const u16,
        index: u32,
        data: *mut DisplayDevice,
        flags: u32,
    ) -> i32;
    fn GetDisplayConfigBufferSizes(flags: u32, paths: *mut u32, modes: *mut u32) -> i32;
    fn QueryDisplayConfig(
        flags: u32,
        paths: *mut u32,
        path_array: *mut DisplayPath,
        modes: *mut u32,
        mode_array: *mut DisplayMode,
        topology: *mut u32,
    ) -> i32;
    fn DisplayConfigGetDeviceInfo(info: *mut InfoHeader) -> i32;
}
#[link(name = "gdi32")]
extern "system" {
    fn CreateDCW(
        driver: *const u16,
        device: *const u16,
        output: *const u16,
        init: *const c_void,
    ) -> Handle;
    fn GetDeviceGammaRamp(dc: Handle, ramp: *mut c_void) -> i32;
    fn SetDeviceGammaRamp(dc: Handle, ramp: *const c_void) -> i32;
    fn DeleteDC(dc: Handle) -> i32;
}
#[link(name = "mscms")]
extern "system" {
    fn WcsGetUsePerUserProfiles(device: *const u16, class: u32, enabled: *mut i32) -> i32;
    fn WcsSetUsePerUserProfiles(device: *const u16, class: u32, enabled: i32) -> i32;
    fn WcsGetDefaultColorProfileSize(
        scope: u32,
        device: *const u16,
        kind: u32,
        subtype: u32,
        id: u32,
        size: *mut u32,
    ) -> i32;
    fn WcsGetDefaultColorProfile(
        scope: u32,
        device: *const u16,
        kind: u32,
        subtype: u32,
        id: u32,
        size: u32,
        name: *mut u16,
    ) -> i32;
    fn WcsSetDefaultColorProfile(
        scope: u32,
        device: *const u16,
        kind: u32,
        subtype: u32,
        id: u32,
        name: *const u16,
    ) -> i32;
    fn WcsAssociateColorProfileWithDevice(
        scope: u32,
        profile: *const u16,
        device: *const u16,
    ) -> i32;
    fn InstallColorProfileW(machine: *const u16, path: *const u16) -> i32;
}
#[link(name = "kernel32")]
extern "system" {
    fn GetSystemDirectoryW(buffer: *mut u16, size: u32) -> u32;
    fn LoadLibraryExW(path: *const u16, file: Handle, flags: u32) -> Handle;
    fn GetProcAddress(library: Handle, name: *const c_char) -> *mut c_void;
    fn FreeLibrary(library: Handle) -> i32;
    fn CreateToolhelp32Snapshot(flags: u32, id: u32) -> Handle;
    fn Process32FirstW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn Process32NextW(snapshot: Handle, entry: *mut ProcessEntry) -> i32;
    fn CloseHandle(handle: Handle) -> i32;
}

pub fn monitors() -> Result<Vec<Monitor>> {
    let mut results = Vec::new();
    for index in 0..256 {
        let mut adapter = DisplayDevice::new();
        if unsafe { EnumDisplayDevicesW(ptr::null(), index, &mut adapter, 0) } == 0 {
            break;
        }
        if adapter.flags & 1 == 0 || adapter.flags & 8 != 0 {
            continue;
        }
        let mut monitor = DisplayDevice::new();
        if unsafe { EnumDisplayDevicesW(adapter.name.as_ptr(), 0, &mut monitor, 0) } == 0 {
            continue;
        }
        let id = text(&monitor.id);
        if id.is_empty() {
            continue;
        }
        let name = text(&adapter.name);
        results.push(Monitor {
            device_name: name.clone(),
            monitor_id: id,
            label: format!(
                "{} · {}{}",
                text(&monitor.description),
                name.trim_start_matches(r"\\.\"),
                if adapter.flags & 4 != 0 {
                    " · 主屏"
                } else {
                    ""
                }
            ),
            primary: adapter.flags & 4 != 0,
        });
    }
    results.sort_by_key(|m| !m.primary);
    Ok(results)
}
fn resolve(m: &Monitor) -> Result<Monitor> {
    monitors()?
        .into_iter()
        .find(|v| v.monitor_id == m.monitor_id)
        .ok_or_else(|| "目标显示器已断开，备份会保留；请重新连接后恢复".into())
}
fn advanced_color(m: &Monitor) -> Result<bool> {
    for _ in 0..3 {
        let (mut paths, mut modes) = (0, 0);
        code(
            unsafe { GetDisplayConfigBufferSizes(2, &mut paths, &mut modes) },
            "读取显示拓扑",
        )?;
        if paths > 256 || modes > 1024 {
            return Err("显示拓扑异常".into());
        }
        let mut p: Vec<DisplayPath> = (0..paths).map(|_| unsafe { zeroed() }).collect();
        let mut d: Vec<DisplayMode> = (0..modes).map(|_| unsafe { zeroed() }).collect();
        let result = unsafe {
            QueryDisplayConfig(
                2,
                &mut paths,
                p.as_mut_ptr(),
                &mut modes,
                d.as_mut_ptr(),
                ptr::null_mut(),
            )
        };
        if result == 122 {
            continue;
        }
        code(result, "读取显示拓扑")?;
        let mut found = false;
        for path in p.iter().take(paths as usize) {
            let mut source = SourceName {
                header: InfoHeader {
                    kind: 1,
                    size: size_of::<SourceName>() as u32,
                    adapter: path.source.adapter,
                    id: path.source.id,
                },
                name: [0; 32],
            };
            code(
                unsafe { DisplayConfigGetDeviceInfo(&mut source.header) },
                "读取显示器名称",
            )?;
            if !text(&source.name).eq_ignore_ascii_case(&m.device_name) {
                continue;
            }
            found = true;
            let mut color = AdvancedColor {
                header: InfoHeader {
                    kind: 9,
                    size: size_of::<AdvancedColor>() as u32,
                    adapter: path.target.adapter,
                    id: path.target.id,
                },
                value: 0,
                encoding: 0,
                bits: 0,
            };
            code(
                unsafe { DisplayConfigGetDeviceInfo(&mut color.header) },
                "检查 HDR/高级色彩",
            )?;
            if color.value & 2 != 0 {
                return Ok(true);
            }
        }
        return if found {
            Ok(false)
        } else {
            Err("无法匹配显示拓扑".into())
        };
    }
    Err("显示拓扑正在变化，请稍后重试".into())
}
fn sdr(m: &Monitor) -> Result<()> {
    if advanced_color(m)? {
        Err("请先关闭所选显示器的 HDR/高级色彩，再应用或恢复 SDR 曲线".into())
    } else {
        Ok(())
    }
}
struct Dc(Handle);
impl Dc {
    fn open(m: &Monitor) -> Result<Self> {
        let handle = unsafe {
            CreateDCW(
                wide("DISPLAY").as_ptr(),
                wide(&m.device_name).as_ptr(),
                ptr::null(),
                ptr::null(),
            )
        };
        if handle.is_null() {
            Err(format!(
                "打开显示器失败：{}",
                std::io::Error::last_os_error()
            ))
        } else {
            Ok(Self(handle))
        }
    }
}
impl Drop for Dc {
    fn drop(&mut self) {
        unsafe {
            DeleteDC(self.0);
        }
    }
}
fn read_ramp(m: &Monitor) -> Result<Vec<u16>> {
    let dc = Dc::open(m)?;
    let mut ramp = vec![0_u16; 768];
    check(
        unsafe { GetDeviceGammaRamp(dc.0, ramp.as_mut_ptr().cast()) },
        "读取 Gamma Ramp",
    )?;
    Ok(ramp)
}
fn default_profile(id: &str) -> Result<Option<String>> {
    let device = wide(id);
    let mut size = 0;
    if unsafe { WcsGetDefaultColorProfileSize(1, device.as_ptr(), 0, 4, 0, &mut size) } == 0 {
        let error = std::io::Error::last_os_error();
        if matches!(error.raw_os_error(), Some(2 | 1168 | 2015 | 2016)) {
            return Ok(None);
        }
        return Err(format!("读取默认 ICC 大小失败：{error}"));
    }
    if size == 0 || size > 65536 || size % 2 != 0 {
        return Err("ICC 名称长度无效".into());
    }
    let mut name = vec![0_u16; size as usize / 2];
    check(
        unsafe { WcsGetDefaultColorProfile(1, device.as_ptr(), 0, 4, 0, size, name.as_mut_ptr()) },
        "读取默认 ICC",
    )?;
    let name = text(&name);
    Ok(if name.is_empty() { None } else { Some(name) })
}
fn set_default(id: &str, name: Option<&str>) -> Result<()> {
    let profile = name.map(wide);
    check(
        unsafe {
            WcsSetDefaultColorProfile(
                1,
                wide(id).as_ptr(),
                0,
                4,
                0,
                profile.as_ref().map_or(ptr::null(), |n| n.as_ptr()),
            )
        },
        "设置默认 ICC",
    )
}
fn system_dir() -> Result<PathBuf> {
    let mut b = vec![0_u16; 32768];
    let n = unsafe { GetSystemDirectoryW(b.as_mut_ptr(), b.len() as u32) };
    if n == 0 || n as usize >= b.len() {
        return Err("无法读取 Windows 系统目录".into());
    }
    Ok(PathBuf::from(text(&b)))
}
pub fn system_icc_dir() -> Result<PathBuf> {
    Ok(system_dir()?.join("spool/drivers/color"))
}
fn installed(path: &str) -> Result<String> {
    let path = Path::new(path);
    icc::read(path)?;
    let name = path
        .file_name()
        .and_then(|p| p.to_str())
        .ok_or("ICC 文件名无效")?
        .to_string();
    let dest = system_icc_dir()?.join(&name);
    if dest.exists() {
        if storage::read_bytes(path, icc::MAX_BYTES)? != storage::read_bytes(&dest, icc::MAX_BYTES)?
        {
            return Err("系统内存在同名但内容不同的 ICC，请重命名后导入".into());
        }
    } else {
        check(
            unsafe { InstallColorProfileW(ptr::null(), wide(&path.to_string_lossy()).as_ptr()) },
            "安装 ICC（若被拒绝，请先在 Windows 色彩管理中安装该文件）",
        )?;
    }
    Ok(name)
}

#[repr(C)]
struct DvcInfo {
    version: u32,
    current: i32,
    min: i32,
    max: i32,
    default: i32,
}
type Query = unsafe extern "C" fn(u32) -> *mut c_void;
type Initialize = unsafe extern "C" fn() -> i32;
type Associate = unsafe extern "C" fn(*const c_char, *mut Handle) -> i32;
type GetDvc = unsafe extern "C" fn(Handle, u32, *mut DvcInfo) -> i32;
type SetDvc = unsafe extern "C" fn(Handle, u32, *mut DvcInfo) -> i32;
struct Nvidia {
    library: Handle,
    associate: Associate,
    get: GetDvc,
    set: SetDvc,
}
fn nvcheck(status: i32, op: &str) -> Result<()> {
    if status == 0 {
        Ok(())
    } else {
        Err(format!(
            "{op}失败（NVAPI {status}），请检查输出连接和驱动版本"
        ))
    }
}
impl Nvidia {
    fn new() -> Result<Self> {
        // Only the installed System32 driver can provide executable NVAPI code.
        let path = system_dir()?.join("nvapi64.dll");
        let library = unsafe {
            LoadLibraryExW(
                wide(&path.to_string_lossy()).as_ptr(),
                ptr::null_mut(),
                0x800,
            )
        };
        if library.is_null() {
            return Err(format!(
                "NVIDIA 驱动不可用：{}",
                std::io::Error::last_os_error()
            ));
        }
        let result = (|| {
            let address = unsafe { GetProcAddress(library, c"nvapi_QueryInterface".as_ptr()) };
            if address.is_null() {
                return Err("驱动缺少 NVAPI QueryInterface".into());
            }
            // Each address is checked and converted to its specific documented ABI.
            let query: Query = unsafe { std::mem::transmute(address) };
            let entry = |id| {
                let p = unsafe { query(id) };
                if p.is_null() {
                    Err(format!("驱动未提供 NVAPI 入口 0x{id:08X}"))
                } else {
                    Ok(p)
                }
            };
            let initialize: Initialize = unsafe { std::mem::transmute(entry(0x0150E828)?) };
            nvcheck(unsafe { initialize() }, "初始化 NVAPI")?;
            Ok(Self {
                library,
                associate: unsafe {
                    std::mem::transmute::<*mut c_void, Associate>(entry(0x35C29134)?)
                },
                get: unsafe { std::mem::transmute::<*mut c_void, GetDvc>(entry(0x0E45002D)?) },
                set: unsafe { std::mem::transmute::<*mut c_void, SetDvc>(entry(0x4A82C2B1)?) },
            })
        })();
        if result.is_err() {
            unsafe {
                FreeLibrary(library);
            }
        }
        result
    }
    fn handle(&self, m: &Monitor) -> Result<Handle> {
        let name = CString::new(m.device_name.as_bytes()).map_err(|e| e.to_string())?;
        let mut handle = ptr::null_mut();
        nvcheck(
            unsafe { (self.associate)(name.as_ptr(), &mut handle) },
            "匹配 NVIDIA 显示器",
        )?;
        if handle.is_null() {
            Err("所选显示器未由 NVIDIA 直接输出".into())
        } else {
            Ok(handle)
        }
    }
    fn read(&self, handle: Handle) -> Result<DvcInfo> {
        let mut info = DvcInfo {
            version: size_of::<DvcInfo>() as u32 | 0x10000,
            current: 0,
            min: 0,
            max: 0,
            default: 0,
        };
        nvcheck(unsafe { (self.get)(handle, 0, &mut info) }, "读取数字振动")?;
        if info.max <= info.min || info.current < info.min || info.current > info.max {
            return Err("驱动返回的数字振动范围无效".into());
        }
        Ok(info)
    }
    fn raw(&self, m: &Monitor) -> Result<i32> {
        Ok(self.read(self.handle(m)?)?.current)
    }
    fn percent(&self, m: &Monitor) -> Result<i32> {
        let i = self.read(self.handle(m)?)?;
        Ok(
            ((i.current as f64 - i.min as f64) * 100.0 / (i.max as f64 - i.min as f64)).round()
                as i32,
        )
    }
    fn write(&self, m: &Monitor, raw: Option<i32>, percent: Option<i32>) -> Result<()> {
        let handle = self.handle(m)?;
        let mut info = self.read(handle)?;
        let level = match percent {
            Some(p) => (info.min as f64 + p as f64 / 100.0 * (info.max as f64 - info.min as f64))
                .round() as i32,
            None => raw.ok_or("缺少数字振动值")?,
        };
        if level < info.min || level > info.max {
            return Err("驱动数字振动范围已改变，无法恢复旧值".into());
        }
        info.current = level;
        nvcheck(unsafe { (self.set)(handle, 0, &mut info) }, "设置数字振动")?;
        if self.read(handle)?.current != level {
            return Err("数字振动回读校验失败".into());
        }
        Ok(())
    }
}
impl Drop for Nvidia {
    fn drop(&mut self) {
        unsafe {
            FreeLibrary(self.library);
        }
    }
}
pub fn nvidia_status(m: &Monitor) -> Result<i32> {
    Nvidia::new()?.percent(&resolve(m)?)
}

pub struct WindowsBackend;
impl Backend for WindowsBackend {
    fn capture(&mut self, m: &Monitor, require: bool) -> Result<Snapshot> {
        let m = resolve(m)?;
        sdr(&m)?;
        let mut per_user = 0;
        check(
            unsafe {
                WcsGetUsePerUserProfiles(wide(&m.monitor_id).as_ptr(), 0x6D6E7472, &mut per_user)
            },
            "读取 ICC 用户配置",
        )?;
        let nv = Nvidia::new().and_then(|n| n.raw(&m));
        let vibrance = if require { Some(nv?) } else { nv.ok() };
        Ok(Snapshot {
            gamma_ramp: read_ramp(&m)?,
            per_user_profiles: per_user != 0,
            user_icc_profile: default_profile(&m.monitor_id)?,
            nvidia_vibrance_raw: vibrance,
            monitor: m,
            applied_icc_profiles: vec![],
        })
    }
    fn metadata(&mut self, s: &Snapshot, p: Option<&Profile>) -> Result<()> {
        let m = resolve(&s.monitor)?;
        sdr(&m)?;
        let device = wide(&m.monitor_id);
        let icc_result = (|| {
            if let Some(path) = p.and_then(|p| p.icc_path.as_ref()) {
                let name = installed(path)?;
                check(
                    unsafe { WcsSetUsePerUserProfiles(device.as_ptr(), 0x6D6E7472, 1) },
                    "启用当前用户 ICC",
                )?;
                check(
                    unsafe {
                        WcsAssociateColorProfileWithDevice(1, wide(&name).as_ptr(), device.as_ptr())
                    },
                    "关联 ICC",
                )?;
                set_default(&m.monitor_id, Some(&name))?;
            } else if !s.applied_icc_profiles.is_empty() {
                set_default(&m.monitor_id, s.user_icc_profile.as_deref())?;
                check(
                    unsafe {
                        WcsSetUsePerUserProfiles(
                            device.as_ptr(),
                            0x6D6E7472,
                            s.per_user_profiles as i32,
                        )
                    },
                    "恢复 ICC 用户配置",
                )?;
            }
            Ok(())
        })();
        let percent = p.and_then(|p| p.color.digital_vibrance);
        let nv_result = if percent.is_some() || s.nvidia_vibrance_raw.is_some() {
            Nvidia::new().and_then(|n| n.write(&m, s.nvidia_vibrance_raw, percent))
        } else {
            Ok(())
        };
        match (icc_result, nv_result) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(a), Err(b)) => Err(format!("{a}；{b}")),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
    fn read_ramp(&mut self, m: &Monitor) -> Result<Vec<u16>> {
        read_ramp(&resolve(m)?)
    }
    fn write_ramp(&mut self, m: &Monitor, ramp: &[u16]) -> Result<()> {
        if ramp.len() != 768 {
            return Err("曲线长度无效".into());
        }
        let m = resolve(m)?;
        sdr(&m)?;
        let dc = Dc::open(&m)?;
        check(
            unsafe { SetDeviceGammaRamp(dc.0, ramp.as_ptr().cast()) },
            "设置 Gamma（请尝试降低调节幅度）",
        )
    }
}
pub fn processes() -> Result<Vec<String>> {
    let handle = unsafe { CreateToolhelp32Snapshot(2, 0) };
    if handle as isize == -1 {
        return Err(format!(
            "读取运行进程失败：{}",
            std::io::Error::last_os_error()
        ));
    }
    struct SnapshotHandle(Handle);
    impl Drop for SnapshotHandle {
        fn drop(&mut self) {
            unsafe {
                CloseHandle(self.0);
            }
        }
    }
    let _guard = SnapshotHandle(handle);
    let mut entry: ProcessEntry = unsafe { zeroed() };
    entry.size = size_of::<ProcessEntry>() as u32;
    let mut ok = unsafe { Process32FirstW(handle, &mut entry) };
    let mut names = Vec::new();
    while ok != 0 {
        let name = text(&entry.name);
        if name.to_ascii_lowercase().ends_with(".exe") {
            names.push(name);
        }
        ok = unsafe { Process32NextW(handle, &mut entry) };
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() != Some(18) {
        return Err(format!("枚举进程失败：{error}"));
    }
    names.sort_by_key(|p| p.to_ascii_lowercase());
    names.dedup_by(|a, b| a.eq_ignore_ascii_case(b));
    Ok(names)
}
const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_NAME: &str = "DeltaForceFilterManager";
fn startup_command() -> Result<String> {
    Ok(format!(
        "\"{}\" --tray",
        std::env::current_exe()
            .map_err(|e| e.to_string())?
            .display()
    ))
}
pub fn startup_enabled() -> Result<bool> {
    let key = match RegKey::predef(HKEY_CURRENT_USER).open_subkey(RUN_KEY) {
        Ok(k) => k,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(e) => return Err(e.to_string()),
    };
    match key.get_value::<String, _>(RUN_NAME) {
        Ok(value) => Ok(value == startup_command()?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(e.to_string()),
    }
}
pub fn startup_set(enabled: bool) -> Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
        .create_subkey(RUN_KEY)
        .map_err(|e| e.to_string())?;
    if enabled {
        key.set_value(RUN_NAME, &startup_command()?)
            .map_err(|e| e.to_string())?;
    } else {
        match key.delete_value(RUN_NAME) {
            Ok(()) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(e.to_string()),
        }
    }
    if startup_enabled()? != enabled {
        return Err("开机启动写入校验失败".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ffi_layouts() {
        assert_eq!(size_of::<DisplayDevice>(), 840);
        assert_eq!(size_of::<Luid>(), 8);
        assert_eq!(size_of::<PathSource>(), 20);
        assert_eq!(size_of::<PathTarget>(), 48);
        assert_eq!(size_of::<DisplayPath>(), 72);
        assert_eq!(size_of::<DisplayMode>(), 64);
        assert_eq!(size_of::<SourceName>(), 84);
        assert_eq!(size_of::<AdvancedColor>(), 32);
        assert_eq!(size_of::<DvcInfo>(), 20);
        assert_eq!(size_of::<ProcessEntry>(), 568);
    }
}

pub fn error_dialog(message: &str) {
    if unsafe {
        MessageBoxW(
            ptr::null_mut(),
            wide(message).as_ptr(),
            wide("DeltaForce Filter Manager").as_ptr(),
            0x10,
        )
    } == 0
    {
        eprintln!("{message}");
    }
}
