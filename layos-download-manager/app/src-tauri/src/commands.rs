//! What the window can ask the app to do.

use std::path::PathBuf;
use std::sync::Arc;

use ldm_engine::{Download, DownloadKind, DownloadStatus, ProbeInfo, RequestInfo, TorrentSpec};
use ldm_ipc::{BrowserDownload, MediaPick};
use ldm_torrent::privacy;
use serde::{Deserialize, Serialize};
use tauri::State;

use crate::settings::AppSettings;
use crate::state::AppState;

type S<'a> = State<'a, Arc<AppState>>;
type R<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    version: &'static str,
    ytdlp: bool,
    ffmpeg: bool,
    max_connections: u8,
    section_presets: Vec<u8>,
}

#[tauri::command]
pub fn app_info(state: S) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        ytdlp: state.tools.ytdlp.is_some(),
        ffmpeg: state.tools.ffmpeg.is_some(),
        max_connections: ldm_engine::MAX_CONNECTIONS,
        section_presets: ldm_engine::SECTION_PRESETS.to_vec(),
    }
}

#[tauri::command]
pub fn list(state: S) -> Vec<Download> {
    state.engine.list()
}

/// What the "add download" window sends.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AddArgs {
    url: String,
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    save_dir: Option<PathBuf>,
    #[serde(default)]
    connections: Option<u8>,
    #[serde(default)]
    start: bool,
    #[serde(default)]
    referer: Option<String>,
    #[serde(default)]
    cookies: Option<String>,
    #[serde(default)]
    user_agent: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    media: Option<MediaPick>,
    /// A .torrent file on this computer.
    #[serde(default)]
    torrent_file: Option<PathBuf>,
}

#[tauri::command]
pub async fn add(state: S<'_>, args: AddArgs) -> R<i64> {
    let mut new = if let Some(path) = args.torrent_file.clone() {
        ldm_engine::NewDownload {
            request: RequestInfo::new(format!("file://{}", path.display())),
            kind: DownloadKind::Torrent(TorrentSpec {
                torrent_file: Some(path),
                ..Default::default()
            }),
            ..Default::default()
        }
    } else {
        let url = args.url.trim().to_string();
        if url.is_empty() {
            return Err("Enter a download address".into());
        }
        ldm_service::to_new_download(
            BrowserDownload {
                url,
                referer: args.referer,
                cookies: args.cookies,
                user_agent: args.user_agent,
                file_name: args.file_name.clone(),
                description: args.description,
                media: args.media,
                ..Default::default()
            },
            false,
        )
    };
    if let Some(name) = args.file_name.filter(|n| !n.trim().is_empty()) {
        new.file_name = Some(name);
    }
    new.save_dir = args.save_dir.filter(|d| !d.as_os_str().is_empty());
    new.connections = args.connections;
    new.start = args.start;
    state.engine.add(new).await.map_err(err)
}

