pub mod cli;
mod app;
mod codes;
mod config;
mod desktop;
mod drop;
mod identity;
mod pairing;
mod protocol;
mod storage;
mod update;

use std::path::PathBuf;
use std::sync::Arc;

use app::{App, CodeInspect, PeerInfo, RoomInfo, StatusSnapshot, UiMessage};
use cli::{DemoKind, DemoRole, LaunchArgs};
use serde::Serialize;
use tauri::Manager;

fn default_data_dir() -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join("ratline")
}

fn map_err(e: anyhow::Error) -> String {
    let s = format!("{e:#}");
    let lower = s.to_lowercase();
    if lower.contains("ourself") {
        return "that's your own code - give it to someone else".into();
    }
    if lower.contains("being used")
        || lower.contains("access is denied")
        || lower.contains("os error 32")
        || lower.contains("os error 5")
    {
        return "file locked - close ratline and try /update again (or reinstall from github)".into();
    }
    s.lines().next().unwrap_or(&s).to_string()
}

#[cfg(windows)]
fn show_fatal(title: &str, msg: &str) {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "user32")]
    extern "system" {
        fn MessageBoxW(
            hwnd: *mut core::ffi::c_void,
            text: *const u16,
            caption: *const u16,
            flags: u32,
        ) -> i32;
    }
    fn wide(s: &str) -> Vec<u16> {
        OsStr::new(s).encode_wide().chain(Some(0)).collect()
    }
    let t = wide(title);
    let m = wide(msg);
    unsafe {
        MessageBoxW(std::ptr::null_mut(), m.as_ptr(), t.as_ptr(), 0x10);
    }
}

#[cfg(not(windows))]
fn show_fatal(title: &str, msg: &str) {
    eprintln!("{title}: {msg}");
}

fn write_crash_log(data_dir: &std::path::Path, msg: &str) {
    let _ = std::fs::create_dir_all(data_dir);
    let path = data_dir.join("crash.log");
    let stamp = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    let body = format!("[{stamp}] {msg}\n");
    let _ = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .and_then(|mut f| std::io::Write::write_all(&mut f, body.as_bytes()));
}

#[derive(Clone, Serialize)]
struct LaunchConfig {
    demo: Option<String>,
    role: String,
    demo_code: Option<String>,
    version: String,
}

#[tauri::command]
async fn get_status(state: tauri::State<'_, Arc<App>>) -> Result<StatusSnapshot, String> {
    state.status().map_err(map_err)
}

#[tauri::command]
async fn list_peers(state: tauri::State<'_, Arc<App>>) -> Result<Vec<PeerInfo>, String> {
    state.list_peers().map_err(map_err)
}

#[tauri::command]
async fn list_rooms(state: tauri::State<'_, Arc<App>>) -> Result<Vec<RoomInfo>, String> {
    state.list_rooms().map_err(map_err)
}

#[tauri::command]
async fn start_pairing(state: tauri::State<'_, Arc<App>>) -> Result<String, String> {
    state.start_pairing().await.map_err(map_err)
}

#[tauri::command]
async fn inspect_code(
    state: tauri::State<'_, Arc<App>>,
    code: String,
) -> Result<CodeInspect, String> {
    state.inspect_code(code).await.map_err(map_err)
}

#[tauri::command]
async fn join_code(
    state: tauri::State<'_, Arc<App>>,
    code: String,
) -> Result<serde_json::Value, String> {
    state.join_code(code).await.map_err(map_err)
}

#[tauri::command]
async fn start_room(state: tauri::State<'_, Arc<App>>) -> Result<(String, String), String> {
    state.start_room().await.map_err(map_err)
}

#[tauri::command]
async fn connect_named(
    state: tauri::State<'_, Arc<App>>,
    name: String,
) -> Result<PeerInfo, String> {
    state.connect_named(&name).await.map_err(map_err)
}

