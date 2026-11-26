//! The public face of the engine: add, start, pause, remove and watch downloads.

use std::collections::{BTreeMap, HashMap};
use std::fs::OpenOptions;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use reqwest::Client;
use tokio::sync::{broadcast, watch};
use tokio_util::sync::CancellationToken;

use crate::category::Category;
use crate::error::{Error, Result};
use crate::filename;
use crate::http;
use crate::limiter::Limiter;
use crate::model::{
    Download, DownloadId, DownloadStatus, Event, NewDownload, ProbeInfo, RequestInfo, Segment,
    UNKNOWN_END,
};
use crate::store::Store;
use crate::transfer::{self, Progress, Transfer, TransferOptions};

/// Most sections (parallel connections) one download may use.
pub const MAX_CONNECTIONS: u8 = 64;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// Database file. `None` keeps everything in memory (tests).
    pub db_path: Option<PathBuf>,
    /// Main download folder.
    pub download_dir: PathBuf,
    /// Save into `<download_dir>/<Category>` instead of straight into `download_dir`.
    pub category_folders: bool,
    pub default_connections: u8,
    pub min_segment_size: u64,
    pub max_retries: u32,
    pub retry_delay: Duration,
    /// Bytes per second across all downloads, 0 = unlimited.
    pub global_speed_limit: u64,
    pub progress_interval: Duration,
}

impl EngineConfig {
    pub fn new(download_dir: impl Into<PathBuf>) -> Self {
        Self {
            db_path: None,
            download_dir: download_dir.into(),
            category_folders: true,
            default_connections: 8,
            min_segment_size: 1024 * 1024,
            max_retries: 8,
            retry_delay: Duration::from_secs(1),
            global_speed_limit: 0,
            progress_interval: Duration::from_millis(500),
        }
    }

    fn dir_for(&self, category: Category) -> PathBuf {
        if self.category_folders {
            self.download_dir.join(category.folder_name())
        } else {
            self.download_dir.clone()
        }
    }
}

#[derive(Clone)]
pub struct Engine {
    inner: Arc<Inner>,
}

struct Inner {
    config: Mutex<EngineConfig>,
    client: Client,
    store: Mutex<Store>,
    downloads: Mutex<BTreeMap<DownloadId, Download>>,
    running: Mutex<HashMap<DownloadId, Running>>,
    global_limiter: Arc<Limiter>,
    events: broadcast::Sender<Event>,
    /// Held while picking a free file name and creating the part file.
    naming: tokio::sync::Mutex<()>,
}

