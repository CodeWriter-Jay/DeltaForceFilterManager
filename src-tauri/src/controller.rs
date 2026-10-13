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
    time::{Duration, Instant},
};

pub struct Shared {
    pub inner: Mutex<Controller>,
    pub wake: Condvar,
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct View {
    pub app_version: String,
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
    pub live_enabled: bool,
    pub live_verifying: bool,
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
    pub live_enabled: bool,
    pub updating: bool,
    live_verification: Option<(Instant, usize)>,
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
        let live_enabled = settings.live_adjust && (cfg!(windows) || smoke);
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
            live_enabled,
            updating: false,
            live_verification: None,
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
            app_version: env!("CARGO_PKG_VERSION").into(),
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
            live_enabled: self.live_enabled && (cfg!(windows) || self.smoke),
            live_verifying: self.live_verification.is_some(),
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
                self.nvidia_message = format!("NVIDIA 已连接 · 上次读取 {percent}%");
            }
            Err(e) => {
                self.nvidia_percent = None;
                self.nvidia_message = e;
            }
        }
        Ok(())
    }
    pub fn ensure(&self) -> Result<()> {
        if self.updating {
            Err("正在升级，请等待程序重新启动".into())
        } else if self.blocked {
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
        if self.settings.follow_game != settings.follow_game {
            self.watcher = Watcher::default();
            self.game_running = false;
        }
        let monitor_changed = self.settings.monitor_id != settings.monitor_id;
        if self.settings.live_adjust != settings.live_adjust {
            self.live_enabled = settings.live_adjust;
        }
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
        self.live_enabled = self.settings.live_adjust;
        self.live_verification = None;
        self.draft = p;
        Ok(())
    }
    pub fn apply_live(&mut self, profile: Profile) -> Result<()> {
        self.ensure()?;
        if !self.live_enabled {
            return Err("实时调色已暂停，请重新开启实时调色".into());
        }
        if !cfg!(windows) && !self.smoke {
            return Err("实时显示器调色仅支持 Windows".into());
        }
        if let Err(error) = self.session.apply_live(&self.monitor()?, &profile) {
            self.live_enabled = false;
            self.live_verification = None;
            return Err(error);
        }
        self.draft = profile;
        self.live_verification = Some((Instant::now() + Duration::from_millis(200), 0));
        Ok(())
    }
    pub fn set_live(&mut self, enabled: bool, profile: Profile) -> Result<()> {
        self.save_settings(Settings {
            live_adjust: enabled,
            ..self.settings.clone()
        })?;
        self.live_enabled = enabled;
        if enabled {
            self.apply_live(profile)?;
        }
        Ok(())
    }
    pub fn live_wait(&self) -> Option<Duration> {
        self.live_verification
            .map(|(due, _)| due.saturating_duration_since(Instant::now()))
    }
    pub fn verify_live(&mut self) -> Result<bool> {
        let Some((due, step)) = self.live_verification else {
            return Ok(false);
        };
        if Instant::now() < due {
            return Ok(false);
        }
        if let Err(error) = self.session.verify_live(step == 3) {
            self.live_enabled = false;
            self.live_verification = None;
            self.auto_owned = false;
            return Err(error);
        }
        self.live_verification = if step == 3 {
            None
        } else {
            Some((
                Instant::now() + Duration::from_millis([400, 800, 400][step]),
                step + 1,
            ))
        };
        Ok(step == 3)
    }
    pub fn compare(&mut self) -> Result<()> {
        self.live_enabled = false;
        self.live_verification = None;
        self.session.compare()
    }
    pub fn prepare_update(&mut self) -> Result<PathBuf> {
        self.ensure()?;
        let backup = self.root.join(format!(
            "settings.before-update-{}.json",
            Profile::default().id
        ));
        storage::write(&backup, &self.settings)?;
        self.restore(true)?;
        self.updating = true;
        Ok(backup)
    }
    pub fn restore(&mut self, suppress: bool) -> Result<()> {
        self.live_enabled = false;
        self.live_verification = None;
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
        if self.live_enabled {
            self.apply_live(p.clone())?;
        } else if self.session.active.is_some() {
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
        let new_selection = self.draft.id != p.id;
        let mut settings = self.settings.clone();
        match settings.profiles.iter_mut().find(|v| v.id == p.id) {
            Some(old) => *old = p.clone(),
            None => settings.profiles.push(p.clone()),
        }
        settings.selected_profile_id = Some(p.id.clone());
        self.save_settings(settings)?;
        self.draft = p.clone();
        if new_selection && self.live_enabled {
            self.apply_live(p)?;
        }
        Ok(())
    }
    pub fn update_profile(&mut self, id: &str, name: String, processes: Vec<String>) -> Result<()> {
        let mut settings = self.settings.clone();
        let profile = settings
            .profiles
            .iter_mut()
            .find(|p| p.id == id)
            .ok_or("方案不存在")?;
        profile.name = name;
        profile.process_names = processes;
        let profile = profile.clone();
        self.save_settings(settings)?;
        // Metadata changes must preserve unsaved color adjustments and the display session.
        if self.draft.id == id {
            self.draft.name = profile.name.clone();
            self.draft.process_names = profile.process_names.clone();
        }
        if let Some(active) = self.session.active.as_mut().filter(|p| p.id == id) {
            active.name = profile.name;
            active.process_names = profile.process_names;
        }
        Ok(())
    }
    pub fn reorder(&mut self, ids: Vec<String>) -> Result<()> {
        if ids.len() != self.settings.profiles.len() {
            return Err("排序必须包含所有方案且不能重复".into());
        }
        let mut remaining = self.settings.profiles.clone();
        let mut settings = self.settings.clone();
        // A missing selection ID used to mean the first row; moving it must not select another scheme.
        settings.selected_profile_id = Some(self.settings.selected().id.clone());
        settings.profiles.clear();
        for id in ids {
            let index = remaining
                .iter()
                .position(|p| p.id == id)
                .ok_or("排序中包含重复或不存在的方案")?;
            settings.profiles.push(remaining.remove(index));
        }
        self.save_settings(settings)
    }
    pub fn delete(&mut self, id: &str) -> Result<()> {
        if !self.settings.profiles.iter().any(|p| p.id == id) {
            return Err("方案不存在".into());
        }
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
        if self.draft.id == id {
            self.draft = self.settings.selected().clone();
        }
        Ok(())
    }
    pub fn poll(&mut self, on_start: impl FnOnce()) -> Result<bool> {
        if !self.settings.follow_game
            || self.blocked
            || self.updating
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
        self.poll_processes(&names, on_start)
    }
    fn poll_processes(&mut self, names: &[String], on_start: impl FnOnce()) -> Result<bool> {
        let profile = self.settings.matching_profile(names).cloned();
        let running = profile.is_some();
        let changed = self.game_running != running;
        self.game_running = running;
        // Commit the observation only after the display operation succeeds, so failures can be retried.
        let mut watcher = self.watcher.clone();
        match watcher.observe_profile(profile.as_ref().map(|p| p.id.as_str())) {
            Transition::Started => {
                on_start();
                self.apply(profile.ok_or("匹配的方案不存在")?)?;
                self.auto_owned = true;
            }
            Transition::Exited => {
                if self.auto_owned {
                    on_start();
                    self.restore(false)?;
                }
            }
            Transition::None => {
                self.watcher = watcher;
                return Ok(changed);
            }
        }
        self.watcher = watcher;
        Ok(true)
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

    #[test]
    fn bound_games_switch_by_priority_and_restore_the_original_snapshot() {
        let mut c = controller();
        let mut settings = c.settings.clone();
        settings.follow_game = true;
        settings.game_processes.clear();
        settings.profiles[0].process_names = vec!["a.exe".into()];
        settings.profiles[1].process_names = vec!["b.exe".into()];
        let a = settings.profiles[0].clone();
        let b = settings.profiles[1].clone();
        c.save_settings(settings).unwrap();
        assert!(c.poll_processes(&["B.EXE".into()], || {}).unwrap());
        assert_eq!(c.session.active.as_ref().unwrap().id, b.id);
        let original = c.session.snapshot.as_ref().unwrap().gamma_ramp.clone();
        assert!(!c
            .poll_processes(&["b.exe".into()], || panic!("unchanged target"))
            .unwrap());
        assert!(c
            .poll_processes(&["a.exe".into(), "b.exe".into()], || {})
            .unwrap());
        assert_eq!(c.session.active.as_ref().unwrap().id, a.id);
        assert_eq!(c.session.snapshot.as_ref().unwrap().gamma_ramp, original);
        let mut ids: Vec<_> = c.settings.profiles.iter().map(|p| p.id.clone()).collect();
        ids.swap(0, 1);
        c.reorder(ids).unwrap();
        assert!(c
            .poll_processes(&["a.exe".into(), "b.exe".into()], || {})
            .unwrap());
        assert_eq!(c.session.active.as_ref().unwrap().id, b.id);
        assert!(c.poll_processes(&[], || {}).unwrap());
        assert!(!c.auto_owned);
        assert!(c.session.snapshot.is_none());
        let monitor = c.monitor().unwrap();
        assert_eq!(c.session.backend.read_ramp(&monitor).unwrap(), original);
        std::fs::remove_dir_all(c.root).unwrap();
    }

    #[test]
    fn manual_restore_stays_suppressed_across_binding_edits_and_game_switches() {
        let mut c = controller();
        let mut settings = c.settings.clone();
        settings.follow_game = true;
        settings.profiles[0].process_names = vec!["a.exe".into()];
        settings.profiles[1].process_names = vec!["b.exe".into()];
        c.save_settings(settings).unwrap();
        c.poll_processes(&["a.exe".into()], || {}).unwrap();
        c.restore(true).unwrap();
        let b = c.settings.profiles[1].id.clone();
        c.update_profile(
            &b,
            "Game B".into(),
            vec!["b.exe".into(), "launcher.exe".into()],
        )
        .unwrap();
        assert!(!c
            .poll_processes(&["b.exe".into()], || panic!("suppressed"))
            .unwrap());
        assert!(c.session.active.is_none());
        c.poll_processes(&[], || {}).unwrap();
        c.poll_processes(&["b.exe".into()], || {}).unwrap();
        assert_eq!(c.session.active.as_ref().unwrap().id, b);
        c.restore(false).unwrap();
        std::fs::remove_dir_all(c.root).unwrap();
    }

    #[test]
    fn failed_automatic_apply_can_retry_the_same_game() {
        let mut c = controller();
        let mut settings = c.settings.clone();
        settings.follow_game = true;
        settings.monitor_id = Some("disconnected".into());
        settings.profiles[0].process_names = vec!["game.exe".into()];
        c.save_settings(settings).unwrap();
        assert!(c.poll_processes(&["game.exe".into()], || {}).is_err());
        assert!(c.session.active.is_none());
        c.save_settings(Settings {
            monitor_id: None,
            ..c.settings.clone()
        })
        .unwrap();
        assert!(c.poll_processes(&["game.exe".into()], || {}).unwrap());
        assert!(c.auto_owned);
        c.restore(false).unwrap();
        std::fs::remove_dir_all(c.root).unwrap();
    }

    #[test]
    fn managing_schemes_preserves_drafts_and_persists_order_and_bindings() {
        let mut c = controller();
        let selected = c.settings.selected().id.clone();
        c.draft.color.brightness = 25;
        c.update_profile(&selected, "Renamed".into(), vec!["game.exe".into()])
            .unwrap();
        assert_eq!(c.draft.color.brightness, 25);
        assert_eq!(c.settings.selected().color.brightness, 0);
        let mut ids: Vec<_> = c.settings.profiles.iter().map(|p| p.id.clone()).collect();
        ids.reverse();
        c.reorder(ids.clone()).unwrap();
        assert_eq!(c.settings.selected().id, selected);
        assert_eq!(c.settings.game_profile().id, selected);
        assert_eq!(c.draft.color.brightness, 25);
        assert!(c.reorder(vec![selected.clone(); ids.len()]).is_err());
        let path = c.root.join("settings.json");
        let saved: Settings = storage::read(&path).unwrap();
        assert_eq!(
            saved
                .profiles
                .iter()
                .map(|p| p.id.clone())
                .collect::<Vec<_>>(),
            ids
        );
        assert_eq!(saved.selected().name, "Renamed");
        assert_eq!(saved.selected().process_names, ["game.exe"]);
        let other = c.settings.profiles[0].id.clone();
        c.settings.game_profile_id = Some(other.clone());
        c.delete(&other).unwrap();
        assert_eq!(c.draft.color.brightness, 25);
        assert_eq!(c.settings.selected().id, selected);
        assert!(c.settings.game_profile_id.is_none());
        let reopened = Controller::new(c.root.clone(), Some(c.root.join("report.json"))).unwrap();
        assert_eq!(reopened.settings.profiles, c.settings.profiles);
        assert_eq!(reopened.settings.selected().id, selected);
        c.apply(c.draft.clone()).unwrap();
        assert!(c.delete(&selected).is_err());
        c.restore(false).unwrap();
        for id in ids.into_iter().filter(|id| id != &selected && id != &other) {
            c.delete(&id).unwrap();
        }
        assert!(c.delete(&selected).is_err());
        std::fs::remove_dir_all(c.root).unwrap();
    }

    #[test]
    fn live_color_drafts_are_unsaved_and_restore_cancels_pending_verification() {
        let mut c = controller();
        let mut profile = c.draft.clone();
        profile.color.gamma = 1.3;
        c.apply_live(profile.clone()).unwrap();
        assert_eq!(c.session.active.as_ref().unwrap().color, profile.color);
        assert_ne!(c.settings.selected().color, profile.color);
        assert!(c.view().live_verifying);
        for step in 0..4 {
            c.live_verification = Some((Instant::now(), step));
            c.verify_live().unwrap();
        }
        assert!(!c.view().live_verifying);
        c.restore(true).unwrap();
        assert!(!c.view().live_enabled);
        assert!(c.live_wait().is_none());
        assert!(c.apply_live(profile.clone()).is_err());
        c.set_live(true, profile).unwrap();
        c.restore(false).unwrap();
        std::fs::remove_dir_all(c.root).unwrap();
    }

    #[test]
    fn updating_backs_up_settings_restores_display_and_reopens_the_same_schemes() {
        let mut c = controller();
        let mut profile = c.draft.clone();
        profile.name = "升级保留方案".into();
        profile.process_names = vec!["game.exe".into()];
        profile.color.brightness = 35;
        c.save_profile(profile.clone()).unwrap();
        c.apply_live(profile.clone()).unwrap();
        let backup = c.prepare_update().unwrap();
        let saved: Settings = storage::read(&backup).unwrap();
        assert_eq!(saved.profiles, c.settings.profiles);
        assert!(c.session.snapshot.is_none());
        assert!(c.updating);
        assert!(c.apply_live(profile).is_err());
        let reopened = Controller::new(c.root.clone(), Some(c.root.join("report.json"))).unwrap();
        assert_eq!(reopened.settings.profiles, c.settings.profiles);
        std::fs::remove_dir_all(c.root).unwrap();
    }
}
