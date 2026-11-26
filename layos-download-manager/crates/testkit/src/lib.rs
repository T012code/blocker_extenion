//! A tiny HTTP/1.1 file server for tests. It can refuse ranges, hide the size, go slow,
//! drop connections part-way, and refuse links, so every engine path can be exercised.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

#[derive(Clone)]
pub struct ServerOptions {
    pub ranges: bool,
    pub send_length: bool,
    /// Bytes per second per connection.
    pub throttle: Option<u64>,
    /// Responses starting at or after this offset go at this (slower) rate: (offset, bytes/s).
    pub slow_tail: Option<(u64, u64)>,
    /// The first `drops` responses are cut off after this many body bytes.
    pub drop_after: Option<u64>,
    pub drops: usize,
    pub content_disposition: Option<String>,
    pub content_type: Option<String>,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self {
            ranges: true,
            send_length: true,
            throttle: None,
            slow_tail: None,
            drop_after: None,
            drops: 0,
            content_disposition: None,
            content_type: Some("application/octet-stream".into()),
        }
    }
}

pub struct TestServer {
    pub base: String,
    pub state: Arc<State>,
}

pub struct State {
    pub body: Mutex<Arc<Vec<u8>>>,
    pub options: Mutex<ServerOptions>,
    pub requests: AtomicUsize,
    pub active: AtomicUsize,
    pub max_active: AtomicUsize,
    pub drops_left: AtomicUsize,
    /// `Range` headers received, in order.
    pub ranges_seen: Mutex<Vec<String>>,
}

impl TestServer {
    pub async fn start(body: Vec<u8>, options: ServerOptions) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let state = Arc::new(State {
            body: Mutex::new(Arc::new(body)),
            drops_left: AtomicUsize::new(options.drops),
            options: Mutex::new(options),
            requests: AtomicUsize::new(0),
            active: AtomicUsize::new(0),
            max_active: AtomicUsize::new(0),
            ranges_seen: Mutex::new(Vec::new()),
        });
        let s = state.clone();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let s = s.clone();
                tokio::spawn(async move {
                    let _ = handle(sock, s).await;
                });
            }
        });
        Self {
            base: format!("http://{addr}"),
            state,
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}{}", self.base, path)
    }

    pub fn set_options(&self, f: impl FnOnce(&mut ServerOptions)) {
        f(&mut self.state.options.lock().unwrap());
    }

    pub fn set_body(&self, body: Vec<u8>) {
        *self.state.body.lock().unwrap() = Arc::new(body);
    }
}

/// Deterministic, non-repeating-looking test data.
pub fn test_data(len: usize) -> Vec<u8> {
    let mut x: u32 = 0x1234_5678;
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            (x & 0xff) as u8
        })
        .collect()
}

async fn handle(mut sock: TcpStream, state: Arc<State>) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let mut lines = head.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let path = request_line
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .to_string();
    let mut range = None;
    for line in lines {
        if let Some((k, v)) = line.split_once(':')
            && k.eq_ignore_ascii_case("range")
        {
            range = Some(v.trim().to_string());
        }
    }
    state.requests.fetch_add(1, Ordering::SeqCst);
    if let Some(r) = &range {
        state.ranges_seen.lock().unwrap().push(r.clone());
    }

    let path_only = path.split('?').next().unwrap_or("/");
    match path_only {
        "/expired" => return respond_status(&mut sock, "403 Forbidden").await,
        "/missing" => return respond_status(&mut sock, "404 Not Found").await,
        "/redirect" => {
            let resp = "HTTP/1.1 302 Found\r\nLocation: /files/redirected.bin\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
            return sock.write_all(resp.as_bytes()).await;
        }
        _ => {}
    }

    let opts = state.options.lock().unwrap().clone();
    let body = state.body.lock().unwrap().clone();
    let total = body.len() as u64;

    let mut status = "200 OK";
    let (mut from, mut to) = (0u64, total); // to is exclusive
    let mut content_range = None;
    if opts.ranges
        && let Some(r) = range.as_deref().and_then(|r| r.strip_prefix("bytes="))
    {
        let (a, b) = r.split_once('-').unwrap();
        let a: u64 = a.parse().unwrap();
        let b: u64 = if b.is_empty() {
            total.saturating_sub(1)
        } else {
            b.parse::<u64>().unwrap().min(total.saturating_sub(1))
        };
        if a >= total && total > 0 {
            return respond_status(&mut sock, "416 Range Not Satisfiable").await;
        }
        from = a;
        to = b + 1;
        status = "206 Partial Content";
        content_range = Some(format!("bytes {a}-{b}/{total}"));
    }

    let mut head = format!("HTTP/1.1 {status}\r\nConnection: close\r\n");
    if opts.ranges {
        head.push_str("Accept-Ranges: bytes\r\n");
    }
    if let Some(cr) = &content_range {
        head.push_str(&format!("Content-Range: {cr}\r\n"));
    }
    if opts.send_length {
        head.push_str(&format!("Content-Length: {}\r\n", to - from));
    }
    if let Some(ct) = &opts.content_type {
        head.push_str(&format!("Content-Type: {ct}\r\n"));
    }
    if let Some(cd) = &opts.content_disposition {
        head.push_str(&format!("Content-Disposition: {cd}\r\n"));
    }
    head.push_str("\r\n");
    sock.write_all(head.as_bytes()).await?;

    let now = state.active.fetch_add(1, Ordering::SeqCst) + 1;
    state.max_active.fetch_max(now, Ordering::SeqCst);
    let res = send_body(
        &mut sock,
        &body[from as usize..to as usize],
        from,
        &opts,
        &state,
    )
    .await;
    state.active.fetch_sub(1, Ordering::SeqCst);
    res
}

