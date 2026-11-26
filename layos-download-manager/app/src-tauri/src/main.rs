//! Layó's Download Manager for Windows. Developed by Layó (https://layothedeveloper.com).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod associations;
mod commands;
mod power;
mod settings;
mod state;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use tauri::{AppHandle, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

/// Adds or removes the "start with Windows" entry.
pub fn apply_autostart(app: &AppHandle, on: bool) {
    let a = app.autolaunch();
    let _ = if on { a.enable() } else { a.disable() };
}

/// Magnet links and .torrent files that Windows passed on the command line.
fn items_from_args() -> Vec<ldm_ipc::BrowserDownload> {
    std::env::args()
        .skip(1)
        .filter_map(|a| {
            let lower = a.to_ascii_lowercase();
            if lower.starts_with("magnet:?") {
                Some(a)
            } else if lower.ends_with(".torrent") && std::path::Path::new(&a).is_file() {
                std::fs::canonicalize(&a)
                    .ok()
                    .map(|p| format!("file://{}", p.display()))
            } else {
                None
            }
        })
        .map(|url| ldm_ipc::BrowserDownload {
            url,
            ..Default::default()
        })
        .collect()
}

fn main() {
    // Run by the uninstaller: remove file associations and the startup entry, then quit.
    if std::env::args().any(|a| a == "--unregister-associations") {
        associations::apply(false);
        associations::remove_startup_entry();
        return;
    }
    let from_browser = std::env::args().any(|a| a == "--from-browser");
    let from_startup = std::env::args().any(|a| a == "--autostart");
    let open_items = items_from_args();

    // Only one copy runs: a second launch hands over what it was opened with and
    // brings the first one to the front.
    let path = ldm_ipc::default_path();
    let handover = open_items.clone();
    let already = tauri::async_runtime::block_on(async {
        if !ldm_ipc::is_running(&path).await {
            return false;
        }
        let req = if handover.is_empty() {
            ldm_ipc::Request::ShowApp
        } else {
            ldm_ipc::Request::Download {
                items: handover,
                source: ldm_ipc::Source::Link,
                ask: None,
            }
        };
        let _ = ldm_ipc::request(&path, &req, Duration::from_secs(10)).await;
        true
    });
    if already {
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ))
        .setup(move |app| {
            let data_dir = app.path().app_local_data_dir()?;
            let downloads = app
                .path()
                .download_dir()
                .unwrap_or_else(|_| data_dir.join("Downloads"))
                .join("Layó's Download Manager");
            let state = tauri::async_runtime::block_on(state::AppState::start(
                app.handle().clone(),
                data_dir,
                downloads,
            ))
            .map_err(|e| e.to_string())?;
            let settings = state.settings.lock().unwrap().clone();
            apply_autostart(app.handle(), settings.launch_on_startup);
            associations::apply(settings.handle_torrent_links);
            // Opened with magnet links or .torrent files: add them.
            if !open_items.is_empty() {
                let service = state.service.clone();
                let items = open_items.clone();
                tauri::async_runtime::spawn(async move {
                    service
                        .handle(ldm_ipc::Request::Download {
                            items,
                            source: ldm_ipc::Source::Link,
                            ask: None,
                        })
                        .await;
                });
            }
            app.manage(state);
            tray::create(app)?;
            // Started by the browser for a download, or with Windows: stay in the tray.
            let hide = from_browser || (from_startup && settings.start_minimized);
            if let (true, Some(w)) = (hide, app.get_webview_window("main")) {
                let _ = w.hide();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            let state = window.state::<Arc<state::AppState>>();
            match event {
                WindowEvent::CloseRequested { api, .. } => {
                    if state.settings.lock().unwrap().close_to_tray {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
                // Minimizing hides the window to the tray, if that's the setting.
                WindowEvent::Resized(_)
                    if state.settings.lock().unwrap().minimize_to_tray
                        && window.is_minimized().unwrap_or(false) =>
                {
                    let _ = window.hide();
                }
                _ => {}
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list,
            commands::add,
            commands::probe,
            commands::media_formats,
            commands::pause,
            commands::resume,
            commands::pause_all,
            commands::resume_all,
            commands::remove,
            commands::set_connections,
            commands::set_speed_limit,
            commands::refresh_address,
            commands::path_of,
            commands::get_settings,
            commands::save_settings,
            commands::network_adapters,
            commands::privacy_status,
            commands::test_proxy,
            commands::open_download,
            commands::show_in_folder,
            commands::open_portfolio,
            commands::queues_list,
            commands::queue_create,
            commands::queue_update,
            commands::queue_start,
            commands::queue_stop,
            commands::queue_delete,
            commands::set_queue,
            commands::queue_schedule,
            commands::grab_site,
            commands::cancel_shutdown,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Layó's Download Manager");
}
