#[cfg(windows)]
#[path = "windows.rs"]
mod windows;
#[cfg(windows)]
pub use windows::*;
#[cfg(not(windows))]
mod unsupported {
    use crate::{models::Monitor, Result};
    pub fn error_dialog(message: &str) {
        eprintln!("{message}");
    }
    pub fn monitors() -> Result<Vec<Monitor>> {
        Ok(vec![])
    }
    pub fn processes() -> Result<Vec<String>> {
        Err("进程联动仅支持 Windows".into())
    }
    pub fn startup_enabled() -> Result<bool> {
        Ok(false)
    }
    pub fn startup_set(_: bool) -> Result<()> {
        Err("开机启动仅支持 Windows".into())
    }
    pub fn nvidia_status(_: &Monitor) -> Result<i32> {
        Err("NVIDIA 调节仅支持 Windows".into())
    }
    pub fn system_icc_dir() -> Result<std::path::PathBuf> {
        Err("ICC 系统目录仅支持 Windows".into())
    }
}
#[cfg(not(windows))]
pub use unsupported::*;