async fn send_body(
    sock: &mut TcpStream,
    data: &[u8],
    from: u64,
    opts: &ServerOptions,
    state: &State,
) -> std::io::Result<()> {
    let rate = match opts.slow_tail {
        Some((offset, slow)) if from >= offset => Some(slow),
        _ => opts.throttle,
    };
    // Only cut off responses that are long enough to cut, so the probe isn't used up.
    let cut = match opts.drop_after {
        Some(n) if (data.len() as u64) > n => {
            let took = state
                .drops_left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |d| d.checked_sub(1))
                .is_ok();
            if took { Some(n as usize) } else { None }
        }
        _ => None,
    };
    let data = match cut {
        Some(n) => &data[..n],
        None => data,
    };
    let chunk = 16 * 1024;
    for part in data.chunks(chunk) {
        sock.write_all(part).await?;
        if let Some(rate) = rate {
            tokio::time::sleep(Duration::from_secs_f64(part.len() as f64 / rate as f64)).await;
        }
    }
    sock.flush().await?;
    if cut.is_some() {
        // Close without sending the rest.
        let _ = sock.shutdown().await;
    }
    Ok(())
}

async fn respond_status(sock: &mut TcpStream, status: &str) -> std::io::Result<()> {
    let resp = format!("HTTP/1.1 {status}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    sock.write_all(resp.as_bytes()).await
}

/// Serves the files in a folder over HTTP, with byte ranges. For media tests.
pub struct StaticServer {
    pub base: String,
}

impl StaticServer {
    pub async fn start(dir: std::path::PathBuf) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((sock, _)) = listener.accept().await else {
                    break;
                };
                let dir = dir.clone();
                tokio::spawn(async move {
                    let _ = serve_file(sock, &dir).await;
                });
            }
        });
        Self {
            base: format!("http://{addr}"),
        }
    }

    pub fn url(&self, path: &str) -> String {
        format!("{}/{}", self.base, path.trim_start_matches('/'))
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "m3u8" => "application/vnd.apple.mpegurl",
        "mpd" => "application/dash+xml",
        "ts" => "video/mp2t",
        "mp4" | "m4s" => "video/mp4",
        "webm" => "video/webm",
        "html" => "text/html; charset=utf-8",
        "js" => "text/javascript",
        _ => "application/octet-stream",
    }
}

