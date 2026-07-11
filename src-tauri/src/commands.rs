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
    crate::set_widget_visible(&app, &provider, visible);
}

/// Latest headline usage % per provider, for the tray tooltip.
#[derive(Default)]
pub struct Tooltip {
    pub claude: Option<f64>,
    pub codex: Option<f64>,
}

/// Widgets report their headline % after each fetch → live tray tooltip.
#[tauri::command]
pub fn report_usage(app: AppHandle, provider: String, used_pct: Option<f64>) {
    let state = match app.try_state::<std::sync::Mutex<Tooltip>>() {
        Some(s) => s,
        None => return,
    };
    let tip = {
        let mut t = state.lock().unwrap();
        match provider.as_str() {
            "claude" => t.claude = used_pct,
            "codex" => t.codex = used_pct,
            _ => {}
        }
        let mut parts = Vec::new();
        if let Some(c) = t.claude {
            parts.push(format!("Claude {}%", c.round() as i64));
        }
        if let Some(x) = t.codex {
            parts.push(format!("Codex {}%", x.round() as i64));
        }
        if parts.is_empty() {
            "QuotaPeek — AI usage".to_string()
        } else {
            parts.join("  ·  ")
        }
    };
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_tooltip(Some(tip));
    }
}

/// Show a native notification (used for near-limit warnings from the widgets).
#[tauri::command]
pub fn notify(app: AppHandle, title: String, body: String) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app.notification().builder().title(title).body(body).show();
}

/// Reset widget positions (exposed to Settings; shares the tray logic).
#[tauri::command]
pub fn reset_positions(app: AppHandle) {
    crate::reset_positions(&app);
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
