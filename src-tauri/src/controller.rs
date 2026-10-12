use filter_core::{
    models::*,
    platform,
    session::{Backend, FileRecovery, Session, Transition, Watcher},
    storage, Result,
};
use serde::Serialize;
use std::{
    path::PathBuf,
    sync::{Condvar, Mutex},
    time::Duration,
};

pub struct Shared {
    pub inner: Mutex<Controller>,
    pub wake: Condvar,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub settings: Settings,
    pub monitors: Vec<Monitor>,
    pub icc_profiles: Vec<String>,
    pub nvidia_percent: Option<i32>,
    pub nvidia_message: String,
    pub active_profile: Option<Profile>,
    pub comparing: bool,
    pub pending_recovery: bool,
    pub game_running: bool,
    pub auto_owned: bool,
    pub error: Option<String>,
    pub draft: Profile,
    pub has_reference: bool,
    pub smoke: bool,
    pub supported: bool,
    pub startup_actual: bool,
}
pub struct Controller {
    pub settings: Settings,
    pub session: Session,
    pub monitors: Vec<Monitor>,
    pub icc_profiles: Vec<String>,
    pub root: PathBuf,
    pub error: Option<String>,
    pub invalid_recovery: bool,
    pub blocked: bool,
    pub draft: Profile,
    pub reference: Option<PathBuf>,
    pub smoke: bool,
    pub watcher: Watcher,
    pub game_running: bool,
    pub auto_owned: bool,
    nvidia_percent: Option<i32>,
    nvidia_message: String,
    pub smoke_report: Option<PathBuf>,
    pub startup_actual: bool,
}
impl Controller {
    pub fn new(root: PathBuf, smoke_report: Option<PathBuf>) -> Result<Self> {
        let smoke = smoke_report.is_some();
        let settings_path = root.join("settings.json");
        let loaded = if settings_path.exists() {
            storage::read::<Settings>(&settings_path).and_then(|s| s.validate().map(|_| s))
        } else {
            Ok(Settings::default())
        };
        let (settings, error, blocked) = match loaded {
            Ok(s) => (s, None, false),
            Err(e) => (Settings::default(), Some(e), true),
        };
        let draft = settings.selected().clone();
        #[cfg(windows)]
        let backend: Box<dyn Backend> = if smoke {
            Box::new(Simulated::default())
        } else {
            Box::new(platform::WindowsBackend)
        };
        #[cfg(not(windows))]
        let backend: Box<dyn Backend> = Box::new(Simulated::default());
        let mut c = Self {
            settings,
            session: Session::new(backend, Box::new(FileRecovery(root.join("recovery.json")))),
            monitors: vec![],
            icc_profiles: vec![],
            root,
            error,
            invalid_recovery: false,
            blocked,
            draft,
            reference: None,
            smoke,
            watcher: Watcher::default(),
            game_running: false,
            auto_owned: false,
            nvidia_percent: None,
            nvidia_message: String::new(),
            smoke_report,
            startup_actual: false,
        };
        if let Err(e) = c.refresh() {
            c.error = Some(e);
        }
        let recovery = c.root.join("recovery.json");
        if recovery.exists() {
            let restore = storage::read::<Snapshot>(&recovery)
                .and_then(|s| c.session.attach(s))
                .and_then(|_| c.session.restore());
            if let Err(e) = restore {
                c.error = Some(format!("启动恢复未完成：{e}。原始备份已保留"));
                if c.session.snapshot.is_none() {
                    c.blocked = true;
                    c.invalid_recovery = true;
                }
            }
        }
        Ok(c)
    }
    pub fn view(&self) -> View {
        View {
            settings: self.settings.clone(),
            monitors: self.monitors.clone(),
            icc_profiles: self.icc_profiles.clone(),
            nvidia_percent: self.nvidia_percent,
            nvidia_message: self.nvidia_message.clone(),
            active_profile: self.session.active.clone(),
            comparing: self.session.comparing,
            pending_recovery: self.invalid_recovery
                || self.session.snapshot.is_some()
                    && self.session.active.is_none()
                    && !self.session.comparing,
            game_running: self.game_running,
            auto_owned: self.auto_owned,
            error: self.error.clone(),
            draft: self.draft.clone(),
            has_reference: self.reference.is_some(),
            smoke: self.smoke,
            supported: cfg!(windows) || self.smoke,
            startup_actual: self.startup_actual,
        }
    }
    pub fn monitor(&self) -> Result<Monitor> {
        self.monitors
            .iter()
            .find(|m| Some(&m.monitor_id) == self.settings.monitor_id.as_ref())
            .or_else(|| {
                if self.settings.monitor_id.is_none() {
                    self.monitors.first()
                } else {
                    None
                }
            })
            .cloned()
            .ok_or_else(|| "请连接并选择目标显示器".into())
    }
    pub fn refresh(&mut self) -> Result<()> {
        self.monitors = if self.smoke {
            vec![Monitor {
                device_name: r"\\.\DISPLAY1".into(),
                monitor_id: "MONITOR\\CI".into(),
                label: "CI 模拟显示器 · DISPLAY1".into(),
                primary: true,
            }]
        } else {
            platform::monitors()?
        };
        self.startup_actual = if self.smoke {
            self.settings.start_with_windows
        } else {
            platform::startup_enabled()?
        };
        self.icc_profiles.clear();
        let mut dirs = vec![self.root.join("icc")];
        if let Ok(dir) = platform::system_icc_dir() {
            dirs.push(dir);
        }
        for dir in dirs {
            if !dir.exists() {
                continue;
            }
            for entry in std::fs::read_dir(&dir).map_err(|e| format!("读取 ICC 目录失败：{e}"))?
            {
                let path = entry.map_err(|e| e.to_string())?.path();
                if path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|s| s.eq_ignore_ascii_case("icc") || s.eq_ignore_ascii_case("icm"))
                {
                    self.icc_profiles.push(path.to_string_lossy().to_string());
                }
            }
        }
        self.icc_profiles.sort();
        self.icc_profiles.dedup();
        let nv = if self.smoke {
            Ok(50)
        } else {
            self.monitor().and_then(|m| platform::nvidia_status(&m))
        };
        match nv {
            Ok(percent) => {
                self.nvidia_percent = Some(percent);
                self.nvidia_message = format!("NVIDIA 已连接 · 当前 {percent}%");
            }
            Err(e) => {
                self.nvidia_percent = None;
                self.nvidia_message = e;
            }
        }
        Ok(())
    }
    pub fn ensure(&self) -> Result<()> {
        if self.blocked {
            Err("配置或备份损坏，请先保留并修复原文件；已禁止覆盖".into())
        } else {
            Ok(())
        }
    }
    pub fn save_settings(&mut self, settings: Settings) -> Result<()> {
        self.ensure()?;
        settings.validate()?;
        if let Some(s) = &self.session.snapshot {
            if settings
                .monitor_id
                .as_ref()
                .is_some_and(|id| id != &s.monitor.monitor_id)
            {
                return Err("请先恢复原始色彩，再切换显示器".into());
            }
        }
        if self.auto_owned && !settings.follow_game {
            self.restore(false)?;
        }
        let old_actual = self.startup_actual;
        if settings.start_with_windows != old_actual && !self.smoke {
            platform::startup_set(settings.start_with_windows)?;
        }
        if let Err(e) = storage::write(&self.root.join("settings.json"), &settings) {
            if settings.start_with_windows != old_actual && !self.smoke {
                if let Err(rollback) = platform::startup_set(old_actual) {
                    return Err(format!("保存失败：{e}；自启回滚失败：{rollback}"));
                }
            }
            return Err(e);
        }
        if self.settings.follow_game != settings.follow_game
            || self.settings.game_processes != settings.game_processes
        {
            self.watcher = Watcher::default();
            self.game_running = false;
        }
        let monitor_changed = self.settings.monitor_id != settings.monitor_id;
        self.startup_actual = settings.start_with_windows;
        self.settings = settings;
        if monitor_changed {
            self.refresh()?;
        }
        Ok(())
    }
    pub fn apply(&mut self, p: Profile) -> Result<()> {
        self.ensure()?;
        if !cfg!(windows) && !self.smoke {
            return Err("本机仅可预览界面，显示器调节需要 Windows".into());
        }
        self.session.apply(&self.monitor()?, &p)?;
        self.draft = p;
        Ok(())
    }
    pub fn restore(&mut self, suppress: bool) -> Result<()> {
        if self.invalid_recovery {
            return Err("恢复备份损坏，原文件已保留；无法宣称恢复成功".into());
        }
        self.session.restore()?;
        self.auto_owned = false;
        if suppress {
            self.watcher.suppress();
        }
        Ok(())
    }
    pub fn toggle(&mut self) -> Result<()> {
        if self.session.active.is_some() || self.session.comparing {
            self.restore(true)
        } else {
            self.apply(self.draft.clone())
        }
    }
    pub fn select(&mut self, id: &str) -> Result<()> {
        self.ensure()?;
        let p = self
            .settings
            .profiles
            .iter()
            .find(|p| p.id == id)
            .cloned()
            .ok_or("方案不存在")?;
        // Switching an enabled scheme must update the physical display, including every parameter.
        if self.session.active.is_some() {
            self.apply(p.clone())?;
        }
        let mut settings = self.settings.clone();
        settings.selected_profile_id = Some(id.into());
        self.save_settings(settings)?;
        self.draft = p;
        Ok(())
    }
    pub fn save_profile(&mut self, p: Profile) -> Result<()> {
        p.validate()?;
        let mut settings = self.settings.clone();
        match settings.profiles.iter_mut().find(|v| v.id == p.id) {
            Some(old) => *old = p.clone(),
            None => settings.profiles.push(p.clone()),
        }
        settings.selected_profile_id = Some(p.id.clone());
        self.save_settings(settings)?;
        self.draft = p;
        Ok(())
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if self.settings.profiles.len() == 1 {
            return Err("至少保留一个方案".into());
        }
        if self.session.active.as_ref().is_some_and(|p| p.id == id) {
            return Err("请先恢复原始色彩，再删除正在应用的方案".into());
        }
        let mut settings = self.settings.clone();
        settings.profiles.retain(|p| p.id != id);
        if settings.selected_profile_id.as_deref() == Some(id) {
            settings.selected_profile_id = None;
        }
        if settings.game_profile_id.as_deref() == Some(id) {
            settings.game_profile_id = None;
        }
        self.save_settings(settings)?;
        self.draft = self.settings.selected().clone();
        Ok(())
    }
    pub fn poll(&mut self, on_start: impl FnOnce()) -> Result<bool> {
        if !self.settings.follow_game
            || self.blocked
            || self.session.snapshot.is_some()
                && self.session.active.is_none()
                && !self.session.comparing
        {
            return Ok(false);
        }
        let names = if self.smoke {
            vec![]
        } else {
            platform::processes()?
        };
        let running = names.iter().any(|p| {
            self.settings
                .game_processes
                .iter()
                .any(|g| p.eq_ignore_ascii_case(g))
        });
        self.game_running = running;
        match self.watcher.observe(running) {
            Transition::Started => {
                on_start();
                let p = self.settings.game_profile().clone();
                self.apply(p)?;
                self.auto_owned = true;
                Ok(true)
            }
            Transition::Exited => {
                if self.auto_owned {
                    on_start();
                    self.restore(false)?;
                }
                Ok(true)
            }
            Transition::None => Ok(false),
        }
    }
}
#[derive(Default)]
struct Simulated {
    ramp: Option<Vec<u16>>,
    raw: i32,
}
impl Backend for Simulated {
    fn capture(&mut self, m: &Monitor, _: bool) -> Result<Snapshot> {
        Ok(Snapshot {
            monitor: m.clone(),
            gamma_ramp: self
                .ramp
                .clone()
                .unwrap_or_else(filter_core::tone::identity),
            per_user_profiles: false,
            user_icc_profile: None,
            nvidia_vibrance_raw: Some(50),
            applied_icc_profiles: vec![],
        })
    }
    fn metadata(&mut self, s: &Snapshot, p: Option<&Profile>) -> Result<()> {
        self.raw = p
            .and_then(|p| p.color.digital_vibrance)
            .unwrap_or(s.nvidia_vibrance_raw.unwrap_or(50));
        Ok(())
    }
    fn read_ramp(&mut self, _: &Monitor) -> Result<Vec<u16>> {
        Ok(self
            .ramp
            .clone()
            .unwrap_or_else(filter_core::tone::identity))
    }
    fn write_ramp(&mut self, _: &Monitor, r: &[u16]) -> Result<()> {
        self.ramp = Some(r.to_vec());
        Ok(())
    }
    fn settle(&mut self, _: Duration) {}
}

