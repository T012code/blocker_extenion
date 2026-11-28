//! The tray icon: left-click opens Smart Clip; the menu pauses syncing or quits.

use smart_clip_core::Status;
use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

const ID: &str = "tray";

struct TrayItems {
    pause: CheckMenuItem<tauri::Wry>,
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Smart Clip", true, None::<&str>)?;
    let pause = CheckMenuItem::with_id(app, "pause", "Pause syncing", true, false, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit Smart Clip", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &pause, &PredefinedMenuItem::separator(app)?, &quit])?;
    app.manage(TrayItems { pause });

    let mut builder = TrayIconBuilder::with_id(ID)
        .tooltip("Smart Clip")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "open" => crate::open_main(app),
            "pause" => {
                if let Some(state) = app.try_state::<AppState>() {
                    let paused = !state.settings().paused;
                    state.settings().paused = paused;
                    state.save();
                    state.engine.set_paused(paused);
                    let _ = app.emit("settings", state.settings().clone());
                }
            }
            "quit" => {
                crate::win::float::stop();
                app.exit(0);
            }
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                crate::open_main(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon().cloned() {
        builder = builder.icon(icon);
    }
    builder.build(app)?;
    Ok(())
}

/// Shows the sync state in the tooltip and the pause tick.
pub fn update(app: &AppHandle, status: &Status) {
    let tip = if !status.signed_in {
        "Smart Clip: not signed in"
    } else if status.paused {
        "Smart Clip: paused"
    } else if status.online {
        "Smart Clip: syncing"
    } else {
        "Smart Clip: offline, will catch up"
    };
    if let Some(tray) = app.tray_by_id(ID) {
        let _ = tray.set_tooltip(Some(tip));
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let _ = items.pause.set_checked(status.paused);
    }
}
