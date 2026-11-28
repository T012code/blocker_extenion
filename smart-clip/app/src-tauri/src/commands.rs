//! What the windows can ask of the app.

use serde::Serialize;
use serde_json::Value;
use smart_clip_core::engine::Content;
use smart_clip_core::{ClipSummary, Status};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::settings::{HOTKEYS, Settings};
use crate::{AppState, picker, win};

type Result<T> = std::result::Result<T, String>;

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    status: Status,
    settings: Settings,
    autostart: bool,
    hotkeys: &'static [&'static str],
}

#[tauri::command]
pub fn app_info(app: AppHandle, state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        status: state.engine.status(),
        settings: state.settings().clone(),
        autostart: app.autolaunch().is_enabled().unwrap_or(false),
        hotkeys: HOTKEYS,
    }
}

#[tauri::command]
pub async fn send_code(state: State<'_, AppState>, email: String) -> Result<()> {
    state.engine.send_code(&email).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn verify_code(state: State<'_, AppState>, email: String, code: String) -> Result<()> {
    state.engine.verify_code(&email, &code).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn sign_out(state: State<'_, AppState>) -> Result<()> {
    state.engine.sign_out().await;
    Ok(())
}

#[tauri::command]
pub fn list_clips(state: State<'_, AppState>) -> Vec<ClipSummary> {
    state.engine.clips()
}

/// Puts a clip on this PC's clipboard.
async fn to_clipboard(state: &AppState, id: &str) -> Result<()> {
    let content = state.engine.content(id).await.map_err(|e| e.to_string())?;
    let owner = win::watcher::hwnd().ok_or("Smart Clip isn't ready yet. Try again in a moment.")?;
    let owner_raw = owner.0 as isize;
    let ok = match content {
        Content::Text(text) => win::clipboard::write_text(owner, &text),
        Content::Image(bytes) => {
            // Decoding and converting a big picture takes a moment: off the main thread.
            tauri::async_runtime::spawn_blocking(move || {
                let Some(img) = smart_clip_core::image::decode(&bytes) else { return false };
                let png = smart_clip_core::image::to_png(&img);
                let dib = smart_clip_core::image::to_dib(&img);
                win::clipboard::write_image(win::hwnd_from(owner_raw), &png, &dib)
            })
            .await
            .unwrap_or(false)
        }
    };
    if ok { Ok(()) } else { Err("Couldn't use the clipboard (another app may be holding it). Try again.".into()) }
}

/// From the main window: copy, without pasting.
#[tauri::command]
pub async fn copy_clip(state: State<'_, AppState>, id: String) -> Result<()> {
    to_clipboard(&state, &id).await
}

/// From the picker: copy, close the picker, and paste into the app that was in use.
#[tauri::command]
pub async fn paste_clip(app: AppHandle, state: State<'_, AppState>, id: String) -> Result<()> {
    to_clipboard(&state, &id).await?;
    picker::hide(&app);
    let target = picker::target();
    let ours = app.webview_windows().values().any(|w| w.hwnd().map(|h| h.0 as isize == target).unwrap_or(false));
    if !ours {
        win::paste::paste_into(target);
    }
    Ok(())
}

#[tauri::command]
pub async fn delete_clip(state: State<'_, AppState>, id: String) -> Result<()> {
    state.engine.delete(&id).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn clear_clips(state: State<'_, AppState>) -> Result<()> {
    state.engine.clear_all().await.map_err(|e| e.to_string())
}

/// Changes settings: `patch` holds only the ones that changed.
#[tauri::command]
pub fn save_settings(app: AppHandle, state: State<'_, AppState>, patch: Value) -> Result<Settings> {
    let before = state.settings().clone();
    let mut merged = serde_json::to_value(&before).map_err(|e| e.to_string())?;
    if let (Some(m), Some(p)) = (merged.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            // The id is this PC's, and first-run is internal: neither changes from a window.
            if k != "device_id" && k != "first_run_done" && m.contains_key(k) {
                m.insert(k.clone(), v.clone());
            }
        }
    }
    let mut next: Settings = serde_json::from_value(merged).map_err(|_| "That setting isn't valid.".to_string())?;
    if !matches!(next.theme.as_str(), "system" | "light" | "dark") {
        next.theme = before.theme.clone();
    }
    if !HOTKEYS.contains(&next.hotkey.as_str()) {
        next.hotkey = before.hotkey.clone();
    }
    next.device_name = next.device_name.trim().chars().take(64).collect();
    if next.device_name.is_empty() {
        next.device_name = before.device_name.clone();
    }
    next.hidden_apps = next.hidden_apps.iter().map(|a| a.trim().to_lowercase()).filter(|a| !a.is_empty()).collect();
    next.hidden_apps.dedup();
    *state.settings() = next.clone();
    state.save();

    if next.paused != before.paused {
        state.engine.set_paused(next.paused);
    }
    if next.sync_images != before.sync_images {
        state.engine.set_sync_images(next.sync_images);
    }
    if next.device_name != before.device_name {
        state.engine.set_device_name(&next.device_name);
    }
    if next.hotkey != before.hotkey {
        crate::apply_hotkey(&app);
    }
    if next.float_icon != before.float_icon || next.hidden_apps != before.hidden_apps {
        crate::refresh_float(&app, state.engine.status().signed_in);
    }
    let _ = app.emit("settings", &next);
    Ok(next)
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, on: bool) -> Result<bool> {
    let al = app.autolaunch();
    let r = if on { al.enable() } else { al.disable() };
    r.map_err(|_| "Windows didn't allow that change.".to_string())?;
    Ok(al.is_enabled().unwrap_or(on))
}

#[tauri::command]
pub fn show_app(app: AppHandle) {
    picker::hide(&app);
    crate::open_main(&app);
}

#[tauri::command]
pub fn close_picker(app: AppHandle) {
    picker::hide(&app);
}

#[tauri::command]
pub fn open_site(app: AppHandle) {
    let _ = app.opener().open_url("https://layothedeveloper.com", None::<&str>);
}
