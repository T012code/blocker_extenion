//! Data types shared by the engine, the store and the app.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::category::Category;

pub type DownloadId = i64;

/// Marks a segment whose end is not known (the server sent no size).
pub const UNKNOWN_END: u64 = u64::MAX;

/// Everything needed to repeat the browser's request for a file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RequestInfo {
    pub url: String,
    #[serde(default)]
    pub referer: Option<String>,
    /// Raw `Cookie` header value captured from the browser.
    #[serde(default)]
    pub cookies: Option<String>,
    #[serde(default)]
    pub user_agent: Option<String>,
    /// Extra headers, e.g. `Authorization`.
    #[serde(default)]
    pub headers: Vec<(String, String)>,
}

impl RequestInfo {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            ..Default::default()
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DownloadStatus {
    /// Added but not started (or waiting in a queue).
    Queued,
    /// Asking the server for size, name and resume support.
    Connecting,
    Downloading,
    Paused,
    Completed,
    Failed,
    /// The server refused the link (401/403/404/410). It has probably expired:
    /// capture it again from the browser to continue.
    NeedsRefresh,
    /// A finished torrent still sharing with other people.
    Seeding,
    /// Held back by privacy protection: waiting for the VPN or proxy (kill switch).
    Blocked,
}

impl DownloadStatus {
    /// Running now (pausing it makes sense).
    pub fn is_active(self) -> bool {
        matches!(
            self,
            Self::Connecting | Self::Downloading | Self::Seeding | Self::Blocked
        )
    }
}

/// What a download is. Web files are fetched by the engine itself; the other kinds
/// are handed to a [`Runner`](crate::Runner) registered for them.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum DownloadKind {
    /// A file at an http(s) address, fetched in sections.
    #[default]
    Http,
    /// A video or audio stream from a web page (HLS, DASH, or a site like YouTube).
    Media(MediaSpec),
    /// A BitTorrent download from a magnet link or a .torrent file.
    Torrent(TorrentSpec),
}

impl DownloadKind {
    /// The name runners are registered under.
    pub fn runner_name(&self) -> Option<&'static str> {
        match self {
            Self::Http => None,
            Self::Media(_) => Some("media"),
            Self::Torrent(_) => Some("torrent"),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaSpec {
    /// The page (or stream playlist) to download from.
    pub page_url: String,
    /// Which format to fetch, as the media tool understands it. `None` = best.
    #[serde(default)]
    pub format: Option<String>,
    /// What the user picked, e.g. "1080p MP4".
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub title: Option<String>,
    /// Which video of a post with several (an Instagram carousel), counting from 1.
    #[serde(default)]
    pub item: Option<u32>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TorrentSpec {
    /// The magnet link, when added from one.
    #[serde(default)]
    pub magnet: Option<String>,
    /// A saved copy of the .torrent file (also saved for magnets once known).
    #[serde(default)]
    pub torrent_file: Option<PathBuf>,
    #[serde(default)]
    pub info_hash: Option<String>,
    /// Indexes of the files to download; `None` = all.
    #[serde(default)]
    pub only_files: Option<Vec<usize>>,
    /// Bytes shared with other people so far.
    #[serde(default)]
    pub uploaded: u64,
    /// Keep sharing until uploaded / size reaches this. 0 = stop as soon as done.
    #[serde(default)]
    pub seed_ratio: Option<f32>,
}

/// One byte range of the file. Bytes `start..pos` are on disk, `pos..end` are still to fetch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Segment {
    pub start: u64,
    pub end: u64,
    pub pos: u64,
}

impl Segment {
    pub fn new(start: u64, end: u64) -> Self {
        Self {
            start,
            end,
            pos: start,
        }
    }
    pub fn done(&self) -> u64 {
        self.pos - self.start
    }
    pub fn remaining(&self) -> u64 {
        self.end.saturating_sub(self.pos)
    }
    pub fn is_complete(&self) -> bool {
        self.pos >= self.end
    }
}

/// A download as stored in the database and shown in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Download {
    pub id: DownloadId,
    pub request: RequestInfo,
    /// URL after redirects.
    pub final_url: Option<String>,
    pub file_name: String,
    /// True when the user typed the name, so the server's name must not replace it.
    #[serde(default)]
    pub file_name_locked: bool,
    pub save_dir: PathBuf,
    /// True when the user picked the folder, so it doesn't follow the category.
    #[serde(default)]
    pub save_dir_locked: bool,
    pub category: Category,
    pub size: Option<u64>,
    pub resumable: bool,
    pub content_type: Option<String>,
    pub status: DownloadStatus,
    pub error: Option<String>,
    pub downloaded: u64,
    pub segments: Vec<Segment>,
    /// Number of sections downloaded in parallel, one connection each (1-64).
    pub connections: u8,
    /// Bytes per second, 0 for no limit.
    pub speed_limit: u64,
    pub queue_id: Option<i64>,
    /// Set once the part file has been created.
    pub temp_path: Option<PathBuf>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: DownloadKind,
    /// Started in the background while the "add download" window waits for the user
    /// to confirm (as IDM does). Hidden from the list until confirmed; removed if not.
    #[serde(default)]
    pub pending: bool,
}

impl Download {
    pub fn final_path(&self) -> PathBuf {
        self.save_dir.join(&self.file_name)
    }
}

/// What to add. Missing fields are filled from the server's reply and the settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NewDownload {
    pub request: RequestInfo,
    #[serde(default)]
    pub file_name: Option<String>,
    #[serde(default)]
    pub save_dir: Option<PathBuf>,
    #[serde(default)]
    pub connections: Option<u8>,
    #[serde(default)]
    pub speed_limit: Option<u64>,
    #[serde(default)]
    pub queue_id: Option<i64>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub kind: DownloadKind,
    /// Start right away. When false the download waits as `Queued`.
    #[serde(default)]
    pub start: bool,
    /// Start in the background, unconfirmed (see `Download::pending`).
    #[serde(default)]
    pub pending: bool,
}

/// What the server said about a link before downloading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ProbeInfo {
    pub final_url: String,
    pub size: Option<u64>,
    pub resumable: bool,
    pub file_name: String,
    pub content_type: Option<String>,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SegmentView {
    pub start: u64,
    pub end: u64,
    pub pos: u64,
    pub active: bool,
}

/// Sent to the UI as things change.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Event {
    Added {
        download: Download,
    },
    /// Status, name, size or other fields changed.
    Updated {
        download: Download,
    },
    Progress {
        id: DownloadId,
        downloaded: u64,
        size: Option<u64>,
        /// Bytes per second, smoothed.
        speed: u64,
        segments: Vec<SegmentView>,
    },
    Removed {
        id: DownloadId,
    },
}
