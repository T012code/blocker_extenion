//! Smart Clip: your clipboard on all your PCs. Copy on one, paste on another.
//! Developed by Layó (https://layothedeveloper.com).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("Smart Clip is a Windows app. Build it on Windows (the core crate's tests run anywhere).");

mod commands;
mod picker;
mod settings;
mod tray;
mod win;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock};

use smart_clip_core::{ClipSummary, Config, Engine, Host, Status, StoredSession};
use tauri::{AppHandle, Emitter, Manager, RunEvent, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use settings::Settings;

/// The Supabase project, from config/supabase.json (filled in when the project exists).
const SUPABASE: &str = include_str!("../../../config/supabase.json");

pub struct AppState {
    pub engine: Engine,
    pub settings: Mutex<Settings>,
    pub dir: PathBuf,
}

impl AppState {
    pub fn settings(&self) -> MutexGuard<'_, Settings> {
        self.settings.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn save(&self) {
        let s = self.settings().clone();
        s.save(&self.dir);
    }
}

/// Passes the engine's news to the windows, the tray and the floating icon.
struct AppHost {
    app: OnceLock<AppHandle>,
    dir: PathBuf,
}

impl Host for AppHost {
    fn clips_changed(&self, clips: Vec<ClipSummary>) {
        if let Some(app) = self.app.get() {
            let _ = app.emit("clips", clips);
        }
    }

    fn status_changed(&self, status: Status) {
        let Some(app) = self.app.get() else { return };
        let _ = app.emit("status", &status);
        tray::update(app, &status);
        refresh_float(app, status.signed_in);
    }

    fn save_session(&self, session: Option<StoredSession>) {
        settings::session::save(&self.dir, session.as_ref());
    }
}

/// Starts or stops the floating icon to match the settings and the sign-in.
pub fn refresh_float(app: &AppHandle, signed_in: bool) {
    let Some(state) = app.try_state::<AppState>() else { return };
    let (on, hidden) = {
        let s = state.settings();
        (s.float_icon, s.hidden_apps.clone())
    };
    win::float::set_hidden_apps(&hidden);
    if on && signed_in {
        win::float::start();
    } else {
        win::float::stop();
    }
}

/// Registers the picker shortcut from the settings (none for "Off").
pub fn apply_hotkey(app: &AppHandle) {
    let gs = app.global_shortcut();
    let _ = gs.unregister_all();
    let Some(state) = app.try_state::<AppState>() else { return };
    let hotkey = state.settings().hotkey.clone();
    if hotkey != "Off"
        && let Err(e) = gs.register(hotkey.as_str())
    {
        tracing::warn!("shortcut {hotkey} not available: {e}");
    }
}

/// Opens (or brings back) the main window.
pub fn open_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
        return;
    }
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Smart Clip")
        .inner_size(440.0, 660.0)
        .min_inner_size(380.0, 500.0)
        .center()
        .focused(true)
        .build();
}

fn main() {
    let config: Config = serde_json::from_str(SUPABASE).unwrap_or_default();
    let hidden_start = std::env::args().any(|a| a == "--hidden");

    let app = tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| open_main(app)))
        .plugin(tauri_plugin_autostart::init(MacosLauncher::LaunchAgent, Some(vec!["--hidden"])))
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, _shortcut, event| {
                    if event.state() != ShortcutState::Pressed {
                        return;
                    }
                    if picker::is_open(app) {
                        picker::hide(app);
                        return;
                    }
                    let signed_in = app.try_state::<AppState>().is_some_and(|s| s.engine.status().signed_in);
                    if !signed_in {
                        open_main(app);
                        return;
                    }
                    let (anchor, target) = win::shortcut_anchor();
                    picker::open(app, anchor, target);
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::send_code,
            commands::verify_code,
            commands::sign_out,
            commands::list_clips,
            commands::copy_clip,
            commands::paste_clip,
            commands::delete_clip,
            commands::clear_clips,
            commands::save_settings,
            commands::set_autostart,
            commands::show_app,
            commands::close_picker,
            commands::open_site,
        ])
        .setup(move |app| {
            let handle = app.handle().clone();
            let dir = app.path().app_local_data_dir().unwrap_or_else(|_| std::env::temp_dir().join("SmartClip"));
            let _ = std::fs::create_dir_all(&dir);
            let mut s = Settings::load(&dir);

            // First run: launch with Windows, quietly in the tray.
            if !s.first_run_done {
                let _ = app.autolaunch().enable();
                s.first_run_done = true;
            }
            s.save(&dir);

            let host = Arc::new(AppHost { app: OnceLock::new(), dir: dir.clone() });
            let _ = host.app.set(handle.clone());
            let engine = Engine::new(config.clone(), host, s.device_id.clone(), s.device_name.clone());
            engine.set_paused(s.paused);
            engine.set_sync_images(s.sync_images);
            let stored = settings::session::load(&dir);
            let signed_in = stored.is_some();
            if let Some(stored) = stored {
                engine.restore(stored);
            }
            tauri::async_runtime::spawn(engine.clone().run());

            app.manage(AppState { engine: engine.clone(), settings: Mutex::new(s), dir });

            // Copies, locking and sleep.
            let e = engine.clone();
            win::watcher::start(move |event| match event {
                win::watcher::Event::Copied(c) => {
                    let e = e.clone();
                    tauri::async_runtime::spawn(async move { e.capture(c).await });
                }
                win::watcher::Event::Away(away) => e.set_suspended(away),
            });

            // The floating icon.
            let h = handle.clone();
            win::float::set_callback(move |event| {
                let h2 = h.clone();
                let _ = h.run_on_main_thread(move || match event {
                    win::float::FloatEvent::Open { anchor, target } => picker::open(&h2, anchor, target),
                    win::float::FloatEvent::HideIn(name) => {
                        if let Some(state) = h2.try_state::<AppState>() {
                            {
                                let mut s = state.settings();
                                if !s.hidden_apps.contains(&name) {
                                    s.hidden_apps.push(name);
                                }
                            }
                            state.save();
                            let _ = h2.emit("settings", state.settings().clone());
                            refresh_float(&h2, state.engine.status().signed_in);
                        }
                    }
                    win::float::FloatEvent::TurnOff => {
                        if let Some(state) = h2.try_state::<AppState>() {
                            state.settings().float_icon = false;
                            state.save();
                            let _ = h2.emit("settings", state.settings().clone());
                            refresh_float(&h2, false);
                        }
                    }
                });
            });
            refresh_float(&handle, signed_in);
            apply_hotkey(&handle);
            tray::create(&handle)?;
            tray::update(&handle, &engine.status());

            // Opened by hand: show the window. At Windows start-up: stay quietly in the tray.
            if !hidden_start {
                open_main(&handle);
            }
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("Smart Clip couldn't start");

    app.run(|_app, event| {
        // Closing the window keeps Smart Clip running in the tray; only Quit ends it.
        if let RunEvent::ExitRequested { api, code: None, .. } = event {
            api.prevent_exit();
        }
    });
}
