//! BitTorrent downloads for Layó's Download Manager, using the librqbit engine.
//!
//! A torrent is a [`DownloadKind::Torrent`] item in the normal download list, so it
//! pauses, continues and shows progress like any other download. Pausing keeps every
//! verified piece on disk; continuing checks what's there and fetches only the rest.
//! When the download finishes it keeps sharing ("seeding") until it has uploaded as
//! much as `seed_ratio` times its size, then stops.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, Weak};
use std::time::Duration;

use bytes::Bytes;
use futures_util::future::BoxFuture;
use ldm_engine::{
    DownloadId, DownloadKind, DownloadStatus, Error, Job, Result, Runner, TorrentSpec, filename,
};
use librqbit::api::TorrentIdOrHash;
use librqbit::limits::LimitsConfig;
use librqbit::{
    AddTorrent, AddTorrentOptions, AddTorrentResponse, ConnectionOptions, ListOnlyResponse,
    ListenerOptions, ManagedTorrent, Session, SessionOptions,
};
use tokio::sync::watch;

pub use librqbit::{CreateTorrentOptions, create_torrent};

pub mod privacy;
use privacy::{Plan, Privacy, Protection};

#[derive(Debug, Clone)]
pub struct TorrentOptions {
    /// Where copies of .torrent files are kept, so magnets don't need fetching again.
    pub data_dir: PathBuf,
    /// Port for incoming connections. `None` picks a free one.
    pub listen_port: Option<u16>,
    /// Find peers through the DHT and local network (off in tests).
    pub dht: bool,
    /// Ask the router to open the port (UPnP).
    pub upnp: bool,
    /// Keep sharing until uploaded / size reaches this; 0 stops as soon as it's done.
    pub seed_ratio: f32,
    /// Download / upload limits in bytes per second, `None` = unlimited.
    pub download_limit: Option<u32>,
    pub upload_limit: Option<u32>,
    /// Peers to try straight away, besides trackers and the DHT (tests).
    pub extra_peers: Vec<SocketAddr>,
    /// VPN / proxy protection and what's shared. See [`privacy`].
    pub privacy: Privacy,
    /// How often the kill switch checks the VPN or proxy.
    pub check_every: Duration,
}

impl TorrentOptions {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            listen_port: None,
            dht: true,
            upnp: true,
            seed_ratio: 1.0,
            download_limit: None,
            upload_limit: None,
            extra_peers: Vec::new(),
            privacy: Privacy::default(),
            check_every: Duration::from_secs(1),
        }
    }
}

pub struct TorrentRunner {
    session: Arc<Session>,
    options: TorrentOptions,
    /// Torrents in the session, by download id.
    handles: Mutex<HashMap<DownloadId, Arc<ManagedTorrent>>>,
    /// Lets `run` (which only gets `&self`) hand an owned runner to the download task.
    me: Weak<Self>,
    plan: Plan,
    /// The kill switch: whether torrents may run right now.
    guard: watch::Receiver<Protection>,
}

fn other(e: impl std::fmt::Display) -> Error {
    Error::Other(e.to_string())
}

fn limits(down: Option<u32>, up: Option<u32>) -> LimitsConfig {
    LimitsConfig {
        download_bps: down.and_then(NonZeroU32::new),
        upload_bps: up.and_then(NonZeroU32::new),
    }
}

