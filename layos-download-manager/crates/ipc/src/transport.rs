use std::future::Future;
use std::time::Duration;

use serde::Serialize;
use serde::de::DeserializeOwned;
use thiserror::Error;
use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};

use crate::protocol::{Request, Response};

/// Longest message accepted, so a broken client can't eat memory.
pub const MAX_MESSAGE: usize = 8 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("the download manager is not running")]
    NotRunning,
    #[error("another copy of the download manager is already running")]
    AlreadyRunning,
    #[error("the download manager did not answer in time")]
    Timeout,
    #[error("connection error: {0}")]
    Io(#[from] std::io::Error),
    #[error("bad message: {0}")]
    Json(#[from] serde_json::Error),
    #[error("message too long")]
    TooLong,
    #[error("the connection closed without an answer")]
    Closed,
}

/// The pipe (Windows) or socket (elsewhere) the app listens on.
/// `LDM_IPC_PATH` overrides it, which tests use.
pub fn default_path() -> String {
    if let Ok(p) = std::env::var("LDM_IPC_PATH")
        && !p.is_empty()
    {
        return p;
    }
    let user: String = std::env::var("USERNAME")
        .or_else(|_| std::env::var("USER"))
        .unwrap_or_else(|_| "user".into())
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
        .collect();
    #[cfg(windows)]
    {
        format!(r"\\.\pipe\layos-download-manager-{user}")
    }
    #[cfg(not(windows))]
    {
        let dir = std::env::var("XDG_RUNTIME_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::env::temp_dir());
        dir.join(format!("layos-download-manager-{user}.sock"))
            .to_string_lossy()
            .into_owned()
    }
}

pub async fn write_message<W: AsyncWrite + Unpin, T: Serialize>(
    w: &mut W,
    msg: &T,
) -> Result<(), IpcError> {
    let mut line = serde_json::to_vec(msg)?;
    line.push(b'\n');
    w.write_all(&line).await?;
    w.flush().await?;
    Ok(())
}

/// Reads one line. `Ok(None)` when the other side closed cleanly.
pub async fn read_message<R: AsyncRead + Unpin, T: DeserializeOwned>(
    r: &mut BufReader<R>,
) -> Result<Option<T>, IpcError> {
    let mut line = Vec::new();
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            return if line.is_empty() {
                Ok(None)
            } else {
                Err(IpcError::Closed)
            };
        }
        if let Some(i) = buf.iter().position(|&b| b == b'\n') {
            line.extend_from_slice(&buf[..i]);
            r.consume(i + 1);
            break;
        }
        let n = buf.len();
        line.extend_from_slice(buf);
        r.consume(n);
        if line.len() > MAX_MESSAGE {
            return Err(IpcError::TooLong);
        }
    }
    if line.len() > MAX_MESSAGE {
        return Err(IpcError::TooLong);
    }
    Ok(Some(serde_json::from_slice(&line)?))
}

/// Sends one request to the app and waits for the answer.
pub async fn request(path: &str, req: &Request, timeout: Duration) -> Result<Response, IpcError> {
    let work = async {
        let stream = connect(path).await?;
        let (r, mut w) = tokio::io::split(stream);
        write_message(&mut w, req).await?;
        let mut r = BufReader::new(r);
        read_message::<_, Response>(&mut r)
            .await?
            .ok_or(IpcError::Closed)
    };
    tokio::time::timeout(timeout, work)
        .await
        .map_err(|_| IpcError::Timeout)?
}

#[cfg(windows)]
async fn connect(path: &str) -> Result<tokio::net::windows::named_pipe::NamedPipeClient, IpcError> {
    use tokio::net::windows::named_pipe::ClientOptions;
    // ERROR_PIPE_BUSY: all instances are busy, wait a moment.
    const ERROR_PIPE_BUSY: i32 = 231;
    for _ in 0..50 {
        match ClientOptions::new().open(path) {
            Ok(c) => return Ok(c),
            Err(e) if e.raw_os_error() == Some(ERROR_PIPE_BUSY) => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Err(IpcError::NotRunning),
            Err(e) => return Err(e.into()),
        }
    }
    Err(IpcError::Timeout)
}

#[cfg(not(windows))]
async fn connect(path: &str) -> Result<tokio::net::UnixStream, IpcError> {
    match tokio::net::UnixStream::connect(path).await {
        Ok(s) => Ok(s),
        Err(e)
            if matches!(
                e.kind(),
                std::io::ErrorKind::NotFound | std::io::ErrorKind::ConnectionRefused
            ) =>
        {
            Err(IpcError::NotRunning)
        }
        Err(e) => Err(e.into()),
    }
}

