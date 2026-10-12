use crate::Result;
use serde::{de::DeserializeOwned, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};
pub fn read_bytes(path: &Path, max: u64) -> Result<Vec<u8>> {
    let file = File::open(path).map_err(|e| format!("读取 {} 失败：{e}", path.display()))?;
    if file.metadata().map_err(|e| e.to_string())?.len() > max {
        return Err("文件过大".into());
    }
    let mut bytes = Vec::new();
    file.take(max + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    if bytes.len() as u64 > max {
        return Err("文件过大".into());
    }
    Ok(bytes)
}
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    serde_json::from_slice(&read_bytes(path, 1024 * 1024)?)
        .map_err(|e| format!("配置 {} 无效，原文件已保留：{e}", path.display()))
}
pub fn write<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let parent = path.parent().ok_or("配置路径无效")?;
    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = parent.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let bytes = serde_json::to_vec_pretty(value).map_err(|e| e.to_string())?;
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|e| e.to_string())?;
        drop(file);
        replace(&temp, path)
    })();
    if temp.exists() {
        let cleanup = fs::remove_file(&temp);
        if result.is_ok() {
            cleanup.map_err(|e| e.to_string())?;
        }
    }
    result
}
#[cfg(not(windows))]
fn replace(from: &Path, to: &Path) -> Result<()> {
    fs::rename(from, to).map_err(|e| e.to_string())
}
#[cfg(windows)]
fn replace(from: &Path, to: &Path) -> Result<()> {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn MoveFileExW(from: *const u16, to: *const u16, flags: u32) -> i32;
    }
    let a: Vec<u16> = from.as_os_str().encode_wide().chain(Some(0)).collect();
    let b: Vec<u16> = to.as_os_str().encode_wide().chain(Some(0)).collect();
    // Replace atomically on the same volume, including an existing recovery file.
    if unsafe { MoveFileExW(a.as_ptr(), b.as_ptr(), 1 | 8) } == 0 {
        Err(std::io::Error::last_os_error().to_string())
    } else {
        Ok(())
    }
}
