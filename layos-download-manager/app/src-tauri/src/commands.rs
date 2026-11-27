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
    /// Put it in this queue (it then starts when the queue gets to it).
    #[serde(default)]
    queue_id: Option<i64>,
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
    new.start = args.start && args.queue_id.is_none();
    let id = state.engine.add(new).await.map_err(err)?;
    if let Some(q) = args.queue_id {
        state.queues.assign(&[id], Some(q))?;
    }
    Ok(id)
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

/// A new link pasted for a download: it continues from where it stopped (or starts
/// over, if the file behind the new link is a different size). The old page and
/// browser stay; its cookies are kept only for the same site.
#[tauri::command]
pub async fn refresh_address(state: S<'_>, id: i64, url: String) -> R<()> {
    let old = state.engine.get(id).map_err(err)?.request;
    let url = url.trim().to_string();
    let host = |u: &str| reqwest_host(u);
    let same_site = host(&url).is_some() && host(&url) == host(&old.url);
    let request = RequestInfo {
        cookies: if same_site { old.cookies } else { None },
        headers: if same_site { old.headers } else { Vec::new() },
        url,
        referer: old.referer,
        user_agent: old.user_agent,
    };
    state
        .engine
        .refresh_address(id, request)
        .await
        .map_err(err)?;
    state.service.cancel_refresh(id);
    state.engine.start(id).map_err(err)
}