#[cfg(test)]
mod tests {
    use super::*;
    fn controller() -> Controller {
        let root = std::env::temp_dir().join(format!("df-controller-{}", Profile::default().id));
        Controller::new(root.clone(), Some(root.join("report.json"))).unwrap()
    }
    #[test]
    fn enabled_scheme_switch_preserves_auto_restoration() {
        let mut c = controller();
        c.save_settings(Settings {
            follow_game: true,
            ..c.settings.clone()
        })
        .unwrap();
        let a = c.settings.profiles[0].clone();
        let b = c.settings.profiles[1].clone();
        c.apply(a.clone()).unwrap();
        c.auto_owned = true;
        c.select(&b.id).unwrap();
        assert!(c.auto_owned);
        assert_eq!(c.session.active.as_ref().unwrap().color, b.color);
        c.select(&a.id).unwrap();
        assert_eq!(c.session.active.as_ref().unwrap().color, a.color);
        c.save_settings(Settings {
            follow_game: false,
            ..c.settings.clone()
        })
        .unwrap();
        assert!(c.session.snapshot.is_none());
        assert!(!c.auto_owned);
        std::fs::remove_dir_all(c.root).unwrap();
    }
    #[test]
    fn close_and_reopen_draft_is_distinct_from_saved_scheme() {
        let mut c = controller();
        c.draft.color.brightness = 20;
        assert_ne!(c.view().draft.color, c.settings.selected().color);
        c.save_profile(c.draft.clone()).unwrap();
        assert_eq!(c.view().draft.color, c.settings.selected().color);
        std::fs::remove_dir_all(c.root).unwrap();
    }
    #[test]
    fn invalid_settings_are_preserved() {
        let root = std::env::temp_dir().join(format!("df-corrupt-{}", Profile::default().id));
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(root.join("settings.json"), b"corrupt").unwrap();
        let mut c = Controller::new(root.clone(), Some(root.join("report.json"))).unwrap();
        assert!(c.blocked);
        assert!(c.save_profile(Profile::default()).is_err());
        assert_eq!(
            std::fs::read(root.join("settings.json")).unwrap(),
            b"corrupt"
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
