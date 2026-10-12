use crate::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase", deny_unknown_fields)]
pub struct Color {
    pub brightness: i32,
    pub contrast: i32,
    pub gamma: f64,
    pub shadows: i32,
    pub highlights: i32,
    pub red_gain: i32,
    pub green_gain: i32,
    pub blue_gain: i32,
    pub digital_vibrance: Option<i32>,
}
impl Default for Color {
    fn default() -> Self {
        Self {
            brightness: 0,
            contrast: 0,
            gamma: 1.0,
            shadows: 0,
            highlights: 0,
            red_gain: 0,
            green_gain: 0,
            blue_gain: 0,
            digital_vibrance: None,
        }
    }
}
impl Color {
    pub fn validate(&self) -> Result<()> {
        for (value, min, max, name) in [
            (self.brightness, -125, 125, "亮度"),
            (self.contrast, -82, 82, "对比度"),
            (self.shadows, 0, 50, "暗部提升"),
            (self.highlights, 0, 50, "高光压缩"),
            (self.red_gain, -100, 100, "红色增益"),
            (self.green_gain, -100, 100, "绿色增益"),
            (self.blue_gain, -100, 100, "蓝色增益"),
        ] {
            if !(min..=max).contains(&value) {
                return Err(format!("{name}必须在 {min}–{max} 之间"));
            }
        }
        if !self.gamma.is_finite() || !(0.4..=2.8).contains(&self.gamma) {
            return Err("Gamma 必须在 0.4–2.8 之间".into());
        }
        if self
            .digital_vibrance
            .is_some_and(|v| !(0..=100).contains(&v))
        {
            return Err("数字振动必须在 0–100% 之间".into());
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(default, rename_all = "PascalCase", deny_unknown_fields)]
pub struct Profile {
    pub id: String,
    pub name: String,
    pub icc_path: Option<String>,
    pub color: Color,
}
impl Default for Profile {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().simple().to_string(),
            name: "我的方案".into(),
            icc_path: None,
            color: Color::default(),
        }
    }
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.id.len() != 32
            || !self.id.bytes().all(|c| c.is_ascii_hexdigit())
            || self.name.trim().is_empty()
            || self.name.chars().count() > 80
        {
            return Err("方案 ID 或名称无效（名称最多 80 个字符）".into());
        }
        if self
            .icc_path
            .as_ref()
            .is_some_and(|p| p.len() > 4096 || !std::path::Path::new(p).is_absolute())
        {
            return Err("ICC 路径必须为完整路径".into());
        }
        self.color.validate()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default, rename_all = "PascalCase", deny_unknown_fields)]
pub struct Settings {
    pub schema_version: u32,
    pub profiles: Vec<Profile>,
    pub selected_profile_id: Option<String>,
    pub game_profile_id: Option<String>,
    pub monitor_id: Option<String>,
    pub follow_game: bool,
    pub game_processes: Vec<String>,
    pub poll_seconds: u64,
    pub start_with_windows: bool,
    pub close_to_tray: bool,
    pub restore_on_exit: bool,
}
impl Default for Settings {
    fn default() -> Self {
        let profiles = [
            ("均衡 · 三角洲", 1.10, 14, 8),
            ("室内 · 暗部可见", 1.18, 24, 10),
            ("室外 · 保留高光", 1.04, 8, 24),
            ("中性 · 校准基准", 1.0, 0, 0),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (name, gamma, shadows, highlights))| Profile {
            id: format!("{:032x}", i + 1),
            name: name.into(),
            icc_path: None,
            color: Color {
                gamma,
                shadows,
                highlights,
                ..Color::default()
            },
        })
        .collect();
        Self {
            schema_version: 1,
            profiles,
            selected_profile_id: None,
            game_profile_id: None,
            monitor_id: None,
            follow_game: false,
            game_processes: vec![
                "DeltaForceClient-Win64-Shipping.exe".into(),
                "DeltaForceClient.exe".into(),
            ],
            poll_seconds: 3,
            start_with_windows: false,
            close_to_tray: true,
            restore_on_exit: true,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<()> {
        if self.schema_version != 1 || !(1..=100).contains(&self.profiles.len()) {
            return Err("配置版本无效或方案数量不在 1–100 之间".into());
        }
        let mut ids = HashSet::new();
        for p in &self.profiles {
            p.validate()?;
            if !ids.insert(&p.id) {
                return Err("方案 ID 重复".into());
            }
        }
        for id in [&self.selected_profile_id, &self.game_profile_id]
            .into_iter()
            .flatten()
        {
            if !ids.contains(id) {
                return Err("选中的方案不存在".into());
            }
        }
        if !(2..=30).contains(&self.poll_seconds) || !(1..=16).contains(&self.game_processes.len())
        {
            return Err("检测间隔应为 2–30 秒，需要 1–16 个游戏进程名".into());
        }
        for p in &self.game_processes {
            if p.trim().is_empty()
                || p.len() > 800
                || p.contains(['/', '\\', ':', '*', '?', '"', '\0'])
                || !p.to_ascii_lowercase().ends_with(".exe")
            {
                return Err("请填写进程文件名（如 game.exe），不要填写路径".into());
            }
        }
        Ok(())
    }
    pub fn selected(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| Some(&p.id) == self.selected_profile_id.as_ref())
            .unwrap_or(&self.profiles[0])
    }
    pub fn game_profile(&self) -> &Profile {
        self.profiles
            .iter()
            .find(|p| Some(&p.id) == self.game_profile_id.as_ref())
            .unwrap_or_else(|| self.selected())
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Monitor {
    pub device_name: String,
    pub monitor_id: String,
    pub label: String,
    pub primary: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase", deny_unknown_fields)]
pub struct Snapshot {
    pub monitor: Monitor,
    pub gamma_ramp: Vec<u16>,
    pub per_user_profiles: bool,
    pub user_icc_profile: Option<String>,
    pub nvidia_vibrance_raw: Option<i32>,
    #[serde(default)]
    pub applied_icc_profiles: Vec<String>,
}
impl Snapshot {
    pub fn validate(&self) -> Result<()> {
        if self.gamma_ramp.len() != 768
            || self.monitor.monitor_id.is_empty()
            || !self.monitor.device_name.starts_with(r"\\.\DISPLAY")
            || self.applied_icc_profiles.len() > 100
            || self
                .applied_icc_profiles
                .iter()
                .any(|p| p.is_empty() || p.contains(['/', '\\', '\0']))
        {
            return Err("恢复备份无效".into());
        }
        Ok(())
    }
}
