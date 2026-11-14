//! Tauri commands, shared app state, and the pure helpers behind them.

use crate::health::{self, HealthIssue};
use crate::settings::{self, Settings};
use crate::supervisor::{Status, Supervisor};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Manager, State};
use tauri_plugin_autostart::ManagerExt;

/// Results of the process-spawning health probes (firewall rule, network
/// profile), cached so a crash loop's status events don't spawn dozens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeResult {
    pub firewall_missing: bool,
    pub public_network: bool,
}

pub const PROBE_TTL: Duration = Duration::from_secs(15);

/// True while a probe result taken at `cached_at` may still be used.
pub fn fresh(now: Instant, cached_at: Instant, ttl: Duration) -> bool {
    now.saturating_duration_since(cached_at) < ttl
}

/// Managed state: the supervisor (its status callback only emits events, so
/// there is no reference cycle), the current settings and the probe cache.
pub struct AppState {
    pub supervisor: Supervisor,
    pub settings: Mutex<Settings>,
    pub probes: Mutex<Option<(Instant, ProbeResult)>>,
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// True when launched with `--hidden` (the autostart entry): no window.
pub fn should_start_hidden(args: &[String]) -> bool {
    args.iter().skip(1).any(|a| a == "--hidden")
}

/// Environment UxPlay needs to find its bundled GStreamer plugins. HOME points
/// at our data dir so UxPlay's `-reg` file and `~/.uxplayrc` lookup stay there
/// (HOME is normally unset on Windows, or set by unrelated tools).
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
        ("HOME".to_string(), data_dir.to_string_lossy().to_string()),
    ]
}

/// Compose the user-facing health messages from already-gathered inputs.
pub fn health_messages(
    uxplay_missing: bool,
    firewall_rule_missing: bool,
    public_network: bool,
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
    if public_network {
        add(HealthIssue::PublicNetwork);
    }
    port_issues.into_iter().for_each(&mut add);
    if let Some(i) = error_text.and_then(|t| health::classify_error_with_base(t, base)) {
        add(i);
    }
    issues.iter().map(health::issue_message).collect()
}

/// Status line for the tray and its tooltip. An error is always shown as a
/// plain sentence, never as raw log text. `base` is the configured port.
pub fn status_text(s: &Status, base: Option<u16>) -> String {
    match s {
        Status::Stopped => "Stopped".to_string(),
        Status::Starting => "Starting…".to_string(),
        Status::Waiting => "Waiting for a device".to_string(),
        Status::Connected { device } => format!("Connected: {device}"),
        Status::Error(msg) => format!("Problem: {}", health::error_sentence(msg, base)),
    }
}

pub fn tray_toggle_label(s: &Status) -> &'static str {
    match s {
        Status::Starting | Status::Waiting | Status::Connected { .. } => "Stop",
        Status::Stopped => "Start",
        // UxPlay may still be running in a live error: start would be a no-op.
        Status::Error(_) => "Restart",
    }
}

