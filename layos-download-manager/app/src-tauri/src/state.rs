//! Everything the app runs: the engine with its torrent and video runners, the
//! browser bridge, and the link to the window.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use ldm_engine::{DownloadKind, DownloadStatus, Engine, EngineConfig, Event};
use ldm_ipc::{BrowserDownload, Source};
use ldm_media::{MediaRunner, Tools};
use ldm_service::queues::Queues;
use ldm_service::{Frontend, Service};
use ldm_torrent::{TorrentOptions, TorrentRunner};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::settings::{AppSettings, Duplicates};

pub struct AppState {
    pub engine: Engine,
    pub service: Service,
    pub queues: Arc<Queues>,
    pub settings: Mutex<AppSettings>,
    pub tools: Tools,
    pub windows: crate::windows::Windows,
    /// Shared with the browser bridge: show the "add download" window for browser downloads.
    ask: Arc<Mutex<bool>>,
    data_dir: PathBuf,
    torrents: tokio::sync::Mutex<Option<Arc<TorrentRunner>>>,
}

/// Downloads sent by the browser, for the "add download" window.
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserRequest {
    pub items: Vec<BrowserDownload>,
    pub source: Source,
    /// Per item: the download already started in the background (unconfirmed), if any.
    pub ids: Vec<Option<i64>>,
}

struct WindowFrontend {
    app: AppHandle,
    ask: Arc<Mutex<bool>>,
}

impl Frontend for WindowFrontend {
    /// "Download all links" always lets you pick; single downloads follow the setting.
    fn will_ask(&self, source: Source) -> bool {
        source == Source::AllLinks || *self.ask.lock().unwrap()
    }

    fn ask(&self, items: &[BrowserDownload], ids: &[Option<i64>], source: Source) {
        show_window(&self.app);
        let _ = self.app.emit(
            "browser-download",
            BrowserRequest {
                items: items.to_vec(),
                source,
                ids: ids.to_vec(),
            },
        );
    }

    fn duplicate(&self, existing: &ldm_engine::Download, item: &BrowserDownload) -> bool {
        let Some(state) = self.app.try_state::<Arc<AppState>>() else {
            return false;
        };
        let choice = state.settings.lock().unwrap().duplicates;
        match choice {
            Duplicates::Again => false,
            Duplicates::Existing => {
                state.use_existing(&self.app, existing);
                true
            }
            Duplicates::Ask => {
                // The existing download's own window asks what to do.
                state.windows.set_duplicate(existing.id, item.clone());
                let _ = crate::windows::open_download(&self.app, existing.id, true);
                let _ = self.app.emit("duplicate", existing.id);
                true
            }
        }
    }

    fn show(&self) {
        show_window(&self.app);
    }

    fn refreshed(&self, d: &ldm_engine::Download) {
        let _ = self.app.emit(
            "link-refreshed",
            serde_json::json!({ "id": d.id, "fileName": d.file_name }),
        );
    }
}

pub fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.unminimize();
        let _ = w.show();
        let _ = w.set_focus();
    }
}

impl AppState {
    pub async fn start(
        app: AppHandle,
        data_dir: PathBuf,
        default_downloads: PathBuf,
    ) -> anyhow_like::Result<Arc<Self>> {
        std::fs::create_dir_all(&data_dir)?;
        let mut config = EngineConfig::new(&default_downloads);
        config.db_path = Some(data_dir.join("downloads.db"));
        let engine = Engine::open(config)?;
        let settings = AppSettings::load(&engine, default_downloads);
        settings.apply(&engine);
        settings.save(&engine)?;

        let app_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(PathBuf::from));
        let tools = Tools::find(app_dir.as_deref());
        engine.set_runner(
            "media",
            MediaRunner::new(tools.clone(), data_dir.join("media-work")),
        );

        let ask = Arc::new(Mutex::new(settings.ask_for_browser_downloads));
        let frontend = Arc::new(WindowFrontend {
            app: app.clone(),
            ask: ask.clone(),
        });
        let service = Service::new(engine.clone(), frontend).with_media_tools(tools.clone());

