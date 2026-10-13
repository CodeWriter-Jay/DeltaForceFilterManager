use filter_core::{storage, Result};
use reqwest::blocking::Client;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Child, Command},
    time::{Duration, Instant},
};

const REPOSITORY: &str = "CodeWriter-Jay/DeltaForceFilterManager";
const MAX_RELEASE_BYTES: u64 = 1024 * 1024;
const MAX_EXE_BYTES: u64 = 100 * 1024 * 1024;

#[derive(Deserialize)]
struct Release {
    tag_name: String,
    html_url: String,
    body: Option<String>,
    draft: bool,
    prerelease: bool,
    assets: Vec<Asset>,
}
#[derive(Clone, Deserialize)]
struct Asset {
    name: String,
    browser_download_url: String,
    state: String,
    size: u64,
    digest: Option<String>,
}
#[derive(Clone)]
struct Candidate {
    version: String,
    asset: Asset,
    sha256: String,
}
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current_version: String,
    latest_version: String,
    available: bool,
    release_url: String,
    notes: String,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Progress {
    pub downloaded: u64,
    pub total: u64,
}
#[derive(Default)]
pub struct Updater {
    candidate: Option<Candidate>,
}

fn client(timeout: Duration) -> Result<Client> {
    Client::builder()
        .user_agent(concat!(
            "DeltaForceFilterManager/",
            env!("CARGO_PKG_VERSION")
        ))
        .https_only(true)
        .connect_timeout(Duration::from_secs(10))
        .timeout(timeout)
        .redirect(reqwest::redirect::Policy::custom(|attempt| {
            let url = attempt.url();
            // GitHub release downloads redirect to its asset CDN; no arbitrary update hosts.
            if attempt.previous().len() >= 5 {
                return attempt.error("重定向次数过多");
            }
            if url.scheme() == "https"
                && matches!(
                    url.host_str(),
                    Some(
                        "github.com"
                            | "api.github.com"
                            | "release-assets.githubusercontent.com"
                            | "objects.githubusercontent.com"
                    )
                )
            {
                attempt.follow()
            } else {
                attempt.error("更新下载被重定向到不受信任的地址")
            }
        }))
        .build()
        .map_err(|error| format!("初始化更新连接失败：{error}"))
}
fn read_limited(mut reader: impl Read, max: u64) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .by_ref()
        .take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("更新响应过大".into());
    }
    Ok(bytes)
}
fn parse_release(bytes: &[u8], current: &str) -> Result<(UpdateInfo, Option<Candidate>)> {
    let release: Release =
        serde_json::from_slice(bytes).map_err(|e| format!("GitHub Release 响应无效：{e}"))?;
    let version = Version::parse(release.tag_name.trim_start_matches('v'))
        .map_err(|e| format!("Release 版本号无效：{e}"))?;
    if release.draft || release.prerelease || !version.pre.is_empty() {
        return Err("只支持正式 Release 更新".into());
    }
    let expected_page = format!(
        "https://github.com/{REPOSITORY}/releases/tag/{}",
        release.tag_name
    );
    if release.html_url != expected_page {
        return Err("Release 不属于本项目".into());
    }
    let available = version > Version::parse(current).map_err(|e| e.to_string())?;
    let candidate = if available {
        let name = format!("DeltaForceFilterManager-{}-win-x64.exe", release.tag_name);
        let asset = release
            .assets
            .into_iter()
            .find(|a| a.name == name)
            .ok_or("Release 缺少 Windows x64 单文件 EXE")?;
        let expected_url = format!(
            "https://github.com/{REPOSITORY}/releases/download/{}/{name}",
            release.tag_name
        );
        if asset.browser_download_url != expected_url
            || asset.state != "uploaded"
            || asset.size == 0
            || asset.size > MAX_EXE_BYTES
        {
            return Err("Release EXE 地址或大小无效".into());
        }
        let sha256 = asset
            .digest
            .as_deref()
            .and_then(|digest| digest.strip_prefix("sha256:"))
            .ok_or("Release 缺少 SHA256 校验信息，已禁止自动升级")?
            .to_ascii_lowercase();
        if sha256.len() != 64 || !sha256.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err("Release SHA256 校验信息无效".into());
        }
        Some(Candidate {
            version: version.to_string(),
            asset,
            sha256,
        })
    } else {
        None
    };
    Ok((
        UpdateInfo {
            current_version: current.into(),
            latest_version: version.to_string(),
            available,
            release_url: release.html_url,
            notes: release.body.unwrap_or_default(),
        },
        candidate,
    ))
}
impl Updater {
    pub fn check(&mut self) -> Result<UpdateInfo> {
        self.candidate = None;
        let response = client(Duration::from_secs(20))?
            .get(format!(
                "https://api.github.com/repos/{REPOSITORY}/releases/latest"
            ))
            .header("Accept", "application/vnd.github+json")
            .send()
            .map_err(|e| format!("检查更新失败：{}", e.without_url()))?;
        if response.status() == reqwest::StatusCode::FORBIDDEN
            || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
        {
            return Err("GitHub 请求达到限额，请稍后再试".into());
        }
        let bytes = read_limited(
            response
                .error_for_status()
                .map_err(|e| format!("检查更新失败：{e}"))?,
            MAX_RELEASE_BYTES,
        )?;
        let (info, candidate) = parse_release(&bytes, env!("CARGO_PKG_VERSION"))?;
        self.candidate = candidate;
        Ok(info)
    }
    pub fn download(
        &mut self,
        root: &Path,
        mut progress: impl FnMut(Progress),
    ) -> Result<InstallJob> {
        let candidate = self.candidate.as_ref().ok_or("请先检查更新")?;
        let folder = root
            .join("updates")
            .join(filter_core::models::Profile::default().id);
        fs::create_dir_all(&folder).map_err(|e| format!("创建更新目录失败：{e}"))?;
        let target = std::env::current_exe().map_err(|e| e.to_string())?;
        let parent = target.parent().ok_or("当前 EXE 路径无效")?;
        let id = folder.file_name().ok_or("更新目录无效")?.to_string_lossy();
        let staged = parent.join(format!(".delta-force-update-{id}.exe"));
        let backup = parent.join(format!(".delta-force-previous-{id}.exe"));
        let result = (|| {
            // Verify directory access before downloading; stage on the target volume for atomic renames.
            let mut sibling = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&staged)
                .map_err(|e| format!("EXE 所在目录不可写，无法原地升级：{e}"))?;
            let response = client(Duration::from_secs(300))?
                .get(&candidate.asset.browser_download_url)
                .send()
                .and_then(|r| r.error_for_status())
                .map_err(|e| format!("下载更新失败：{}", e.without_url()))?;
            let payload = folder.join("download.exe");
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&payload)
                .map_err(|e| e.to_string())?;
            download_verified(
                response,
                file,
                candidate.asset.size,
                &candidate.sha256,
                &mut progress,
            )?;
            std::io::copy(
                &mut File::open(&payload).map_err(|e| e.to_string())?,
                &mut sibling,
            )
            .and_then(|_| sibling.sync_all())
            .map_err(|e| e.to_string())?;
            fs::remove_file(&payload).map_err(|e| e.to_string())?;
            let job = InstallJob {
                target: target.clone(),
                staged: staged.clone(),
                backup: backup.clone(),
                sha256: candidate.sha256.clone(),
                version: candidate.version.clone(),
                parent_pid: std::process::id(),
            };
            storage::write(&folder.join("job.json"), &job)?;
            fs::copy(
                std::env::current_exe().map_err(|e| e.to_string())?,
                folder.join("helper.exe"),
            )
            .map_err(|e| format!("准备更新辅助程序失败：{e}"))?;
            Ok(job)
        })();
        if let Err(error) = &result {
            if staged.exists() {
                if let Err(cleanup) = fs::remove_file(&staged) {
                    return Err(format!("{}；清理暂存 EXE 失败：{cleanup}", error));
                }
            }
            if let Err(cleanup) = fs::remove_dir_all(&folder) {
                return Err(format!("{}；清理更新文件失败：{cleanup}", error));
            }
        }
        result
    }
}
fn download_verified(
    mut reader: impl Read,
    mut writer: impl Write,
    size: u64,
    expected: &str,
    progress: &mut impl FnMut(Progress),
) -> Result<()> {
    let mut hash = Sha256::new();
    let mut downloaded = 0;
    let mut buffer = [0; 64 * 1024];
    let mut last_progress = Instant::now();
    progress(Progress {
        downloaded: 0,
        total: size,
    });
    loop {
        let count = reader
            .read(&mut buffer)
            .map_err(|e| format!("下载读取失败：{e}"))?;
        if count == 0 {
            break;
        }
        downloaded += count as u64;
        if downloaded > size || downloaded > MAX_EXE_BYTES {
            return Err("更新文件超过声明大小".into());
        }
        writer
            .write_all(&buffer[..count])
            .map_err(|e| format!("保存更新失败：{e}"))?;
        hash.update(&buffer[..count]);
        if last_progress.elapsed() >= Duration::from_millis(100) {
            progress(Progress {
                downloaded,
                total: size,
            });
            last_progress = Instant::now();
        }
    }
    if downloaded != size {
        return Err("更新文件下载不完整".into());
    }
    if format!("{:x}", hash.finalize()) != expected {
        return Err("更新文件 SHA256 不匹配，已禁止安装".into());
    }
    writer.flush().map_err(|e| e.to_string())?;
    progress(Progress {
        downloaded,
        total: size,
    });
    Ok(())
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct InstallJob {
    target: PathBuf,
    staged: PathBuf,
    backup: PathBuf,
    sha256: String,
    version: String,
    parent_pid: u32,
}
impl InstallJob {
    pub fn cleanup(&self) -> Result<()> {
        if self.staged.exists() {
            fs::remove_file(&self.staged).map_err(|e| format!("清理暂存 EXE 失败：{e}"))?;
        }
        let root = data_root()?;
        let id = self
            .staged
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix(".delta-force-update-"))
            .ok_or("暂存 EXE 路径无效")?;
        fs::remove_dir_all(root.join("updates").join(id))
            .map_err(|e| format!("清理更新目录失败：{e}"))
    }
    pub fn launch_helper(&self) -> Result<Child> {
        let root = data_root()?;
        let id = self
            .staged
            .file_stem()
            .and_then(|s| s.to_str())
            .and_then(|s| s.strip_prefix(".delta-force-update-"))
            .ok_or("暂存 EXE 路径无效")?;
        let folder = root.join("updates").join(id);
        let mut child = Command::new(folder.join("helper.exe"))
            .arg("--install-update")
            .arg(folder.join("job.json"))
            .spawn()
            .map_err(|e| format!("启动更新辅助程序失败：{e}"))?;
        let deadline = Instant::now() + Duration::from_secs(5);
        while !folder.join("ready.json").exists() {
            if child.try_wait().map_err(|e| e.to_string())?.is_some() {
                return Err("更新辅助程序启动失败，原程序保持运行".into());
            }
            if Instant::now() >= deadline {
                child
                    .kill()
                    .and_then(|_| child.wait())
                    .map_err(|e| format!("停止更新辅助程序失败：{e}"))?;
                return Err("更新辅助程序启动超时，已取消升级".into());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        Ok(child)
    }
}
pub fn data_root() -> Result<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("LOCALAPPDATA")
            .map(|p| PathBuf::from(p).join("DeltaForceFilterManager"))
            .ok_or_else(|| "缺少 LOCALAPPDATA".into())
    }
    #[cfg(not(windows))]
    {
        Ok(std::env::temp_dir().join("DeltaForceFilterManager-preview"))
    }
}
fn read_job(job_path: &Path) -> Result<(InstallJob, PathBuf)> {
    let job: InstallJob = storage::read(job_path)?;
    let folder = job_path.parent().ok_or("更新任务路径无效")?;
    let id = folder
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("更新任务 ID 无效")?;
    if id.len() != 32
        || !id.bytes().all(|b| b.is_ascii_hexdigit())
        || folder.parent() != Some(data_root()?.join("updates").as_path())
    {
        return Err("更新任务路径或进程无效".into());
    }
    let parent = job.target.parent().ok_or("目标 EXE 路径无效")?;
    if !job.target.is_absolute()
        || job.staged != parent.join(format!(".delta-force-update-{id}.exe"))
        || job.backup != parent.join(format!(".delta-force-previous-{id}.exe"))
    {
        return Err("更新任务包含无效的替换路径".into());
    }
    Ok((job, folder.to_path_buf()))
}
pub fn confirm_startup(job_path: &Path) -> Result<()> {
    let (job, folder) = read_job(job_path)?;
    if job.version != env!("CARGO_PKG_VERSION") {
        return Err("更新 EXE 的实际版本与 Release 不一致".into());
    }
    storage::write(&folder.join("success.json"), &job.version)
}
pub fn run_helper(job_path: &Path) -> Result<()> {
    let (job, folder) = read_job(job_path)?;
    if job.parent_pid == std::process::id() {
        return Err("更新任务不能等待自身退出".into());
    }
    let result = (|| {
        wait_for_parent(job.parent_pid, &folder.join("ready.json"))?;
        replace_executable(&job, || {
            let mut child = Command::new(&job.target)
                .arg("--after-update")
                .arg(job_path)
                .spawn()
                .map_err(|e| e.to_string())?;
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                if folder.join("success.json").exists() {
                    return Ok(());
                }
                if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                    return Err(format!("新版本启动失败（{status}）"));
                }
                if Instant::now() >= deadline {
                    child
                        .kill()
                        .and_then(|_| child.wait())
                        .map_err(|e| format!("停止启动失败的新版本失败：{e}"))?;
                    return Err("新版本启动验证超时".into());
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        })
    })();
    if let Err(error) = &result {
        let context = if job.backup.exists() {
            format!(
                "升级失败：{error}。方案数据保留，旧 EXE 备份位于 {}",
                job.backup.display()
            )
        } else {
            format!(
                "升级失败：{error}。方案数据保留，可重新打开 {}",
                job.target.display()
            )
        };
        if let Err(log_error) = storage::write(&folder.join("error.json"), &context) {
            return Err(format!("{context}；保存更新错误记录失败：{log_error}"));
        }
        return Err(context);
    }
    result
}
fn replace_executable(job: &InstallJob, launch: impl FnOnce() -> Result<()>) -> Result<()> {
    let bytes = storage::read_bytes(&job.staged, MAX_EXE_BYTES)?;
    if format!("{:x}", Sha256::digest(&bytes)) != job.sha256 {
        return Err("暂存 EXE 校验失败".into());
    }
    if job.backup.exists() {
        return Err("旧 EXE 备份路径已存在".into());
    }
    fs::rename(&job.target, &job.backup).map_err(|e| format!("备份旧 EXE 失败：{e}"))?;
    let install = fs::rename(&job.staged, &job.target)
        .map_err(|e| format!("替换 EXE 失败：{e}"))
        .and_then(|_| launch());
    if let Err(error) = install {
        if job.target.exists() {
            fs::remove_file(&job.target)
                .map_err(|e| format!("{error}；移除失败的新 EXE 失败：{e}"))?;
        }
        fs::rename(&job.backup, &job.target)
            .map_err(|e| format!("{error}；旧 EXE 回滚失败：{e}"))?;
        return Err(format!("{error}；已恢复旧 EXE，可重新打开程序"));
    }
    Ok(())
}
#[cfg(windows)]
fn wait_for_parent(pid: u32, ready: &Path) -> Result<()> {
    use std::ffi::c_void;
    #[link(name = "kernel32")]
    extern "system" {
        fn OpenProcess(access: u32, inherit: i32, pid: u32) -> *mut c_void;
        fn WaitForSingleObject(handle: *mut c_void, milliseconds: u32) -> u32;
        fn CloseHandle(handle: *mut c_void) -> i32;
    }
    let handle = unsafe { OpenProcess(0x00100000, 0, pid) };
    if handle.is_null() {
        return Err(format!(
            "无法等待原进程退出：{}",
            std::io::Error::last_os_error()
        ));
    }
    if let Err(error) = storage::write(ready, &true) {
        unsafe {
            CloseHandle(handle);
        }
        return Err(error);
    }
    let status = unsafe { WaitForSingleObject(handle, 30000) };
    let wait_error = (status == u32::MAX).then(std::io::Error::last_os_error);
    unsafe {
        CloseHandle(handle);
    }
    if let Some(error) = wait_error {
        return Err(format!("等待原进程退出失败：{error}"));
    }
    if status == 0x102 {
        return Err("原进程未在 30 秒内退出，已取消替换 EXE".into());
    }
    if status != 0 {
        return Err(format!("等待原进程退出失败（{status:#x}）"));
    }
    Ok(())
}
#[cfg(not(windows))]
fn wait_for_parent(_: u32, _: &Path) -> Result<()> {
    Err("自动升级仅支持 Windows".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn release() -> serde_json::Value {
        json!({"tag_name":"v0.3.0", "html_url":format!("https://github.com/{REPOSITORY}/releases/tag/v0.3.0"), "body":"新版本", "draft":false, "prerelease":false,
          "assets":[{"name":"DeltaForceFilterManager-v0.3.0-win-x64.exe", "browser_download_url":format!("https://github.com/{REPOSITORY}/releases/download/v0.3.0/DeltaForceFilterManager-v0.3.0-win-x64.exe"), "state":"uploaded", "size":10, "digest":format!("sha256:{}", "a".repeat(64))}]})
    }
    #[test]
    fn release_check_compares_versions_and_requires_the_project_exe_and_digest() {
        let value = release();
        let bytes = serde_json::to_vec(&value).unwrap();
        let (info, candidate) = parse_release(&bytes, "0.2.1").unwrap();
        assert!(info.available);
        assert_eq!(candidate.unwrap().version, "0.3.0");
        assert!(!parse_release(&bytes, "0.3.0").unwrap().0.available);
        assert!(!parse_release(&bytes, "0.4.0").unwrap().0.available);
        for field in ["browser_download_url", "digest", "name"] {
            let mut invalid = release();
            invalid["assets"][0][field] = json!("invalid");
            assert!(parse_release(&serde_json::to_vec(&invalid).unwrap(), "0.2.1").is_err());
        }
        let mut preview = release();
        preview["prerelease"] = json!(true);
        assert!(parse_release(&serde_json::to_vec(&preview).unwrap(), "0.2.1").is_err());
    }
    #[test]
    fn download_requires_complete_bytes_matching_the_hash() {
        let bytes = b"verified update";
        let hash = format!("{:x}", Sha256::digest(bytes));
        let mut output = Vec::new();
        let mut progress = Vec::new();
        download_verified(
            bytes.as_slice(),
            &mut output,
            bytes.len() as u64,
            &hash,
            &mut |p| progress.push(p),
        )
        .unwrap();
        assert_eq!(output, bytes);
        assert_eq!(progress.last().unwrap().downloaded, bytes.len() as u64);
        assert!(download_verified(
            bytes.as_slice(),
            Vec::new(),
            bytes.len() as u64 + 1,
            &hash,
            &mut |_| {}
        )
        .is_err());
        assert!(download_verified(
            bytes.as_slice(),
            Vec::new(),
            bytes.len() as u64 - 1,
            &hash,
            &mut |_| {}
        )
        .is_err());
        assert!(download_verified(
            bytes.as_slice(),
            Vec::new(),
            bytes.len() as u64,
            &"0".repeat(64),
            &mut |_| {}
        )
        .is_err());
    }
    fn job() -> InstallJob {
        let folder = std::env::temp_dir().join(format!(
            "df-update-中文 空格-{}",
            filter_core::models::Profile::default().id
        ));
        fs::create_dir_all(&folder).unwrap();
        let job = InstallJob {
            target: folder.join("original.exe"),
            staged: folder.join("new.exe"),
            backup: folder.join("previous.exe"),
            sha256: format!("{:x}", Sha256::digest(b"new")),
            version: "0.3.0".into(),
            parent_pid: 1,
        };
        fs::write(&job.target, b"old").unwrap();
        fs::write(&job.staged, b"new").unwrap();
        fs::write(folder.join("settings.json"), b"preserve these schemes").unwrap();
        job
    }
    #[test]
    fn exe_replacement_keeps_settings_and_rolls_back_if_startup_fails() {
        let job = job();
        replace_executable(&job, || Ok(())).unwrap();
        assert_eq!(fs::read(&job.target).unwrap(), b"new");
        assert_eq!(fs::read(&job.backup).unwrap(), b"old");
        assert_eq!(
            fs::read(job.target.parent().unwrap().join("settings.json")).unwrap(),
            b"preserve these schemes"
        );
        fs::remove_dir_all(job.target.parent().unwrap()).unwrap();
        let job = self::job();
        assert!(replace_executable(&job, || Err("startup failed".into()))
            .unwrap_err()
            .contains("已恢复旧 EXE"));
        assert_eq!(fs::read(&job.target).unwrap(), b"old");
        fs::remove_dir_all(job.target.parent().unwrap()).unwrap();
    }
    #[test]
    fn a_modified_staged_exe_never_replaces_the_running_file() {
        let job = job();
        fs::write(&job.staged, b"modified").unwrap();
        assert!(replace_executable(&job, || panic!("must not launch")).is_err());
        assert_eq!(fs::read(&job.target).unwrap(), b"old");
        assert!(!job.backup.exists());
        fs::remove_dir_all(job.target.parent().unwrap()).unwrap();
    }
    #[test]
    #[ignore = "需要访问 GitHub；常规回归测试不依赖外部网络"]
    fn checks_the_real_github_release_over_https() {
        let info = Updater::default().check().unwrap();
        Version::parse(&info.latest_version).unwrap();
        assert_eq!(info.current_version, env!("CARGO_PKG_VERSION"));
        assert!(info
            .release_url
            .starts_with(&format!("https://github.com/{REPOSITORY}/releases/tag/")));
        let response = client(Duration::from_secs(20))
            .unwrap()
            .get(format!(
                "https://api.github.com/repos/{REPOSITORY}/releases/latest"
            ))
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        let bytes = read_limited(response, MAX_RELEASE_BYTES).unwrap();
        let (_, candidate) = parse_release(&bytes, "0.0.0").unwrap();
        let candidate = candidate.unwrap();
        let response = client(Duration::from_secs(60))
            .unwrap()
            .get(&candidate.asset.browser_download_url)
            .send()
            .unwrap()
            .error_for_status()
            .unwrap();
        download_verified(
            response,
            std::io::sink(),
            candidate.asset.size,
            &candidate.sha256,
            &mut |_| {},
        )
        .unwrap();
    }
}
