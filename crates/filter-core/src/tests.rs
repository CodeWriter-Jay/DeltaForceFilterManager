use crate::{icc, models::*, session::*, storage, tone};
use std::{
    path::PathBuf,
    sync::{Arc, Mutex},
    time::Duration,
};
fn monitor() -> Monitor {
    Monitor {
        device_name: r"\\.\DISPLAY1".into(),
        monitor_id: "MONITOR\\TEST".into(),
        label: "Test".into(),
        primary: true,
    }
}
#[derive(Default)]
struct StoreState {
    saved: Option<Snapshot>,
    clears: u32,
    fail_clear: bool,
    saves: u32,
}
struct Store(Arc<Mutex<StoreState>>);
impl Recovery for Store {
    fn save(&mut self, s: &Snapshot) -> crate::Result<()> {
        let mut state = self.0.lock().unwrap();
        state.saved = Some(s.clone());
        state.saves += 1;
        Ok(())
    }
    fn clear(&mut self) -> crate::Result<()> {
        let mut s = self.0.lock().unwrap();
        if s.fail_clear {
            return Err("disk failure".into());
        }
        s.saved = None;
        s.clears += 1;
        Ok(())
    }
}
struct Hardware {
    ramp: Vec<u16>,
    raw: i32,
    events: Vec<&'static str>,
    delayed: usize,
    persistent: bool,
    reject: bool,
    fail_metadata: bool,
    writes: usize,
}
impl Default for Hardware {
    fn default() -> Self {
        Self {
            ramp: tone::identity(),
            raw: 50,
            events: vec![],
            delayed: 0,
            persistent: false,
            reject: false,
            fail_metadata: false,
            writes: 0,
        }
    }
}
struct Fake(Arc<Mutex<Hardware>>);
impl Backend for Fake {
    fn capture(&mut self, m: &Monitor, _: bool) -> crate::Result<Snapshot> {
        let s = self.0.lock().unwrap();
        Ok(Snapshot {
            monitor: m.clone(),
            gamma_ramp: s.ramp.clone(),
            per_user_profiles: false,
            user_icc_profile: None,
            nvidia_vibrance_raw: Some(s.raw),
            applied_icc_profiles: vec![],
        })
    }
    fn metadata(&mut self, snapshot: &Snapshot, p: Option<&Profile>) -> crate::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.events.push("metadata");
        // A driver mutation may invalidate an earlier GDI write.
        s.ramp = tone::identity();
        s.raw = p
            .and_then(|p| p.color.digital_vibrance)
            .unwrap_or(snapshot.nvidia_vibrance_raw.unwrap());
        if p.is_none() {
            s.reject = false;
            s.fail_metadata = false;
        }
        if s.fail_metadata {
            Err("metadata failure".into())
        } else {
            Ok(())
        }
    }
    fn read_ramp(&mut self, _: &Monitor) -> crate::Result<Vec<u16>> {
        Ok(self.0.lock().unwrap().ramp.clone())
    }
    fn write_ramp(&mut self, _: &Monitor, ramp: &[u16]) -> crate::Result<()> {
        let mut s = self.0.lock().unwrap();
        s.events.push("lut");
        s.writes += 1;
        if !s.reject {
            s.ramp = ramp.to_vec();
        }
        Ok(())
    }
    fn settle(&mut self, _: Duration) {
        let mut s = self.0.lock().unwrap();
        if s.persistent || s.delayed > 0 {
            s.ramp = tone::identity();
            s.delayed = s.delayed.saturating_sub(1);
        }
    }
}
fn session() -> (Session, Arc<Mutex<Hardware>>, Arc<Mutex<StoreState>>) {
    let h = Arc::new(Mutex::new(Hardware::default()));
    let r = Arc::new(Mutex::new(StoreState::default()));
    (
        Session::new(Box::new(Fake(h.clone())), Box::new(Store(r.clone()))),
        h,
        r,
    )
}
fn p() -> Profile {
    Profile {
        color: Color {
            brightness: 20,
            gamma: 1.2,
            digital_vibrance: Some(64),
            ..Color::default()
        },
        ..Profile::default()
    }
}
#[test]
fn neutral_identity() {
    assert_eq!(
        tone::build(&Color::default(), &tone::identity()).unwrap(),
        tone::identity()
    );
}
#[test]
fn monotonic_supported_range() {
    for c in [
        Color {
            brightness: 125,
            contrast: 82,
            gamma: 2.8,
            shadows: 50,
            highlights: 50,
            ..Color::default()
        },
        Color {
            brightness: -125,
            contrast: -82,
            gamma: 0.4,
            ..Color::default()
        },
    ] {
        let r = tone::build(&c, &tone::identity()).unwrap();
        assert!(r.chunks(256).all(|v| v.windows(2).all(|p| p[1] >= p[0])));
    }
}
#[test]
fn shadow_and_highlight() {
    let c = Color {
        shadows: 30,
        highlights: 30,
        ..Color::default()
    };
    assert!(tone::map(0.1, &c) > 0.1);
    assert!(tone::map(1.0, &c) < 1.0);
}
#[test]
fn rgb_and_calibration() {
    let c = Color {
        red_gain: -100,
        ..Color::default()
    };
    let r = tone::build(&c, &tone::identity()).unwrap();
    assert!(r[255] < r[511]);
    let base: Vec<_> = tone::identity().iter().map(|x| x / 2).collect();
    assert_eq!(tone::build(&Color::default(), &base).unwrap(), base);
}
#[test]
fn validation_nan_and_bounds() {
    let mut c = Color {
        gamma: f64::NAN,
        ..Color::default()
    };
    assert!(c.validate().is_err());
    c.gamma = 1.0;
    c.digital_vibrance = Some(101);
    assert!(c.validate().is_err());
    assert!(tone::build(&Color::default(), &[]).is_err());
}
#[test]
fn quantization_tolerance() {
    let r = tone::identity();
    let mut a = r.clone();
    a[5] += 257;
    assert!(tone::matches(&r, &a));
    a[5] += 1;
    assert!(!tone::matches(&r, &a));
}
#[test]
fn a_b_a_is_complete_and_not_accumulated() {
    let (mut s, h, r) = session();
    let a = p();
    let mut b = p();
    b.color = Color {
        gamma: 0.85,
        brightness: -10,
        ..Color::default()
    };
    s.apply(&monitor(), &a).unwrap();
    let expected = h.lock().unwrap().ramp.clone();
    s.apply(&monitor(), &b).unwrap();
    assert_eq!(h.lock().unwrap().raw, 50);
    s.apply(&monitor(), &a).unwrap();
    assert_eq!(h.lock().unwrap().ramp, expected);
    assert_eq!(h.lock().unwrap().raw, 64);
    assert_eq!(
        r.lock().unwrap().saved.as_ref().unwrap().gamma_ramp,
        tone::identity()
    );
}
#[test]
fn metadata_precedes_final_lut() {
    let (mut s, h, _) = session();
    s.apply(&monitor(), &p()).unwrap();
    assert_eq!(h.lock().unwrap().events[..2], ["metadata", "lut"]);
}
#[test]
fn delayed_reset_reapplies_full_curve() {
    let (mut s, h, _) = session();
    h.lock().unwrap().delayed = 2;
    s.apply(&monitor(), &p()).unwrap();
    let state = h.lock().unwrap();
    assert_eq!(state.writes, 3);
    assert_eq!(
        state.ramp,
        tone::build(&p().color, &tone::identity()).unwrap()
    );
    assert!(s.active.is_some());
}
#[test]
fn persistent_reset_is_not_reported_active() {
    let (mut s, h, _) = session();
    h.lock().unwrap().persistent = true;
    assert!(s.apply(&monitor(), &p()).is_err());
    assert!(s.active.is_none());
}
#[test]
fn driver_silent_rejection_rolls_back() {
    let (mut s, h, r) = session();
    h.lock().unwrap().reject = true;
    assert!(s.apply(&monitor(), &p()).unwrap_err().contains("已恢复"));
    assert!(s.active.is_none());
    assert!(s.snapshot.is_none());
    assert!(r.lock().unwrap().saved.is_none());
}
#[test]
fn metadata_failure_rolls_back() {
    let (mut s, h, _) = session();
    h.lock().unwrap().fail_metadata = true;
    assert!(s.apply(&monitor(), &p()).is_err());
    assert!(s.snapshot.is_none());
    assert_eq!(h.lock().unwrap().raw, 50);
}
#[test]
fn restore_writes_after_nvidia_and_preserves_original() {
    let (mut s, h, _) = session();
    s.apply(&monitor(), &p()).unwrap();
    h.lock().unwrap().events.clear();
    s.restore().unwrap();
    let h = h.lock().unwrap();
    assert_eq!(h.events[..2], ["metadata", "lut"]);
    assert_eq!(h.ramp, tone::identity());
    assert_eq!(h.raw, 50);
}
#[test]
fn restore_failure_retains_backup_and_blocks_new_apply() {
    let (mut s, h, r) = session();
    s.apply(&monitor(), &p()).unwrap();
    r.lock().unwrap().fail_clear = true;
    assert!(s.restore().is_err());
    assert!(s.snapshot.is_some());
    assert!(r.lock().unwrap().saved.is_some());
    assert!(s.apply(&monitor(), &p()).is_err());
    drop(h);
}
#[test]
fn compare_preserves_snapshot() {
    let (mut s, h, r) = session();
    s.apply(&monitor(), &p()).unwrap();
    s.compare().unwrap();
    assert!(s.comparing);
    assert_eq!(h.lock().unwrap().ramp, tone::identity());
    assert!(r.lock().unwrap().saved.is_some());
    s.apply(&monitor(), &p()).unwrap();
    s.restore().unwrap();
}
#[test]
fn different_monitor_rejected() {
    let (mut s, _, _) = session();
    s.apply(&monitor(), &p()).unwrap();
    let mut m = monitor();
    m.monitor_id = "other".into();
    assert!(s.apply(&m, &p()).is_err());
}
#[test]
fn missing_nvidia_backup_rejected() {
    let (mut s, _, _) = session();
    s.apply(&monitor(), &p()).unwrap();
    s.snapshot.as_mut().unwrap().nvidia_vibrance_raw = None;
    assert!(s.apply(&monitor(), &p()).is_err());
}
#[test]
fn game_transitions_and_suppression() {
    let mut w = Watcher::default();
    assert_eq!(w.observe(true), Transition::Started);
    w.suppress();
    assert_eq!(w.observe(true), Transition::None);
    assert_eq!(w.observe(false), Transition::Exited);
    assert_eq!(w.observe(true), Transition::Started);
}
fn icc_header(tags: usize, size: usize) -> Vec<u8> {
    let mut b = vec![0; size];
    b[..4].copy_from_slice(&(size as u32).to_be_bytes());
    b[12..16].copy_from_slice(b"mntr");
    b[16..20].copy_from_slice(b"RGB ");
    b[36..40].copy_from_slice(b"acsp");
    b[128..132].copy_from_slice(&(tags as u32).to_be_bytes());
    b
}
fn icc_tag(data: &[u8]) -> Vec<u8> {
    let mut b = icc_header(1, 144 + data.len());
    b[132..136].copy_from_slice(b"vcgt");
    b[136..140].copy_from_slice(&144_u32.to_be_bytes());
    b[140..144].copy_from_slice(&(data.len() as u32).to_be_bytes());
    b[144..].copy_from_slice(data);
    b
}
#[test]
fn icc_without_vcgt() {
    assert!(icc::parse(&icc_header(0, 132)).unwrap().is_none());
}
#[test]
fn icc_table_8bit_single_channel() {
    let mut d = vec![0; 20];
    d[..4].copy_from_slice(b"vcgt");
    d[12..14].copy_from_slice(&1_u16.to_be_bytes());
    d[14..16].copy_from_slice(&2_u16.to_be_bytes());
    d[16..18].copy_from_slice(&1_u16.to_be_bytes());
    d[19] = 255;
    assert_eq!(icc::parse(&icc_tag(&d)).unwrap().unwrap(), tone::identity());
}
#[test]
fn icc_formula() {
    let mut d = vec![0; 48];
    d[..4].copy_from_slice(b"vcgt");
    d[8..12].copy_from_slice(&1_u32.to_be_bytes());
    for ch in 0..3 {
        d[12 + ch * 12..16 + ch * 12].copy_from_slice(&65536_u32.to_be_bytes());
        d[20 + ch * 12..24 + ch * 12].copy_from_slice(&65536_u32.to_be_bytes());
    }
    assert_eq!(icc::parse(&icc_tag(&d)).unwrap().unwrap(), tone::identity());
}
#[test]
fn icc_bounds_and_nonmonitor() {
    let mut b = icc_header(1, 144);
    b[136..140].copy_from_slice(&u32::MAX.to_be_bytes());
    assert!(icc::parse(&b).is_err());
    let mut b = icc_header(0, 132);
    b[12..16].copy_from_slice(b"prtr");
    assert!(icc::parse(&b).is_err());
    assert!(icc::parse(&[]).is_err());
}
#[test]
fn settings_and_old_json_compatibility() {
    let s = Settings::default();
    s.validate().unwrap();
    let json = serde_json::to_string(&s).unwrap();
    assert!(json.contains("SchemaVersion"));
    let parsed: Settings = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.selected(), s.selected());
    assert!(serde_json::from_str::<Settings>(r#"{"Unknown":1}"#).is_err());
    let mut bad = s.clone();
    bad.game_processes = vec!["C:\\game.exe".into()];
    assert!(bad.validate().is_err());
    bad = s;
    bad.profiles.push(bad.profiles[0].clone());
    assert!(bad.validate().is_err());
}
#[test]
fn atomic_storage_replaces_and_rejects_corruption() {
    let root = std::env::temp_dir().join(format!("df-core-{}", uuid::Uuid::new_v4()));
    let path: PathBuf = root.join("settings.json");
    let mut s = Settings::default();
    storage::write(&path, &s).unwrap();
    s.follow_game = true;
    storage::write(&path, &s).unwrap();
    assert!(storage::read::<Settings>(&path).unwrap().follow_game);
    std::fs::write(&path, b"corrupt").unwrap();
    assert!(storage::read::<Settings>(&path).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"corrupt");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn icc_table_16bit_and_nonmonotonic() {
    let mut data = vec![0; 30];
    data[..4].copy_from_slice(b"vcgt");
    data[12..14].copy_from_slice(&3_u16.to_be_bytes());
    data[14..16].copy_from_slice(&2_u16.to_be_bytes());
    data[16..18].copy_from_slice(&2_u16.to_be_bytes());
    for ch in 0..3 {
        data[20 + ch * 4..22 + ch * 4].copy_from_slice(&65535_u16.to_be_bytes());
    }
    assert_eq!(
        icc::parse(&icc_tag(&data)).unwrap().unwrap(),
        tone::identity()
    );
    data[18..20].copy_from_slice(&65535_u16.to_be_bytes());
    data[20..22].copy_from_slice(&0_u16.to_be_bytes());
    assert!(icc::parse(&icc_tag(&data)).is_err());
}
#[test]
fn icc_duplicate_and_formula_bounds() {
    let mut data = vec![0; 48];
    data[..4].copy_from_slice(b"vcgt");
    data[8..12].copy_from_slice(&1_u32.to_be_bytes());
    for ch in 0..3 {
        data[12 + ch * 12..16 + ch * 12].copy_from_slice(&65536_u32.to_be_bytes());
        data[20 + ch * 12..24 + ch * 12].copy_from_slice(&65536_u32.to_be_bytes());
    }
    let mut bytes = icc_header(2, 156 + data.len());
    for at in [132, 144] {
        bytes[at..at + 4].copy_from_slice(b"vcgt");
        bytes[at + 4..at + 8].copy_from_slice(&156_u32.to_be_bytes());
        bytes[at + 8..at + 12].copy_from_slice(&(data.len() as u32).to_be_bytes());
    }
    bytes[156..].copy_from_slice(&data);
    assert!(icc::parse(&bytes).is_err());
    data[12..16].copy_from_slice(&0_u32.to_be_bytes());
    assert!(icc::parse(&icc_tag(&data)).is_err());
}
#[test]
fn switching_icc_and_back_uses_original_calibration() {
    let dir = std::env::temp_dir().join(format!("df-icc-{}", Profile::default().id));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("display.icc");
    std::fs::write(&path, icc_header(0, 132)).unwrap();
    let (mut s, h, r) = session();
    let original: Vec<_> = tone::identity().iter().map(|v| v / 2).collect();
    h.lock().unwrap().ramp = original.clone();
    let a = p();
    s.apply(&monitor(), &a).unwrap();
    let expected = h.lock().unwrap().ramp.clone();
    let mut b = p();
    b.icc_path = Some(path.to_string_lossy().into());
    s.apply(&monitor(), &b).unwrap();
    assert_ne!(h.lock().unwrap().ramp, expected);
    assert_eq!(
        r.lock()
            .unwrap()
            .saved
            .as_ref()
            .unwrap()
            .applied_icc_profiles,
        vec!["display.icc"]
    );
    s.apply(&monitor(), &a).unwrap();
    assert_eq!(h.lock().unwrap().ramp, expected);
    s.restore().unwrap();
    assert_eq!(h.lock().unwrap().ramp, original);
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn persistent_reset_that_prevents_rollback_keeps_original_backup() {
    let (mut s, h, r) = session();
    h.lock().unwrap().ramp = tone::identity().iter().map(|v| v / 2).collect();
    h.lock().unwrap().persistent = true;
    assert!(s
        .apply(&monitor(), &p())
        .unwrap_err()
        .contains("恢复未完成"));
    assert!(s.snapshot.is_some());
    assert!(s.active.is_none());
    assert!(r.lock().unwrap().saved.is_some());
    assert!(s.apply(&monitor(), &p()).is_err());
}

#[test]
fn actual_v01_configuration_migrates_without_computed_getters() {
    // Generated by v0.1.0's actual Storage.Write, not by the Rust serializer.
    let mut settings: Settings =
        serde_json::from_str(include_str!("../tests/fixtures/settings-v0.1.0.json")).unwrap();
    settings.validate().unwrap();
    assert_eq!(settings.profiles.len(), 4);
    assert!(settings.follow_game);
    assert_eq!(settings.poll_seconds, 5);
    assert_eq!(settings.selected().color.gamma, 1.1);
    assert_eq!(settings.game_profile().color.highlights, 24);
    assert!(settings.legacy_selected_profile.is_some());
    assert!(settings.legacy_game_profile.is_some());
    settings.selected_profile_id = Some(settings.profiles[1].id.clone());
    assert_eq!(settings.selected().color.gamma, 1.18);
    let serialized = serde_json::to_value(&settings).unwrap();
    assert!(serialized.get("SelectedProfile").is_none());
    assert!(serialized.get("GameProfile").is_none());
    assert!(serialized.get("SelectedProfileId").is_some());
    let reloaded: Settings = serde_json::from_value(serialized).unwrap();
    reloaded.validate().unwrap();
    assert_eq!(reloaded.selected(), settings.selected());
}

#[test]
fn actual_v01_recovery_can_restore_and_clear() {
    let snapshot: Snapshot =
        serde_json::from_str(include_str!("../tests/fixtures/recovery-v0.1.0.json")).unwrap();
    snapshot.validate().unwrap();
    assert_eq!(snapshot.gamma_ramp, tone::identity());
    assert_eq!(snapshot.nvidia_vibrance_raw, Some(50));
    let (mut s, h, r) = session();
    r.lock().unwrap().saved = Some(snapshot.clone());
    h.lock().unwrap().ramp = tone::build(&p().color, &tone::identity()).unwrap();
    s.attach(snapshot).unwrap();
    s.restore().unwrap();
    assert_eq!(h.lock().unwrap().ramp, tone::identity());
    assert!(s.snapshot.is_none());
    assert!(r.lock().unwrap().saved.is_none());
}

#[test]
fn profile_process_bindings_preserve_legacy_json_and_validate_names() {
    let mut settings: Settings =
        serde_json::from_str(include_str!("../tests/fixtures/settings-v0.1.0.json")).unwrap();
    assert!(settings.profiles.iter().all(|p| p.process_names.is_empty()));
    settings.game_processes = vec!["legacy.exe".into(), "LEGACY.EXE".into()];
    settings.validate().unwrap();
    assert!(serde_json::to_value(&settings.profiles[0])
        .unwrap()
        .get("ProcessNames")
        .is_none());
    settings.game_processes.clear();
    settings.profiles[0].process_names = vec!["Game-A.exe".into(), "GameLauncher.exe".into()];
    settings.validate().unwrap();
    let json = serde_json::to_value(&settings).unwrap();
    assert_eq!(json["Profiles"][0]["ProcessNames"][0], "Game-A.exe");
    let reloaded: Settings = serde_json::from_value(json).unwrap();
    assert_eq!(reloaded.profiles, settings.profiles);
    for invalid in [
        vec!["C:\\game.exe".into()],
        vec!["game.exe".into(), "GAME.EXE".into()],
        vec!["game.exe\n".into()],
        vec!["game?.exe".into()],
        vec!["game".into()],
        (0..17).map(|i| format!("game{i}.exe")).collect(),
    ] {
        settings.profiles[0].process_names = invalid;
        assert!(settings.validate().is_err());
    }
}

#[test]
fn process_matching_uses_profile_order_before_legacy_fallback() {
    let mut settings = Settings::default();
    settings.game_processes = vec!["legacy.exe".into()];
    settings.game_profile_id = Some(settings.profiles[2].id.clone());
    settings.profiles[0].process_names = vec!["game-a.exe".into(), "shared.exe".into()];
    settings.profiles[1].process_names = vec!["game-b.exe".into(), "shared.exe".into()];
    let running = vec![
        "GAME-B.EXE".into(),
        "GAME-A.EXE".into(),
        "legacy.exe".into(),
    ];
    assert_eq!(
        settings.matching_profile(&running).unwrap().id,
        settings.profiles[0].id
    );
    settings.profiles.swap(0, 1);
    assert_eq!(
        settings.matching_profile(&running).unwrap().id,
        settings.profiles[0].id
    );
    assert_eq!(
        settings
            .matching_profile(&["SHARED.EXE".into()])
            .unwrap()
            .id,
        settings.profiles[0].id
    );
    assert_eq!(
        settings
            .matching_profile(&["LEGACY.EXE".into()])
            .unwrap()
            .id,
        settings.profiles[2].id
    );
    assert!(settings.matching_profile(&[]).is_none());
    assert!(settings.matching_profile(&["unbound.exe".into()]).is_none());
}

#[test]
fn profile_watcher_switches_targets_and_suppresses_until_all_games_exit() {
    let mut watcher = Watcher::default();
    assert_eq!(watcher.observe_profile(Some("a")), Transition::Started);
    assert_eq!(watcher.observe_profile(Some("a")), Transition::None);
    assert_eq!(watcher.observe_profile(Some("b")), Transition::Started);
    watcher.suppress();
    assert_eq!(watcher.observe_profile(Some("a")), Transition::None);
    assert_eq!(watcher.observe_profile(Some("b")), Transition::None);
    assert_eq!(watcher.observe_profile(None), Transition::Exited);
    assert_eq!(watcher.observe_profile(Some("b")), Transition::Started);
}

#[test]
fn live_updates_keep_one_backup_and_do_not_accumulate_color_or_reapply_metadata() {
    let (mut session, hardware, recovery) = session();
    let mut profile = p();
    session.apply_live(&monitor(), &profile).unwrap();
    for gamma in [1.3, 1.4, 1.1, 1.2] {
        profile.color.gamma = gamma;
        session.apply_live(&monitor(), &profile).unwrap();
        assert_eq!(
            hardware.lock().unwrap().ramp,
            tone::build(&profile.color, &tone::identity()).unwrap()
        );
    }
    assert_eq!(recovery.lock().unwrap().saves, 1);
    assert_eq!(
        hardware
            .lock()
            .unwrap()
            .events
            .iter()
            .filter(|event| **event == "metadata")
            .count(),
        1
    );
    profile.color.digital_vibrance = Some(75);
    session.apply_live(&monitor(), &profile).unwrap();
    assert_eq!(hardware.lock().unwrap().raw, 75);
    session.restore().unwrap();
    assert_eq!(hardware.lock().unwrap().ramp, tone::identity());
    assert_eq!(hardware.lock().unwrap().raw, 50);
}

#[test]
fn live_verification_repairs_late_resets_and_rolls_back_a_failed_final_check() {
    let (mut session, hardware, recovery) = session();
    let profile = p();
    session.apply_live(&monitor(), &profile).unwrap();
    hardware.lock().unwrap().ramp = tone::identity();
    session.verify_live(false).unwrap();
    assert_eq!(
        hardware.lock().unwrap().ramp,
        tone::build(&profile.color, &tone::identity()).unwrap()
    );
    hardware.lock().unwrap().ramp = tone::identity();
    assert!(session
        .verify_live(true)
        .unwrap_err()
        .contains("已恢复原始色彩"));
    assert!(session.active.is_none());
    assert!(recovery.lock().unwrap().saved.is_none());
    assert_eq!(hardware.lock().unwrap().ramp, tone::identity());
}

#[test]
fn v020_schemes_without_new_fields_keep_their_colors_ids_and_icc_paths() {
    let mut settings: Settings =
        serde_json::from_str(include_str!("../tests/fixtures/settings-v0.1.0.json")).unwrap();
    settings.profiles[0].icc_path = Some(
        std::env::temp_dir()
            .join("old-calibration.icc")
            .to_string_lossy()
            .into_owned(),
    );
    let mut old_json = serde_json::to_value(&settings).unwrap();
    old_json.as_object_mut().unwrap().remove("LiveAdjust");
    let restored: Settings = serde_json::from_value(old_json).unwrap();
    restored.validate().unwrap();
    assert_eq!(restored.profiles, settings.profiles);
    assert!(restored.live_adjust);
}
