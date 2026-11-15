//! In-app updates (OTA): check GitHub for a newer signed release and install it.
//! All logic is in Rust; the UI only shows the text it is given.
use crate::commands::{self, AppState};
use serde::Serialize;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: Option<String>,
}

/// Plain sentence for a failed check or install.
pub fn plain_error(raw: &str) -> String {
    format!("Couldn't reach the update server. Check your internet connection and try again. ({raw})")
}

#[tauri::command]
pub fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let updater = app.updater().map_err(|e| plain_error(&e.to_string()))?;
    let found = updater.check().await.map_err(|e| plain_error(&e.to_string()))?;
    Ok(found.map(|u| UpdateInfo { version: u.version.clone(), notes: u.body.clone() }))
}

/// Downloads and installs the update. UxPlay is stopped first so the
/// installer can replace its files; it is started again if the install fails.
/// On success the installer takes over and this process exits.
#[tauri::command]
pub async fn install_update(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let updater = app.updater().map_err(|e| plain_error(&e.to_string()))?;
    let Some(update) = updater.check().await.map_err(|e| plain_error(&e.to_string()))? else {
        return Ok(());
    };
    state.supervisor.stop();
    if let Err(e) = update.download_and_install(|_, _| {}, || {}).await {
        if let Some(s) = app.try_state::<AppState>() {
            commands::start(&s);
        }
        return Err(format!("The update couldn't be installed: {e}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_error_keeps_the_raw_reason_last() {
        let m = plain_error("timeout");
        assert!(m.starts_with("Couldn't reach the update server."));
        assert!(m.ends_with("(timeout)"));
    }
}
