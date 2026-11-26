//! The tray icon: open the window, pause or continue everything, quit.

use std::sync::Arc;

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{App, Manager};

use crate::state::{AppState, show_window};

pub fn create(app: &App) -> tauri::Result<()> {
    let open = MenuItem::with_id(
        app,
        "open",
        "Open Layó's Download Manager",
        true,
        None::<&str>,
    )?;
    let pause = MenuItem::with_id(app, "pause_all", "Pause all", true, None::<&str>)?;
    let resume = MenuItem::with_id(app, "resume_all", "Continue all", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &open,
            &sep,
            &pause,
            &resume,
            &PredefinedMenuItem::separator(app)?,
            &quit,
        ],
    )?;
    let mut tray = TrayIconBuilder::with_id("main")
        .tooltip("Layó's Download Manager")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| {
            let state = app.state::<Arc<AppState>>().inner().clone();
            match event.id().as_ref() {
                "open" => show_window(app),
                "pause_all" => {
                    tauri::async_runtime::spawn(async move { state.engine.pause_all().await });
                }
                "resume_all" => {
                    for d in state.engine.list() {
                        if d.status == ldm_engine::DownloadStatus::Paused {
                            let _ = state.engine.start(d.id);
                        }
                    }
                }
                "quit" => app.exit(0),
                _ => {}
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_window(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    tray.build(app)?;
    Ok(())
}
