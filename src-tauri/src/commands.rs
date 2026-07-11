use crate::models::UsageSnapshot;
use crate::settings::{self, Settings};
use crate::{claude, codex};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

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
