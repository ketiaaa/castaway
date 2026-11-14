//! System tray: status line, Start/Stop, Settings…, Quit.

use crate::commands::{AppState, start, status_text, tray_toggle_label};
use crate::supervisor::Status;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager};

const TRAY_ID: &str = "main";
const WINDOW: &str = "main";

/// Menu items kept in managed state so status changes can update them.
pub struct TrayItems {
    status: MenuItem<tauri::Wry>,
    toggle: MenuItem<tauri::Wry>,
}

pub fn show_main_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window(WINDOW) {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

/// Stop the server, then exit. Leaves no uxplay.exe behind.
pub fn quit(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        state.supervisor.stop();
    }
    app.exit(0);
}

pub fn build(app: &AppHandle, initial: &Status) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", status_text(initial), false, None::<&str>)?;
    let toggle = MenuItem::with_id(app, "toggle", tray_toggle_label(initial), true, None::<&str>)?;
    let settings = MenuItem::with_id(app, "settings", "Settings…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &sep, &toggle, &settings, &quit_item])?;
    app.manage(TrayItems { status, toggle });

    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .tooltip(status_text(initial))
        .on_menu_event(|app, event| match event.id().as_ref() {
            "toggle" => {
                if let Some(state) = app.try_state::<AppState>() {
                    if tray_toggle_label(&state.supervisor.status()) == "Stop" {
                        state.supervisor.stop();
                    } else {
                        start(&state);
                    }
                }
            }
            "settings" => show_main_window(app),
            "quit" => quit(app),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Refresh the status line, toggle label and tooltip.
pub fn update(app: &AppHandle, s: &Status) {
    let text = status_text(s);
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.status.set_text(&text);
        let _ = items.toggle.set_text(tray_toggle_label(s));
    }
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(text));
    }
}
