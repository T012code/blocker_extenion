//! Turns requests from the browser into downloads.
//!
//! The desktop app and the headless runner both use [`Service`]. The app plugs in a
//! [`Frontend`] that shows the "add download" window and brings itself to the front.

pub mod grabber;
pub mod queues;

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use std::time::Duration;

use ldm_engine::{
    Download, DownloadKind, DownloadStatus, Engine, Event, MediaSpec, NewDownload, RequestInfo,
    TorrentSpec,
};
use ldm_ipc::{BrowserDownload, DownloadSummary, MediaFormatChoice, Request, Response, Source};
use ldm_media::Tools;

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// What the user sees when the browser sends something.
pub trait Frontend: Send + Sync {
    /// Shows the "add download" window for these files and returns true, or returns
    /// false to have them added and started straight away.
    fn ask(&self, _items: &[BrowserDownload], _source: Source) -> bool {
        false
    }
    fn show(&self) {}
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
        }
    }

    /// Where to find yt-dlp and ffmpeg for listing video formats.
    pub fn with_media_tools(mut self, tools: Tools) -> Self {
        self.media_tools = tools;
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
            Request::MediaFormats { item } => self.media_formats(*item).await,
        }
    }

    async fn media_formats(&self, item: BrowserDownload) -> Response {
        if !is_web_url(&item.url) {
            return Response::error("not a web address");
        }
        let req = to_new_download(item, false).request;
        match ldm_media::formats(&self.media_tools, &req, Duration::from_secs(90)).await {
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
                    })
                    .collect(),
            },
            Err(e) => Response::error(e.to_string()),
        }
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
        // Several links always go to the window so the user can pick, when there is one.
        let wants_window = ask.unwrap_or(true) || source == Source::AllLinks;
        if wants_window && self.frontend.ask(&items, source) {
            return Response::Accepted { ids: Vec::new() };
        }
        let mut ids = Vec::new();
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