#[tauri::command]
async fn send_text(
    state: tauri::State<'_, Arc<App>>,
    conversation_id: String,
    body: String,
) -> Result<UiMessage, String> {
    state
        .send_text(&conversation_id, body)
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn send_typing(
    state: tauri::State<'_, Arc<App>>,
    conversation_id: String,
    active: bool,
) -> Result<(), String> {
    state
        .send_typing(&conversation_id, active)
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn send_file(
    state: tauri::State<'_, Arc<App>>,
    conversation_id: String,
    path: String,
) -> Result<UiMessage, String> {
    state
        .send_file(&conversation_id, PathBuf::from(path))
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn download_file(
    state: tauri::State<'_, Arc<App>>,
    from_endpoint: String,
    hash: String,
    dest: String,
) -> Result<(), String> {
    state
        .download_file(&from_endpoint, &hash, PathBuf::from(dest))
        .await
        .map_err(map_err)
}

#[tauri::command]
async fn get_history(
    state: tauri::State<'_, Arc<App>>,
    conversation_id: String,
) -> Result<Vec<UiMessage>, String> {
    state.history(&conversation_id).map_err(map_err)
}

#[tauri::command]
async fn wipe_history(
    state: tauri::State<'_, Arc<App>>,
    conversation_id: String,
) -> Result<usize, String> {
    state.wipe(&conversation_id).map_err(map_err)
}

#[tauri::command]
async fn burn_identity(state: tauri::State<'_, Arc<App>>, app: tauri::AppHandle) -> Result<(), String> {
    state.burn_all().map_err(map_err)?;
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn set_history(state: tauri::State<'_, Arc<App>>, enabled: bool) -> Result<(), String> {
    state.set_history(enabled).map_err(map_err)
}

#[tauri::command]
async fn set_sound(state: tauri::State<'_, Arc<App>>, enabled: bool) -> Result<(), String> {
    state.set_sound(enabled).map_err(map_err)
}

#[tauri::command]
async fn set_hotkey(
    state: tauri::State<'_, Arc<App>>,
    app: tauri::AppHandle,
    hotkey: String,
) -> Result<String, String> {
    let normalized = config::normalize_hotkey(&hotkey);
    let hk = state.set_hotkey(&normalized).map_err(map_err)?;
    desktop::reregister_hotkey(&app, &hk)?;
    Ok(hk)
}

#[tauri::command]
async fn fingerprint(
    state: tauri::State<'_, Arc<App>>,
    who: Option<String>,
) -> Result<String, String> {
    match who {
        Some(w) if !w.trim().is_empty() => state.fingerprint_peer(&w).map_err(map_err),
        _ => Ok(state.fingerprint_self()),
    }
}

#[tauri::command]
async fn set_verified(
    state: tauri::State<'_, Arc<App>>,
    endpoint_id: String,
    verified: bool,
) -> Result<(), String> {
    state.set_verified(&endpoint_id, verified).map_err(map_err)
}

#[tauri::command]
async fn peer_fingerprint(
    state: tauri::State<'_, Arc<App>>,
    endpoint_id: String,
) -> Result<String, String> {
    let mut id = [0u8; 32];
    let raw = hex::decode(endpoint_id.trim()).map_err(|e| e.to_string())?;
    if raw.len() != 32 {
        return Err("bad peer id".into());
    }
    id.copy_from_slice(&raw);
    Ok(crate::codes::fingerprint(&id))
}

#[tauri::command]
async fn seal_drop(
    state: tauri::State<'_, Arc<App>>,
    peer: String,
    kind: String,
    name: String,
    body: String,
    dest: String,
) -> Result<String, String> {
    let bytes = if kind == "file" {
        tokio::fs::read(&body).await.map_err(|e| e.to_string())?
    } else {
        body.into_bytes()
    };
    state
        .seal_drop(&peer, &kind, &name, &bytes, PathBuf::from(dest))
        .map_err(map_err)
}

#[tauri::command]
async fn open_drop(
    state: tauri::State<'_, Arc<App>>,
    path: String,
) -> Result<crate::drop::OpenedDrop, String> {
    state.open_drop(PathBuf::from(path)).map_err(map_err)
}

#[tauri::command]
async fn save_drop_file(
    state: tauri::State<'_, Arc<App>>,
    dest: String,
    bytes: Vec<u8>,
) -> Result<(), String> {
    state
        .save_drop_bytes(PathBuf::from(dest), &bytes)
        .map_err(map_err)
}

#[tauri::command]
async fn cancel_invite(state: tauri::State<'_, Arc<App>>) -> Result<(), String> {
    state.cancel_invite().map_err(map_err)
}

#[tauri::command]
async fn quit_app(app: tauri::AppHandle) -> Result<(), String> {
    app.exit(0);
    Ok(())
}

#[tauri::command]
async fn set_label(
    state: tauri::State<'_, Arc<App>>,
    endpoint_id: String,
    label: String,
) -> Result<(), String> {
    state.set_conv_label(&endpoint_id, &label).map_err(map_err)
}

#[tauri::command]
async fn leave_conversation(
    state: tauri::State<'_, Arc<App>>,
    conversation_id: String,
) -> Result<(), String> {
    state.leave(&conversation_id).map_err(map_err)
}

#[tauri::command]
async fn pick_file(app: tauri::AppHandle) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_file()
            .and_then(|p| {
                p.into_path()
                    .ok()
                    .map(|pb| pb.to_string_lossy().into_owned())
            })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn pick_save(app: tauri::AppHandle, default_name: String) -> Result<Option<String>, String> {
    use tauri_plugin_dialog::DialogExt;
    tokio::task::spawn_blocking(move || {
        app.dialog()
            .file()
            .set_file_name(&default_name)
            .blocking_save_file()
            .and_then(|p| {
                p.into_path()
                    .ok()
                    .map(|pb| pb.to_string_lossy().into_owned())
            })
    })
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
async fn hide_window(app: tauri::AppHandle) -> Result<(), String> {
    if let Some(w) = app.get_webview_window("main") {
        w.hide().map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[tauri::command]
async fn get_launch_config(state: tauri::State<'_, LaunchArgs>) -> Result<LaunchConfig, String> {
    Ok(LaunchConfig {
        demo: state.demo.map(|d| d.as_str().to_string()),
        role: state.demo_role.as_str().to_string(),
        demo_code: state.demo_code.clone(),
        version: env!("CARGO_PKG_VERSION").into(),
    })
}

#[tauri::command]
async fn check_update() -> Result<update::UpdateInfo, String> {
    update::check_update().await.map_err(map_err)
}

#[tauri::command]
async fn run_update(app: tauri::AppHandle) -> Result<(), String> {
    update::run_update(app).await.map_err(map_err)
}

#[tauri::command]
async fn spawn_demo_peer(
    launch: tauri::State<'_, LaunchArgs>,
    code: String,
) -> Result<(), String> {
    let demo = launch
        .demo
        .ok_or_else(|| "not in demo mode".to_string())?;
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let peer_dir = std::env::temp_dir().join(format!(
        "ratline-demo-{}-joiner",
        std::process::id()
    ));
    let _ = std::fs::create_dir_all(&peer_dir);
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("--demo")
        .arg(demo.as_str())
        .arg("--demo-role")
        .arg("joiner")
        .arg("--demo-code")
        .arg(&code)
        .arg("--data-dir")
        .arg(&peer_dir);
    cmd.spawn().map_err(|e| e.to_string())?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    run_with_args(cli::parse_args(std::env::args()));
}

pub fn run_with_args(args: LaunchArgs) {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "ratline=info,iroh=warn".into()),
        )
        .init();

    let data = args
        .data_dir
        .clone()
        .unwrap_or_else(default_data_dir);

    let data = if args.demo.is_some() && args.data_dir.is_none() {
        let d = std::env::temp_dir().join(format!("ratline-demo-{}-host", std::process::id()));
        let _ = std::fs::create_dir_all(&d);
        d
    } else {
        data
    };

    let window_title = match (args.demo, args.demo_role) {
        (Some(DemoKind::Conversation), DemoRole::Host) => "ratline · demo a",
        (Some(DemoKind::Conversation), DemoRole::Joiner) => "ratline · demo b",
        (Some(DemoKind::Commands), DemoRole::Host) => "ratline · demo host",
        (Some(DemoKind::Commands), DemoRole::Joiner) => "ratline · demo peer",
        _ => "ratline",
    };

    let boot_cfg = config::Config::load(&data);
    let hotkey = boot_cfg.hotkey.clone();
    let args_managed = args.clone();
    let data_for_setup = data.clone();

    let result = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_notification::init())
        // register hotkey in setup — never panic the process over a bad chord
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .manage(args_managed)
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title(window_title);
            }
            let handle = app.handle().clone();
            if let Err(e) = desktop::setup_tray_and_close(&handle) {
                tracing::warn!("tray setup: {e}");
            }

            let mut active_hotkey = hotkey.clone();
            if let Err(e) = desktop::reregister_hotkey(&handle, &active_hotkey) {
                tracing::warn!("hotkey '{active_hotkey}' failed: {e}");
                active_hotkey = config::default_hotkey_str().into();
                if let Err(e2) = desktop::reregister_hotkey(&handle, &active_hotkey) {
                    tracing::warn!("default hotkey also failed: {e2}");
                } else {
                    // persist the working default so next launch is clean
                    let mut cfg = config::Config::load(&data_for_setup);
                    cfg.hotkey = active_hotkey.clone();
                    let _ = cfg.save(&data_for_setup);
                }
            }

            let state = match tauri::async_runtime::block_on(App::bootstrap(data_for_setup.clone()))
            {
                Ok(s) => s,
                Err(e) => {
                    let msg = format!("{e:#}");
                    write_crash_log(&data_for_setup, &msg);
                    show_fatal(
                        "ratline failed to start",
                        &format!("{msg}\n\nDetails written to:\n{}", data_for_setup.join("crash.log").display()),
                    );
                    return Err(msg.into());
                }
            };
            // keep config hotkey in sync with what we registered
            {
                let mut cfg = state.config.lock();
                if cfg.hotkey != active_hotkey {
                    cfg.hotkey = active_hotkey;
                    let _ = cfg.save(&state.data_dir);
                }
            }
            state.set_app_handle(handle);
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_status,
            list_peers,
            list_rooms,
            start_pairing,
            inspect_code,
            join_code,
            start_room,
            connect_named,
            send_text,
            send_typing,
            send_file,
            download_file,
            get_history,
            wipe_history,
            burn_identity,
            set_history,
            set_sound,
            set_hotkey,
            fingerprint,
            set_verified,
            peer_fingerprint,
            seal_drop,
            open_drop,
            save_drop_file,
            cancel_invite,
            quit_app,
            set_label,
            leave_conversation,
            pick_file,
            pick_save,
            hide_window,
            get_launch_config,
            spawn_demo_peer,
            check_update,
            run_update,
        ])
        .run(tauri::generate_context!());

    if let Err(e) = result {
        let msg = format!("{e}");
        write_crash_log(&data, &msg);
        show_fatal("ratline failed to start", &msg);
    }
}