#[tauri::command]
pub async fn probe(state: S<'_>, url: String) -> R<ProbeInfo> {
    state
        .engine
        .probe(&RequestInfo::new(url.trim()))
        .await
        .map_err(err)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Formats {
    title: String,
    choices: Vec<ldm_media::MediaChoice>,
}

#[tauri::command]
pub async fn media_formats(state: S<'_>, url: String) -> R<Formats> {
    let info = ldm_media::formats(
        &state.tools,
        &RequestInfo::new(url.trim()),
        std::time::Duration::from_secs(90),
    )
    .await
    .map_err(err)?;
    Ok(Formats {
        title: info.title,
        choices: info.choices,
    })
}

#[tauri::command]
pub async fn pause(state: S<'_>, ids: Vec<i64>) -> R<()> {
    for id in ids {
        state.engine.pause(id).await.map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn resume(state: S, ids: Vec<i64>) -> R<()> {
    for id in ids {
        state.engine.start(id).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn pause_all(state: S<'_>) -> R<()> {
    state.engine.pause_all().await;
    Ok(())
}

#[tauri::command]
pub fn resume_all(state: S) -> R<()> {
    for d in state.engine.list() {
        if matches!(
            d.status,
            DownloadStatus::Paused | DownloadStatus::Queued | DownloadStatus::Failed
        ) {
            let _ = state.engine.start(d.id);
        }
    }
    Ok(())
}

#[tauri::command]
pub async fn remove(state: S<'_>, ids: Vec<i64>, delete_files: bool) -> R<()> {
    for id in ids {
        state.engine.remove(id, delete_files).await.map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn set_connections(state: S, id: i64, connections: u8) -> R<u8> {
    state.engine.set_connections(id, connections).map_err(err)
}

#[tauri::command]
pub fn set_speed_limit(state: S, id: i64, bytes_per_sec: u64) -> R<()> {
    state.engine.set_speed_limit(id, bytes_per_sec).map_err(err)
}

#[tauri::command]
pub async fn refresh_address(state: S<'_>, id: i64, url: String) -> R<()> {
    state
        .engine
        .refresh_address(id, RequestInfo::new(url.trim()))
        .await
        .map_err(err)?;
    state.engine.start(id).map_err(err)
}

/// The file on disk (or its folder for a multi-file torrent).
#[tauri::command]
pub fn path_of(state: S, id: i64) -> R<PathBuf> {
    Ok(state.engine.get(id).map_err(err)?.final_path())
}

#[tauri::command]
pub fn get_settings(state: S) -> AppSettings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub async fn save_settings(state: S<'_>, settings: AppSettings) -> R<()> {
    // Reject a broken privacy setup before anything changes.
    privacy::plan(&settings.privacy, cfg!(windows))?;
    let old = state.settings.lock().unwrap().clone();
    settings.save(&state.engine).map_err(err)?;
    settings.apply(&state.engine);
    state.set_ask(settings.ask_for_browser_downloads);
    let restart = old.privacy != settings.privacy || old.seed_ratio != settings.seed_ratio;
    *state.settings.lock().unwrap() = settings;
    if restart {
        state.start_torrents().await;
    }
    Ok(())
}

/// Network adapters, for choosing the VPN's.
#[tauri::command]
pub fn network_adapters() -> Vec<String> {
    privacy::adapters()
}

/// Is the privacy protection in place right now? `None` = OK, otherwise why not.
#[tauri::command]
pub async fn privacy_status(state: S<'_>) -> R<Option<String>> {
    let p = state.settings.lock().unwrap().privacy.clone();
    Ok(match privacy::check(&p).await {
        privacy::Protection::Ok => None,
        privacy::Protection::Blocked(why) => Some(why),
    })
}

#[tauri::command]
pub async fn test_proxy(url: String) -> R<()> {
    privacy::proxy_alive(&url).await
}

/// Opens a finished download with its usual app. Only downloads in the list can be opened.
#[tauri::command]
pub fn open_download(state: S, id: i64) -> R<()> {
    let d = state.engine.get(id).map_err(err)?;
    if d.status != DownloadStatus::Completed {
        return Err("The download isn't finished yet".into());
    }
    tauri_plugin_opener::open_path(d.final_path(), None::<&str>).map_err(err)
}

/// Shows a download (or its part file) in its folder.
#[tauri::command]
pub fn show_in_folder(state: S, id: i64) -> R<()> {
    let d = state.engine.get(id).map_err(err)?;
    let path = [Some(d.final_path()), d.temp_path.clone()]
        .into_iter()
        .flatten()
        .find(|p| p.exists());
    match path {
        Some(p) => tauri_plugin_opener::reveal_item_in_dir(p).map_err(err),
        None => tauri_plugin_opener::open_path(&d.save_dir, None::<&str>).map_err(err),
    }
}

/// The "Developed by Layó" link.
#[tauri::command]
pub fn open_portfolio() -> R<()> {
    tauri_plugin_opener::open_url("https://layothedeveloper.com", None::<&str>).map_err(err)
}
