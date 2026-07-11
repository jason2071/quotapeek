use crate::models::UsageSnapshot;
use crate::settings::{self, Settings};
use crate::{claude, codex};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_updater::UpdaterExt;

#[tauri::command]
pub async fn get_usage(provider: String) -> UsageSnapshot {
    match provider.as_str() {
        "codex" => codex::fetch_usage().await,
        _ => claude::fetch_usage().await,
    }
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    settings::load(&app)
}

#[tauri::command]
pub fn set_show(app: AppHandle, provider: String, visible: bool) {
    let mut s = settings::load(&app);
    match provider.as_str() {
        "claude" => s.show_claude = visible,
        "codex" => s.show_codex = visible,
        _ => {}
    }
    settings::save(&app, &s);
    if let Some(w) = app.get_webview_window(&provider) {
        let _ = if visible { w.show() } else { w.hide() };
    }
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) {
    let mut s = settings::load(&app);
    s.autostart = enabled;
    settings::save(&app, &s);
    let manager = app.autolaunch();
    let _ = if enabled {
        manager.enable()
    } else {
        manager.disable()
    };
}

#[tauri::command]
pub fn set_always_on_top(app: AppHandle, enabled: bool) {
    let mut s = settings::load(&app);
    s.always_on_top = enabled;
    settings::save(&app, &s);
    for label in ["claude", "codex"] {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_always_on_top(enabled);
        }
    }
}

#[tauri::command]
pub fn set_refresh(app: AppHandle, secs: u64) {
    let mut s = settings::load(&app);
    s.refresh_secs = secs;
    settings::save(&app, &s);
    // Push the new cadence to already-open widgets (no restart needed).
    let _ = app.emit("settings-changed", secs);
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateStatus {
    pub status: String, // "available" | "uptodate" | "error"
    pub version: Option<String>,
    pub message: Option<String>,
}

/// Check the updater endpoint without installing. Returns a status for the UI.
#[tauri::command]
pub async fn check_update(app: AppHandle) -> UpdateStatus {
    let updater = match app.updater() {
        Ok(u) => u,
        Err(e) => {
            return UpdateStatus {
                status: "error".into(),
                version: None,
                message: Some(e.to_string()),
            }
        }
    };
    match updater.check().await {
        Ok(Some(update)) => UpdateStatus {
            status: "available".into(),
            version: Some(update.version.clone()),
            message: None,
        },
        Ok(None) => UpdateStatus {
            status: "uptodate".into(),
            version: None,
            message: None,
        },
        Err(e) => UpdateStatus {
            status: "error".into(),
            version: None,
            message: Some(e.to_string()),
        },
    }
}

/// Download + install the pending update, then restart.
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater
        .check()
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "No update available".to_string())?;
    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart();
}
