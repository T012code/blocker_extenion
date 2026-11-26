//! Layó's Download Manager for Windows. Developed by Layó (https://layothedeveloper.com).
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod settings;
mod state;
mod tray;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Manager, WindowEvent};

fn main() {
    let from_browser = std::env::args().any(|a| a == "--from-browser");

    // Only one copy runs: a second launch brings the first one to the front.
    let path = ldm_ipc::default_path();
    let already = tauri::async_runtime::block_on(async {
        if ldm_ipc::is_running(&path).await {
            let _ =
                ldm_ipc::request(&path, &ldm_ipc::Request::ShowApp, Duration::from_secs(3)).await;
            true
        } else {
            false
        }
    });
    if already {
        return;
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_notification::init())
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
            app.manage(state);
            tray::create(app)?;
            // Started by the browser for a download: stay in the tray until needed.
            if from_browser && let Some(w) = app.get_webview_window("main") {
                let _ = w.hide();
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let state = window.state::<Arc<state::AppState>>();
                if state.settings.lock().unwrap().close_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
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
        ])
        .run(tauri::generate_context!())
        .expect("error while running Layó's Download Manager");
}