/// The configured fixed port, if any.
pub fn port_base(state: &AppState) -> Option<u16> {
    lock(&state.settings).port_base
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutostartAction {
    None,
    Enable,
    Disable,
}

/// Decide what to do about "Start with Windows". `currently_enabled` is None
/// when the state could not be read; then only enabling is attempted (a
/// disable of an unset entry may error and there is nothing to undo).
pub fn autostart_action(currently_enabled: Option<bool>, wanted: bool) -> AutostartAction {
    match (currently_enabled, wanted) {
        (Some(c), w) if c == w => AutostartAction::None,
        (_, true) => AutostartAction::Enable,
        (Some(_), false) => AutostartAction::Disable,
        (None, false) => AutostartAction::None,
    }
}

/// Result of save_settings once settings are saved and applied: an autostart
/// failure is reported, saying the settings themselves were kept.
pub fn save_outcome(autostart: Result<(), String>) -> Result<(), String> {
    autostart.map_err(|e| {
        format!("Settings were saved, but 'Start with Windows' could not be changed: {e}")
    })
}

/// Start the server with the current settings (no-op if already running).
pub fn start(state: &AppState) {
    let args = settings::to_args(&lock(&state.settings));
    state.supervisor.start(args);
}

/// Stop whatever is running (even a child stuck in a live error) and start
/// again with the current settings. The settings lock is released before the
/// supervisor is touched, as in save_settings.
pub fn restart(state: &AppState) {
    let args = settings::to_args(&lock(&state.settings));
    state.supervisor.restart(args);
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
    let want_autostart = s.autostart;
    let args = settings::to_args(&s);
    *lock(&state.settings) = s;
    if state.supervisor.status() != Status::Stopped {
        state.supervisor.restart(args);
    }
    // Autostart last: its failure must not undo or skip the saved settings.
    save_outcome(apply_autostart(&app, want_autostart))
}

fn apply_autostart(app: &AppHandle, wanted: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    match autostart_action(autolaunch.is_enabled().ok(), wanted) {
        AutostartAction::None => Ok(()),
        AutostartAction::Enable => autolaunch.enable().map_err(|e| e.to_string()),
        AutostartAction::Disable => autolaunch.disable().map_err(|e| e.to_string()),
    }
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

#[tauri::command]
pub fn restart_server(state: State<'_, AppState>) {
    restart(&state);
}

/// Plain sentence for an error status text (the UI banner headline).
#[tauri::command]
pub fn describe_error(state: State<'_, AppState>, msg: String) -> String {
    health::error_sentence(&msg, port_base(&state))
}

/// Firewall and network-profile probes, reusing a result younger than
/// [`PROBE_TTL`] unless `force` is set (the "Check again" button).
fn probes(state: &AppState, force: bool) -> ProbeResult {
    let now = Instant::now();
    if !force {
        if let Some((at, r)) = *lock(&state.probes) {
            if fresh(now, at, PROBE_TTL) {
                return r;
            }
        }
    }
    let r = ProbeResult {
        firewall_missing: !health::firewall_rule_exists(health::FIREWALL_RULE_NAME),
        public_network: health::network_categories()
            .is_some_and(|c| health::has_public_network(&c)),
    };
    *lock(&state.probes) = Some((Instant::now(), r));
    r
}

/// Async so the blocking firewall/network/port probes run off the main thread.
#[tauri::command]
pub async fn get_health(
    app: AppHandle,
    state: State<'_, AppState>,
    force: Option<bool>,
) -> Result<Vec<String>, String> {
    let base = port_base(&state);
    let status = state.supervisor.status();
    let uxplay_missing = health::check_uxplay(&crate::paths::uxplay_path(&app)).is_some();
    let probe = probes(&state, force.unwrap_or(false));
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
        probe.firewall_missing,
        probe.public_network,
        ports,
        error_text,
        base,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_env_sets_gstreamer_vars_and_home() {
        let env = launch_env(Path::new("C:/app/resources/uxplay"), Path::new("C:/data"));
        let plugins = Path::new("C:/app/resources/uxplay").join("gst-plugins");
        let plugins = plugins.to_string_lossy().to_string();
        let s = |a: &str, b: &str| (a.to_string(), b.to_string());
        assert_eq!(
            env,
            vec![
                s("GST_PLUGIN_PATH", &plugins),
                s("GST_PLUGIN_SYSTEM_PATH", &plugins),
                s(
                    "GST_REGISTRY",
                    &Path::new("C:/data").join("gst-registry.bin").to_string_lossy()
                ),
                s("GST_REGISTRY_FORK", "no"),
                s("HOME", &Path::new("C:/data").to_string_lossy()),
            ]
        );
    }

    #[test]
    fn health_all_good_is_empty() {
        assert!(health_messages(false, false, false, vec![], None, Some(7000)).is_empty());
    }

    #[test]
    fn health_composes_and_maps_each_issue() {
        let m = health_messages(true, true, true, vec![HealthIssue::PortInUse(7001)], None, Some(7000));
        assert_eq!(
            m,
            vec![
                health::issue_message(&HealthIssue::UxplayMissing),
                health::issue_message(&HealthIssue::FirewallRuleMissing),
                health::issue_message(&HealthIssue::PublicNetwork),
                health::issue_message(&HealthIssue::PortInUse(7001)),
            ]
        );
        let m = health_messages(false, false, true, vec![], None, None);
        assert_eq!(m, vec![health::issue_message(&HealthIssue::PublicNetwork)]);
    }

    #[test]
    fn health_classifies_error_text_with_base_and_dedupes() {
        let err = "*** ERROR: Error initialising socket 10013";
        let m = health_messages(false, false, false, vec![HealthIssue::PortInUse(7001)], Some(err), Some(7000));
        assert_eq!(m, vec![health::issue_message(&HealthIssue::PortInUse(7001))]);
        let m = health_messages(false, false, false, vec![], Some(err), None);
        assert!(m.is_empty());
        let m = health_messages(false, false, false, vec![], Some("Required gstreamer plugin 'level' not found"), None);
        assert_eq!(m.len(), 1);
        assert!(m[0].contains("level"));
        let m = health_messages(
            false,
            false,
            false,
            vec![],
            Some("UxPlay keeps stopping: Required gstreamer plugin 'app' not found"),
            None,
        );
        assert_eq!(m, vec![health::issue_message(&HealthIssue::GStreamerPluginMissing("app".into()))]);
    }

    #[test]
    fn status_text_covers_all_states() {
        assert_eq!(status_text(&Status::Stopped, None), "Stopped");
        assert_eq!(status_text(&Status::Starting, None), "Starting…");
        assert_eq!(status_text(&Status::Waiting, None), "Waiting for a device");
        assert_eq!(status_text(&Status::Connected { device: "iPhone".into() }, None), "Connected: iPhone");
        assert_eq!(
            status_text(&Status::Error("boom".into()), None),
            "Problem: Castaway's streaming engine stopped unexpectedly. Try Restart, or reinstall Castaway."
        );
        assert_eq!(
            status_text(
                &Status::Error("UxPlay keeps stopping: Required gstreamer plugin 'app' not found".into()),
                None
            ),
            "Problem: A video component (app) is missing. Reinstall Castaway."
        );
        assert_eq!(
            status_text(&Status::Error("*** ERROR: Error initialising socket 10013".into()), Some(7000)),
            format!("Problem: {}", health::issue_message(&HealthIssue::PortInUse(7001)))
        );
    }

    #[test]
    fn probe_cache_freshness() {
        let t0 = Instant::now();
        let ttl = Duration::from_secs(15);
        assert!(fresh(t0, t0, ttl));
        assert!(fresh(t0 + Duration::from_secs(14), t0, ttl));
        assert!(!fresh(t0 + Duration::from_secs(15), t0, ttl));
        assert!(!fresh(t0 + Duration::from_secs(60), t0, ttl));
        // A clock reading earlier than the cache time counts as fresh.
        assert!(fresh(t0, t0 + Duration::from_secs(1), ttl));
        assert_eq!(PROBE_TTL, ttl);
    }

    #[test]
    fn should_start_hidden_only_with_flag() {
        let v = |s: &[&str]| s.iter().map(|x| x.to_string()).collect::<Vec<_>>();
        assert!(should_start_hidden(&v(&["castaway.exe", "--hidden"])));
        assert!(should_start_hidden(&v(&["castaway.exe", "x", "--hidden"])));
        assert!(!should_start_hidden(&v(&["castaway.exe"])));
        assert!(!should_start_hidden(&v(&[])));
        assert!(!should_start_hidden(&v(&["--hidden"])));
        assert!(!should_start_hidden(&v(&["castaway.exe", "--hidden=no", "-hidden"])));
    }

    #[test]
    fn toggle_label_per_state() {
        assert_eq!(tray_toggle_label(&Status::Stopped), "Start");
        assert_eq!(tray_toggle_label(&Status::Error("x".into())), "Restart");
        assert_eq!(tray_toggle_label(&Status::Starting), "Stop");
        assert_eq!(tray_toggle_label(&Status::Waiting), "Stop");
        assert_eq!(tray_toggle_label(&Status::Connected { device: "d".into() }), "Stop");
    }

    #[test]
    fn autostart_action_all_combinations() {
        use AutostartAction::*;
        assert_eq!(autostart_action(Some(true), true), None);
        assert_eq!(autostart_action(Some(false), false), None);
        assert_eq!(autostart_action(Some(false), true), Enable);
        assert_eq!(autostart_action(Some(true), false), Disable);
        assert_eq!(autostart_action(Option::None, true), Enable);
        assert_eq!(autostart_action(Option::None, false), None);
    }

    #[test]
    fn save_outcome_ok_and_error_message() {
        assert_eq!(save_outcome(Ok(())), Ok(()));
        let e = save_outcome(Err("access denied".into())).unwrap_err();
        assert!(e.contains("Settings were saved"));
        assert!(e.contains("Start with Windows"));
        assert!(e.contains("access denied"));
    }
}
