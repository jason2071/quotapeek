mod claude;
mod codex;
mod commands;
mod credentials;
mod logging;
mod models;
mod settings;
mod transcript;

use std::sync::Mutex;
use tauri::{
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, PhysicalPosition, WebviewWindow, WindowEvent, Wry,
};
use tauri_plugin_autostart::MacosLauncher;
use tauri_plugin_window_state::StateFlags;

const WIDGETS: [&str; 2] = ["claude", "codex"];
const DEFAULT_POS: [(&str, i32, i32); 2] = [("claude", 60, 60), ("codex", 420, 60)];

/// Tray checkable items, kept in state so visibility changes anywhere (settings,
/// left-click, reset) can sync the checkmarks.
pub(crate) struct TrayMenu {
    pub claude: CheckMenuItem<Wry>,
    pub codex: CheckMenuItem<Wry>,
}

/// Show/hide a widget window and keep its tray checkmark in sync. Does NOT persist
/// (callers that should persist do so explicitly).
pub(crate) fn set_widget_visible(app: &AppHandle, label: &str, visible: bool) {
    if let Some(w) = app.get_webview_window(label) {
        let _ = if visible { w.show() } else { w.hide() };
    }
    if let Some(tm) = app.try_state::<TrayMenu>() {
        let item = match label {
            "claude" => Some(&tm.claude),
            "codex" => Some(&tm.codex),
            _ => None,
        };
        if let Some(i) = item {
            let _ = i.set_checked(visible);
        }
    }
}

fn primary_origin(app: &AppHandle) -> (i32, i32) {
    app.get_webview_window("claude")
        .and_then(|w| w.primary_monitor().ok().flatten())
        .map(|m| (m.position().x, m.position().y))
        .unwrap_or((0, 0))
}

/// Snap an off-screen widget (after a monitor/DPI change) back on screen.
fn clamp_on_screen(app: &AppHandle, window: &WebviewWindow) {
    let pos = match window.outer_position() {
        Ok(p) => p,
        Err(_) => return,
    };
    let monitors = window.available_monitors().unwrap_or_default();
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

/// Move both widgets to defaults and show the enabled ones. Callable from the tray
/// and from Settings.
pub(crate) fn reset_positions(app: &AppHandle) {
    let s = settings::load(app);
    let (ox, oy) = primary_origin(app);
    for (label, dx, dy) in DEFAULT_POS {
        if let Some(w) = app.get_webview_window(label) {
            let _ = w.set_position(PhysicalPosition::new(ox + dx, oy + dy));
        }
    }
    set_widget_visible(app, "claude", s.show_claude);
    set_widget_visible(app, "codex", s.show_codex);
    tracing::info!("reset widget positions");
}

/// Tray left-click: hide all widgets if any is showing, else show the enabled ones
/// (or open Settings if none are enabled).
fn toggle_widgets(app: &AppHandle) {
    let any_visible = WIDGETS.iter().any(|l| {
        app.get_webview_window(l)
            .and_then(|w| w.is_visible().ok())
            .unwrap_or(false)
    });
    if any_visible {
        for l in WIDGETS {
            set_widget_visible(app, l, false);
        }
    } else {
        let s = settings::load(app);
        if !s.show_claude && !s.show_codex {
            if let Some(w) = app.get_webview_window("settings") {
                let _ = w.show();
                let _ = w.set_focus();
            }
            return;
        }
        set_widget_visible(app, "claude", s.show_claude);
        set_widget_visible(app, "codex", s.show_codex);
    }
}

/// Persist a widget's visibility (tray checkbox toggled) and apply it.
fn persist_show(app: &AppHandle, label: &str, visible: bool) {
    let mut s = settings::load(app);
    match label {
        "claude" => s.show_claude = visible,
        "codex" => s.show_codex = visible,
        _ => {}
    }
    settings::save(app, &s);
    set_widget_visible(app, label, visible);
}

/// Tray "Check for updates": check (no auto-install) and show a notification.
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
                // Persist POSITION only — widgets auto-fit their size to content.
                .with_state_flags(StateFlags::POSITION)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostarted"]),
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .manage(Mutex::new(commands::Tooltip::default()))
        .invoke_handler(tauri::generate_handler![
            commands::get_usage,
            commands::get_settings,
            commands::set_show,
            commands::set_autostart,
            commands::set_always_on_top,
            commands::set_refresh,
            commands::check_update,
            commands::install_update,
            commands::report_usage,
            commands::notify,
            commands::reset_positions,
        ])
        .setup(|app| {
            let handle = app.handle().clone();

            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let s = settings::load(&handle);

            // ---- Tray menu ----
            let settings_i = MenuItem::with_id(app, "settings", "Settings", true, None::<&str>)?;
            let claude_i = CheckMenuItem::with_id(
                app,
                "show_claude",
                "Show Claude widget",
                true,
                s.show_claude,
                None::<&str>,
            )?;
            let codex_i = CheckMenuItem::with_id(
                app,
                "show_codex",
                "Show Codex widget",
                true,
                s.show_codex,
                None::<&str>,
            )?;
            let refresh_i = MenuItem::with_id(app, "refresh", "Refresh now", true, None::<&str>)?;
            let reset_i = MenuItem::with_id(app, "reset", "Reset positions", true, None::<&str>)?;
            let update_i =
                MenuItem::with_id(app, "update", "Check for updates", true, None::<&str>)?;
            let quit_i = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let sep1 = PredefinedMenuItem::separator(app)?;
            let sep2 = PredefinedMenuItem::separator(app)?;
            let sep3 = PredefinedMenuItem::separator(app)?;
            let menu = Menu::with_items(
                app,
                &[
                    &settings_i, &sep1, &claude_i, &codex_i, &refresh_i, &sep2, &reset_i, &update_i,
                    &sep3, &quit_i,
                ],
            )?;

            app.manage(TrayMenu {
                claude: claude_i.clone(),
                codex: codex_i.clone(),
            });

            let mut tray = TrayIconBuilder::with_id("main")
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
                    "show_claude" => {
                        if let Some(tm) = app.try_state::<TrayMenu>() {
                            let v = tm.claude.is_checked().unwrap_or(true);
                            persist_show(app, "claude", v);
                        }
                    }
                    "show_codex" => {
                        if let Some(tm) = app.try_state::<TrayMenu>() {
                            let v = tm.codex.is_checked().unwrap_or(true);
                            persist_show(app, "codex", v);
                        }
                    }
                    "refresh" => {
                        let _ = app.emit("force-refresh", ());
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
            #[cfg(target_os = "macos")]
            if let Ok(img) =
                tauri::image::Image::from_bytes(include_bytes!("../icons/tray-template.png"))
            {
                tray = tray.icon(img).icon_as_template(true);
            }
            tray.build(app)?;

            // ---- Apply saved settings + clamp off-screen widgets ----
            for label in WIDGETS {
                if let Some(w) = handle.get_webview_window(label) {
                    clamp_on_screen(&handle, &w);
                    let _ = w.set_always_on_top(s.always_on_top);
                }
            }
            set_widget_visible(&handle, "claude", s.show_claude);
            set_widget_visible(&handle, "codex", s.show_codex);
            Ok(())
        })
        .on_window_event(|window, event| {
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
