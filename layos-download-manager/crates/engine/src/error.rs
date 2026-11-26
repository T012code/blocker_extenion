use thiserror::Error;

use crate::model::DownloadId;

#[derive(Debug, Error)]
pub enum Error {
    #[error("no download with id {0}")]
    NotFound(DownloadId),
    #[error("invalid address: {0}")]
    InvalidUrl(String),
    #[error("the server refused the link (HTTP {0}); it may have expired")]
    LinkRefused(u16),
    #[error("the server answered HTTP {0}")]
    HttpStatus(u16),
    #[error("the server sent a different part of the file than was asked for")]
    BadRange,
    #[error("the file on the server changed since the download started")]
    FileChanged,
    #[error("network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("disk error: {0}")]
    Io(#[from] std::io::Error),
    #[error("database error: {0}")]
    Db(#[from] rusqlite::Error),
    #[error("data error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("this download is already running")]
    AlreadyRunning,
    #[error("cancelled")]
    Cancelled,
}

impl Error {
    /// Worth trying again after a pause (dropped connection, server overload).
    pub fn is_transient(&self) -> bool {
        match self {
            Error::Network(_) | Error::Io(_) => true,
            Error::HttpStatus(code) => *code == 408 || *code == 429 || *code >= 500,
            _ => false,
        }
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;
