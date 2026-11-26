//! Turns requests from the browser into downloads.
//!
//! The desktop app and the headless runner both use [`Service`]. The app plugs in a
//! [`Frontend`] that shows the "add download" window and brings itself to the front.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use ldm_engine::{Download, DownloadStatus, Engine, Event, NewDownload, RequestInfo};
use ldm_ipc::{BrowserDownload, DownloadSummary, Request, Response, Source};

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
}

impl Service {
    /// Must be called inside a Tokio runtime: it follows the engine's progress events.
    pub fn new(engine: Engine, frontend: Arc<dyn Frontend>) -> Self {
        let live: Live = Arc::default();
        let mut events = engine.subscribe();
        let l = live.clone();
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
        }
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
        }
    }

    async fn download(
        &self,
        items: Vec<BrowserDownload>,
        source: Source,
        ask: Option<bool>,
    ) -> Response {
        let items: Vec<_> = items.into_iter().filter(|i| is_web_url(&i.url)).collect();
        if items.is_empty() {
            return Response::error("no http or https links to download");
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

/// Maps what the browser sent to an engine download.
pub fn to_new_download(item: BrowserDownload, start: bool) -> NewDownload {
    let non_empty = |s: Option<String>| s.map(|s| s.trim().to_string()).filter(|s| !s.is_empty());
    NewDownload {
        request: RequestInfo {
            url: item.url,
            referer: non_empty(item.referer),
            cookies: non_empty(item.cookies),
            user_agent: non_empty(item.user_agent),
            headers: Vec::new(),
        },
        // The browser already worked out the name from the server's headers.
        file_name: non_empty(item.file_name),
        description: non_empty(item.description),
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
    }
}