struct Running {
    cancel: CancellationToken,
    limiter: Arc<Limiter>,
    done: watch::Receiver<bool>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Engine {
    pub fn open(config: EngineConfig) -> Result<Self> {
        let store = match &config.db_path {
            Some(p) => Store::open(p)?,
            None => Store::in_memory()?,
        };
        let mut downloads = BTreeMap::new();
        for mut d in store.load_all()? {
            // The app closed while these were running.
            if d.status.is_active() {
                d.status = DownloadStatus::Paused;
                store.save(&d)?;
            }
            downloads.insert(d.id, d);
        }
        let (events, _) = broadcast::channel(4096);
        Ok(Self {
            inner: Arc::new(Inner {
                global_limiter: Arc::new(Limiter::new(config.global_speed_limit)),
                config: Mutex::new(config),
                client: http::build_client()?,
                store: Mutex::new(store),
                downloads: Mutex::new(downloads),
                running: Mutex::new(HashMap::new()),
                events,
                naming: tokio::sync::Mutex::new(()),
            }),
        })
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.inner.events.subscribe()
    }

    pub fn config(&self) -> EngineConfig {
        self.inner.config.lock().unwrap().clone()
    }

    /// Changes settings that apply to new downloads. Running downloads keep theirs.
    pub fn update_config(&self, f: impl FnOnce(&mut EngineConfig)) {
        let mut c = self.inner.config.lock().unwrap();
        f(&mut c);
        self.inner.global_limiter.set_rate(c.global_speed_limit);
    }

    pub fn store_setting(&self, key: &str, value: &str) -> Result<()> {
        self.inner.store.lock().unwrap().set_setting(key, value)
    }

    pub fn load_setting(&self, key: &str) -> Result<Option<String>> {
        self.inner.store.lock().unwrap().get_setting(key)
    }

    pub fn list(&self) -> Vec<Download> {
        self.inner
            .downloads
            .lock()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }

    pub fn get(&self, id: DownloadId) -> Result<Download> {
        self.inner
            .downloads
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or(Error::NotFound(id))
    }

    pub fn is_running(&self, id: DownloadId) -> bool {
        self.inner.running.lock().unwrap().contains_key(&id)
    }

    pub fn running_count(&self) -> usize {
        self.inner.running.lock().unwrap().len()
    }

    /// Asks the server about a link without downloading it (for the "add download" dialog).
    pub async fn probe(&self, request: &RequestInfo) -> Result<ProbeInfo> {
        http::probe(&self.inner.client, request).await
    }

    pub async fn add(&self, new: NewDownload) -> Result<DownloadId> {
        let url = reqwest::Url::parse(&new.request.url)
            .map_err(|e| Error::InvalidUrl(format!("{}: {e}", new.request.url)))?;
        if !matches!(url.scheme(), "http" | "https") {
            return Err(Error::InvalidUrl(format!(
                "{}: only http and https are supported",
                new.request.url
            )));
        }
        let config = self.config();
        let (file_name, name_locked) = match new
            .file_name
            .as_deref()
            .map(str::trim)
            .filter(|n| !n.is_empty())
        {
            Some(n) => (filename::sanitize(n), true),
            None => (
                filename::from_url(&new.request.url)
                    .map(|n| filename::sanitize(&n))
                    .unwrap_or_else(|| "download".into()),
                false,
            ),
        };
        let category = Category::from_file_name(&file_name);
        let (save_dir, dir_locked) = match new.save_dir {
            Some(d) => (d, true),
            None => (config.dir_for(category), false),
        };
        let id = self.inner.store.lock().unwrap().next_id()?;
        let d = Download {
            id,
            request: new.request,
            final_url: None,
            file_name,
            file_name_locked: name_locked,
            save_dir,
            save_dir_locked: dir_locked,
            category,
            size: None,
            resumable: false,
            content_type: None,
            status: DownloadStatus::Queued,
            error: None,
            downloaded: 0,
            segments: Vec::new(),
            connections: new
                .connections
                .unwrap_or(config.default_connections)
                .clamp(1, MAX_CONNECTIONS),
            speed_limit: new.speed_limit.unwrap_or(0),
            queue_id: new.queue_id,
            temp_path: None,
            created_at: now_ms(),
            completed_at: None,
            description: new.description,
        };
        self.inner.store.lock().unwrap().save(&d)?;
        self.inner.downloads.lock().unwrap().insert(id, d.clone());
        let _ = self.inner.events.send(Event::Added { download: d });
        if new.start {
            self.start(id)?;
        }
        Ok(id)
    }

    /// Starts or resumes a download. Does nothing if it is already running or complete.
    pub fn start(&self, id: DownloadId) -> Result<()> {
        let d = self.get(id)?;
        if d.status == DownloadStatus::Completed {
            return Ok(());
        }
        let mut running = self.inner.running.lock().unwrap();
        if running.contains_key(&id) {
            return Ok(());
        }
        let cancel = CancellationToken::new();
        let limiter = Arc::new(Limiter::new(d.speed_limit));
        let (done_tx, done_rx) = watch::channel(false);
        running.insert(
            id,
            Running {
                cancel: cancel.clone(),
                limiter: limiter.clone(),
                done: done_rx,
            },
        );
        drop(running);
        let inner = self.inner.clone();
        tokio::spawn(async move {
            let result = drive(&inner, id, &cancel, limiter).await;
            inner.running.lock().unwrap().remove(&id);
            let (status, error) = match result {
                Ok(()) => (DownloadStatus::Completed, None),
                Err(_) if cancel.is_cancelled() => (DownloadStatus::Paused, None),
                Err(e @ Error::LinkRefused(_)) => {
                    (DownloadStatus::NeedsRefresh, Some(e.to_string()))
                }
                Err(e) => (DownloadStatus::Failed, Some(e.to_string())),
            };
            if status != DownloadStatus::Completed {
                inner.update(id, true, |d| {
                    d.status = status;
                    d.error = error;
                });
            }
            let _ = done_tx.send(true);
        });
        Ok(())
    }

    /// Stops a running download, keeping what's been fetched. Waits until it has stopped.
    pub async fn pause(&self, id: DownloadId) -> Result<()> {
        let running = self
            .inner
            .running
            .lock()
            .unwrap()
            .get(&id)
            .map(|r| (r.cancel.clone(), r.done.clone()));
        match running {
            Some((cancel, mut done)) => {
                cancel.cancel();
                let _ = done.wait_for(|d| *d).await;
            }
            None => {
                // A queued download being "paused" just stays put.
                self.get(id)?;
            }
        }
        Ok(())
    }

    pub async fn pause_all(&self) {
        let ids: Vec<_> = self.inner.running.lock().unwrap().keys().copied().collect();
        for id in ids {
            let _ = self.pause(id).await;
        }
    }

    /// Removes a download from the list. The unfinished part file is always deleted;
    /// a finished file only when `delete_file` is true.
    pub async fn remove(&self, id: DownloadId, delete_file: bool) -> Result<()> {
        self.pause(id).await?;
        let d = self
            .inner
            .downloads
            .lock()
            .unwrap()
            .remove(&id)
            .ok_or(Error::NotFound(id))?;
        self.inner.store.lock().unwrap().delete(id)?;
        if let Some(tmp) = &d.temp_path {
            let _ = std::fs::remove_file(tmp);
        }
        if delete_file && d.status == DownloadStatus::Completed {
            let _ = std::fs::remove_file(d.final_path());
        }
        let _ = self.inner.events.send(Event::Removed { id });
        Ok(())
    }

    /// Gives a stopped download a fresh link (e.g. after it expired), keeping its progress.
    /// If the file behind the new link has a different size, the download starts over.
    pub async fn refresh_address(&self, id: DownloadId, request: RequestInfo) -> Result<()> {
        if self.is_running(id) {
            return Err(Error::AlreadyRunning);
        }
        reqwest::Url::parse(&request.url)
            .map_err(|e| Error::InvalidUrl(format!("{}: {e}", request.url)))?;
        self.inner.update(id, true, |d| {
            d.request = request;
            d.final_url = None;
            d.error = None;
            if d.status == DownloadStatus::NeedsRefresh || d.status == DownloadStatus::Failed {
                d.status = DownloadStatus::Paused;
            }
        });
        Ok(())
    }

    /// Changes where a download that hasn't started yet will be saved, or its name.
    pub fn set_target(
        &self,
        id: DownloadId,
        save_dir: Option<PathBuf>,
        file_name: Option<String>,
    ) -> Result<()> {
        let d = self.get(id)?;
        if d.temp_path.is_some() || self.is_running(id) {
            return Err(Error::AlreadyRunning);
        }
        self.inner.update(id, true, |d| {
            if let Some(dir) = save_dir {
                d.save_dir = dir;
                d.save_dir_locked = true;
            }
            if let Some(name) = file_name {
                d.file_name = filename::sanitize(&name);
                d.file_name_locked = true;
                d.category = Category::from_file_name(&d.file_name);
            }
        });
        Ok(())
    }

    /// Bytes per second for one download, 0 = unlimited. Applies at once if it's running.
    pub fn set_speed_limit(&self, id: DownloadId, bytes_per_sec: u64) -> Result<()> {
        self.get(id)?;
        if let Some(r) = self.inner.running.lock().unwrap().get(&id) {
            r.limiter.set_rate(bytes_per_sec);
        }
        self.inner
            .update(id, true, |d| d.speed_limit = bytes_per_sec);
        Ok(())
    }

    pub fn set_connections(&self, id: DownloadId, connections: u8) -> Result<()> {
        self.get(id)?;
        self.inner.update(id, true, |d| {
            d.connections = connections.clamp(1, MAX_CONNECTIONS)
        });
        Ok(())
    }

    pub fn set_queue(&self, id: DownloadId, queue_id: Option<i64>) -> Result<()> {
        self.get(id)?;
        self.inner.update(id, true, |d| d.queue_id = queue_id);
        Ok(())
    }

    pub fn set_global_speed_limit(&self, bytes_per_sec: u64) {
        self.update_config(|c| c.global_speed_limit = bytes_per_sec);
    }
}

impl Inner {
    /// Changes a download, saves it, and (when `announce`) tells the UI.
    fn update(
        &self,
        id: DownloadId,
        announce: bool,
        f: impl FnOnce(&mut Download),
    ) -> Option<Download> {
        let snapshot = {
            let mut map = self.downloads.lock().unwrap();
            let d = map.get_mut(&id)?;
            f(d);
            d.clone()
        };
        if let Err(e) = self.store.lock().unwrap().save(&snapshot) {
            tracing::error!("could not save download {id}: {e}");
        }
        if announce {
            let _ = self.events.send(Event::Updated {
                download: snapshot.clone(),
            });
        }
        Some(snapshot)
    }

