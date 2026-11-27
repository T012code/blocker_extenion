//! When the network goes away, downloads pause and keep what they have (or stop, if
//! the server can't continue them); they only continue when the user says so.
//! Also: choosing where a download goes while it runs or after it finished, downloads
//! held for confirmation, and the category folders.

use std::path::Path;
use std::time::{Duration, Instant};

use ldm_engine::{
    Category, DownloadKind, DownloadStatus, Engine, EngineConfig, NewDownload, PAUSED_OFFLINE,
    RequestInfo, STOPPED_OFFLINE, TorrentSpec,
};
use ldm_testkit::{ServerOptions, TestServer, test_data};

const SIZE: usize = 2 * 1024 * 1024;

fn config(dir: &Path) -> EngineConfig {
    let mut c = EngineConfig::new(dir.join("Downloads"));
    c.min_segment_size = 64 * 1024;
    c.retry_delay = Duration::from_millis(50);
    c.offline_retries = 2;
    c.progress_interval = Duration::from_millis(50);
    c
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(30);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn slow_server(ranges: bool) -> TestServer {
    TestServer::start(
        test_data(SIZE),
        ServerOptions {
            ranges,
            throttle: Some(300_000),
            ..Default::default()
        },
    )
    .await
}

fn add(url: String) -> NewDownload {
    NewDownload {
        request: RequestInfo::new(url),
        connections: Some(4),
        start: true,
        ..Default::default()
    }
}

/// Waits until it has made some progress (seen in its events), then cuts the network.
async fn run_then_cut(engine: &Engine, server: &TestServer, id: i64) {
    let mut rx = engine.subscribe();
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        assert!(Instant::now() < end, "no progress");
        if let Ok(Ok(ldm_engine::Event::Progress {
            id: pid,
            downloaded,
            ..
        })) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await
            && pid == id
            && downloaded > 200_000
        {
            break;
        }
    }
    server.set_options(|o| o.offline = true);
}

#[tokio::test]
async fn losing_the_network_pauses_and_keeps_progress() {
    let dir = tempfile::tempdir().unwrap();
    let server = slow_server(true).await;
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine.add(add(server.url("/files/big.bin"))).await.unwrap();
    run_then_cut(&engine, &server, id).await;
    until("paused", || {
        engine.get(id).unwrap().status == DownloadStatus::Paused && !engine.is_running(id)
    })
    .await;
    let d = engine.get(id).unwrap();
    assert_eq!(d.error.as_deref(), Some(PAUSED_OFFLINE));
    assert!(d.downloaded > 200_000, "kept what it had: {}", d.downloaded);
    // It stays paused: nothing restarts it when the network is back.
    server.set_options(|o| {
        o.offline = false;
        o.throttle = None;
    });
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(engine.get(id).unwrap().status, DownloadStatus::Paused);
    // The user continues it and it finishes correctly.
    engine.start(id).unwrap();
    until("done", || {
        engine.get(id).unwrap().status == DownloadStatus::Completed
    })
    .await;
    let d = engine.get(id).unwrap();
    assert_eq!(std::fs::read(d.final_path()).unwrap(), test_data(SIZE));
    assert_eq!(d.error, None);
}

#[tokio::test]
async fn a_server_that_cannot_continue_stops_with_a_reason() {
    let dir = tempfile::tempdir().unwrap();
    let server = slow_server(false).await;
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(add(server.url("/files/once.bin")))
        .await
        .unwrap();
    run_then_cut(&engine, &server, id).await;
    until("stopped", || {
        engine.get(id).unwrap().status == DownloadStatus::Failed && !engine.is_running(id)
    })
    .await;
    assert_eq!(
        engine.get(id).unwrap().error.as_deref(),
        Some(STOPPED_OFFLINE)
    );
}

#[tokio::test]
async fn starting_without_a_network_waits_paused() {
    let dir = tempfile::tempdir().unwrap();
    let server = slow_server(true).await;
    server.set_options(|o| o.offline = true);
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(add(server.url("/files/later.bin")))
        .await
        .unwrap();
    until("paused", || {
        engine.get(id).unwrap().status == DownloadStatus::Paused && !engine.is_running(id)
    })
    .await;
    assert_eq!(
        engine.get(id).unwrap().error.as_deref(),
        Some(PAUSED_OFFLINE)
    );
}