        let queues = Queues::start(engine.clone());
        let state = Arc::new(Self {
            engine,
            service,
            queues,
            settings: Mutex::new(settings),
            tools,
            windows: Default::default(),
            ask,
            data_dir,
            torrents: tokio::sync::Mutex::new(None),
        });
        state.start_torrents().await;
        state.forward_events(app.clone());
        state.watch_network(app.clone());
        state.settings.lock().unwrap().ensure_folders(&state.engine);
        // A scheduled queue finished: quit, sleep or shut down if it says so.
        let mut frx = state.queues.subscribe_finished();
        let fapp = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok((queue, action)) = frx.recv().await {
                crate::power::queue_finished(&fapp, &queue, action);
            }
        });
        // Queue changes go to the window too.
        let mut qrx = state.queues.subscribe();
        let qapp = app.clone();
        tauri::async_runtime::spawn(async move {
            while let Ok(list) = qrx.recv().await {
                let _ = qapp.emit("queues", &list);
            }
        });

        // Listen for the browser extension.
        let service = state.service.clone();
        tauri::async_runtime::spawn(async move {
            let path = ldm_ipc::default_path();
            if let Err(e) = ldm_ipc::serve(&path, move |req| {
                let s = service.clone();
                async move { s.handle(req).await }
            })
            .await
            {
                tracing::error!("browser bridge unavailable: {e}");
            }
        });
        Ok(state)
    }

    /// Starts (or restarts, after a settings change) torrent support with the current
    /// privacy settings. Torrents that were running are paused and continued.
    pub async fn start_torrents(&self) {
        let mut slot = self.torrents.lock().await;
        let running: Vec<i64> = self
            .engine
            .list()
            .into_iter()
            .filter(|d| matches!(d.kind, DownloadKind::Torrent(_)) && self.engine.is_running(d.id))
            .map(|d| d.id)
            .collect();
        for id in &running {
            let _ = self.engine.pause(*id).await;
        }
        if let Some(old) = slot.take() {
            old.shutdown().await;
        }
        let s = self.settings.lock().unwrap().clone();
        let mut opts = TorrentOptions::new(self.data_dir.join("torrents"));
        opts.privacy = s.privacy.clone();
        opts.seed_ratio = s.seed_ratio;
        match TorrentRunner::start(opts).await {
            Ok(runner) => {
                self.engine.set_runner("torrent", runner.clone());
                *slot = Some(runner);
                for id in running {
                    let _ = self.engine.start(id);
                }
            }
            Err(e) => tracing::error!("torrents unavailable: {e:#}"),
        }
    }

    /// Sends engine events to the window, and shows a notification when a download finishes.
    fn forward_events(self: &Arc<Self>, app: AppHandle) {
        let mut rx = self.engine.subscribe();
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            use tokio::sync::broadcast::error::RecvError;
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        let _ = app.emit("engine", &ev);
                        match &ev {
                            Event::Added { download } => {
                                me.windows.added(download.id);
                                let s = me.settings.lock().unwrap().clone();
                                me.windows.updated(&app, download, &s);
                            }
                            Event::Updated { download } => {
                                let s = me.settings.lock().unwrap().clone();
                                if download.status == DownloadStatus::Completed
                                    && s.notify_on_complete
                                    && !download.pending
                                {
                                    notify_done(&app, &download.file_name);
                                }
                                me.windows.updated(&app, download, &s);
                            }
                            Event::Removed { id } => me.windows.removed(&app, *id),
                            Event::Progress { .. } => {}
                        }
                    }
                    Err(RecvError::Lagged(_)) => {}
                    Err(RecvError::Closed) => break,
                }
            }
        });
    }

    /// For a link downloaded before: open the finished file, or continue the
    /// unfinished download; either way its window shows.
    pub fn use_existing(&self, app: &AppHandle, d: &ldm_engine::Download) {
        if d.status == DownloadStatus::Completed {
            let _ = tauri_plugin_opener::open_path(d.final_path(), None::<&str>);
        } else if !self.engine.is_running(d.id) {
            let _ = self.engine.start(d.id);
        }
        let _ = crate::windows::open_download(app, d.id, true);
    }

    /// Watches the network. When it goes away, every running download pauses (or
    /// stops, if its server can't continue); nothing restarts by itself when it's back.
    fn watch_network(self: &Arc<Self>, app: AppHandle) {
        let me = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut online = true;
            let mut misses = 0u32;
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                let now = crate::network::online();
                if now {
                    misses = 0;
                    if !online {
                        online = true;
                        let _ = app.emit("network", serde_json::json!({ "online": true }));
                    }
                    continue;
                }
                misses += 1;
                // Two checks in a row, so a quick reconnect doesn't pause everything.
                if online && misses >= 2 {
                    online = false;
                    let paused = me.engine.pause_all_offline().await;
                    let _ = app.emit(
                        "network",
                        serde_json::json!({ "online": false, "paused": paused.len() }),
                    );
                    if !paused.is_empty() {
                        use tauri_plugin_notification::NotificationExt;
                        let n = paused.len();
                        let _ = app
                            .notification()
                            .builder()
                            .title("No internet connection")
                            .body(if n == 1 {
                                "A download was paused. Continue it when you're back online.".to_string()
                            } else {
                                format!("{n} downloads were paused. Continue them when you're back online.")
                            })
                            .show();
                    }
                }
            }
        });
    }

    pub fn set_ask(&self, ask: bool) {
        *self.ask.lock().unwrap() = ask;
    }
}

fn notify_done(app: &AppHandle, name: &str) {
    use tauri_plugin_notification::NotificationExt;
    let _ = app
        .notification()
        .builder()
        .title("Download complete")
        .body(name)
        .show();
}

/// A tiny error type so `?` works across the engine's and std's errors.
pub mod anyhow_like {
    pub type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
}
