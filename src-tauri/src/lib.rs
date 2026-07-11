mod claude;
mod codex;
mod commands;
mod credentials;
mod models;
mod settings;
mod transcript;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::StateFlags;

const WIDGETS: [&str; 2] = ["claude", "codex"];

fn set_visible(app: &AppHandle, label: &str, visible: bool) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = if visible { w.show() } else { w.hide() };
    }
}

/// Tray left-click: hide all widgets if any is showing, else show the enabled ones.
fn toggle_widgets(app: &AppHandle) {
    let any_visible = WIDGETS.iter().any(|l| {
        app.get_webview_window(l)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false)
    });
    if any_visible {
        for l in WIDGETS {
            set_visible(app, l, false);
        }
    } else {
        let s = settings::load(app);
        set_visible(app, "claude", s.show_claude);
        set_visible(app, "codex", s.show_codex);
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(
            tauri_plugin_window_state::Builder::default()
                // Persist size + position only — NOT visibility (we control that).
                .with_state_flags(StateFlags::SIZE | StateFlags::POSITION)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostarted"]),
        ))
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::get_settings,
            commands::set_show,
            commands::set_autostart,
            commands::set_always_on_top,
            commands::set_refresh,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // ---- Tray icon + menu ----
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_i, &quit_i])?;

            let mut tray = TrayIconBuilder::new()
                .tooltip("AI Usage")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        toggle_widgets(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon() {
                tray = tray.icon(icon.clone());
            }
            tray.build(app)?;

            // ---- Apply saved settings ----
            let s = settings::load(&handle);
            set_visible(&handle, "claude", s.show_claude);
            set_visible(&handle, "codex", s.show_codex);
            for label in WIDGETS {
                if let Some(w) = handle.get_webview_window(label) {
                    let _ = w.set_always_on_top(s.always_on_top);
                }
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing a window (only the settings panel has an affordance) hides it
            // instead of quitting — the app lives in the tray until "Quit".
            if let WindowEvent::CloseRequested { api, .. } = event {
                if window.label() == "settings" {
                    let _ = window.hide();
                    api.prevent_close();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