fn reqwest_host(u: &str) -> Option<String> {
    let rest = u.split_once("://")?.1;
    let host = rest.split(['/', '?', '#']).next()?;
    Some(host.rsplit('@').next()?.to_ascii_lowercase())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RefreshResult {
    /// The old link works again; the download is continuing.
    resumed: bool,
    /// Otherwise the page opened in the browser, to download the file again from.
    page_url: Option<String>,
}

/// "Refresh link": try the same link again; if the server still refuses it, open the
/// page the file came from (or the link itself) in the browser and wait for the file
/// to be downloaded there again, then continue this download with that fresh link.
#[tauri::command]
pub async fn refresh_link(state: S<'_>, id: i64, open: bool) -> R<RefreshResult> {
    match state.service.refresh_link(id).await? {
        ldm_service::Refresh::Resumed => Ok(RefreshResult {
            resumed: true,
            page_url: None,
        }),
        ldm_service::Refresh::OpenPage(page) => {
            if open {
                tauri_plugin_opener::open_url(&page, None::<&str>).map_err(err)?;
            }
            Ok(RefreshResult {
                resumed: false,
                page_url: Some(page),
            })
        }
    }
}

/// Opens a page in the browser (for "Open the page" again in the refresh dialog).
#[tauri::command]
pub fn open_page(url: String) -> R<()> {
    if !(url.starts_with("https://") || url.starts_with("http://")) {
        return Err("only web pages can be opened".into());
    }
    tauri_plugin_opener::open_url(&url, None::<&str>).map_err(err)
}

/// Stop waiting for a fresh link (the refresh dialog was closed).
#[tauri::command]
pub fn cancel_refresh(state: S, id: i64) {
    state.service.cancel_refresh(id);
}

/// A download's window asks the main window to refresh its link.
#[tauri::command]
pub fn request_refresh(app: tauri::AppHandle, id: i64) -> R<()> {
    use tauri::Emitter;
    crate::state::show_window(&app);
    app.emit("refresh-request", id).map_err(err)
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
pub async fn save_settings(app: tauri::AppHandle, state: S<'_>, settings: AppSettings) -> R<()> {
    // Reject a broken privacy setup before anything changes.
    privacy::plan(&settings.privacy, cfg!(windows))?;
    let old = state.settings.lock().unwrap().clone();
    settings.save(&state.engine).map_err(err)?;
    settings.apply(&state.engine);
    settings.ensure_folders(&state.engine);
    state.set_ask(settings.ask_for_browser_downloads);
    if old.handle_torrent_links != settings.handle_torrent_links {
        crate::associations::apply(settings.handle_torrent_links);
    }
    if old.launch_on_startup != settings.launch_on_startup {
        crate::apply_autostart(&app, settings.launch_on_startup);
    }
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

// ---------------------------------------------------------------- queues

use ldm_service::queues::Queue;

#[tauri::command]
pub fn queues_list(state: S) -> Vec<Queue> {
    state.queues.list()
}

#[tauri::command]
pub fn queue_create(state: S, name: String, max_active: u8) -> Queue {
    state.queues.create(&name, max_active)
}

#[tauri::command]
pub fn queue_update(state: S, id: i64, name: Option<String>, max_active: Option<u8>) -> R<Queue> {
    state.queues.update(id, name.as_deref(), max_active)
}

#[tauri::command]
pub fn queue_start(state: S, id: i64) -> R<()> {
    state.queues.start_queue(id)
}

#[tauri::command]
pub async fn queue_stop(state: S<'_>, id: i64) -> R<()> {
    state.queues.stop_queue(id).await
}

/// Deletes a queue; its downloads stay in the list unless `remove_downloads`.
#[tauri::command]
pub async fn queue_delete(
    state: S<'_>,
    id: i64,
    remove_downloads: bool,
    delete_files: bool,
) -> R<()> {
    state
        .queues
        .delete_queue(id, remove_downloads, delete_files)
        .await
}

/// Moves downloads into a queue, or out of any queue with `None`.
#[tauri::command]
pub fn set_queue(state: S, ids: Vec<i64>, queue_id: Option<i64>) -> R<()> {
    state.queues.assign(&ids, queue_id)
}

// ---------------------------------------------------------------- scheduler and grabber

use ldm_service::grabber::{GrabOptions, GrabResult};
use ldm_service::queues::Schedule;

/// Sets (or clears, with `None`) when a queue starts and stops by itself.
#[tauri::command]
pub fn queue_schedule(state: S, id: i64, schedule: Option<Schedule>) -> R<Queue> {
    state.queues.set_schedule(id, schedule)
}

/// Lists the files a website links to (nothing is downloaded yet).
#[tauri::command]
pub async fn grab_site(options: GrabOptions) -> R<GrabResult> {
    let client = ldm_engine::http::build_client().map_err(err)?;
    ldm_service::grabber::grab(&client, &options).await
}

/// Cancels a shutdown a finished queue asked for.
#[tauri::command]
pub fn cancel_shutdown() -> R<()> {
    crate::power::cancel_shutdown()
}

// ---------- download windows and the mini window

#[tauri::command]
pub fn get(state: S, id: i64) -> R<Download> {
    state.engine.get(id).map_err(err)
}

/// Opens a download's own window (from the list, its menu or the mini window).
#[tauri::command]
pub async fn open_download_window(app: tauri::AppHandle, id: i64) -> R<()> {
    crate::windows::open_download(&app, id, true).map_err(err)
}

#[tauri::command]
pub async fn show_mini(app: tauri::AppHandle, state: S<'_>) -> R<()> {
    state.windows.set_mini_dismissed(false);
    crate::windows::show_mini(&app).map_err(err)
}

/// The mini window's close button: hide it until it's asked for again.
#[tauri::command]
pub fn mini_dismiss(state: S, window: tauri::WebviewWindow) -> R<()> {
    state.windows.set_mini_dismissed(true);
    window.hide().map_err(err)
}

/// The mini window hides itself when nothing is downloading.
#[tauri::command]
pub fn window_hide(window: tauri::WebviewWindow) -> R<()> {
    window.hide().map_err(err)
}

#[tauri::command]
pub fn window_close(window: tauri::WebviewWindow) -> R<()> {
    window.close().map_err(err)
}

/// A small window switching between its full and one-line forms.
#[tauri::command]
pub fn window_size(window: tauri::WebviewWindow, width: f64, height: f64) -> R<()> {
    let (w, h) = (width.clamp(240.0, 720.0), height.clamp(48.0, 720.0));
    crate::windows::resize(&window, w, h).map_err(err)
}

#[tauri::command]
pub fn window_title(window: tauri::WebviewWindow, title: String) -> R<()> {
    window.set_title(&title).map_err(err)
}

#[tauri::command]
pub fn show_app(app: tauri::AppHandle) {
    crate::state::show_window(&app);
}

/// The Exit button (and Ctrl+Q): closes the app for real.
#[tauri::command]
pub async fn exit_app(app: tauri::AppHandle) {
    crate::exit(&app).await;
}

// ---------- downloads that start before you confirm them (as in IDM)

/// The "add download" window checked a link: start it in the background, unconfirmed,
/// so it's already downloading by the time you press Start.
#[tauri::command]
pub async fn start_background(state: S<'_>, url: String) -> R<i64> {
    let url = url.trim().to_string();
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("only web links start early".into());
    }
    let mut new = ldm_service::to_new_download(
        BrowserDownload {
            url,
            ..Default::default()
        },
        true,
    );
    new.pending = true;
    state.engine.add(new).await.map_err(err)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmArgs {
    #[serde(default)]
    file_name: Option<String>,
    #[serde(default)]
    save_dir: Option<PathBuf>,
    #[serde(default)]
    connections: Option<u8>,
    #[serde(default)]
    queue_id: Option<i64>,
    /// Start now; otherwise it waits (paused, keeping what it has, or in its queue).
    #[serde(default)]
    start: bool,
}

/// You confirmed a download that started in the background: it moves to the place and
/// name you chose (when it finishes), shows in the list, and opens its window.
#[tauri::command]
pub async fn confirm_download(state: S<'_>, id: i64, args: ConfirmArgs) -> R<()> {
    let d = state.engine.get(id).map_err(err)?;
    let name = args
        .file_name
        .map(|n| n.trim().to_string())
        .filter(|n| !n.is_empty() && *n != d.file_name);
    let dir = args.save_dir.filter(|p| !p.as_os_str().is_empty());
    if name.is_some() || dir.is_some() {
        state.engine.set_target(id, dir, name).await.map_err(err)?;
    }
    if let Some(n) = args.connections.filter(|n| *n != d.connections) {
        state.engine.set_connections(id, n).map_err(err)?;
    }
    if args.queue_id.is_some() || !args.start {
        state.engine.pause(id).await.map_err(err)?;
    }
    state.engine.confirm(id).map_err(err)?;
    if let Some(q) = args.queue_id {
        state.engine.requeue(id).map_err(err)?;
        state.queues.assign(&[id], Some(q))?;
    }
    Ok(())
}

/// The window was cancelled: the background download and its part file go away.
#[tauri::command]
pub async fn discard_download(state: S<'_>, id: i64) -> R<()> {
    match state.engine.get(id) {
        Ok(d) if d.pending => state.engine.remove(id, true).await.map_err(err),
        _ => Ok(()),
    }
}

// ---------- links downloaded before

/// A download already in the list for this link, if any.
#[tauri::command]
pub fn find_duplicate(state: S, url: String) -> Option<Download> {
    state.service.find_duplicate(url.trim())
}

/// The link that came in again for this download, while its window asks what to do.
#[tauri::command]
pub fn get_duplicate(state: S, id: i64) -> Option<BrowserDownload> {
    state.windows.duplicate(id)
}

/// The answer: "again" adds it again under a numbered name, "existing" opens the
/// finished file or continues the download, "dismiss" does nothing. `remember` makes
/// it the answer from now on (Settings → Downloads).
#[tauri::command]
pub async fn resolve_duplicate(
    app: tauri::AppHandle,
    state: S<'_>,
    id: i64,
    choice: String,
    remember: bool,
) -> R<Option<i64>> {
    let item = state.windows.take_duplicate(id);
    let mut added = None;
    match choice.as_str() {
        "again" => {
            let item = match item {
                Some(i) => i,
                None => {
                    let d = state.engine.get(id).map_err(err)?;
                    BrowserDownload {
                        url: d.request.url,
                        referer: d.request.referer,
                        cookies: d.request.cookies,
                        user_agent: d.request.user_agent,
                        ..Default::default()
                    }
                }
            };
            added = Some(
                state
                    .engine
                    .add(ldm_service::to_new_download(item, true))
                    .await
                    .map_err(err)?,
            );
        }
        "existing" => {
            let d = state.engine.get(id).map_err(err)?;
            state.use_existing(&app, &d);
        }
        _ => {}
    }
    if remember && choice != "dismiss" {
        let mut s = state.settings.lock().unwrap().clone();
        s.duplicates = if choice == "again" {
            crate::settings::Duplicates::Again
        } else {
            crate::settings::Duplicates::Existing
        };
        s.save(&state.engine).map_err(err)?;
        *state.settings.lock().unwrap() = s;
    }
    Ok(added)
}
