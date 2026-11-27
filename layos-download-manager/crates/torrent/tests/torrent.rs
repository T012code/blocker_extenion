//! Real BitTorrent transfers between a seeding librqbit session and the download
//! manager, over localhost (no trackers, no DHT).

use std::net::{Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::Bytes;
use ldm_engine::{
    DownloadKind, DownloadStatus, Engine, EngineConfig, Event, NewDownload, RequestInfo,
    TorrentSpec,
};
use ldm_testkit::{Honeypot, Socks5Proxy};
use ldm_torrent::privacy::{Privacy, PrivacyMode};
use ldm_torrent::{CreateTorrentOptions, TorrentOptions, TorrentRunner, create_torrent};
use librqbit::limits::LimitsConfig;
use librqbit::spawn_utils::BlockingSpawner;
use librqbit::{AddTorrent, AddTorrentOptions, ListenerOptions, Session, SessionOptions};

fn data(len: usize, seed: u32) -> Vec<u8> {
    let mut x = seed.max(1);
    (0..len)
        .map(|_| {
            x ^= x << 13;
            x ^= x >> 17;
            x ^= x << 5;
            x as u8
        })
        .collect()
}

struct Seeder {
    _session: Arc<Session>,
    addr: SocketAddr,
    torrent: Bytes,
    hash: String,
}

/// Shares `path` (a file or a folder) from its own session, optionally slowly.
async fn seed(path: &Path, upload_bps: Option<u32>) -> Seeder {
    let created = create_torrent(
        path,
        CreateTorrentOptions {
            piece_length: Some(64 * 1024),
            ..Default::default()
        },
        &BlockingSpawner::new(1),
    )
    .await
    .unwrap();
    let torrent = created.as_bytes().unwrap();
    let hash = created.info_hash().as_string();
    let session = Session::new_with_opts(
        path.parent().unwrap().join("seeder-session"),
        SessionOptions {
            dht: None,
            disable_local_service_discovery: true,
            persistence: None,
            listen: Some(ListenerOptions {
                listen_addr: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                enable_upnp_port_forwarding: false,
                ipv4_only: true,
                ..Default::default()
            }),
            ipv4_only: true,
            ratelimits: LimitsConfig {
                upload_bps: upload_bps.and_then(NonZeroU32::new),
                download_bps: None,
            },
            ..Default::default()
        },
    )
    .await
    .unwrap();
    // The data is already there: point the seeder at the folder holding it.
    let output = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent().unwrap().to_path_buf()
    };
    let handle = session
        .add_torrent(
            AddTorrent::TorrentFileBytes(torrent.clone()),
            Some(AddTorrentOptions {
                output_folder: Some(output.to_string_lossy().into_owned()),
                overwrite: true,
                ..Default::default()
            }),
        )
        .await
        .unwrap()
        .into_handle()
        .unwrap();
    handle.wait_until_completed().await.unwrap();
    let port = session.listen_addr().unwrap().port();
    Seeder {
        _session: session,
        addr: SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        torrent,
        hash,
    }
}

async fn manager(dir: &Path, peer: SocketAddr, seed_ratio: f32) -> Engine {
    manager_with(
        dir,
        peer,
        seed_ratio,
        Privacy {
            mode: PrivacyMode::Standard,
            require_protection: false,
            ..Default::default()
        },
    )
    .await
}

async fn manager_with(dir: &Path, peer: SocketAddr, seed_ratio: f32, privacy: Privacy) -> Engine {
    let engine = Engine::open(EngineConfig::new(dir.join("Downloads"))).unwrap();
    let mut opts = TorrentOptions::new(dir.join("ldm-data"));
    opts.dht = false;
    opts.upnp = false;
    opts.seed_ratio = seed_ratio;
    opts.extra_peers = vec![peer];
    opts.privacy = privacy;
    opts.check_every = Duration::from_millis(200);
    engine.set_runner("torrent", TorrentRunner::start(opts).await.unwrap());
    engine
}

async fn wait_status(engine: &Engine, id: i64, status: DownloadStatus, secs: u64) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        let d = engine.get(id).unwrap();
        if d.status == status {
            return;
        }
        if d.status == DownloadStatus::Failed {
            panic!("torrent failed: {:?}", d.error);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never reached {status:?}: {:?}", engine.get(id).unwrap());
}

