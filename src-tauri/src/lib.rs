pub mod cli;
mod app;
mod codes;
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
    if s.to_lowercase().contains("ourself") {
        return "that's your own code - give it to someone else".into();
    }
    // keep first clause only
    s.lines().next().unwrap_or(&s).to_string()
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
async fn join_pairing(
    state: tauri::State<'_, Arc<App>>,
    code: String,
) -> Result<PeerInfo, String> {
    state.join_pairing(code).await.map_err(map_err)
}

#[tauri::command]
async fn start_room(state: tauri::State<'_, Arc<App>>) -> Result<(String, String), String> {
    state.start_room().await.map_err(map_err)
}

#[tauri::command]
async fn join_room(state: tauri::State<'_, Arc<App>>, code: String) -> Result<RoomInfo, String> {
    state.join_room_code(code).await.map_err(map_err)
}

#[tauri::command]
async fn connect_peer(
    state: tauri::State<'_, Arc<App>>,
    endpoint_id: String,
) -> Result<(), String> {
    state.connect_peer(&endpoint_id).await.map_err(map_err)
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
async fn set_history(
    state: tauri::State<'_, Arc<App>>,
    enabled: bool,
) -> Result<(), String> {
    state.set_history(enabled).map_err(map_err)
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
        app.dialog().file().blocking_pick_file().and_then(|p| {
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

/// Host-only: spawn a second process that joins with the given code.
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

    // Demo host gets an isolated data dir so it never touches the user's identity.
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

    let args_managed = args.clone();
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(args_managed)
        .setup(move |app| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.set_title(window_title);
            }
            let handle = app.handle().clone();
            let state = tauri::async_runtime::block_on(App::bootstrap(data))
                .expect("bootstrap ratline");
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
            join_pairing,
            start_room,
            join_room,
            connect_peer,
            send_text,
            send_typing,
            send_file,
            download_file,
            get_history,
            wipe_history,
            set_history,
            set_label,
            leave_conversation,
            pick_file,
            pick_save,
            get_launch_config,
            spawn_demo_peer,
            check_update,
            run_update,
        ])
        .run(tauri::generate_context!())
        .expect("error while running ratline");
}
