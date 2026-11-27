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
    Download, DownloadId, DownloadKind, DownloadStatus, Event, NewDownload, ProbeInfo, RequestInfo,
    Segment, UNKNOWN_END,
};
use crate::runner::{Job, Reporter, Runner};
use crate::store::Store;
use crate::transfer::{self, Progress, Sections, Transfer, TransferOptions};

/// Most sections (parallel connections) one download may use.
pub const MAX_CONNECTIONS: u8 = 64;

/// Section counts offered in the app's drop-downs. Any value from 1 to
/// [`MAX_CONNECTIONS`] can also be typed in.
pub const SECTION_PRESETS: [u8; 9] = [1, 2, 4, 8, 16, 24, 32, 48, 64];

/// Settings key for the default number of sections.
const DEFAULT_SECTIONS_KEY: &str = "default_connections";

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
    /// Connection failures in a row (the network is gone) before a download pauses.
    pub offline_retries: u32,
    /// Folders chosen for some categories instead of `<download_dir>/<Category>`.
    pub category_dirs: HashMap<Category, PathBuf>,
    /// The user's own file-extension lists for some categories (instead of the built-in).
    pub category_extensions: HashMap<Category, Vec<String>>,
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
            offline_retries: 3,
            category_dirs: HashMap::new(),
            category_extensions: HashMap::new(),
            global_speed_limit: 0,
            progress_interval: Duration::from_millis(500),
        }
    }

    /// Which category a file belongs to, by its extension and the user's lists.
    pub fn category_of(&self, file_name: &str) -> Category {
        Category::from_file_name_with(file_name, &self.category_extensions)
    }

    /// Where a category's files go: the folder chosen for it, else its sub-folder of the
    /// download folder (or the download folder itself when sorting is off).
    pub fn dir_for(&self, category: Category) -> PathBuf {
        if let Some(d) = self
            .category_dirs
            .get(&category)
            .filter(|d| !d.as_os_str().is_empty())
        {
            return d.clone();
        }
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

pub(crate) struct Inner {
    config: Mutex<EngineConfig>,
    client: Client,
    store: Mutex<Store>,
    pub(crate) downloads: Mutex<BTreeMap<DownloadId, Download>>,
    runners: Mutex<HashMap<&'static str, Arc<dyn Runner>>>,
    running: Mutex<HashMap<DownloadId, Running>>,
    global_limiter: Arc<Limiter>,
    pub(crate) events: broadcast::Sender<Event>,
    /// Held while picking a free file name and creating the part file.
    naming: tokio::sync::Mutex<()>,
}

struct Running {
    cancel: CancellationToken,
    limiter: Arc<Limiter>,
    sections: Arc<Sections>,
    done: watch::Receiver<bool>,
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl Engine {
    pub fn open(mut config: EngineConfig) -> Result<Self> {
        let store = match &config.db_path {
            Some(p) => Store::open(p)?,
            None => Store::in_memory()?,
        };
        // The user's saved choice wins over the built-in default.
        if let Some(n) = store
            .get_setting(DEFAULT_SECTIONS_KEY)?
            .and_then(|v| v.parse::<u8>().ok())
        {
            config.default_connections = n;
        }
        config.default_connections = config.default_connections.clamp(1, MAX_CONNECTIONS);
        let mut downloads = BTreeMap::new();
        for mut d in store.load_all()? {
            // Started for an "add download" window that was never answered.
            if d.pending {
                if let Some(tmp) = &d.temp_path {
                    let _ = std::fs::remove_file(tmp);
                }
                store.delete(d.id)?;
                continue;
            }
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
                runners: Mutex::new(HashMap::new()),
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
        let scheme_ok = match &new.kind {
            DownloadKind::Torrent(_) => {
                matches!(url.scheme(), "magnet" | "http" | "https" | "file")
            }
            _ => matches!(url.scheme(), "http" | "https"),
        };
        if !scheme_ok {
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
                match &new.kind {
                    DownloadKind::Http => filename::from_url(&new.request.url)
                        .map(|n| filename::sanitize(&n))
                        .unwrap_or_else(|| "download".into()),
                    DownloadKind::Media(m) => {
                        filename::sanitize(m.title.as_deref().unwrap_or("video"))
                    }
                    DownloadKind::Torrent(_) => magnet_name(&new.request.url)
                        .map(|n| filename::sanitize(&n))
                        .unwrap_or_else(|| "torrent".into()),
                },
                false,
            ),
        };
        let category = match &new.kind {
            DownloadKind::Media(m)
                if m.label
                    .as_deref()
                    .is_some_and(|l| l.to_lowercase().contains("audio")) =>
            {
                Category::Music
            }
            DownloadKind::Media(_) => Category::Video,
            DownloadKind::Torrent(_) => Category::Torrents,
            _ => config.category_of(&file_name),
        };
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
            kind: new.kind,
            pending: new.pending,
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
        let sections = Arc::new(Sections::new(d.connections as usize));
        let (done_tx, done_rx) = watch::channel(false);
        running.insert(
            id,
            Running {
                cancel: cancel.clone(),
                limiter: limiter.clone(),
                sections: sections.clone(),
                done: done_rx,
            },
        );
        drop(running);
        let inner = self.inner.clone();
        let runner = match d.kind.runner_name() {
            Some(name) => match self.inner.runners.lock().unwrap().get(name).cloned() {
                Some(r) => Some(r),
                None => {
                    self.inner.running.lock().unwrap().remove(&id);
                    let msg = format!("this version can't download {name} items");
                    self.inner.update(id, true, |d| {
                        d.status = DownloadStatus::Failed;
                        d.error = Some(msg.clone());
                    });
                    return Err(Error::Other(msg));
                }
            },
            None => None,
        };
        tokio::spawn(async move {
            let result = match runner {
                Some(runner) => run_with(&inner, id, runner, cancel.clone()).await,
                None => drive(&inner, id, &cancel, limiter, sections).await,
            };
            inner.running.lock().unwrap().remove(&id);
            let (status, error) = match result {
                Ok(()) => (DownloadStatus::Completed, None),
                Err(_) if cancel.is_cancelled() => (DownloadStatus::Paused, None),
                Err(e @ Error::LinkRefused(_)) => {
                    (DownloadStatus::NeedsRefresh, Some(e.to_string()))
                }
                // The network went away: pause, keeping everything, unless the server
                // can't continue a download (then continuing has to start over).
                Err(e) if e.is_connection_lost() => {
                    let d = inner.get(id).ok();
                    if d.is_some_and(|d| loses_progress(&d)) {
                        (DownloadStatus::Failed, Some(STOPPED_OFFLINE.into()))
                    } else {
                        (DownloadStatus::Paused, Some(PAUSED_OFFLINE.into()))
                    }
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
        if let Some(name) = d.kind.runner_name()
            && let Some(r) = self.inner.runners.lock().unwrap().get(name).cloned()
        {
            r.removed(&d, delete_file);
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

    /// Changes where a download is saved, or its name, at any time: before it starts,
    /// while it runs (its part file stays put and the finished file goes to the new
    /// place), or after it finished (the file is moved now). This is what lets a download
    /// start in the background while the "add download" window waits for an answer.
    pub async fn set_target(
        &self,
        id: DownloadId,
        save_dir: Option<PathBuf>,
        file_name: Option<String>,
    ) -> Result<()> {
        let d = self.get(id)?;
        let name = file_name
            .map(|n| filename::sanitize(&n))
            .filter(|n| !n.is_empty());
        if d.status == DownloadStatus::Completed && !matches!(d.kind, DownloadKind::Torrent(_)) {
            let dir = save_dir.clone().unwrap_or_else(|| d.save_dir.clone());
            let wanted = name.clone().unwrap_or_else(|| d.file_name.clone());
            let from = d.final_path();
            let to = {
                let _naming = self.inner.naming.lock().await;
                std::fs::create_dir_all(&dir)?;
                let to = if dir.join(&wanted) == from {
                    from.clone()
                } else {
                    filename::unique_path(&dir, &wanted)
                };
                if to != from {
                    move_file(&from, &to)?;
                }
                to
            };
            let final_name = to
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or(&wanted)
                .to_string();
            self.inner.update(id, true, |d| {
                d.save_dir = dir;
                d.save_dir_locked = true;
                d.file_name = final_name;
                d.file_name_locked = true;
            });
            return Ok(());
        }
        self.inner.update(id, true, |d| {
            if let Some(dir) = save_dir {
                d.save_dir = dir;
                d.save_dir_locked = true;
            }
            if let Some(name) = name {
                d.category = self.config().category_of(&name);
                d.file_name = name;
                d.file_name_locked = true;
            }
        });
        Ok(())
    }

    /// The user confirmed a download that started in the background: it shows in the list.
    pub fn confirm(&self, id: DownloadId) -> Result<()> {
        self.get(id)?;
        self.inner.update(id, true, |d| d.pending = false);
        Ok(())
    }

    /// The network went away: every running download pauses, keeping what it has.
    /// Downloads from servers that can't continue stop instead (continuing starts them
    /// over). Nothing restarts by itself; the user continues them.
    pub async fn pause_all_offline(&self) -> Vec<DownloadId> {
        let ids: Vec<_> = self.inner.running.lock().unwrap().keys().copied().collect();
        for id in &ids {
            let _ = self.pause(*id).await;
            self.inner.update(*id, true, |d| {
                if d.status == DownloadStatus::Completed {
                    return;
                }
                if loses_progress(d) {
                    d.status = DownloadStatus::Failed;
                    d.error = Some(STOPPED_OFFLINE.into());
                } else {
                    d.status = DownloadStatus::Paused;
                    d.error = Some(PAUSED_OFFLINE.into());
                }
            });
        }
        ids
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

    /// Sets how many sections (parallel connections) a download uses, 1-64.
    /// On a running download it applies at once: new connections open straight away,
    /// or surplus ones close after saving what they have.
    pub fn set_connections(&self, id: DownloadId, connections: u8) -> Result<u8> {
        self.get(id)?;
        let n = connections.clamp(1, MAX_CONNECTIONS);
        if let Some(r) = self.inner.running.lock().unwrap().get(&id) {
            r.sections.set(n as usize);
        }
        self.inner.update(id, true, |d| d.connections = n);
        Ok(n)
    }

    /// The number of sections new downloads get unless the user picks another.
    pub fn default_connections(&self) -> u8 {
        self.config().default_connections
    }

    /// Sets and saves the default number of sections for new downloads, 1-64.
    pub fn set_default_connections(&self, connections: u8) -> Result<u8> {
        let n = connections.clamp(1, MAX_CONNECTIONS);
        self.inner
            .store
            .lock()
            .unwrap()
            .set_setting(DEFAULT_SECTIONS_KEY, &n.to_string())?;
        self.update_config(|c| c.default_connections = n);
        Ok(n)
    }

    pub fn set_queue(&self, id: DownloadId, queue_id: Option<i64>) -> Result<()> {
        self.get(id)?;
        self.inner.update(id, true, |d| d.queue_id = queue_id);
        Ok(())
    }

    /// Marks a stopped download as waiting (for a queue to start it). Running and
    /// finished downloads are left alone.
    pub fn requeue(&self, id: DownloadId) -> Result<()> {
        self.get(id)?;
        if self.is_running(id) {
            return Ok(());
        }
        self.inner.update(id, true, |d| {
            if matches!(d.status, DownloadStatus::Paused | DownloadStatus::Failed) {
                d.status = DownloadStatus::Queued;
            }
        });
        Ok(())
    }

    /// Registers the runner for a kind of download ("torrent" or "media").
    pub fn set_runner(&self, name: &'static str, runner: Arc<dyn Runner>) {
        self.inner.runners.lock().unwrap().insert(name, runner);
    }

    pub fn set_global_speed_limit(&self, bytes_per_sec: u64) {
        self.update_config(|c| c.global_speed_limit = bytes_per_sec);
    }
}

impl Inner {
    /// Changes a download, saves it, and (when `announce`) tells the UI.
    pub(crate) fn update(
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
    sections: Arc<Sections>,
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
        d.category = config.category_of(&d.file_name);
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
            (Some(size), true) => transfer::plan(size, sections.wanted(), config.min_segment_size),
            (Some(0), false) => Vec::new(),
            (Some(size), false) => vec![Segment::new(0, size)],
            (None, _) => vec![Segment::new(0, UNKNOWN_END)],
        };
        if let (Some(size), true) = (d.size, d.resumable) {
            file.set_len(size)?;
        }
    }
    d.status = DownloadStatus::Downloading;
    let segments = d.segments.clone();
    inner.update(id, true, |rec| {
        // A new place or name chosen while it was connecting, and confirmation, stay.
        let dir = rec.save_dir_locked.then(|| rec.save_dir.clone());
        let name = rec.file_name_locked.then(|| rec.file_name.clone());
        let pending = rec.pending;
        *rec = d.clone();
        if let Some(dir) = dir {
            rec.save_dir = dir;
            rec.save_dir_locked = true;
        }
        if let Some(name) = name {
            rec.file_name = name;
            rec.file_name_locked = true;
        }
        rec.pending = pending;
    });

    let transfer = Arc::new(Transfer {
        client: inner.client.clone(),
        request: request.clone(),
        url: probe.final_url.clone(),
        resumable: d.resumable,
        file: Arc::new(file),
        limiters: vec![inner.global_limiter.clone(), limiter],
        cancel: cancel.child_token(),
        sections: sections.clone(),
        live: Default::default(),
        options: TransferOptions {
            min_split: config.min_segment_size,
            max_retries: config.max_retries,
            retry_delay: config.retry_delay,
            offline_retries: config.offline_retries,
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
    // Where it goes now (the place may have changed while it downloaded).
    let now = inner.get(id)?;
    let final_path = {
        let _naming = inner.naming.lock().await;
        std::fs::create_dir_all(&now.save_dir)?;
        let wanted = now.save_dir.join(&now.file_name);
        let target = if wanted.exists() {
            filename::unique_path(&now.save_dir, &now.file_name)
        } else {
            wanted
        };
        move_file(&temp_path, &target)?;
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

/// A file download from a server that said it can't continue downloads (no byte
/// ranges): stopping it loses what it has. (Never reached the server: unknown, so it
/// just waits paused.)
fn loses_progress(d: &Download) -> bool {
    matches!(d.kind, DownloadKind::Http) && d.final_url.is_some() && !d.resumable
}

pub const PAUSED_OFFLINE: &str =
    "Paused: the internet connection was lost. Continue it when you're back online.";
pub const STOPPED_OFFLINE: &str = "Stopped: the internet connection was lost, and this server can't continue a download, so continuing starts it again.";

/// Moves a file, also to another drive (copy, then delete).
fn move_file(from: &Path, to: &Path) -> Result<()> {
    match rename(from, to) {
        Ok(()) => Ok(()),
        Err(_) if from.exists() => {
            std::fs::copy(from, to)?;
            std::fs::remove_file(from)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
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

/// Runs a download through its runner and marks it complete when it finishes.
async fn run_with(
    inner: &Arc<Inner>,
    id: DownloadId,
    runner: Arc<dyn Runner>,
    cancel: CancellationToken,
) -> Result<()> {
    let download = inner.get(id)?;
    inner.update(id, true, |d| {
        d.status = DownloadStatus::Connecting;
        d.error = None;
    });
    let report = Reporter {
        inner: inner.clone(),
        id,
    };
    runner
        .run(Job {
            download,
            cancel,
            report,
        })
        .await?;
    inner.update(id, true, |d| {
        d.status = DownloadStatus::Completed;
        d.error = None;
        if let Some(size) = d.size {
            d.downloaded = size;
        }
        d.completed_at = Some(now_ms());
    });
    Ok(())
}

/// The `dn` (display name) of a magnet link.
fn magnet_name(url: &str) -> Option<String> {
    let query = url.strip_prefix("magnet:?")?;
    query.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == "dn").then(|| {
            percent_encoding::percent_decode_str(&v.replace('+', " "))
                .decode_utf8_lossy()
                .into_owned()
        })
    })
}