impl TorrentRunner {
    pub async fn start(options: TorrentOptions) -> anyhow::Result<Arc<Self>> {
        std::fs::create_dir_all(options.data_dir.join("torrents"))?;
        // Some computers have IPv6 turned off; then everything must use IPv4.
        let ipv4_only = std::net::UdpSocket::bind("[::]:0").is_err();
        let plan = privacy::plan(&options.privacy, cfg!(windows)).map_err(anyhow::Error::msg)?;
        // In VPN mode, only accept incoming connections on the VPN's own address.
        let listen_ip = match &options.privacy.mode {
            privacy::PrivacyMode::Vpn { interface } => privacy::interface_ip(interface),
            _ => None,
        }
        .unwrap_or(Ipv4Addr::UNSPECIFIED.into());
        let blocklist_url = options
            .privacy
            .blocklist
            .as_deref()
            .map(str::trim)
            .filter(|b| !b.is_empty())
            .map(|b| {
                if b.contains("://") {
                    b.to_string()
                } else {
                    format!("file://{}", Path::new(b).display())
                }
            });
        let session = Session::new_with_opts(
            options.data_dir.join("incoming"),
            SessionOptions {
                dht: (plan.dht && options.dht).then(Default::default),
                disable_local_service_discovery: !(plan.local_discovery && options.dht),
                persistence: None,
                fastresume: false,
                listen: plan.listen.then(|| ListenerOptions {
                    listen_addr: SocketAddr::new(listen_ip, options.listen_port.unwrap_or(0)),
                    enable_upnp_port_forwarding: plan.upnp && options.upnp,
                    ipv4_only,
                    ..Default::default()
                }),
                connect: Some(ConnectionOptions {
                    proxy_url: plan.proxy.clone(),
                    ..Default::default()
                }),
                bind_device_name: plan.bind_device.clone(),
                blocklist_url,
                ipv4_only,
                ratelimits: limits(options.download_limit, options.upload_limit),
                ..Default::default()
            },
        )
        .await?;
        let guard = privacy::watch(options.privacy.clone(), options.check_every);
        Ok(Arc::new_cyclic(|me| Self {
            session,
            options,
            handles: Mutex::new(HashMap::new()),
            me: me.clone(),
            plan,
            guard,
        }))
    }

    /// Pauses every torrent and closes the session (before replacing it with new settings).
    pub async fn shutdown(&self) {
        let handles: Vec<_> = self
            .handles
            .lock()
            .unwrap()
            .drain()
            .map(|(_, h)| h)
            .collect();
        for h in handles {
            self.pause(&h).await;
        }
        self.session.stop().await;
    }

    /// The address other peers can reach this session on.
    pub fn listen_addr(&self) -> Option<SocketAddr> {
        self.session.listen_addr()
    }