fn add_magnet(hash: &str, name: &str) -> NewDownload {
    NewDownload {
        request: RequestInfo::new(format!("magnet:?xt=urn:btih:{hash}&dn={name}")),
        kind: DownloadKind::Torrent(TorrentSpec::default()),
        start: true,
        ..Default::default()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_from_a_torrent_file() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("seed");
    std::fs::create_dir_all(&src).unwrap();
    let content = data(3 * 1024 * 1024 + 777, 7);
    std::fs::write(src.join("ubuntu-mini.iso"), &content).unwrap();
    let seeder = seed(&src.join("ubuntu-mini.iso"), None).await;

    let torrent_path = dir.path().join("ubuntu-mini.iso.torrent");
    std::fs::write(&torrent_path, &seeder.torrent).unwrap();
    let engine = manager(dir.path(), seeder.addr, 0.0).await;
    let id = engine
        .add(NewDownload {
            request: RequestInfo::new(format!("file://{}", torrent_path.display())),
            kind: DownloadKind::Torrent(TorrentSpec {
                torrent_file: Some(torrent_path.clone()),
                ..Default::default()
            }),
            start: true,
            ..Default::default()
        })
        .await
        .unwrap();
    wait_status(&engine, id, DownloadStatus::Completed, 60).await;
    let d = engine.get(id).unwrap();
    assert_eq!(d.file_name, "ubuntu-mini.iso");
    assert_eq!(d.size, Some(content.len() as u64));
    assert_eq!(d.downloaded, content.len() as u64);
    assert_eq!(
        std::fs::read(d.save_dir.join("ubuntu-mini.iso")).unwrap(),
        content
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_from_a_magnet_link_with_pause_and_continue() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("seed");
    std::fs::create_dir_all(&src).unwrap();
    let content = data(24 * 1024 * 1024, 11); // big enough to pause part-way, even locally
    std::fs::write(src.join("film.mkv"), &content).unwrap();
    // Share slowly so there's time to pause.
    let seeder = seed(&src.join("film.mkv"), Some(1024 * 1024)).await;

    let engine = manager(dir.path(), seeder.addr, 0.0).await;
    let mut events = engine.subscribe();
    let id = engine
        .add(add_magnet(&seeder.hash, "film.mkv"))
        .await
        .unwrap();
    assert_eq!(engine.get(id).unwrap().file_name, "film.mkv");

    // Wait for real progress, then pause.
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        assert!(
            Instant::now() < end,
            "no progress: {:?}",
            engine.get(id).unwrap()
        );
        if let Ok(Ok(Event::Progress {
            id: pid,
            downloaded,
            ..
        })) = tokio::time::timeout(Duration::from_secs(1), events.recv()).await
            && pid == id
            && downloaded > 256 * 1024
        {
            break;
        }
    }
    engine.pause(id).await.unwrap();
    let paused = engine.get(id).unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert!(
        paused.downloaded > 0 && paused.downloaded < content.len() as u64,
        "{paused:?}"
    );
    let DownloadKind::Torrent(spec) = &paused.kind else {
        panic!()
    };
    assert!(
        spec.torrent_file.as_ref().is_some_and(|p| p.exists()),
        "the magnet's .torrent is saved for later"
    );
    assert_eq!(spec.info_hash.as_deref(), Some(seeder.hash.as_str()));
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert_eq!(
        engine.get(id).unwrap().downloaded,
        paused.downloaded,
        "nothing moves while paused"
    );

    engine.start(id).unwrap();
    wait_status(&engine, id, DownloadStatus::Completed, 60).await;
    let d = engine.get(id).unwrap();
    assert_eq!(std::fs::read(d.save_dir.join("film.mkv")).unwrap(), content);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn multi_file_torrent_gets_its_own_folder() {
    let dir = tempfile::tempdir().unwrap();
    let album = dir.path().join("seed").join("Great Album");
    std::fs::create_dir_all(album.join("disc 2")).unwrap();
    let a = data(300_000, 1);
    let b = data(500_000, 2);
    std::fs::write(album.join("01 intro.flac"), &a).unwrap();
    std::fs::write(album.join("disc 2").join("02 outro.flac"), &b).unwrap();
    let seeder = seed(&album, None).await;

    let engine = manager(dir.path(), seeder.addr, 0.0).await;
    let id = engine
        .add(add_magnet(&seeder.hash, "Great+Album"))
        .await
        .unwrap();
    wait_status(&engine, id, DownloadStatus::Completed, 60).await;
    let d = engine.get(id).unwrap();
    let folder: PathBuf = d.save_dir.join("Great Album");
    assert_eq!(std::fs::read(folder.join("01 intro.flac")).unwrap(), a);
    assert_eq!(
        std::fs::read(folder.join("disc 2").join("02 outro.flac")).unwrap(),
        b
    );
    assert_eq!(d.size, Some((a.len() + b.len()) as u64));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn keeps_sharing_after_finishing_until_stopped() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("seed");
    std::fs::create_dir_all(&src).unwrap();
    let content = data(600_000, 5);
    std::fs::write(src.join("notes.pdf"), &content).unwrap();
    let seeder = seed(&src.join("notes.pdf"), None).await;

    // Nobody else downloads from us here, so a ratio of 1.0 is never reached.
    let engine = manager(dir.path(), seeder.addr, 1.0).await;
    let id = engine
        .add(add_magnet(&seeder.hash, "notes.pdf"))
        .await
        .unwrap();
    wait_status(&engine, id, DownloadStatus::Seeding, 60).await;
    let d = engine.get(id).unwrap();
    assert_eq!(
        std::fs::read(d.save_dir.join("notes.pdf")).unwrap(),
        content
    );
    assert!(engine.is_running(id), "seeding keeps it running");

    // Stopping a torrent that's only sharing leaves it complete.
    engine.pause(id).await.unwrap();
    assert_eq!(engine.get(id).unwrap().status, DownloadStatus::Completed);
}

// ---------------------------------------------------------------- privacy

async fn wait_bytes(engine: &Engine, id: i64, at_least: u64) {
    let end = Instant::now() + Duration::from_secs(30);
    while engine.get(id).unwrap().downloaded < at_least {
        assert!(
            Instant::now() < end,
            "no progress: {:?}",
            engine.get(id).unwrap()
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

/// Proxy mode: every connection to a peer goes through the proxy, and nothing listens.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn proxy_mode_sends_everything_through_the_proxy() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("seed");
    std::fs::create_dir_all(&src).unwrap();
    let content = data(2 * 1024 * 1024, 21);
    std::fs::write(src.join("talk.mp4"), &content).unwrap();
    let seeder = seed(&src.join("talk.mp4"), None).await;
    let proxy = Socks5Proxy::start().await;

    let privacy = Privacy {
        mode: PrivacyMode::Proxy { url: proxy.url() },
        require_protection: true,
        dht: true,
        blocklist: None,
    };
    let engine = manager_with(dir.path(), seeder.addr, 0.0, privacy).await;
    let id = engine
        .add(add_magnet(&seeder.hash, "talk.mp4"))
        .await
        .unwrap();
    wait_status(&engine, id, DownloadStatus::Completed, 60).await;
    assert_eq!(
        std::fs::read(engine.get(id).unwrap().save_dir.join("talk.mp4")).unwrap(),
        content
    );

    let seen = proxy.connections.lock().unwrap().clone();
    assert!(
        seen.contains(&seeder.addr.to_string()),
        "the peer was reached through the proxy: {seen:?}"
    );
}

/// Kill switch: when the proxy drops, the torrent stops and waits; it carries on when
/// the proxy is back, without ever connecting directly.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn kill_switch_stops_torrents_when_the_proxy_drops() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("seed");
    std::fs::create_dir_all(&src).unwrap();
    let content = data(4 * 1024 * 1024, 22);
    std::fs::write(src.join("course.zip"), &content).unwrap();
    let seeder = seed(&src.join("course.zip"), Some(768 * 1024)).await;
    let proxy = Socks5Proxy::start().await;

    let privacy = Privacy {
        mode: PrivacyMode::Proxy { url: proxy.url() },
        require_protection: true,
        dht: false,
        blocklist: None,
    };
    let engine = manager_with(dir.path(), seeder.addr, 0.0, privacy).await;
    let id = engine
        .add(add_magnet(&seeder.hash, "course.zip"))
        .await
        .unwrap();
    wait_bytes(&engine, id, 512 * 1024).await;

    proxy.switch_off();
    let start = Instant::now();
    wait_status(&engine, id, DownloadStatus::Blocked, 10).await;
    let blocked_after = start.elapsed();
    let held = engine.get(id).unwrap();
    assert!(
        held.error.as_deref().unwrap_or("").contains("proxy"),
        "{:?}",
        held.error
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    let still = engine.get(id).unwrap();
    assert_eq!(still.status, DownloadStatus::Blocked);
    assert!(
        still.downloaded < content.len() as u64,
        "nothing finishes while the proxy is down"
    );
    assert!(
        still.downloaded <= held.downloaded + 256 * 1024,
        "no real progress while blocked"
    );

    proxy.switch_on().await;
    wait_status(&engine, id, DownloadStatus::Completed, 60).await;
    assert_eq!(
        std::fs::read(engine.get(id).unwrap().save_dir.join("course.zip")).unwrap(),
        content
    );
    println!("kill switch engaged {blocked_after:?} after the proxy dropped");
}

/// The default: without a VPN or proxy, torrents wait and contact nobody at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn default_protection_makes_no_connections_without_a_vpn() {
    let dir = tempfile::tempdir().unwrap();
    let honeypot = Honeypot::start().await;
    let engine = manager_with(dir.path(), honeypot.addr, 0.0, Privacy::default()).await;
    let id = engine
        .add(add_magnet(
            "0123456789abcdef0123456789abcdef01234567",
            "secret",
        ))
        .await
        .unwrap();
    wait_status(&engine, id, DownloadStatus::Blocked, 10).await;
    assert!(
        engine
            .get(id)
            .unwrap()
            .error
            .as_deref()
            .unwrap_or("")
            .contains("VPN or proxy")
    );
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(
        honeypot.hits.load(std::sync::atomic::Ordering::SeqCst),
        0,
        "no peer may be contacted"
    );
    // Pausing a waiting torrent works as usual.
    engine.pause(id).await.unwrap();
    assert_eq!(engine.get(id).unwrap().status, DownloadStatus::Paused);
}

/// VPN mode with an adapter that isn't there: blocked, nothing contacted.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn vpn_mode_waits_for_the_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let honeypot = Honeypot::start().await;
    let privacy = Privacy {
        mode: PrivacyMode::Vpn {
            interface: "ldm-test-vpn0".into(),
        },
        require_protection: true,
        dht: false,
        blocklist: None,
    };
    // The adapter doesn't exist, so on Linux binding to it fails at start: that's also safe.
    let engine = Engine::open(EngineConfig::new(dir.path().join("Downloads"))).unwrap();
    let mut opts = TorrentOptions::new(dir.path().join("ldm-data"));
    opts.dht = false;
    opts.upnp = false;
    opts.extra_peers = vec![honeypot.addr];
    opts.privacy = privacy;
    opts.check_every = Duration::from_millis(200);
    match TorrentRunner::start(opts).await {
        Ok(runner) => {
            engine.set_runner("torrent", runner);
            let id = engine
                .add(add_magnet("0123456789abcdef0123456789abcdef01234567", "x"))
                .await
                .unwrap();
            wait_status(&engine, id, DownloadStatus::Blocked, 10).await;
            assert!(
                engine
                    .get(id)
                    .unwrap()
                    .error
                    .as_deref()
                    .unwrap_or("")
                    .contains("ldm-test-vpn0")
            );
        }
        Err(e) => assert!(format!("{e:#}").contains("ldm-test-vpn0"), "{e:#}"),
    }
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert_eq!(honeypot.hits.load(std::sync::atomic::Ordering::SeqCst), 0);
}

/// VPN mode with a working adapter (loopback here): traffic is tied to it and flows.
#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn vpn_mode_downloads_over_the_adapter() {
    let dir = tempfile::tempdir().unwrap();
    let src = dir.path().join("seed");
    std::fs::create_dir_all(&src).unwrap();
    let content = data(1024 * 1024, 23);
    std::fs::write(src.join("doc.pdf"), &content).unwrap();
    let seeder = seed(&src.join("doc.pdf"), None).await;
    let privacy = Privacy {
        mode: PrivacyMode::Vpn {
            interface: "lo".into(),
        },
        require_protection: true,
        dht: false,
        blocklist: None,
    };
    let engine = manager_with(dir.path(), seeder.addr, 0.0, privacy).await;
    let id = engine
        .add(add_magnet(&seeder.hash, "doc.pdf"))
        .await
        .unwrap();
    wait_status(&engine, id, DownloadStatus::Completed, 60).await;
    assert_eq!(
        std::fs::read(engine.get(id).unwrap().save_dir.join("doc.pdf")).unwrap(),
        content
    );
}