async fn serve_file(mut sock: TcpStream, dir: &std::path::Path) -> std::io::Result<()> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
        let n = sock.read(&mut tmp).await?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&tmp[..n]);
    }
    let head = String::from_utf8_lossy(&buf).to_string();
    let path = head
        .split_whitespace()
        .nth(1)
        .unwrap_or("/")
        .split('?')
        .next()
        .unwrap_or("/")
        .to_string();
    let range = head.split("\r\n").find_map(|l| {
        l.split_once(':')
            .filter(|(k, _)| k.eq_ignore_ascii_case("range"))
            .map(|(_, v)| v.trim().to_string())
    });
    let rel = path.trim_start_matches('/');
    if rel.contains("..") {
        return sock
            .write_all(b"HTTP/1.1 403 Forbidden\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    }
    let Ok(body) = std::fs::read(dir.join(rel)) else {
        return sock
            .write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await;
    };
    let total = body.len();
    let (status, from, to) = match range
        .as_deref()
        .and_then(|r| r.strip_prefix("bytes="))
        .and_then(|r| r.split_once('-'))
    {
        Some((a, b)) => {
            let a: usize = a.parse().unwrap_or(0);
            let b: usize = if b.is_empty() {
                total.saturating_sub(1)
            } else {
                b.parse::<usize>().unwrap_or(0).min(total.saturating_sub(1))
            };
            ("206 Partial Content", a, b + 1)
        }
        None => ("200 OK", 0, total),
    };
    let mut head = format!(
        "HTTP/1.1 {status}\r\nConnection: close\r\nAccept-Ranges: bytes\r\nContent-Type: {}\r\nContent-Length: {}\r\n",
        content_type(rel),
        to - from
    );
    if status.starts_with("206") {
        head.push_str(&format!(
            "Content-Range: bytes {from}-{}/{total}\r\n",
            to - 1
        ));
    }
    head.push_str("\r\n");
    sock.write_all(head.as_bytes()).await?;
    sock.write_all(&body[from..to]).await?;
    sock.flush().await
}

/// A SOCKS5 proxy for privacy tests. It records where every connection went, and can be
/// switched off (cutting live connections, like a VPN dropping) and on again.
pub struct Socks5Proxy {
    pub addr: std::net::SocketAddr,
    /// Destinations of every CONNECT request, in order.
    pub connections: Arc<Mutex<Vec<String>>>,
    stop: Mutex<tokio_util::sync::CancellationToken>,
}

impl Socks5Proxy {
    pub async fn start() -> Arc<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let proxy = Arc::new(Self {
            addr,
            connections: Arc::default(),
            stop: Mutex::new(tokio_util::sync::CancellationToken::new()),
        });
        proxy.serve(listener);
        proxy
    }

    pub fn url(&self) -> String {
        format!("socks5://{}", self.addr)
    }

    fn serve(self: &Arc<Self>, listener: TcpListener) {
        let token = self.stop.lock().unwrap().clone();
        let log = self.connections.clone();
        tokio::spawn(async move {
            loop {
                let sock = tokio::select! {
                    _ = token.cancelled() => break,
                    r = listener.accept() => match r { Ok((s, _)) => s, Err(_) => break },
                };
                let (log, token) = (log.clone(), token.clone());
                tokio::spawn(async move {
                    tokio::select! {
                        _ = token.cancelled() => {}
                        _ = relay(sock, log) => {}
                    }
                });
            }
        });
    }

    /// Stops listening and cuts every open connection.
    pub fn switch_off(&self) {
        self.stop.lock().unwrap().cancel();
    }

    /// Listens again on the same address.
    pub async fn switch_on(self: &Arc<Self>) {
        *self.stop.lock().unwrap() = tokio_util::sync::CancellationToken::new();
        let mut tries = 0;
        let listener = loop {
            match TcpListener::bind(self.addr).await {
                Ok(l) => break l,
                Err(e) if tries < 50 => {
                    tries += 1;
                    let _ = e;
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
                Err(e) => panic!("can't listen again on {}: {e}", self.addr),
            }
        };
        self.serve(listener);
    }
}

async fn relay(mut c: TcpStream, log: Arc<Mutex<Vec<String>>>) -> std::io::Result<()> {
    // Greeting: VER NMETHODS METHODS... -> choose "no authentication".
    let mut head = [0u8; 2];
    c.read_exact(&mut head).await?;
    let mut methods = vec![0u8; head[1] as usize];
    c.read_exact(&mut methods).await?;
    c.write_all(&[5, 0]).await?;
    // Request: VER CMD RSV ATYP ADDR PORT
    let mut req = [0u8; 4];
    c.read_exact(&mut req).await?;
    let host = match req[3] {
        1 => {
            let mut a = [0u8; 4];
            c.read_exact(&mut a).await?;
            std::net::Ipv4Addr::from(a).to_string()
        }
        3 => {
            let mut len = [0u8; 1];
            c.read_exact(&mut len).await?;
            let mut name = vec![0u8; len[0] as usize];
            c.read_exact(&mut name).await?;
            String::from_utf8_lossy(&name).into_owned()
        }
        4 => {
            let mut a = [0u8; 16];
            c.read_exact(&mut a).await?;
            format!("[{}]", std::net::Ipv6Addr::from(a))
        }
        _ => return Ok(()),
    };
    let mut port = [0u8; 2];
    c.read_exact(&mut port).await?;
    let target = format!("{host}:{}", u16::from_be_bytes(port));
    log.lock().unwrap().push(target.clone());
    let Ok(mut upstream) = TcpStream::connect(&target).await else {
        c.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        return Ok(());
    };
    c.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
    tokio::io::copy_bidirectional(&mut c, &mut upstream).await?;
    Ok(())
}

/// Accepts TCP connections and only counts them: shows whether anything tried to connect.
pub struct Honeypot {
    pub addr: std::net::SocketAddr,
    pub hits: Arc<AtomicUsize>,
}

impl Honeypot {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let hits = Arc::new(AtomicUsize::new(0));
        let h = hits.clone();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                h.fetch_add(1, Ordering::SeqCst);
                drop(s);
            }
        });
        Self { addr, hits }
    }
}
