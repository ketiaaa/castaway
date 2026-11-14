//! Tauri commands, shared app state, and the pure helpers behind them.

use crate::health::{self, HealthIssue};
use crate::settings::{self, Settings};
use crate::supervisor::{Status, Supervisor};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

/// Managed state: the supervisor (its status callback only emits events, so
/// there is no reference cycle) and the current settings.
pub struct AppState {
    pub supervisor: Supervisor,
    pub settings: Mutex<Settings>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Environment UxPlay needs to find its bundled GStreamer plugins.
pub fn launch_env(uxplay_dir: &Path, data_dir: &Path) -> Vec<(String, String)> {
    let plugins = uxplay_dir.join("gst-plugins").to_string_lossy().to_string();
    vec![
        ("GST_PLUGIN_PATH".to_string(), plugins.clone()),
        ("GST_PLUGIN_SYSTEM_PATH".to_string(), plugins),
        (
            "GST_REGISTRY".to_string(),
            data_dir.join("gst-registry.bin").to_string_lossy().to_string(),
        ),
        ("GST_REGISTRY_FORK".to_string(), "no".to_string()),
    ]
}

/// Compose the user-facing health messages from already-gathered inputs.
pub fn health_messages(
    uxplay_missing: bool,
    firewall_rule_missing: bool,
    port_issues: Vec<HealthIssue>,
    error_text: Option<&str>,
    base: Option<u16>,
) -> Vec<String> {
    let mut issues: Vec<HealthIssue> = Vec::new();
    let mut add = |i: HealthIssue| {
        if !issues.contains(&i) {
            issues.push(i);
        }
    };
    if uxplay_missing {
        add(HealthIssue::UxplayMissing);
    }
    if firewall_rule_missing {
        add(HealthIssue::FirewallRuleMissing);
    }
    port_issues.into_iter().for_each(&mut add);
    if let Some(i) = error_text.and_then(|t| health::classify_error_with_base(t, base)) {
        add(i);
    }
    issues.iter().map(health::issue_message).collect()
}

pub fn status_text(s: &Status) -> String {
    match s {
        Status::Stopped => "Stopped".to_string(),
        Status::Starting => "Starting…".to_string(),
        Status::Waiting => "Waiting for a device".to_string(),
        Status::Connected { device } => format!("Connected: {device}"),
        Status::Error(msg) => format!("Problem: {msg}"),
    }
}

pub fn tray_toggle_label(s: &Status) -> &'static str {
    match s {
        Status::Starting | Status::Waiting | Status::Connected { .. } => "Stop",
        Status::Stopped | Status::Error(_) => "Start",
    }
}

/// Start the server with the current settings (no-op if already running).
pub fn start(state: &AppState) {
    let args = settings::to_args(&lock(&state.settings));
    state.supervisor.start(args);
}

/// Where the child's GStreamer registry (and other app data) lives.
pub fn data_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    Ok(dir)
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    lock(&state.settings).clone()
}

#[tauri::command]
pub fn save_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    s: Settings,
) -> Result<(), String> {
    s.validate()?;
    settings::save(&settings::settings_path(&app), &s)
        .map_err(|e| format!("could not save settings: {e}"))?;
    let autolaunch = app.autolaunch();
    if s.autostart {
        autolaunch
            .enable()
            .map_err(|e| format!("could not enable autostart: {e}"))?;
    } else {
        autolaunch
            .disable()
            .map_err(|e| format!("could not disable autostart: {e}"))?;
    }
    let args = settings::to_args(&s);
    *lock(&state.settings) = s;
    if state.supervisor.status() != Status::Stopped {
        state.supervisor.restart(args);
    }
    Ok(())
}

#[tauri::command]
pub fn get_status(state: State<'_, AppState>) -> Status {
    state.supervisor.status()
}

#[tauri::command]
pub fn start_server(state: State<'_, AppState>) {
    start(&state);
}

