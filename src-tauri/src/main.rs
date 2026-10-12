#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
mod controller;
use base64::Engine;
use controller::{Controller, Shared, View};
use filter_core::{
    icc,
    models::{Profile, Settings},
    storage, Result,
};
use serde::Deserialize;
use std::{
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Emitter, Manager, WebviewUrl, WebviewWindowBuilder,
};
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

#[derive(Deserialize)]
#[serde(tag = "type", deny_unknown_fields)]
enum Request {
    Apply { profile: Profile },
    Restore,
    Compare,
    Select { id: String },
    SaveProfile { profile: Profile },
    Delete { id: String },
    Settings { settings: Settings },
    Toggle,
    Refresh,
}
fn lock(shared: &Shared) -> Result<std::sync::MutexGuard<'_, Controller>> {
    shared
        .inner
        .lock()
        .map_err(|_| "应用状态锁异常，请重启并恢复备份".into())
}
fn announce(app: &tauri::AppHandle, shared: &Shared) {
    if let Ok(c) = lock(shared) {
        if let Err(e) = app.emit("state", c.view()) {
            eprintln!("状态通知失败：{e}");
        }
    }
}
#[tauri::command]
fn bootstrap(shared: tauri::State<'_, Arc<Shared>>) -> Result<View> {
    Ok(lock(&shared)?.view())
}
#[tauri::command]
async fn mutate(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Arc<Shared>>,
    request: Request,
) -> Result<View> {
    let shared = shared.inner().clone();
    app.emit("operation-start", ()).map_err(|e| e.to_string())?;
    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut c = lock(&shared)?;
        c.error = None;
        let result = match request {
            Request::Apply { profile } => c.apply(profile),
            Request::Restore => c.restore(true),
            Request::Compare => c.session.compare(),
            Request::Select { id } => c.select(&id),
            Request::SaveProfile { profile } => c.save_profile(profile),
            Request::Delete { id } => c.delete(&id),
            Request::Settings { settings } => c.save_settings(settings),
            Request::Toggle => c.toggle(),
            Request::Refresh => c.refresh(),
        };
        if let Err(e) = result {
            c.error = Some(e);
        }
        let view = c.view();
        drop(c);
        shared.wake.notify_one();
        Ok::<_, String>(view)
    })
    .await
    .map_err(|e| e.to_string())?;
    app.emit("operation-end", ()).map_err(|e| e.to_string())?;
    result
}
#[tauri::command]
async fn preview_curve(
    shared: tauri::State<'_, Arc<Shared>>,
    profile: Profile,
) -> Result<Vec<u16>> {
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(move || lock(&shared)?.session.preview(&profile))
        .await
        .map_err(|e| e.to_string())?
}
#[tauri::command]
fn remember_draft(shared: tauri::State<'_, Arc<Shared>>, profile: Profile) -> Result<()> {
    profile.validate()?;
    lock(&shared)?.draft = profile;
    Ok(())
}
fn image_data(path: &Path) -> Result<String> {
    let bytes = storage::read_bytes(path, 25 * 1024 * 1024)?;
    let mime = filter_core::image::inspect(&bytes)?;
    Ok(format!(
        "data:{mime};base64,{}",
        base64::engine::general_purpose::STANDARD.encode(bytes)
    ))
}
#[tauri::command]
async fn reference_image(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Arc<Shared>>,
    pick: bool,
    path: Option<String>,
) -> Result<Option<String>> {
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let selected = if pick {
            app.dialog()
                .file()
                .add_filter("参考图片", &["png", "jpg", "jpeg", "bmp"])
                .blocking_pick_file()
                .map(|p| p.into_path())
                .transpose()
                .map_err(|e| e.to_string())?
        } else if let Some(p) = path {
            Some(PathBuf::from(p))
        } else {
            lock(&shared)?.reference.clone()
        };
        let Some(path) = selected else {
            return Ok(None);
        };
        let data = image_data(&path)?;
        lock(&shared)?.reference = Some(path);
        Ok(Some(data))
    })
    .await
    .map_err(|e| e.to_string())?
}
#[tauri::command]
async fn file_action(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Arc<Shared>>,
    kind: String,
    profile: Option<Profile>,
) -> Result<Option<Profile>> {
    let shared = shared.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let (filter, extensions) = if kind.contains("icc") {
            ("ICC 配置", vec!["icc", "icm"])
        } else {
            ("色彩方案", vec!["json"])
        };
        let exporting = kind.starts_with("export");
        let dialog = app.dialog().file().add_filter(filter, &extensions);
        let selected = if exporting {
            dialog.blocking_save_file()
        } else {
            dialog.blocking_pick_file()
        };
        let Some(path) = selected else {
            return Ok(None);
        };
        let path = path.into_path().map_err(|e| e.to_string())?;
        match kind.as_str() {
            "import-profile" => {
                let mut p: Profile = storage::read(&path)?;
                p.validate()?;
                p.id = uuid_id();
                p.icc_path = None;
                lock(&shared)?.save_profile(p.clone())?;
                Ok(Some(p))
            }
            "export-profile" => {
                let p = profile.ok_or("缺少方案")?;
                p.validate()?;
                storage::write(&path, &p)?;
                Ok(None)
            }
            "import-icc" => {
                icc::read(&path)?;
                let root = lock(&shared)?.root.join("icc");
                std::fs::create_dir_all(&root).map_err(|e| e.to_string())?;
                let dest = root.join(path.file_name().ok_or("ICC 文件名无效")?);
                let bytes = storage::read_bytes(&path, icc::MAX_BYTES)?;
                if dest.exists() {
                    if storage::read_bytes(&dest, icc::MAX_BYTES)? != bytes {
                        return Err("已有同名但内容不同的 ICC，请重命名后导入".into());
                    }
                } else {
                    use std::io::Write;
                    let mut f = std::fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(&dest)
                        .map_err(|e| e.to_string())?;
                    f.write_all(&bytes)
                        .and_then(|_| f.sync_all())
                        .map_err(|e| e.to_string())?;
                }
                let mut c = lock(&shared)?;
                c.refresh()?;
                c.draft.icc_path = Some(dest.to_string_lossy().into());
                Ok(Some(c.draft.clone()))
            }
            "export-icc" => {
                let p = profile.ok_or("缺少方案")?;
                let source = p.icc_path.ok_or("请先选择 ICC")?;
                icc::read(Path::new(&source))?;
                std::fs::write(
                    path,
                    storage::read_bytes(Path::new(&source), icc::MAX_BYTES)?,
                )
                .map_err(|e| e.to_string())?;
                Ok(None)
            }
            _ => Err("未知文件操作".into()),
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
fn uuid_id() -> String {
    Profile::default().id
}
#[tauri::command]
async fn running_processes(shared: tauri::State<'_, Arc<Shared>>) -> Result<Vec<String>> {
    let smoke = lock(&shared)?.smoke;
    tauri::async_runtime::spawn_blocking(move || {
        if smoke {
            Ok(vec!["DeltaForceClient.exe".into()])
        } else {
            filter_core::platform::processes()
        }
    })
    .await
    .map_err(|e| e.to_string())?
}
fn open(app: &tauri::AppHandle) -> Result<()> {
    if let Some(window) = app.get_webview_window("main") {
        window.show().map_err(|e| e.to_string())?;
        window.set_focus().map_err(|e| e.to_string())?;
        return Ok(());
    }
    let shared = app
        .try_state::<Arc<Shared>>()
        .ok_or("应用正在初始化，请稍后打开")?;
    let smoke = lock(&shared)?.smoke;
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title(if smoke {
            "DeltaForce Filter Manager · CI simulation"
        } else {
            "DeltaForce Filter Manager"
        })
        .inner_size(1220.0, 800.0)
        .min_inner_size(860.0, 620.0)
        .theme(Some(tauri::Theme::Dark))
        .build()
        .map_err(|e| format!("打开界面失败，请确认 WebView2 已安装：{e}"))?;
    Ok(())
}
#[tauri::command]
fn close_window(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Arc<Shared>>,
    profile: Profile,
) -> Result<()> {
    profile.validate()?;
    let mut c = lock(&shared)?;
    c.draft = profile;
    let tray = c.settings.close_to_tray;
    drop(c);
    if tray {
        if let Some(w) = app.get_webview_window("main") {
            w.destroy().map_err(|e| e.to_string())?;
        }
    } else {
        quit_async(app);
    }
    Ok(())
}
fn operation_async(app: tauri::AppHandle, restore: bool) {
    tauri::async_runtime::spawn(async move {
        let shared = app.state::<Arc<Shared>>().inner().clone();
        if let Err(e) = app.emit("operation-start", ()) {
            eprintln!("通知失败：{e}");
        }
        let copy = shared.clone();
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut c = lock(&copy)?;
            let result = if restore { c.restore(true) } else { c.toggle() };
            c.error = result.err();
            Ok::<_, String>(())
        })
        .await;
        if let Err(e) = result {
            eprintln!("快捷操作失败：{e}");
        }
        announce(&app, &shared);
        if let Err(e) = app.emit("operation-end", ()) {
            eprintln!("通知失败：{e}");
        }
    });
}
fn quit_async(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let shared = app.state::<Arc<Shared>>().inner().clone();
        let copy = shared.clone();
        if let Err(e) = app.emit("operation-start", ()) {
            eprintln!("通知失败：{e}");
        }
        let result = tauri::async_runtime::spawn_blocking(move || {
            let mut c = lock(&copy)?;
            if c.settings.restore_on_exit {
                if let Err(e) = c.restore(false) {
                    c.error = Some(format!("退出前恢复失败：{e}；备份保留，请恢复后退出"));
                    return Err(e);
                }
            }
            Ok::<_, String>(())
        })
        .await;
        match result {
            Ok(Ok(())) => app.exit(0),
            _ => {
                announce(&app, &shared);
                if let Err(e) = open(&app) {
                    eprintln!("显示恢复错误失败：{e}");
                }
                if let Err(e) = app.emit("operation-end", ()) {
                    eprintln!("通知失败：{e}");
                }
            }
        }
    });
}
#[tauri::command]
fn quit(app: tauri::AppHandle) {
    quit_async(app);
}
#[tauri::command]
fn smoke_complete(
    app: tauri::AppHandle,
    shared: tauri::State<'_, Arc<Shared>>,
    report: serde_json::Value,
) -> Result<()> {
    let c = lock(&shared)?;
    let path = c.smoke_report.clone().ok_or("仅 CI 模拟模式可调用")?;
    storage::write(&path, &report)?;
    drop(c);
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(20));
        if let Some(w) = app.get_webview_window("main") {
            if let Err(e) = w.destroy() {
                eprintln!("CI 关闭界面失败：{e}");
            }
        }
        std::thread::sleep(Duration::from_secs(25));
        app.exit(0);
    });
    Ok(())
}
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let smoke = args
        .iter()
        .position(|a| a == "--smoke-test")
        .and_then(|i| args.get(i + 1))
        .map(PathBuf::from);
    let root = if smoke.is_some() {
        std::env::temp_dir().join(format!("df-smoke-{}", uuid_id()))
    } else {
        #[cfg(windows)]
        {
            match std::env::var_os("LOCALAPPDATA") {
                Some(p) => PathBuf::from(p).join("DeltaForceFilterManager"),
                None => {
                    eprintln!("缺少 LOCALAPPDATA，停止启动以免丢失备份");
                    return;
                }
            }
        }
        #[cfg(not(windows))]
        {
            std::env::temp_dir().join("DeltaForceFilterManager-preview")
        }
    };
    let tray_start = args.iter().any(|a| a == "--tray");
    let builder = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            if let Err(e) = open(app) {
                eprintln!("打开窗口失败：{e}");
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    let restore = shortcut.to_string().ends_with("F11");
                    operation_async(app.clone(), restore);
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            mutate,
            preview_curve,
            remember_draft,
            reference_image,
            file_action,
            running_processes,
            close_window,
            quit,
            smoke_complete
        ])
        .on_window_event(|window, event| match event {
            tauri::WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if let Err(e) = window.emit("request-close", ()) {
                    eprintln!("关闭请求失败：{e}");
                }
            }
            tauri::WindowEvent::Resized(_) => match window.is_minimized() {
                Ok(true) => {
                    if let Some(shared) = window.app_handle().try_state::<Arc<Shared>>() {
                        match lock(&shared) {
                            Ok(c) if c.settings.close_to_tray => {
                                drop(c);
                                if let Err(e) = window.emit("request-close", ()) {
                                    eprintln!("最小化请求失败：{e}");
                                }
                            }
                            Ok(_) => (),
                            Err(e) => eprintln!("最小化状态读取失败：{e}"),
                        }
                    }
                }
                Ok(false) => (),
                Err(e) => eprintln!("读取窗口状态失败：{e}"),
            },
            _ => (),
        })
        .setup(move |app| {
            let mut controller = Controller::new(root, smoke)?;
            if args.iter().any(|a| a == "--restore") {
                match controller.restore(false) {
                    Ok(()) => app.handle().exit(0),
                    Err(e) => {
                        eprintln!("恢复失败：{e}");
                        app.handle().exit(1)
                    }
                }
                return Ok(());
            }
            let shared = Arc::new(Shared {
                inner: std::sync::Mutex::new(controller),
                wake: std::sync::Condvar::new(),
            });
            app.manage(shared.clone());
            let open_item = MenuItem::with_id(app, "open", "打开调色工作台", true, None::<&str>)?;
            let toggle = MenuItem::with_id(app, "toggle", "开启 / 关闭滤镜", true, None::<&str>)?;
            let restore = MenuItem::with_id(app, "restore", "恢复原始色彩", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open_item, &toggle, &restore, &quit])?;
            let icon = app.default_window_icon().ok_or("应用图标缺失")?.clone();
            TrayIconBuilder::new()
                .icon(icon)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "open" => {
                        if let Err(e) = open(app) {
                            eprintln!("打开失败：{e}");
                        }
                    }
                    "toggle" => operation_async(app.clone(), false),
                    "restore" => operation_async(app.clone(), true),
                    "quit" => quit_async(app.clone()),
                    _ => (),
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::DoubleClick {
                            button: MouseButton::Left,
                            ..
                        } | TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        if let Err(e) = open(tray.app_handle()) {
                            eprintln!("打开失败：{e}");
                        }
                    }
                })
                .build(app)?;
            for shortcut in ["Ctrl+Shift+F10", "Ctrl+Shift+F11"] {
                if let Err(e) = app.global_shortcut().register(shortcut) {
                    lock(&shared)?.error = Some(format!("快捷键 {shortcut} 注册失败：{e}"));
                }
            }
            let handle = app.handle().clone();
            let worker = shared.clone();
            std::thread::spawn(move || loop {
                let mut c = match lock(&worker) {
                    Ok(c) => c,
                    Err(e) => {
                        eprintln!("进程检测停止：{e}");
                        break;
                    }
                };
                if !c.settings.follow_game {
                    match worker.wake.wait(c) {
                        Ok(_) => continue,
                        Err(e) => {
                            eprintln!("进程等待失败：{e}");
                            break;
                        }
                    }
                }
                let seconds = c.settings.poll_seconds;
                let changed = match c.poll() {
                    Ok(v) => v,
                    Err(e) => {
                        let changed = c.error.as_ref() != Some(&e);
                        c.error = Some(e);
                        changed
                    }
                };
                if changed {
                    if let Err(e) = handle.emit("state", c.view()) {
                        eprintln!("进程状态通知失败：{e}");
                    }
                }
                if let Err(e) = worker.wake.wait_timeout(c, Duration::from_secs(seconds)) {
                    eprintln!("进程等待失败：{e}");
                    break;
                }
            });
            if !tray_start {
                open(app.handle())?;
            }
            Ok(())
        });
    match builder.build(tauri::generate_context!()) {
        Ok(app) => app.run(|_, event| {
            if let tauri::RunEvent::ExitRequested {
                api, code: None, ..
            } = event
            {
                api.prevent_exit();
            }
        }),
        Err(e) => {
            filter_core::platform::error_dialog(&format!(
                "应用启动失败：{e}。请确认 Microsoft Edge WebView2 已安装。"
            ));
            std::process::exit(1)
        }
    }
}