#[tokio::test]
async fn pause_all_offline_pauses_every_running_download_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let server = slow_server(true).await;
    let plain = slow_server(false).await;
    let engine = Engine::open(config(dir.path())).unwrap();
    let a = engine.add(add(server.url("/a.bin"))).await.unwrap();
    let b = engine.add(add(server.url("/b.bin"))).await.unwrap();
    let c = engine.add(add(plain.url("/c.bin"))).await.unwrap();
    until("all downloading", || {
        [a, b, c]
            .iter()
            .all(|&id| engine.get(id).unwrap().status == DownloadStatus::Downloading)
    })
    .await;
    tokio::time::sleep(Duration::from_millis(400)).await;
    let mut paused = engine.pause_all_offline().await;
    paused.sort();
    assert_eq!(paused, vec![a, b, c]);
    for id in [a, b] {
        let d = engine.get(id).unwrap();
        assert_eq!(
            (d.status, d.error.as_deref()),
            (DownloadStatus::Paused, Some(PAUSED_OFFLINE))
        );
    }
    let d = engine.get(c).unwrap();
    assert_eq!(
        (d.status, d.error.as_deref()),
        (DownloadStatus::Failed, Some(STOPPED_OFFLINE))
    );
}

#[tokio::test]
async fn the_place_can_change_while_it_downloads_and_after_it_finished() {
    let dir = tempfile::tempdir().unwrap();
    let server = slow_server(true).await;
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(add(server.url("/files/movie.mkv")))
        .await
        .unwrap();
    until("downloading", || {
        engine.get(id).unwrap().status == DownloadStatus::Downloading
    })
    .await;
    let chosen = dir.path().join("Chosen");
    engine
        .set_target(id, Some(chosen.clone()), Some("My movie.mkv".into()))
        .await
        .unwrap();
    server.set_options(|o| o.throttle = None);
    until("done", || {
        engine.get(id).unwrap().status == DownloadStatus::Completed
    })
    .await;
    let d = engine.get(id).unwrap();
    assert_eq!(d.final_path(), chosen.join("My movie.mkv"));
    assert_eq!(std::fs::read(d.final_path()).unwrap(), test_data(SIZE));
    // No part file left behind in the default folder.
    let video = dir.path().join("Downloads").join("Video");
    assert!(
        std::fs::read_dir(&video).map(|r| r.count()).unwrap_or(0) == 0,
        "left over in {video:?}"
    );
    // Finished: moving it moves the file.
    let later = dir.path().join("Later");
    engine
        .set_target(id, Some(later.clone()), None)
        .await
        .unwrap();
    assert!(later.join("My movie.mkv").is_file());
    assert!(!chosen.join("My movie.mkv").exists());
    assert_eq!(
        engine.get(id).unwrap().final_path(),
        later.join("My movie.mkv")
    );
}

#[tokio::test]
async fn unconfirmed_downloads_are_dropped_when_the_app_starts_again() {
    let dir = tempfile::tempdir().unwrap();
    let server = slow_server(true).await;
    let mut cfg = config(dir.path());
    cfg.db_path = Some(dir.path().join("db.sqlite"));
    let (kept, dropped, part) = {
        let engine = Engine::open(cfg.clone()).unwrap();
        let kept = engine.add(add(server.url("/kept.zip"))).await.unwrap();
        let dropped = engine
            .add(NewDownload {
                pending: true,
                ..add(server.url("/dropped.zip"))
            })
            .await
            .unwrap();
        assert!(engine.get(dropped).unwrap().pending);
        until("part file", || {
            engine.get(dropped).unwrap().temp_path.is_some()
        })
        .await;
        let part = engine.get(dropped).unwrap().temp_path.unwrap();
        engine.confirm(kept).unwrap();
        engine.pause_all().await;
        (kept, dropped, part)
    };
    let engine = Engine::open(cfg).unwrap();
    assert!(engine.get(kept).is_ok());
    assert!(
        engine.get(dropped).is_err(),
        "an unanswered download is gone"
    );
    assert!(!part.exists(), "and its part file too");
}

#[tokio::test]
async fn category_folders_torrents_and_chosen_folders() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    let music = dir.path().join("My music");
    cfg.category_dirs.insert(Category::Music, music.clone());
    let engine = Engine::open(cfg).unwrap();
    let dl = dir.path().join("Downloads");
    let new = |url: &str| NewDownload {
        request: RequestInfo::new(url),
        ..Default::default()
    };
    let zip = engine.add(new("https://example.com/a.zip")).await.unwrap();
    let mp3 = engine.add(new("https://example.com/b.mp3")).await.unwrap();
    let t = engine
        .add(NewDownload {
            request: RequestInfo::new(
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Show",
            ),
            kind: DownloadKind::Torrent(TorrentSpec {
                magnet: Some(
                    "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Show".into(),
                ),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(engine.get(zip).unwrap().save_dir, dl.join("Compressed"));
    assert_eq!(engine.get(mp3).unwrap().save_dir, music);
    let td = engine.get(t).unwrap();
    assert_eq!(
        (td.category, td.save_dir),
        (Category::Torrents, dl.join("Torrents"))
    );
    assert_eq!(Category::ALL.len(), 7);
}