    fn source<'a>(&self, spec: &TorrentSpec, url: &'a str) -> Result<AddTorrent<'a>> {
        if let Some(path) = spec.torrent_file.as_ref().filter(|p| p.exists()) {
            return Ok(AddTorrent::TorrentFileBytes(
                self.clean(Bytes::from(std::fs::read(path)?)),
            ));
        }
        if let Some(path) = url.strip_prefix("file://") {
            return Ok(AddTorrent::TorrentFileBytes(
                self.clean(Bytes::from(std::fs::read(path)?)),
            ));
        }
        if self.plan.strip_udp_trackers && url.starts_with("magnet:") {
            return Ok(AddTorrent::Url(
                privacy::strip_udp_trackers_magnet(url).into(),
            ));
        }
        Ok(AddTorrent::Url(url.into()))
    }

    /// Removes trackers that would bypass the proxy.
    fn clean(&self, torrent: Bytes) -> Bytes {
        if !self.plan.strip_udp_trackers {
            return torrent;
        }
        privacy::strip_udp_trackers_torrent(&torrent)
            .map(Bytes::from)
            .unwrap_or(torrent)
    }

    /// Waits until the kill switch allows torrents, showing why while it waits.
    /// Returns false if the download was paused meanwhile.
    async fn wait_protected(&self, job: &Job, guard: &mut watch::Receiver<Protection>) -> bool {
        loop {
            let state = guard.borrow_and_update().clone();
            match state {
                Protection::Ok => return true,
                Protection::Blocked(reason) => {
                    job.report.update(|d| {
                        d.status = DownloadStatus::Blocked;
                        d.error = Some(reason.clone());
                    });
                }
            }
            tokio::select! {
                _ = job.cancel.cancelled() => return false,
                r = guard.changed() => if r.is_err() { return false },
            }
        }
    }

    /// Reads the torrent's details (fetching them from peers for a magnet link).
    async fn metadata(&self, spec: &TorrentSpec, url: &str) -> Result<ListOnlyResponse> {
        let opts = AddTorrentOptions {
            list_only: true,
            initial_peers: (!self.options.extra_peers.is_empty())
                .then(|| self.options.extra_peers.clone()),
            ..Default::default()
        };
        match self
            .session
            .add_torrent(self.source(spec, url)?, Some(opts))
            .await
            .map_err(other)?
        {
            AddTorrentResponse::ListOnly(info) => Ok(info),
            _ => Err(other("unexpected answer while reading the torrent")),
        }
    }

    async fn run_job(self: Arc<Self>, job: Job) -> Result<()> {
        let id = job.report.id();
        let d = job.download.clone();
        let DownloadKind::Torrent(mut spec) = d.kind.clone() else {
            return Err(other("not a torrent"));
        };
        let cancel = job.cancel.clone();

        // 0. Kill switch: nothing touches the network until the VPN or proxy is in place.
        let mut guard = self.guard.clone();
        if !self.wait_protected(&job, &mut guard).await {
            return Err(Error::Cancelled);
        }
        job.report.update(|d| d.error = None);

        // 1. What's in the torrent? For a magnet this asks peers, which can take a while.
        let meta = tokio::select! {
            _ = cancel.cancelled() => return Err(Error::Cancelled),
            m = self.metadata(&spec, &d.request.url) => m?,
        };
        let name = meta
            .info
            .name()
            .map(|n| n.into_owned())
            .unwrap_or_else(|| "torrent".into());
        let files = meta.info.iter_file_lengths().count();
        let total = meta.info.lengths().total_length();
        let hash = meta.info_hash.as_string();
        if spec.torrent_file.is_none() {
            let path = self
                .options
                .data_dir
                .join("torrents")
                .join(format!("{hash}.torrent"));
            std::fs::write(&path, &meta.torrent_bytes)?;
            spec.torrent_file = Some(path);
        }
        spec.info_hash = Some(hash);
        let safe_name = filename::sanitize(&name);
        // A torrent with several files gets its own folder, named after it.
        let output = if files > 1 {
            d.save_dir.join(&safe_name)
        } else {
            d.save_dir.clone()
        };
        let size = match &spec.only_files {
            Some(only) => meta
                .info
                .iter_file_lengths()
                .enumerate()
                .filter(|(i, _)| only.contains(i))
                .map(|(_, l)| l)
                .sum(),
            None => total,
        };
        job.report.update(|rec| {
            rec.file_name = safe_name.clone();
            rec.size = Some(size);
            rec.kind = DownloadKind::Torrent(spec.clone());
        });

        // 2. Add it for real (or wake it up if it's already in the session).
        // Continuing after a pause: take the old torrent out of the session (keeping its
        // files) and add it again. Pieces that were mid-transfer at the pause otherwise
        // stay marked as "being fetched" and the download can stall one piece short.
        let stale = self.handles.lock().unwrap().remove(&id);
        if let Some(old) = stale {
            let _ = self
                .session
                .delete(TorrentIdOrHash::Id(old.id()), false)
                .await;
        }
        std::fs::create_dir_all(&output)?;
        let mut peers = meta.seen_peers.clone();
        peers.extend(self.options.extra_peers.iter().copied());
        let opts = AddTorrentOptions {
            output_folder: Some(output.to_string_lossy().into_owned()),
            only_files: spec.only_files.clone(),
            // Files already on disk are checked and kept, which is how continuing works.
            overwrite: true,
            initial_peers: (!peers.is_empty()).then_some(peers),
            ratelimits: limits(
                (d.speed_limit > 0).then(|| d.speed_limit.min(u32::MAX as u64) as u32),
                None,
            ),
            ..Default::default()
        };
        let resp = self
            .session
            .add_torrent(
                AddTorrent::TorrentFileBytes(self.clean(meta.torrent_bytes.clone())),
                Some(opts),
            )
            .await
            .map_err(other)?;
        let handle = resp
            .into_handle()
            .ok_or_else(|| other("the torrent could not be added"))?;
        self.handles.lock().unwrap().insert(id, handle.clone());
        if handle.is_paused() {
            self.session.unpause(&handle).await.map_err(other)?;
        }
        job.report.set_status(DownloadStatus::Downloading);

        // 3. Report progress until done (and done sharing), or paused.
        let seed_ratio = spec.seed_ratio.unwrap_or(self.options.seed_ratio);
        let mut seeding = false;
        loop {
            let stats = handle.stats();
            if let Some(e) = &stats.error {
                self.pause(&handle).await;
                return Err(other(e));
            }
            let speed = stats
                .live
                .as_ref()
                .map_or(0, |l| l.download_speed.as_bytes());
            job.report.progress(
                stats.progress_bytes,
                Some(stats.total_bytes),
                if stats.finished { 0 } else { speed },
            );
            let uploaded = stats.uploaded_bytes;
            if uploaded != spec.uploaded {
                spec.uploaded = uploaded;
                let s = spec.clone();
                job.report.update(|rec| rec.kind = DownloadKind::Torrent(s));
            }
            if stats.finished {
                let ratio = if stats.total_bytes == 0 {
                    f32::MAX
                } else {
                    uploaded as f32 / stats.total_bytes as f32
                };
                if seed_ratio <= 0.0 || ratio >= seed_ratio {
                    self.pause(&handle).await;
                    return Ok(());
                }
                if !seeding {
                    seeding = true;
                    job.report.set_status(DownloadStatus::Seeding);
                }
            }
            tokio::select! {
                _ = cancel.cancelled() => {
                    self.pause(&handle).await;
                    // Stopping a torrent that's only sharing leaves it complete.
                    return if stats.finished { Ok(()) } else { Err(Error::Cancelled) };
                }
                // Kill switch: the VPN or proxy dropped. Stop at once, wait, then carry on.
                _ = guard.changed() => {
                    if *guard.borrow() != Protection::Ok {
                        self.pause(&handle).await;
                        if !self.wait_protected(&job, &mut guard).await {
                            return if stats.finished { Ok(()) } else { Err(Error::Cancelled) };
                        }
                        self.session.unpause(&handle).await.map_err(other)?;
                        job.report.update(|d| {
                            d.error = None;
                            d.status = if seeding { DownloadStatus::Seeding } else { DownloadStatus::Downloading };
                        });
                    }
                }
                _ = tokio::time::sleep(Duration::from_millis(500)) => {}
            }
        }
    }

    async fn pause(&self, handle: &Arc<ManagedTorrent>) {
        if !handle.is_paused() {
            let _ = self.session.pause(handle).await;
        }
    }
}

impl Runner for TorrentRunner {
    fn run(&self, job: Job) -> BoxFuture<'static, Result<()>> {
        match self.me.upgrade() {
            Some(me) => Box::pin(me.run_job(job)),
            None => Box::pin(async { Err(other("torrent support has shut down")) }),
        }
    }

    fn removed(&self, download: &ldm_engine::Download, delete_files: bool) {
        let handle = self.handles.lock().unwrap().remove(&download.id);
        let session = self.session.clone();
        let save_dir = download.save_dir.clone();
        let name = download.file_name.clone();
        tokio::spawn(async move {
            if let Some(h) = handle {
                let _ = session
                    .delete(TorrentIdOrHash::Id(h.id()), delete_files)
                    .await;
            } else if delete_files {
                remove_path(&save_dir.join(name));
            }
        });
        if let DownloadKind::Torrent(spec) = &download.kind
            && let Some(f) = &spec.torrent_file
        {
            let _ = std::fs::remove_file(f);
        }
    }
}

fn remove_path(p: &Path) {
    if p.is_dir() {
        let _ = std::fs::remove_dir_all(p);
    } else {
        let _ = std::fs::remove_file(p);
    }
}