    fn get(&self, id: DownloadId) -> Result<Download> {
        self.downloads
            .lock()
            .unwrap()
            .get(&id)
            .cloned()
            .ok_or(Error::NotFound(id))
    }
}

/// One run of a download, from asking the server to renaming the finished file.
async fn drive(
    inner: &Arc<Inner>,
    id: DownloadId,
    cancel: &CancellationToken,
    limiter: Arc<Limiter>,
) -> Result<()> {
    inner.update(id, true, |d| {
        d.status = DownloadStatus::Connecting;
        d.error = None;
    });
    let request = inner.get(id)?.request;
    let probe = tokio::select! {
        biased;
        _ = cancel.cancelled() => return Err(Error::Cancelled),
        p = http::probe(&inner.client, &request) => p?,
    };
    let config = inner.config.lock().unwrap().clone();

    let mut d = inner.get(id)?;
    let has_progress = d.segments.iter().any(|s| s.done() > 0);
    let part_missing = d.temp_path.as_ref().is_some_and(|p| !p.exists());
    let start_over = has_progress && (!probe.resumable || probe.size != d.size || part_missing);
    if start_over {
        d.segments.clear();
        d.downloaded = 0;
    }
    d.final_url = Some(probe.final_url.clone());
    d.size = probe.size;
    d.resumable = probe.resumable;
    d.content_type = probe.content_type.clone();
    if d.temp_path.is_none() && !d.file_name_locked {
        d.file_name = probe.file_name.clone();
        d.category = Category::from_file_name(&d.file_name);
        if !d.save_dir_locked {
            d.save_dir = config.dir_for(d.category);
        }
    }

    // Pick the final name and create the part file.
    let temp_path = match d.temp_path.clone().filter(|p| p.exists()) {
        Some(p) => p,
        None => {
            let _naming = inner.naming.lock().await;
            std::fs::create_dir_all(&d.save_dir)?;
            let target = filename::unique_path(&d.save_dir, &d.file_name);
            d.file_name = target
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&d.file_name)
                .to_string();
            let tmp = filename::part_path(&target);
            std::fs::File::create(&tmp)?;
            d.segments.clear();
            d.downloaded = 0;
            tmp
        }
    };
    d.temp_path = Some(temp_path.clone());

