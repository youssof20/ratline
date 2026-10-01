//! Frameless drop-down summon, tray presence, global hotkey.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, PhysicalSize, WebviewWindow,
};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

static SLIDING: AtomicBool = AtomicBool::new(false);

pub fn setup_tray_and_close(app: &AppHandle) -> tauri::Result<()> {
    setup_tray(app)?;
    if let Some(win) = app.get_webview_window("main") {
        let w = win.clone();
        win.on_window_event(move |event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = w.hide();
            }
        });
    }
    Ok(())
}

fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show_i = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show_i, &quit_i])?;

    let mut builder = TrayIconBuilder::new()
        .menu(&menu)
        .tooltip("ratline")
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => {
                let _ = summon(app);
            }
            "quit" => {
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                let _ = summon(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    builder.build(app)?;
    Ok(())
}

pub fn reregister_hotkey(app: &AppHandle, hotkey: &str) -> Result<(), String> {
    let _ = app.global_shortcut().unregister_all();
    let shortcut: Shortcut = hotkey
        .parse()
        .map_err(|e| format!("bad hotkey '{hotkey}': {e}"))?;
    app.global_shortcut()
        .on_shortcut(shortcut, |app, _s, event| {
            if event.state == ShortcutState::Pressed {
                let _ = toggle_summon(app);
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn toggle_summon(app: &AppHandle) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    if win.is_visible().unwrap_or(false) && win.is_focused().unwrap_or(false) {
        let _ = win.hide();
        return Ok(());
    }
    summon(app)
}

pub fn summon(app: &AppHandle) -> tauri::Result<()> {
    let Some(win) = app.get_webview_window("main") else {
        return Ok(());
    };
    if SLIDING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    let win2 = win.clone();
    tauri::async_runtime::spawn(async move {
        let _ = slide_in(&win2).await;
        SLIDING.store(false, Ordering::SeqCst);
    });
    Ok(())
}

async fn slide_in(win: &WebviewWindow) -> tauri::Result<()> {
    let monitor = win
        .current_monitor()?
        .or(win.primary_monitor()?)
        .ok_or_else(|| tauri::Error::AssetNotFound("no monitor".into()))?;
    let screen = monitor.size();
    let scale = monitor.scale_factor();
    let pos = monitor.position();

    let logical_w = 980.0_f64;
    let logical_h = 660.0_f64;
    let phys_w = (logical_w * scale) as u32;
    let phys_h = (logical_h * scale) as u32;
    let _ = win.set_size(tauri::Size::Physical(PhysicalSize::new(phys_w, phys_h)));

    let x = pos.x + ((screen.width as i32) - phys_w as i32) / 2;
    let target_y = pos.y + (12.0 * scale) as i32;
    let start_y = pos.y - phys_h as i32;

    let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, start_y)));
    let _ = win.show();
    let _ = win.set_focus();
    let _ = win.set_always_on_top(true);

    const STEPS: u32 = 14;
    for i in 1..=STEPS {
        let t = i as f64 / STEPS as f64;
        let eased = 1.0 - (1.0 - t).powi(3);
        let y = start_y + ((target_y - start_y) as f64 * eased) as i32;
        let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, y)));
        tokio::time::sleep(Duration::from_millis(14)).await;
    }
    let _ = win.set_position(tauri::Position::Physical(PhysicalPosition::new(x, target_y)));
    tokio::time::sleep(Duration::from_millis(400)).await;
    let _ = win.set_always_on_top(false);
    Ok(())
}

pub fn window_focused(app: &AppHandle) -> bool {
    app.get_webview_window("main")
        .and_then(|w| w.is_focused().ok())
        .unwrap_or(true)
}
