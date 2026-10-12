use crate::{
    icc,
    models::{Monitor, Profile, Snapshot},
    tone, Result,
};
use std::{path::Path, time::Duration};

pub trait Backend: Send {
    fn capture(&mut self, monitor: &Monitor, require_vibrance: bool) -> Result<Snapshot>;
    /// Set ICC and NVIDIA first. A profile without either component restores its original value.
    fn metadata(&mut self, snapshot: &Snapshot, profile: Option<&Profile>) -> Result<()>;
    fn read_ramp(&mut self, monitor: &Monitor) -> Result<Vec<u16>>;
    fn write_ramp(&mut self, monitor: &Monitor, ramp: &[u16]) -> Result<()>;
    fn settle(&mut self, delay: Duration) {
        std::thread::sleep(delay);
    }
}
pub trait Recovery: Send {
    fn save(&mut self, s: &Snapshot) -> Result<()>;
    fn clear(&mut self) -> Result<()>;
}
pub struct FileRecovery(pub std::path::PathBuf);
impl Recovery for FileRecovery {
    fn save(&mut self, s: &Snapshot) -> Result<()> {
        crate::storage::write(&self.0, s)
    }
    fn clear(&mut self) -> Result<()> {
        match std::fs::remove_file(&self.0) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

pub struct Session {
    pub backend: Box<dyn Backend>,
    recovery: Box<dyn Recovery>,
    pub snapshot: Option<Snapshot>,
    pub active: Option<Profile>,
    pub comparing: bool,
}
impl Session {
    pub fn new(backend: Box<dyn Backend>, recovery: Box<dyn Recovery>) -> Self {
        Self {
            backend,
            recovery,
            snapshot: None,
            active: None,
            comparing: false,
        }
    }
    pub fn attach(&mut self, snapshot: Snapshot) -> Result<()> {
        snapshot.validate()?;
        if self.snapshot.is_some() {
            return Err("已有调节会话".into());
        }
        self.snapshot = Some(snapshot);
        Ok(())
    }
    pub fn preview(&self, p: &Profile) -> Result<Vec<u16>> {
        p.validate()?;
        let base = match &p.icc_path {
            Some(path) => icc::read(Path::new(path))?.unwrap_or_else(tone::identity),
            None => self
                .snapshot
                .as_ref()
                .map(|s| s.gamma_ramp.clone())
                .unwrap_or_else(tone::identity),
        };
        tone::build(&p.color, &base)
    }
    pub fn apply(&mut self, monitor: &Monitor, p: &Profile) -> Result<()> {
        p.validate()?;
        let icc = match &p.icc_path {
            Some(path) => Some(icc::read(Path::new(path))?.unwrap_or_else(tone::identity)),
            None => None,
        };
        if let Some(s) = &self.snapshot {
            if self.active.is_none() && !self.comparing {
                return Err("存在未完成恢复，请先恢复原始色彩".into());
            }
            if s.monitor.monitor_id != monitor.monitor_id {
                return Err("请先恢复当前显示器，再切换显示器".into());
            }
            if p.color.digital_vibrance.is_some() && s.nvidia_vibrance_raw.is_none() {
                return Err("当前会话没有 NVIDIA 备份，请先恢复再启用数字振动".into());
            }
        }
        let mut snapshot = match &self.snapshot {
            Some(s) => s.clone(),
            None => self
                .backend
                .capture(monitor, p.color.digital_vibrance.is_some())?,
        };
        snapshot.validate()?;
        if let Some(path) = &p.icc_path {
            let name = Path::new(path)
                .file_name()
                .and_then(|n| n.to_str())
                .ok_or("ICC 文件名无效")?
                .to_string();
            if !snapshot
                .applied_icc_profiles
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&name))
            {
                snapshot.applied_icc_profiles.push(name);
            }
        }
        snapshot.validate()?;
        let ramp = tone::build(&p.color, icc.as_deref().unwrap_or(&snapshot.gamma_ramp))?;
        self.recovery.save(&snapshot)?;
        self.snapshot = Some(snapshot.clone());
        self.active = None;
        self.comparing = false;
        let result = self
            .backend
            .metadata(&snapshot, Some(p))
            .and_then(|_| self.stabilize(&snapshot.monitor, &ramp));
        match result {
            Ok(()) => {
                self.active = Some(p.clone());
                Ok(())
            }
            Err(apply_error) => match self.restore() {
                Ok(()) => Err(format!("应用失败，已恢复原始色彩：{apply_error}")),
                Err(restore_error) => Err(format!(
                    "应用失败：{apply_error}；恢复未完成：{restore_error}。备份已保留，请先恢复"
                )),
            },
        }
    }
    fn stabilize(&mut self, monitor: &Monitor, ramp: &[u16]) -> Result<()> {
        self.backend.write_ramp(monitor, ramp)?;
        self.verify(monitor, ramp)?;
        // WCS/driver calibration loads can complete after a successful GDI call.
        // Finite retries repair this transition without a permanent background loop.
        for delay in [200, 400, 800] {
            self.backend.settle(Duration::from_millis(delay));
            if !tone::matches(ramp, &self.backend.read_ramp(monitor)?) {
                self.backend.write_ramp(monitor, ramp)?;
                self.verify(monitor, ramp)?;
            }
        }
        // Do not declare success immediately after the last corrective write.
        self.backend.settle(Duration::from_millis(400));
        self.verify(monitor, ramp)
    }
    fn verify(&mut self, monitor: &Monitor, ramp: &[u16]) -> Result<()> {
        if tone::matches(ramp, &self.backend.read_ramp(monitor)?) {
            Ok(())
        } else {
            Err("Gamma 回读不一致：驱动拒绝或其他程序持续覆盖曲线".into())
        }
    }
    pub fn compare(&mut self) -> Result<()> {
        let s = self.snapshot.clone().ok_or("尚未启用滤镜")?;
        self.active = None;
        self.comparing = false;
        self.restore_components(&s)?;
        self.comparing = true;
        Ok(())
    }
    fn restore_components(&mut self, s: &Snapshot) -> Result<()> {
        // Try the complete LUT even if an ICC or DVC restore fails; retain all causes.
        let metadata = self.backend.metadata(s, None);
        let gamma = self.stabilize(&s.monitor, &s.gamma_ramp);
        match (metadata, gamma) {
            (Ok(()), Ok(())) => Ok(()),
            (Err(a), Err(b)) => Err(format!("{a}；{b}")),
            (Err(e), _) | (_, Err(e)) => Err(e),
        }
    }
    pub fn restore(&mut self) -> Result<()> {
        self.active = None;
        self.comparing = false;
        if let Some(s) = self.snapshot.clone() {
            self.restore_components(&s)?;
            self.recovery.clear()?;
            self.snapshot = None;
        }
        Ok(())
    }
}
#[derive(Default)]
pub struct Watcher {
    running: bool,
    suppressed: bool,
}
#[derive(PartialEq, Debug)]
pub enum Transition {
    None,
    Started,
    Exited,
}
impl Watcher {
    pub fn observe(&mut self, running: bool) -> Transition {
        if !running {
            let exited = self.running;
            self.running = false;
            self.suppressed = false;
            return if exited {
                Transition::Exited
            } else {
                Transition::None
            };
        }
        if self.running {
            return Transition::None;
        }
        self.running = true;
        if self.suppressed {
            Transition::None
        } else {
            Transition::Started
        }
    }
    pub fn suppress(&mut self) {
        self.suppressed = true;
    }
}
