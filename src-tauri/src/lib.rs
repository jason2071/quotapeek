mod claude;
mod codex;
mod commands;
mod credentials;
mod logging;
mod models;
mod settings;
mod transcript;

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, WebviewWindow, WindowEvent,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::StateFlags;

const WIDGETS: [&str; 2] = ["claude", "codex"];
// Default on-screen positions (offset from the primary monitor origin).
const DEFAULT_POS: [(&str, i32, i32); 2] = [("claude", 60, 60), ("codex", 420, 60)];

fn set_visible(app: &AppHandle, label: &str, visible: bool) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = if visible { w.show() } else { w.hide() };
    }
}

fn primary_origin(app: &AppHandle) -> (i32, i32) {
    app.get_webview_window("claude")
        .and_then(|w| w.primary_monitor().ok().flatten())
        .map(|m| (m.position().x, m.position().y))
        .unwrap_or((0, 0))
}

/// If a widget's title/drag area isn't inside any monitor (e.g. after a monitor
/// unplug or DPI change), snap it back to a default on-screen position.
fn clamp_on_screen(app: &AppHandle, window: &WebviewWindow) {
    let pos = match window.outer_position() {
        Ok(p) => p,
        Err(_) => return,
    };
    let monitors = window.available_monitors().unwrap_or_default();
    // Anchor near the top-left drag strip — that's what the user needs to reach.
    let ax = pos.x + 24;
    let ay = pos.y + 12;
    let reachable = monitors.iter().any(|m| {
        let mp = m.position();
        let ms = m.size();
        ax >= mp.x && ax < mp.x + ms.width as i32 && ay >= mp.y && ay < mp.y + ms.height as i32
    });
    if !reachable {
        let (ox, oy) = primary_origin(app);
        let (dx, dy) = DEFAULT_POS
            .iter()
            .find(|(l, _, _)| *l == window.label())
            .map(|(_, x, y)| (*x, *y))
            .unwrap_or((60, 60));
        let _ = window.set_position(PhysicalPosition::new(ox + dx, oy + dy));
        tracing::info!(label = %window.label(), "clamped off-screen widget back on screen");
    }
}

/// Tray "Reset positions": move both widgets to defaults and show the enabled ones.
fn reset_positions(app: &AppHandle) {
    let s = settings::load(app);
    let (ox, oy) = primary_origin(app);
    for (label, dx, dy) in DEFAULT_POS {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_position(PhysicalPosition::new(ox + dx, oy + dy));
        }
    }
    set_visible(app, "claude", s.show_claude);
    set_visible(app, "codex", s.show_codex);
    tracing::info!("reset widget positions");
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
        if !s.show_claude && !s.show_codex {
            // Nothing enabled — open Settings so the click isn't a silent no-op.
            if let Some(w) = app.get_webview_window("settings") {
                let _ = w.show();
                let _ = w.set_focus();
            }
            return;
        }
        set_visible(app, "claude", s.show_claude);
        set_visible(app, "codex", s.show_codex);
    }
}

/// Tray "Check for updates": check (no auto-install) and show a notification with
/// the result. Installing happens from Settings so the user stays in control.
async fn tray_check_update(app: AppHandle) {
    use tauri_plugin_notification::NotificationExt;
    let status = commands::check_update(app.clone()).await;
    tracing::info!(status = %status.status, "tray update check");
    let (title, body): (&str, String) = match status.status.as_str() {
        "available" => (
            "Update available",
            format!(
                "QuotaPeek {} is available. Open Settings to install.",
                status.version.unwrap_or_default()
            ),
        ),
        "uptodate" => ("QuotaPeek", "You're up to date.".to_string()),
        _ => ("Update check failed", status.message.unwrap_or_default()),
    };
    let _ = app.notification().builder().title(title).body(body).show();
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    logging::init();
    tracing::info!("QuotaPeek starting");

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
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::get_settings,
            commands::set_show,
            commands::set_autostart,
            commands::set_always_on_top,
            commands::set_refresh,
            commands::check_update,
            commands::install_update,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            // macOS: run as a menubar-only accessory (no Dock icon / Cmd-Tab entry).
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            // ---- Tray icon + menu ----
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let reset_i =
                MenuItem::with_id(app, "reset", "Reset positions", true, None::<&str>)?;
            let update_i =
                MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&settings_i, &reset_i, &update_i, &quit_i])?;

            let mut tray = TrayIconBuilder::new()
                .tooltip("QuotaPeek — AI usage")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "settings" => {
                        if let Some(w) = app.get_webview_window("settings") {
                            let _ = w.show();
                            let _ = w.set_focus();
                        }
                    }
                    "reset" => reset_positions(app),
                    "update" => {
                        let handle = app.clone();
                        tauri::async_runtime::spawn(tray_check_update(handle));
                    }
                    "quit" => {
                        tracing::info!("quit from tray");
                        app.exit(0)
                    }
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
            // macOS: use a monochrome template icon that adapts to the menubar.
            #[cfg(target_os = "macos")]
            if let Ok(img) =
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
            {
                tray = tray.icon(img).icon_as_template(true);
            }
            tray.build(app)?;

            // ---- Apply saved settings + clamp any off-screen widget ----
            let s = settings::load(&handle);
            for label in WIDGETS {
                if let Some(w) = handle.get_webview_window(label) {
                    clamp_on_screen(&handle, &w);
                    let _ = w.set_always_on_top(s.always_on_top);
                }
            }
            set_visible(&handle, "claude", s.show_claude);
            set_visible(&handle, "codex", s.show_codex);
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
