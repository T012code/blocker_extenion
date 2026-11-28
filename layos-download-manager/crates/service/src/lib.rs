//! Turns requests from the browser into downloads.
//!
//! The desktop app and the headless runner both use [`Service`]. The app plugs in a
//! [`Frontend`] that shows the "add download" window and brings itself to the front.

pub mod grabber;
pub mod queues;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use std::time::{Duration, Instant};

use ldm_engine::{
    Download, DownloadKind, DownloadStatus, Engine, Event, MediaSpec, NewDownload, RequestInfo,
    TorrentSpec,
};
use ldm_ipc::{
    BrowserDownload, DownloadSummary, MediaFormatChoice, MediaPick, PlaylistSummary, Request,
    Response, Source,
};
use ldm_media::Tools;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the user sees when the browser sends something.
pub trait Frontend: Send + Sync {
    /// Whether downloads from the browser go to the "add download" window first.
    /// False: they're added and started straight away.
    fn will_ask(&self, _source: Source) -> bool {
        false
    }
    /// Shows the "add download" window. `ids[i]` is the download already started in
    /// the background for `items[i]` (unconfirmed), or None.
    fn ask(&self, _items: &[BrowserDownload], _ids: &[Option<i64>], _source: Source) {}
    /// A link that was downloaded before came in again. Returns true when the frontend
    /// takes care of it (asks, or opens/continues the existing one); false adds it again.
    fn duplicate(&self, _existing: &Download, _item: &BrowserDownload) -> bool {
        false
    }
    fn show(&self) {}
    /// An expired download got a fresh link from the browser and is continuing.
    fn refreshed(&self, _download: &Download) {}
}

/// How long a download waits for the browser to send its fresh link.
const REFRESH_WAIT: Duration = Duration::from_secs(15 * 60);

/// What `refresh_link` did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refresh {
    /// The old link works again: the download is continuing.
    Resumed,
    /// Waiting for a fresh link: open this page (where the file was found, or else the
    /// link itself) and download the file there again.
    OpenPage(String),
}

/// No window: everything is added and started (headless runner, tests).
pub struct NoFrontend;
impl Frontend for NoFrontend {}

/// Most downloads listed for the browser at once.
const LIST_LIMIT: usize = 40;

type Live = Arc<Mutex<HashMap<i64, (u64, u64)>>>;

#[derive(Clone)]
pub struct Service {
    engine: Engine,
    frontend: Arc<dyn Frontend>,
    /// Latest (bytes, speed) per running download, from engine events.
    live: Live,
    /// yt-dlp and ffmpeg, for the video button's format list.
    media_tools: Tools,
    /// Expired downloads waiting for a fresh link from the browser, until when.
    waiting: Arc<Mutex<HashMap<i64, Instant>>>,
    /// The app's queues: a whole playlist gets one.
    queues: Option<Arc<queues::Queues>>,
}

