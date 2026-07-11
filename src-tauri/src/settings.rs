//! Persisted user settings (JSON in the app config dir).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub show_claude: bool,
    pub show_codex: bool,
    pub autostart: bool,
    pub always_on_top: bool,
    pub refresh_secs: u64,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            show_claude: true,
            show_codex: true,
            autostart: false,
            always_on_top: true,
            refresh_secs: 90,
        }
    }
}

fn settings_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_config_dir()
        .ok()
        .map(|d| d.join("settings.json"))
}

pub fn load(app: &AppHandle) -> Settings {
    if let Some(p) = settings_path(app) {
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Ok(s) = serde_json::from_str::<Settings>(&text) {
                return s;
            }
        }
    }
    Settings::default()
}

pub fn save(app: &AppHandle, s: &Settings) {
    if let Some(p) = settings_path(app) {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(text) = serde_json::to_string_pretty(s) {
            let _ = std::fs::write(&p, text);
        }
    }
}