    let file = OpenOptions::new().read(true).write(true).open(&temp_path)?;
    if start_over {
        file.set_len(0)?;
    }
    if d.segments.is_empty() {
        d.segments = match (d.size, d.resumable) {
            (Some(size), true) => {
                transfer::plan(size, d.connections as usize, config.min_segment_size)
            }
            (Some(0), false) => Vec::new(),
            (Some(size), false) => vec![Segment::new(0, size)],
            (None, _) => vec![Segment::new(0, UNKNOWN_END)],
        };
        if let (Some(size), true) = (d.size, d.resumable) {
            file.set_len(size)?;
        }
    }
    d.status = DownloadStatus::Downloading;
    let connections = d.connections as usize;
    let segments = d.segments.clone();
    inner.update(id, true, |rec| *rec = d.clone());

    let transfer = Arc::new(Transfer {
        client: inner.client.clone(),
        request: request.clone(),
        url: probe.final_url.clone(),
        resumable: d.resumable,
        file: Arc::new(file),
        limiters: vec![inner.global_limiter.clone(), limiter],
        cancel: cancel.child_token(),
        options: TransferOptions {
            connections,
            min_split: config.min_segment_size,
            max_retries: config.max_retries,
            retry_delay: config.retry_delay,
        },
    });
    let progress = Arc::new(Progress::new(&segments));

