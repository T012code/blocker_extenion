//! Plug-in point for download kinds the engine doesn't fetch itself (torrents, video
//! streams). The engine still owns the list, the database, pause/continue and events;
//! a runner only does the transfer and reports back through a [`Reporter`].

use std::sync::Arc;

use futures_util::future::BoxFuture;
use tokio_util::sync::CancellationToken;

use crate::engine::Inner;
use crate::error::Result;
use crate::model::{Download, DownloadId, DownloadStatus, Event};

pub trait Runner: Send + Sync + 'static {
    /// Runs the download until it's finished (`Ok`), fails, or `job.cancel` fires. On
    /// cancel return `Err(Error::Cancelled)`, or `Ok` if the download had already finished
    /// (e.g. a torrent that was only seeding).
    fn run(&self, job: Job) -> BoxFuture<'static, Result<()>>;

    /// The download was removed from the list. Clean up partial files, and the
    /// downloaded files too when `delete_files` is true.
    fn removed(&self, _download: &Download, _delete_files: bool) {}
}

pub struct Job {
    pub download: Download,
    pub cancel: CancellationToken,
    pub report: Reporter,
}

/// How a runner tells the engine (and the UI) what's happening.
#[derive(Clone)]
pub struct Reporter {
    pub(crate) inner: Arc<Inner>,
    pub(crate) id: DownloadId,
}

impl Reporter {
    pub fn id(&self) -> DownloadId {
        self.id
    }

    /// The download as stored now.
    pub fn download(&self) -> Option<Download> {
        self.inner.downloads.lock().unwrap().get(&self.id).cloned()
    }

    /// Changes the download, saves it and tells the UI.
    pub fn update(&self, f: impl FnOnce(&mut Download)) -> Option<Download> {
        self.inner.update(self.id, true, f)
    }

    pub fn set_status(&self, status: DownloadStatus) {
        self.inner.update(self.id, true, |d| {
            if d.status != status {
                d.status = status;
            }
        });
    }

    /// Live progress. Saved without a full UI update; a `Progress` event is sent.
    pub fn progress(&self, downloaded: u64, size: Option<u64>, speed: u64) {
        self.inner.update(self.id, false, |d| {
            d.downloaded = downloaded;
            if size.is_some() {
                d.size = size;
            }
        });
        let _ = self.inner.events.send(Event::Progress {
            id: self.id,
            downloaded,
            size,
            speed,
            segments: Vec::new(),
        });
    }
}
