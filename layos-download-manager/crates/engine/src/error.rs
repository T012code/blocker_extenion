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
    /// The network went away (no connection, or it dropped and didn't come back).
    #[error("the internet connection was lost")]
    ConnectionLost,
    /// From a runner (torrent, media tool).
    #[error("{0}")]
    Other(String),
}

impl Error {
    /// The connection itself failed (no network, dropped, timed out), as opposed to
    /// the server answering with an error.
    pub fn is_connection_lost(&self) -> bool {
        match self {
            Error::ConnectionLost => true,
            Error::Network(e) => e.is_connect() || e.is_timeout() || e.is_request() || e.is_body(),
            Error::Io(e) => matches!(
                e.kind(),
                std::io::ErrorKind::ConnectionReset
                    | std::io::ErrorKind::ConnectionAborted
                    | std::io::ErrorKind::ConnectionRefused
                    | std::io::ErrorKind::NotConnected
                    | std::io::ErrorKind::TimedOut
                    | std::io::ErrorKind::BrokenPipe
                    | std::io::ErrorKind::NetworkUnreachable
                    | std::io::ErrorKind::HostUnreachable
                    | std::io::ErrorKind::NetworkDown
            ),
            _ => false,
        }
    }

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
