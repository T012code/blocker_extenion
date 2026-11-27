//! Torrent support that only runs while torrents do.
//!
//! A torrent session keeps a port open, talks to the DHT and checks the VPN or proxy
//! every second. None of that is needed while no torrent is downloading or sharing, so
//! the session starts with the first torrent and closes a minute after the last one
//! stops. In between, the app uses no network and wakes the processor for nothing.

use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use crate::{TorrentOptions, TorrentRunner};
use ldm_engine::{Download, DownloadKind, Error, Job, Result, Runner};

/// How long the session stays after the last torrent stops (a quick restart of a
/// torrent then doesn't start it all over).
const IDLE: Duration = Duration::from_secs(60);

pub struct LazyTorrents {
    options: Mutex<TorrentOptions>,
    session: tokio::sync::Mutex<Option<Arc<TorrentRunner>>>,
    /// Torrents running now.
    active: AtomicUsize,
    /// Bumped whenever the last torrent stops, so only the latest idle timer acts.
    idle_round: AtomicU64,
    idle: Duration,
    me: Weak<Self>,
}

impl LazyTorrents {
    pub fn new(options: TorrentOptions) -> Arc<Self> {
        Self::with_idle(options, IDLE)
    }

    pub fn with_idle(options: TorrentOptions, idle: Duration) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            options: Mutex::new(options),
            session: tokio::sync::Mutex::new(None),
            active: AtomicUsize::new(0),
            idle_round: AtomicU64::new(0),
            idle,
            me: me.clone(),
        })
    }

    /// Whether a session is open right now.
    pub async fn is_running(&self) -> bool {
        self.session.lock().await.is_some()
    }

    /// New settings: the open session (if any) closes; the next torrent starts one with them.
    /// Running torrents must be paused first and started again after.
    pub async fn reconfigure(&self, options: TorrentOptions) {
        *self.options.lock().unwrap() = options;
        if let Some(old) = self.session.lock().await.take() {
            old.shutdown().await;
        }
    }

    async fn runner(&self) -> Result<Arc<TorrentRunner>> {
        let mut slot = self.session.lock().await;
        if let Some(r) = slot.as_ref() {
            return Ok(r.clone());
        }
        let options = self.options.lock().unwrap().clone();
        let r = TorrentRunner::start(options)
            .await
            .map_err(|e| Error::Other(format!("torrents unavailable: {e:#}")))?;
        *slot = Some(r.clone());
        Ok(r)
    }

    fn stopped_one(&self) {
        if self.active.fetch_sub(1, Ordering::SeqCst) == 1 {
            self.close_when_idle();
        }
    }

    fn close_when_idle(&self) {
        let round = self.idle_round.fetch_add(1, Ordering::SeqCst) + 1;
        let Some(me) = self.me.upgrade() else { return };
        tokio::spawn(async move {
            tokio::time::sleep(me.idle).await;
            if me.idle_round.load(Ordering::SeqCst) != round {
                return;
            }
            let mut slot = me.session.lock().await;
            // Checked again under the lock: a torrent may have started meanwhile.
            if me.active.load(Ordering::SeqCst) == 0
                && let Some(old) = slot.take()
            {
                old.shutdown().await;
                tracing::info!("no torrents running: closed the torrent session");
            }
        });
    }
}

impl Runner for LazyTorrents {
    fn run(&self, job: Job) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'static>> {
        let Some(me) = self.me.upgrade() else {
            return Box::pin(async { Err(Error::Other("torrent support has shut down".into())) });
        };
        me.active.fetch_add(1, Ordering::SeqCst);
        Box::pin(async move {
            let result = match me.runner().await {
                Ok(r) => r.run(job).await,
                Err(e) => Err(e),
            };
            me.stopped_one();
            result
        })
    }

    fn removed(&self, download: &Download, delete_files: bool) {
        // With a session open, it lets go of the torrent. Without one, there is nothing
        // to let go of (sessions keep no list between runs): only files to clean up.
        if let Ok(slot) = self.session.try_lock()
            && let Some(r) = slot.as_ref()
        {
            r.removed(download, delete_files);
            return;
        }
        if delete_files {
            let p = download.save_dir.join(&download.file_name);
            let _ = if p.is_dir() {
                std::fs::remove_dir_all(&p)
            } else {
                std::fs::remove_file(&p)
            };
        }
        if let DownloadKind::Torrent(spec) = &download.kind
            && let Some(f) = &spec.torrent_file
        {
            let _ = std::fs::remove_file(f);
        }
    }
}
