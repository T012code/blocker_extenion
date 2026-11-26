//! Turns requests from the browser into downloads.
//!
//! The desktop app and the headless runner both use [`Service`]. The app plugs in a
//! [`Frontend`] that shows the "add download" window and brings itself to the front.

use std::sync::Arc;

use ldm_engine::{Engine, NewDownload, RequestInfo};
use ldm_ipc::{BrowserDownload, Request, Response, Source};

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

#[derive(Clone)]
pub struct Service {
    engine: Engine,
    frontend: Arc<dyn Frontend>,
}

impl Service {
    pub fn new(engine: Engine, frontend: Arc<dyn Frontend>) -> Self {
        Self { engine, frontend }
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