/// Is the app listening?
pub async fn is_running(path: &str) -> bool {
    matches!(
        request(path, &Request::Ping, Duration::from_secs(2)).await,
        Ok(Response::Pong { .. })
    )
}

/// Serves requests until the process ends. Fails with `AlreadyRunning` if another
/// process already owns the pipe, which the app uses to stay a single instance.
pub async fn serve<H, F>(path: &str, handler: H) -> Result<(), IpcError>
where
    H: Fn(Request) -> F + Clone + Send + Sync + 'static,
    F: Future<Output = Response> + Send + 'static,
{
    let mut listener = Listener::bind(path).await?;
    loop {
        let conn = listener.accept().await?;
        let handler = handler.clone();
        tokio::spawn(async move {
            let (r, mut w) = tokio::io::split(conn);
            let mut r = BufReader::new(r);
            loop {
                let resp = match read_message::<_, Request>(&mut r).await {
                    Ok(Some(req)) => handler(req).await,
                    Ok(None) => break,
                    Err(IpcError::Json(e)) => Response::error(format!("bad message: {e}")),
                    Err(_) => break,
                };
                if write_message(&mut w, &resp).await.is_err() {
                    break;
                }
            }
        });
    }
}

#[cfg(windows)]
struct Listener {
    path: String,
    next: tokio::net::windows::named_pipe::NamedPipeServer,
}

#[cfg(windows)]
impl Listener {
    async fn bind(path: &str) -> Result<Self, IpcError> {
        use tokio::net::windows::named_pipe::ServerOptions;
        let next = ServerOptions::new()
            .first_pipe_instance(true)
            .reject_remote_clients(true)
            .create(path)
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::PermissionDenied {
                    IpcError::AlreadyRunning
                } else {
                    e.into()
                }
            })?;
        Ok(Self {
            path: path.to_string(),
            next,
        })
    }

    async fn accept(
        &mut self,
    ) -> Result<tokio::net::windows::named_pipe::NamedPipeServer, IpcError> {
        use tokio::net::windows::named_pipe::ServerOptions;
        self.next.connect().await?;
        let fresh = ServerOptions::new()
            .reject_remote_clients(true)
            .create(&self.path)?;
        Ok(std::mem::replace(&mut self.next, fresh))
    }
}

#[cfg(not(windows))]
struct Listener {
    inner: tokio::net::UnixListener,
}

#[cfg(not(windows))]
impl Listener {
    async fn bind(path: &str) -> Result<Self, IpcError> {
        use std::os::unix::fs::PermissionsExt;
        if std::path::Path::new(path).exists() {
            if is_running(path).await {
                return Err(IpcError::AlreadyRunning);
            }
            // Left over from a crash.
            std::fs::remove_file(path)?;
        }
        let inner = tokio::net::UnixListener::bind(path)?;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self { inner })
    }

    async fn accept(&mut self) -> Result<tokio::net::UnixStream, IpcError> {
        Ok(self.inner.accept().await?.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::BrowserDownload;

    #[tokio::test]
    async fn round_trip_and_single_instance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("ldm.sock").to_string_lossy().into_owned();
        assert!(matches!(
            request(&path, &Request::Ping, Duration::from_secs(1)).await,
            Err(IpcError::NotRunning)
        ));

        let p = path.clone();
        tokio::spawn(async move {
            serve(&p, |req| async move {
                match req {
                    Request::Ping => Response::Pong {
                        app_running: true,
                        app_version: Some("test".into()),
                        host_version: None,
                    },
                    Request::Download { items, .. } => Response::Accepted {
                        ids: (0..items.len() as i64).collect(),
                    },
                    Request::ShowApp => Response::Ok,
                }
            })
            .await
            .unwrap();
        });
        for _ in 0..100 {
            if is_running(&path).await {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        let req = Request::Download {
            items: vec![
                BrowserDownload {
                    url: "https://x/a".into(),
                    ..Default::default()
                };
                3
            ],
            source: Default::default(),
            ask: None,
        };
        assert_eq!(
            request(&path, &req, Duration::from_secs(2)).await.unwrap(),
            Response::Accepted { ids: vec![0, 1, 2] }
        );
        assert!(matches!(
            serve(&path, |_| async { Response::Ok }).await,
            Err(IpcError::AlreadyRunning)
        ));
    }
}