impl Service {
    /// Must be called inside a Tokio runtime: it follows the engine's progress events.
    pub fn new(engine: Engine, frontend: Arc<dyn Frontend>) -> Self {
        let live: Live = Arc::default();
        let mut events = engine.subscribe();
        let l = live.clone();
        let eng = engine.clone();
        tokio::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match events.recv().await {
                    Ok(Event::Progress {
                        id,
                        downloaded,
                        speed,
                        ..
                    }) => {
                        l.lock().unwrap().insert(id, (downloaded, speed));
                    }
                    Ok(Event::Updated { download }) if !download.status.is_active() => {
                        l.lock().unwrap().remove(&download.id);
                        open_downloaded_torrent(&eng, &download).await;
                    }
                    Ok(Event::Removed { id }) => {
                        l.lock().unwrap().remove(&id);
                    }
                    Ok(_) | Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
        Self {
            engine,
            frontend,
            live,
            media_tools: Tools::default(),
            waiting: Arc::default(),
            queues: None,
        }
    }

    /// Where to find yt-dlp and ffmpeg for listing video formats.
    pub fn with_media_tools(mut self, tools: Tools) -> Self {
        self.media_tools = tools;
        self
    }

    /// The app's queues, for whole playlists (each gets a queue of its own).
    pub fn with_queues(mut self, queues: Arc<queues::Queues>) -> Self {
        self.queues = Some(queues);
        self
    }

    /// Unfinished downloads first, then finished ones; newest first in each group.
    pub fn summaries(&self) -> Vec<DownloadSummary> {
        let live = self.live.lock().unwrap().clone();
        let mut list = self.engine.list();
        list.sort_by_key(|d| {
            (
                d.status == DownloadStatus::Completed,
                std::cmp::Reverse(d.id),
            )
        });
        list.into_iter()
            .filter(|d| !d.pending)
            .take(LIST_LIMIT)
            .map(|d| summary(&d, live.get(&d.id).copied()))
            .collect()
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    pub async fn handle(&self, req: Request) -> Response {
        match req {
            Request::Ping => Response::Pong {
                app_running: true,
                app_version: Some(APP_VERSION.into()),
                host_version: None,
            },
            Request::ShowApp => {
                self.frontend.show();
                Response::Ok
            }
            Request::Download { items, source, ask } => self.download(items, source, ask).await,
            Request::List => Response::Downloads {
                app_running: true,
                items: self.summaries(),
            },
            Request::Pause { id } => match self.engine.pause(id).await {
                Ok(()) => Response::Ok,
                Err(e) => Response::error(e.to_string()),
            },
            Request::Resume { id } => match self.engine.start(id) {
                Ok(()) => Response::Ok,
                Err(e) => Response::error(e.to_string()),
            },
            Request::MediaFormats { item, playlist_url } => {
                self.media_formats(*item, playlist_url).await
            }
            Request::RefreshLink { id } => match self.refresh_link(id).await {
                Ok(Refresh::Resumed) => Response::Refresh {
                    resumed: true,
                    page_url: None,
                },
                Ok(Refresh::OpenPage(url)) => Response::Refresh {
                    resumed: false,
                    page_url: Some(url),
                },
                Err(e) => Response::error(e),
            },
        }
    }

    /// Gets a download whose link expired going again. First the same link is tried
    /// (some servers refuse only for a while); if it still fails, the download waits for
    /// the browser to send the same file again, and takes over that fresh link, keeping
    /// what was already downloaded.
    pub async fn refresh_link(&self, id: i64) -> Result<Refresh, String> {
        let d = self.engine.get(id).map_err(|e| e.to_string())?;
        if !matches!(d.kind, DownloadKind::Http) {
            return Err("only file downloads have a link to refresh".into());
        }
        if d.status == DownloadStatus::Completed || self.engine.is_running(id) {
            return Err("this download doesn't need a new link".into());
        }
        if let Ok(info) = self.engine.probe(&d.request).await
            && (d.size.is_none() || info.size.is_none() || info.size == d.size)
        {
            self.engine
                .refresh_address(id, d.request.clone())
                .await
                .map_err(|e| e.to_string())?;
            self.engine.start(id).map_err(|e| e.to_string())?;
            self.waiting.lock().unwrap().remove(&id);
            return Ok(Refresh::Resumed);
        }
        self.waiting
            .lock()
            .unwrap()
            .insert(id, Instant::now() + REFRESH_WAIT);
        let page = d
            .request
            .referer
            .clone()
            .filter(|r| is_web_url(r))
            .unwrap_or_else(|| d.request.url.clone());
        Ok(Refresh::OpenPage(page))
    }

    /// A download already in the list for this link (same address, or the same torrent),
    /// if any. Downloads waiting for confirmation don't count.
    pub fn find_duplicate(&self, url: &str) -> Option<Download> {
        let hash = info_hash(url);
        let mut list = self.engine.list();
        list.sort_by_key(|d| std::cmp::Reverse(d.id));
        list.into_iter().filter(|d| !d.pending).find(|d| {
            same_link(url, &d.request.url)
                || d.final_url.as_deref().is_some_and(|f| same_link(url, f))
                || (hash.is_some() && hash == info_hash(&d.request.url))
        })
    }

    /// Stops waiting for a fresh link for this download.
    pub fn cancel_refresh(&self, id: i64) {
        self.waiting.lock().unwrap().remove(&id);
    }

    /// Ids of downloads waiting for a fresh link.
    pub fn waiting_for_link(&self) -> Vec<i64> {
        let now = Instant::now();
        let mut w = self.waiting.lock().unwrap();
        w.retain(|_, until| *until > now);
        w.keys().copied().collect()
    }

    /// If the browser sent a file that an expired download is waiting for, gives that
    /// download the fresh link and continues it. A download waiting after "Refresh link"
    /// matches by name or by size; any other expired download only when both match.
    async fn take_fresh_link(&self, item: &BrowserDownload) -> Option<i64> {
        if item.media.is_some() || !is_web_url(&item.url) {
            return None;
        }
        let waiting = self.waiting_for_link();
        let names = candidate_names(item);
        let found = self.engine.list().into_iter().find(|d| {
            if !matches!(d.kind, DownloadKind::Http) || d.pending || self.engine.is_running(d.id) {
                return false;
            }
            let name = names.iter().any(|n| n.eq_ignore_ascii_case(&d.file_name));
            let size = d.size.is_some() && item.file_size == d.size;
            if waiting.contains(&d.id) {
                matches!(
                    d.status,
                    DownloadStatus::NeedsRefresh | DownloadStatus::Failed | DownloadStatus::Paused
                ) && (name || size)
            } else {
                d.status == DownloadStatus::NeedsRefresh && name && size
            }
        })?;
        let non_empty = |s: &Option<String>| s.clone().filter(|s| !s.trim().is_empty());
        let request = RequestInfo {
            url: item.url.clone(),
            referer: non_empty(&item.referer).or(found.request.referer.clone()),
            cookies: non_empty(&item.cookies),
            user_agent: non_empty(&item.user_agent).or(found.request.user_agent.clone()),
            headers: Vec::new(),
        };
        if let Err(e) = self.engine.refresh_address(found.id, request).await {
            tracing::warn!("could not use the fresh link for {}: {e}", found.file_name);
            return None;
        }
        self.waiting.lock().unwrap().remove(&found.id);
        if let Err(e) = self.engine.start(found.id) {
            tracing::warn!("could not continue {}: {e}", found.file_name);
        }
        self.frontend.refreshed(&found);
        Some(found.id)
    }

    async fn media_formats(&self, item: BrowserDownload, playlist_url: Option<String>) -> Response {
        if !is_web_url(&item.url) {
            return Response::error("not a web address");
        }
        let req = to_new_download(item, false).request;
        // The page's playlist (if it plays one) is listed alongside: both take a moment.
        let list_req = playlist_url
            .as_deref()
            .and_then(ldm_media::playlist_url)
            .map(|url| RequestInfo { url, ..req.clone() });
        let (formats, playlist) = tokio::join!(
            ldm_media::formats(&self.media_tools, &req, Duration::from_secs(90)),
            async {
                match &list_req {
                    Some(r) => ldm_media::playlist(&self.media_tools, r, Duration::from_secs(60))
                        .await
                        .ok(),
                    None => None,
                }
            }
        );
        let playlist = playlist
            .zip(list_req)
            .filter(|(p, _)| p.entries.len() > 1)
            .map(|(p, r)| PlaylistSummary {
                url: r.url,
                title: p.title,
                count: p.entries.len(),
                choices: ldm_media::PLAYLIST_QUALITIES
                    .iter()
                    .map(|(label, format, audio)| MediaFormatChoice {
                        format: format.to_string(),
                        label: label.to_string(),
                        kind: if *audio { "audio" } else { "video" }.to_string(),
                        ext: if *audio { "m4a" } else { "mp4" }.to_string(),
                        height: None,
                        size: None,
                        item: None,
                    })
                    .collect(),
            });
        match formats {
            Ok(info) => Response::MediaFormats {
                title: info.title,
                duration: info.duration,
                thumbnail: info.thumbnail,
                is_live: info.is_live,
                choices: info
                    .choices
                    .into_iter()
                    .map(|c| MediaFormatChoice {
                        format: c.format,
                        label: c.label,
                        kind: c.kind,
                        ext: c.ext,
                        height: c.height,
                        size: c.size,
                        item: c.item,
                    })
                    .collect(),
                playlist,
            },
            // The video itself failed, but its playlist can still be downloaded.
            Err(_) if playlist.is_some() => Response::MediaFormats {
                title: playlist
                    .as_ref()
                    .map(|p| p.title.clone())
                    .unwrap_or_default(),
                duration: None,
                thumbnail: None,
                is_live: false,
                choices: Vec::new(),
                playlist,
            },
            Err(e) => Response::error(e.to_string()),
        }
    }

    /// "Whole playlist": each video becomes a download of its own, in a new queue
    /// named after the playlist that downloads two at a time.
    async fn download_playlist(&self, item: BrowserDownload) -> Result<Vec<i64>, String> {
        let Some(pick) = item.media.clone() else {
            return Err("not a playlist".into());
        };
        let mut req = to_new_download(item.clone(), false).request;
        req.url = pick.page_url.clone();
        let list = ldm_media::playlist(&self.media_tools, &req, Duration::from_secs(120))
            .await
            .map_err(|e| e.to_string())?;
        if list.entries.is_empty() {
            return Err("This playlist has no videos that can be downloaded.".into());
        }
        let queue = self.queues.as_ref().map(|q| q.create(&list.title, 2));
        let mut ids = Vec::with_capacity(list.entries.len());
        for entry in &list.entries {
            let mut one = item.clone();
            one.url = entry.url.clone();
            one.file_name = None;
            one.media = Some(MediaPick {
                page_url: entry.url.clone(),
                format: pick.format.clone(),
                label: pick.label.clone(),
                title: entry.title.clone(),
                item: None,
                playlist: false,
            });
            let mut new = to_new_download(one, queue.is_none());
            new.queue_id = queue.as_ref().map(|q| q.id);
            match self.engine.add(new).await {
                Ok(id) => ids.push(id),
                Err(e) => tracing::warn!("couldn't add {}: {e}", entry.url),
            }
        }
        if let (Some(queues), Some(q)) = (&self.queues, &queue) {
            let _ = queues.start_queue(q.id);
        }
        Ok(ids)
    }

    async fn download(
        &self,
        items: Vec<BrowserDownload>,
        source: Source,
        ask: Option<bool>,
    ) -> Response {
        let items: Vec<_> = items
            .into_iter()
            .filter(|i| is_web_url(&i.url) || is_magnet(&i.url) || torrent_file(&i.url).is_some())
            .collect();
        if items.is_empty() {
            return Response::error("no links to download");
        }
        // A whole playlist: one download per video, in their own queue.
        let (playlists, items): (Vec<_>, Vec<_>) = items
            .into_iter()
            .partition(|i| i.media.as_ref().is_some_and(|m| m.playlist));
        if !playlists.is_empty() {
            let mut ids = Vec::new();
            for p in playlists {
                match self.download_playlist(p).await {
                    Ok(mut added) => ids.append(&mut added),
                    Err(e) if items.is_empty() && ids.is_empty() => return Response::error(e),
                    Err(e) => tracing::warn!("playlist: {e}"),
                }
            }
            if items.is_empty() {
                return Response::Accepted { ids };
            }
        }
        // Fresh links for expired downloads continue those instead of adding new ones.
        let mut refreshed = Vec::new();
        let mut rest = Vec::new();
        for item in items {
            match self.take_fresh_link(&item).await {
                Some(id) => refreshed.push(id),
                None => rest.push(item),
            }
        }
        if rest.is_empty() {
            return Response::Accepted { ids: refreshed };
        }
        let items = rest;
        // Downloaded before? (One link at a time, as IDM asks.)
        if let [item] = items.as_slice()
            && source != Source::AllLinks
            && let Some(existing) = self.find_duplicate(&item.url)
            && self.frontend.duplicate(&existing, item)
        {
            refreshed.push(existing.id);
            return Response::Accepted { ids: refreshed };
        }
        // Several links always go to the window so the user can pick, when there is one.
        let wants_window = ask.unwrap_or(true) || source == Source::AllLinks;
        if wants_window && self.frontend.will_ask(source) {
            // Like IDM: a file starts downloading while the window waits for an answer.
            let background = items.len() <= 3 && source != Source::AllLinks;
            let mut ids = Vec::with_capacity(items.len());
            for item in &items {
                let id = if background && is_plain_file(item) {
                    let mut new = to_new_download(item.clone(), true);
                    new.pending = true;
                    self.engine.add(new).await.ok()
                } else {
                    None
                };
                ids.push(id);
            }
            self.frontend.ask(&items, &ids, source);
            return Response::Accepted { ids: Vec::new() };
        }
        let mut ids = refreshed;
        let mut last_error = None;
        for item in items {
            match self.engine.add(to_new_download(item, true)).await {
                Ok(id) => ids.push(id),
                Err(e) => last_error = Some(e.to_string()),
            }
        }
        match (ids.is_empty(), last_error) {
            (true, Some(e)) => Response::error(e),
            _ => Response::Accepted { ids },
        }
    }
}

/// The names the browser's file could have: what the browser says, and the link's
/// last path segment (decoded).
fn candidate_names(item: &BrowserDownload) -> Vec<String> {
    let mut names = Vec::new();
    if let Some(n) = item
        .file_name
        .as_deref()
        .map(str::trim)
        .filter(|n| !n.is_empty())
    {
        names.push(n.to_string());
    }
    if let Ok(u) = reqwest::Url::parse(&item.url)
        && let Some(last) = u.path_segments().and_then(|mut s| s.next_back())
        && !last.is_empty()
    {
        names.push(percent_decode(last));
    }
    names
}

fn percent_decode(s: &str) -> String {
    let hex = |c: u8| (c as char).to_digit(16).map(|v| v as u8);
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        match (
            b[i],
            b.get(i + 1).and_then(|&c| hex(c)),
            b.get(i + 2).and_then(|&c| hex(c)),
        ) {
            (b'%', Some(h), Some(l)) => {
                out.push(h * 16 + l);
                i += 3;
            }
            (c, _, _) => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// A file from a web link (not a video format, a magnet link or a .torrent on disk).
fn is_plain_file(item: &BrowserDownload) -> bool {
    is_web_url(&item.url) && item.media.is_none()
}

/// The same address, ignoring what follows "#".
fn same_link(a: &str, b: &str) -> bool {
    let cut = |u: &str| u.split('#').next().unwrap_or(u).trim().to_string();
    !a.trim().is_empty() && cut(a) == cut(b)
}

/// A magnet link's info hash, lowercased.
fn info_hash(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let at = lower.find("xt=urn:btih:")? + "xt=urn:btih:".len();
    Some(lower[at..].split('&').next()?.to_string())
}

fn is_web_url(url: &str) -> bool {
    let lower = url.trim_start().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

fn is_magnet(url: &str) -> bool {
    url.trim_start()
        .to_ascii_lowercase()
        .starts_with("magnet:?")
}

/// A .torrent file on this computer, as `file://…/name.torrent` (opened from Windows).
fn torrent_file(url: &str) -> Option<std::path::PathBuf> {
    let path = url.strip_prefix("file://")?;
    path.to_ascii_lowercase()
        .ends_with(".torrent")
        .then(|| std::path::PathBuf::from(path))
}

/// A .torrent file finished downloading: start the torrent it describes.
async fn open_downloaded_torrent(engine: &Engine, d: &Download) {
    let is_torrent_file = matches!(d.kind, DownloadKind::Http)
        && d.status == DownloadStatus::Completed
        && (d.file_name.to_ascii_lowercase().ends_with(".torrent")
            || d.content_type
                .as_deref()
                .is_some_and(|t| t.contains("bittorrent")));
    if !is_torrent_file
        || engine
            .load_setting("open_torrent_files")
            .ok()
            .flatten()
            .as_deref()
            == Some("false")
    {
        return;
    }
    let path = d.final_path();
    let new = NewDownload {
        request: RequestInfo::new(format!("file://{}", path.display())),
        kind: DownloadKind::Torrent(TorrentSpec {
            torrent_file: Some(path),
            ..Default::default()
        }),
        start: true,
        ..Default::default()
    };
    if let Err(e) = engine.add(new).await {
        tracing::warn!("could not open {}: {e}", d.file_name);
    }
}

/// Maps what the browser sent to an engine download.
pub fn to_new_download(item: BrowserDownload, start: bool) -> NewDownload {
    let non_empty = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    let kind = if let Some(path) = torrent_file(&item.url) {
        DownloadKind::Torrent(TorrentSpec {
            torrent_file: Some(path),
            ..Default::default()
        })
    } else if is_magnet(&item.url) {
        DownloadKind::Torrent(TorrentSpec {
            magnet: Some(item.url.clone()),
            ..Default::default()
        })
    } else if let Some(m) = &item.media {
        DownloadKind::Media(MediaSpec {
            page_url: m.page_url.clone(),
            format: m.format.clone(),
            label: m.label.clone(),
            title: m.title.clone(),
            item: m.item,
        })
    } else {
        DownloadKind::Http
    };
    NewDownload {
        // For a video the title comes from the site; otherwise the browser already
        // worked out the name from the server's headers.
        file_name: if item.media.is_some() {
            None
        } else {
            non_empty(item.file_name)
        },
        description: non_empty(item.description),
        request: RequestInfo {
            url: item.url,
            referer: non_empty(item.referer),
            cookies: non_empty(item.cookies),
            user_agent: non_empty(item.user_agent),
            headers: Vec::new(),
        },
        kind,
        start,
        ..Default::default()
    }
}

fn status_name(s: DownloadStatus) -> &'static str {
    match s {
        DownloadStatus::Queued => "queued",
        DownloadStatus::Connecting => "connecting",
        DownloadStatus::Downloading => "downloading",
        DownloadStatus::Paused => "paused",
        DownloadStatus::Completed => "completed",
        DownloadStatus::Failed => "failed",
        DownloadStatus::NeedsRefresh => "needsRefresh",
        DownloadStatus::Blocked => "blocked",
        DownloadStatus::Seeding => "seeding",
    }
}

fn summary(d: &Download, live: Option<(u64, u64)>) -> DownloadSummary {
    let (downloaded, speed) = match live {
        Some((bytes, speed)) if d.status.is_active() => (bytes, speed),
        _ => (d.downloaded, 0),
    };
    DownloadSummary {
        id: d.id,
        file_name: d.file_name.clone(),
        status: status_name(d.status).into(),
        size: d.size,
        downloaded,
        speed,
        resumable: d.resumable,
        connections: d.connections,
        error: d.error.clone(),
        kind: match d.kind {
            DownloadKind::Http => "file",
            DownloadKind::Media(_) => "video",
            DownloadKind::Torrent(_) => "torrent",
        }
        .into(),
    }
}