#[tauri::command]
pub fn stop_server(state: State<'_, AppState>) {
    state.supervisor.stop();
}

/// Async so the blocking firewall/port probes run off the main thread.
#[tauri::command]
pub async fn get_health(
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Vec<String>, String> {
    let base = lock(&state.settings).port_base;
    let status = state.supervisor.status();
    let uxplay_missing = health::check_uxplay(&crate::paths::uxplay_path(&app)).is_some();
    let firewall_missing = !health::firewall_rule_exists(health::FIREWALL_RULE_NAME);
    // While UxPlay is running it owns the ports, so probing them would only
    // report our own listeners as "in use".
    let idle = matches!(status, Status::Stopped | Status::Error(_));
    let ports = match base {
        Some(b) if idle => health::check_ports(b),
        _ => Vec::new(),
    };
    let error_text = match &status {
        Status::Error(t) => Some(t.as_str()),
        _ => None,
    };
    Ok(health_messages(
        uxplay_missing,
        firewall_missing,
        ports,
        error_text,
        base,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_env_sets_gstreamer_vars() {
        let env = launch_env(Path::new("C:/app/resources/uxplay"), Path::new("C:/data"));
        let get = |k: &str| env.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
        let plugins = Path::new("C:/app/resources/uxplay").join("gst-plugins");
        let plugins = plugins.to_string_lossy().to_string();
        assert_eq!(get("GST_PLUGIN_PATH"), Some(plugins.clone()));
        assert_eq!(get("GST_PLUGIN_SYSTEM_PATH"), Some(plugins));
        assert_eq!(
            get("GST_REGISTRY"),
            Some(Path::new("C:/data").join("gst-registry.bin").to_string_lossy().to_string())
        );
        assert_eq!(get("GST_REGISTRY_FORK"), Some("no".to_string()));
        assert_eq!(env.len(), 4);
    }

    #[test]
    fn health_all_good_is_empty() {
        assert!(health_messages(false, false, vec![], None, Some(7000)).is_empty());
    }

    #[test]
    fn health_composes_and_maps_each_issue() {
        let m = health_messages(true, true, vec![HealthIssue::PortInUse(7001)], None, Some(7000));
        assert_eq!(
            m,
            vec![
                health::issue_message(&HealthIssue::UxplayMissing),
                health::issue_message(&HealthIssue::FirewallRuleMissing),
                health::issue_message(&HealthIssue::PortInUse(7001)),
            ]
        );
    }

    #[test]
    fn health_classifies_error_text_with_base_and_dedupes() {
        let err = "*** ERROR: Error initialising socket 10013";
        let m = health_messages(false, false, vec![HealthIssue::PortInUse(7001)], Some(err), Some(7000));
        assert_eq!(m, vec![health::issue_message(&HealthIssue::PortInUse(7001))]);
        let m = health_messages(false, false, vec![], Some(err), None);
        assert!(m.is_empty());
        let m = health_messages(false, false, vec![], Some("Required gstreamer plugin 'level' not found"), None);
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("level"));
    }

    #[test]
    fn status_text_covers_all_states() {
        assert_eq!(status_text(&Status::Stopped), "Stopped");
        assert_eq!(status_text(&Status::Starting), "Starting…");
        assert_eq!(status_text(&Status::Waiting), "Waiting for a device");
        assert_eq!(status_text(&Status::Connected { device: "iPhone".into() }), "Connected: iPhone");
        assert_eq!(status_text(&Status::Error("boom".into())), "Problem: boom");
    }

    #[test]
    fn toggle_label_per_state() {
        assert_eq!(tray_toggle_label(&Status::Stopped), "Start");
        assert_eq!(tray_toggle_label(&Status::Error("x".into())), "Start");
        assert_eq!(tray_toggle_label(&Status::Starting), "Stop");
        assert_eq!(tray_toggle_label(&Status::Waiting), "Stop");
        assert_eq!(tray_toggle_label(&Status::Connected { device: "d".into() }), "Stop");
    }
}
