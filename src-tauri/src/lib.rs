pub mod commands;
pub mod health;
pub mod log_parser;
pub mod paths;
pub mod settings;
pub mod supervisor;
pub mod tray;

use commands::AppState;
use std::sync::{Arc, Mutex};
use supervisor::{Supervisor, SupervisorConfig};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            tray::show_main_window(app);
        }))
        // Started by Windows at login: tray only, no settings window.
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--hidden"]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::save_settings,
            commands::get_status,
            commands::start_server,
            commands::stop_server,
            commands::restart_server,
            commands::describe_error,
            commands::get_health,
        ])
        .on_window_event(|window, event| {
            // Closing the settings window only hides it; Quit is the way out.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .setup(|app| {
            let handle = app.handle().clone();
            let loaded = settings::load(&settings::settings_path(&handle));
            let uxplay = paths::uxplay_path(&handle);
            let uxplay_dir = uxplay.parent().map(|p| p.to_path_buf()).unwrap_or_default();
            let data_dir = commands::data_dir(&handle)?;
            let cfg = SupervisorConfig {
                env: commands::launch_env(&uxplay_dir, &data_dir),
                cwd: Some(uxplay_dir),
                ..SupervisorConfig::default()
            };

            tray::build(&handle, &supervisor::Status::Stopped)?;

            // The callback only emits; it must not hold the Supervisor.
            let cb_handle = handle.clone();
            let supervisor = Supervisor::new(
                uxplay,
                cfg,
                Arc::new(move |s| {
                    let _ = cb_handle.emit("status-changed", &s);
                    tray::update(&cb_handle, &s);
                }),
            );
            app.manage(AppState {
                supervisor,
                settings: Mutex::new(loaded),
                probes: Mutex::new(None),
            });
            if let Some(state) = app.try_state::<AppState>() {
                commands::start(&state);
            }
            // The window is created hidden (tauri.conf.json); a normal launch shows it.
            let args: Vec<String> = std::env::args().collect();
            if !commands::should_start_hidden(&args) {
                tray::show_main_window(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tauri application");

    app.run(|handle, event| {
        // Any exit path stops UxPlay first.
        if let tauri::RunEvent::Exit = event {
            if let Some(state) = handle.try_state::<AppState>() {
                state.supervisor.stop();
            }
        }
    });
}