    let stop_reporter = CancellationToken::new();
    let reporter = tokio::spawn(report(
        inner.clone(),
        id,
        d.size,
        progress.clone(),
        stop_reporter.clone(),
        config.progress_interval,
    ));
    let result = transfer::run(transfer.clone(), progress.clone()).await;
    stop_reporter.cancel();
    let _ = reporter.await;

    let segments = progress.segments();
    let written = progress.written();
    inner.update(id, false, |d| {
        d.segments = segments;
        d.downloaded = written;
    });
    result?;

    if !progress.all_complete() || d.size.is_some_and(|s| s != written) {
        return Err(Error::Io(std::io::Error::new(
            std::io::ErrorKind::UnexpectedEof,
            "the download ended before the file was complete",
        )));
    }

    // Finish: flush to disk, close, and give the file its real name.
    let file = Arc::try_unwrap(transfer).ok().map(|t| t.file);
    if let Some(f) = file {
        let f = Arc::try_unwrap(f).ok();
        if let Some(f) = f {
            tokio::task::spawn_blocking(move || f.sync_all())
                .await
                .map_err(std::io::Error::other)??;
        }
    }
    let final_path = {
        let _naming = inner.naming.lock().await;
        let wanted = d.save_dir.join(&d.file_name);
        let target = if wanted.exists() {
            filename::unique_path(&d.save_dir, &d.file_name)
        } else {
            wanted
        };
        rename(&temp_path, &target)?;
        target
    };
    let final_name = final_path
        .file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string);
    inner.update(id, true, |d| {
        if let Some(n) = final_name {
            d.file_name = n;
        }
        d.status = DownloadStatus::Completed;
        d.error = None;
        d.size = Some(written);
        d.downloaded = written;
        d.segments.clear();
        d.temp_path = None;
        d.completed_at = Some(now_ms());
    });
    Ok(())
}

fn rename(from: &Path, to: &Path) -> Result<()> {
    // Antivirus scanners on Windows briefly lock new files; try a few times.
    let mut last = None;
    for attempt in 0..10 {
        match std::fs::rename(from, to) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = Some(e);
                std::thread::sleep(Duration::from_millis(100 * (attempt + 1)));
            }
        }
    }
    Err(last.unwrap().into())
}

/// Sends progress to the UI and saves it every few seconds.
async fn report(
    inner: Arc<Inner>,
    id: DownloadId,
    size: Option<u64>,
    progress: Arc<Progress>,
    stop: CancellationToken,
    interval: Duration,
) {
    let mut tick = tokio::time::interval(interval);
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let mut last = progress.received();
    let mut last_at = tokio::time::Instant::now();
    let mut speed = 0f64;
    let mut ticks = 0u32;
    let save_every =
        (Duration::from_secs(2).as_millis() / interval.as_millis().max(1)).max(1) as u32;
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = tick.tick() => {}
        }
        let now = tokio::time::Instant::now();
        let received = progress.received();
        let secs = now.duration_since(last_at).as_secs_f64();
        if secs > 0.0 {
            let current = received.saturating_sub(last) as f64 / secs;
            // Smooth over roughly the last few seconds.
            speed = if speed == 0.0 {
                current
            } else {
                speed * 0.7 + current * 0.3
            };
        }
        last = received;
        last_at = now;
        let _ = inner.events.send(Event::Progress {
            id,
            downloaded: received,
            size,
            speed: speed as u64,
            segments: progress.views(),
        });
        ticks += 1;
        if ticks.is_multiple_of(save_every) {
            let segments = progress.segments();
            let written = progress.written();
            inner.update(id, false, |d| {
                d.segments = segments;
                d.downloaded = written;
            });
        }
    }
}
