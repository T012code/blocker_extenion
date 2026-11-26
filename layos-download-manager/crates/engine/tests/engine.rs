mod support;

use std::path::Path;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use ldm_engine::{
    Category, Download, DownloadStatus, Engine, EngineConfig, Event, NewDownload, RequestInfo,
};
use support::{ServerOptions, TestServer, test_data};

const MB: usize = 1024 * 1024;

fn config(dir: &Path) -> EngineConfig {
    let mut c = EngineConfig::new(dir.join("Downloads"));
    c.min_segment_size = 64 * 1024;
    c.retry_delay = Duration::from_millis(50);
    c.progress_interval = Duration::from_millis(100);
    c
}

fn new_download(url: String) -> NewDownload {
    NewDownload {
        request: RequestInfo::new(url),
        start: true,
        ..Default::default()
    }
}

/// Waits until the download reaches `status` (or fails the test).
async fn wait_status(
    engine: &Engine,
    id: i64,
    status: DownloadStatus,
    timeout: Duration,
) -> Download {
    let deadline = Instant::now() + timeout;
    loop {
        let d = engine.get(id).unwrap();
        if d.status == status && !engine.is_running(id) {
            return d;
        }
        if Instant::now() > deadline {
            panic!(
                "download {id} is {:?} (error: {:?}), wanted {status:?}",
                d.status, d.error
            );
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn wait_bytes(engine: &Engine, id: i64, at_least: u64) {
    let mut rx = engine.subscribe();
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Ok(Ok(Event::Progress {
            id: pid,
            downloaded,
            ..
        })) = tokio::time::timeout(Duration::from_secs(1), rx.recv()).await
            && pid == id
            && downloaded >= at_least
        {
            return;
        }
    }
    panic!("download {id} never reached {at_least} bytes");
}

fn read(d: &Download) -> Vec<u8> {
    std::fs::read(d.final_path()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_over_several_connections() {
    let data = test_data(4 * MB + 123);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            throttle: Some(4 * MB as u64),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let id = engine
        .add(new_download(server.url("/files/big%20file.bin")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(30),
    )
    .await;

    assert_eq!(d.file_name, "big file.bin");
    assert_eq!(d.size, Some(data.len() as u64));
    assert!(d.resumable);
    assert_eq!(read(&d), data);
    assert!(
        server.state.max_active.load(Ordering::SeqCst) > 1,
        "should use several connections"
    );
    assert!(d.temp_path.is_none());
    assert!(
        !dir.path()
            .join("Downloads/General/big file.bin.ldmpart")
            .exists()
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn server_without_ranges_uses_one_connection() {
    let data = test_data(MB);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            ranges: false,
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let id = engine
        .add(new_download(server.url("/a.zip")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(20),
    )
    .await;
    assert!(!d.resumable);
    assert_eq!(read(&d), data);
    assert_eq!(server.state.max_active.load(Ordering::SeqCst), 1);
    assert_eq!(d.category, Category::Compressed);
    assert!(
        d.final_path()
            .starts_with(dir.path().join("Downloads/Compressed"))
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unknown_size_downloads_to_the_end() {
    let data = test_data(700_000);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            ranges: false,
            send_length: false,
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let id = engine
        .add(new_download(server.url("/stream")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(d.size, Some(data.len() as u64));
    assert_eq!(read(&d), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn recovers_from_dropped_connections() {
    let data = test_data(3 * MB);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            drop_after: Some(100_000),
            drops: 6,
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let id = engine
        .add(new_download(server.url("/f.bin")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(read(&d), data);
    assert_eq!(
        server.state.drops_left.load(Ordering::SeqCst),
        0,
        "drops should have happened"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn non_resumable_restarts_after_a_drop() {
    let data = test_data(500_000);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            ranges: false,
            drop_after: Some(200_000),
            drops: 1,
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(new_download(server.url("/f.bin")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(read(&d), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_resume_keeps_progress() {
    let data = test_data(3 * MB);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            throttle: Some(400_000),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let id = engine
        .add(new_download(server.url("/f.bin")))
        .await
        .unwrap();
    wait_bytes(&engine, id, 600_000).await;
    engine.pause(id).await.unwrap();
    let paused = engine.get(id).unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert!(paused.downloaded > 0 && paused.downloaded < data.len() as u64);
    let part = paused.temp_path.clone().unwrap();
    assert!(part.exists());

    server.set_options(|o| o.throttle = None);
    let before = server.state.ranges_seen.lock().unwrap().len();
    engine.start(id).unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(read(&d), data);
    assert!(!part.exists());
    // After resuming, connections continue where they stopped instead of starting at 0.
    let ranges = server.state.ranges_seen.lock().unwrap()[before..].to_vec();
    assert!(
        ranges
            .iter()
            .filter(|r| *r != "bytes=0-0")
            .all(|r| r != "bytes=0-"),
        "{ranges:?}"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn survives_an_app_restart() {
    let data = test_data(2 * MB);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            throttle: Some(300_000),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = config(dir.path());
    cfg.db_path = Some(dir.path().join("ldm.db"));

    let id = {
        let engine = Engine::open(cfg.clone()).unwrap();
        let id = engine
            .add(new_download(server.url("/f.bin")))
            .await
            .unwrap();
        wait_bytes(&engine, id, 300_000).await;
        engine.pause(id).await.unwrap();
        id
    };

    server.set_options(|o| o.throttle = None);
    let engine = Engine::open(cfg).unwrap();
    let d = engine.get(id).unwrap();
    assert_eq!(d.status, DownloadStatus::Paused);
    assert!(d.downloaded >= 300_000);
    engine.start(id).unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(30),
    )
    .await;
    assert_eq!(read(&d), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn expired_link_can_be_refreshed() {
    let data = test_data(MB);
    let server = TestServer::start(data.clone(), ServerOptions::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let id = engine
        .add(new_download(server.url("/expired")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::NeedsRefresh,
        Duration::from_secs(10),
    )
    .await;
    assert!(d.error.unwrap().contains("403"));

    engine
        .refresh_address(id, RequestInfo::new(server.url("/fresh.bin")))
        .await
        .unwrap();
    engine.start(id).unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(read(&d), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn file_change_on_server_restarts_download() {
    let first = test_data(2 * MB);
    let server = TestServer::start(
        first,
        ServerOptions {
            throttle: Some(300_000),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(new_download(server.url("/f.bin")))
        .await
        .unwrap();
    wait_bytes(&engine, id, 300_000).await;
    engine.pause(id).await.unwrap();

    let second: Vec<u8> = test_data(MB + 5).into_iter().rev().collect();
    server.set_body(second.clone());
    server.set_options(|o| o.throttle = None);
    engine.start(id).unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(20),
    )
    .await;
    assert_eq!(read(&d), second);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn names_from_headers_and_never_overwrites() {
    let data = test_data(10_000);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            content_disposition: Some("attachment; filename*=UTF-8''r%C3%A9sum%C3%A9.pdf".into()),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();

    let a = engine
        .add(new_download(server.url("/download?id=1")))
        .await
        .unwrap();
    let a = wait_status(
        &engine,
        a,
        DownloadStatus::Completed,
        Duration::from_secs(10),
    )
    .await;
    let b = engine
        .add(new_download(server.url("/download?id=1")))
        .await
        .unwrap();
    let b = wait_status(
        &engine,
        b,
        DownloadStatus::Completed,
        Duration::from_secs(10),
    )
    .await;

    assert_eq!(a.file_name, "résumé.pdf");
    assert_eq!(a.category, Category::Documents);
    assert_eq!(b.file_name, "résumé (1).pdf");
    assert_eq!(read(&a), data);
    assert_eq!(read(&b), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn follows_redirects() {
    let data = test_data(50_000);
    let server = TestServer::start(data.clone(), ServerOptions::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(new_download(server.url("/redirect")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(d.file_name, "redirected.bin");
    assert!(
        d.final_url
            .as_ref()
            .unwrap()
            .ends_with("/files/redirected.bin")
    );
    assert_eq!(read(&d), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn respects_speed_limit() {
    let data = test_data(600_000);
    let server = TestServer::start(data.clone(), ServerOptions::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let start = Instant::now();
    let id = engine
        .add(NewDownload {
            speed_limit: Some(300_000),
            ..new_download(server.url("/f.bin"))
        })
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(20),
    )
    .await;
    let secs = start.elapsed().as_secs_f64();
    assert!(secs > 1.5, "600 KB at 300 KB/s took only {secs:.2}s");
    assert_eq!(read(&d), data);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn remove_deletes_part_file() {
    let data = test_data(2 * MB);
    let server = TestServer::start(
        data,
        ServerOptions {
            throttle: Some(200_000),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(new_download(server.url("/f.bin")))
        .await
        .unwrap();
    wait_bytes(&engine, id, 100_000).await;
    let part = engine.get(id).unwrap().temp_path.unwrap();
    assert!(part.exists());
    engine.remove(id, false).await.unwrap();
    assert!(!part.exists());
    assert!(engine.get(id).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn empty_file() {
    let server = TestServer::start(Vec::new(), ServerOptions::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(new_download(server.url("/empty.txt")))
        .await
        .unwrap();
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(10),
    )
    .await;
    assert_eq!(read(&d), Vec::<u8>::new());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejects_bad_addresses() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    assert!(engine.add(new_download("not a url".into())).await.is_err());
    assert!(
        engine
            .add(new_download("file:///etc/passwd".into()))
            .await
            .is_err()
    );
}

/// Each download is cut into sections fetched in parallel, one connection per section.
#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
async fn splits_into_8_16_32_and_64_parallel_sections() {
    for sections in [8u8, 16, 32, 64] {
        let data = test_data(8 * MB);
        // Slow each connection down so all sections are open at the same time.
        let server = TestServer::start(
            data.clone(),
            ServerOptions {
                throttle: Some(512 * 1024),
                ..Default::default()
            },
        )
        .await;
        let dir = tempfile::tempdir().unwrap();
        let engine = Engine::open(config(dir.path())).unwrap();

        let id = engine
            .add(NewDownload {
                connections: Some(sections),
                ..new_download(server.url("/f.bin"))
            })
            .await
            .unwrap();

        // Watch the live section map while it downloads.
        let mut rx = engine.subscribe();
        let mut most_active = 0;
        let mut most_sections = 0;
        while engine.get(id).unwrap().status != DownloadStatus::Completed {
            if let Ok(Ok(Event::Progress { segments, .. })) =
                tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
            {
                most_active = most_active.max(segments.iter().filter(|s| s.active).count());
                most_sections = most_sections.max(segments.len());
            }
        }
        let d = wait_status(
            &engine,
            id,
            DownloadStatus::Completed,
            Duration::from_secs(30),
        )
        .await;
        assert_eq!(
            read(&d),
            data,
            "{sections} sections: file must be byte-for-byte correct"
        );

        let max_conns = server.state.max_active.load(Ordering::SeqCst);
        assert!(
            max_conns >= sections as usize,
            "{sections} sections: server saw only {max_conns} parallel connections"
        );
        assert!(
            most_active >= sections as usize,
            "{sections} sections: only {most_active} were active at once"
        );
        // Every non-probe request asked for a distinct byte range.
        let ranges = server.state.ranges_seen.lock().unwrap().clone();
        let starts: std::collections::HashSet<_> =
            ranges.iter().filter(|r| *r != "bytes=0-0").collect();
        assert!(starts.len() >= sections as usize);
        println!(
            "{sections:>2} sections -> {max_conns} parallel connections, {most_sections} pieces in the section map"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn connection_count_is_clamped() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let id = engine
        .add(NewDownload {
            connections: Some(200),
            start: false,
            ..new_download("http://127.0.0.1:9/x".into())
        })
        .await
        .unwrap();
    assert_eq!(
        engine.get(id).unwrap().connections,
        ldm_engine::MAX_CONNECTIONS
    );
    let id = engine
        .add(NewDownload {
            connections: Some(0),
            start: false,
            ..new_download("http://127.0.0.1:9/x".into())
        })
        .await
        .unwrap();
    assert_eq!(engine.get(id).unwrap().connections, 1);
}

/// When one section is slow, finished connections take over half of it, again and again,
/// so the slow part ends up shared across many connections instead of holding up the file.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn idle_connections_take_over_slow_sections() {
    let data = test_data(4 * MB);
    // 8 sections of 512 KB. The last one starts at 3.5 MB and is 8x slower than the rest.
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            throttle: Some(2 * MB as u64),
            slow_tail: Some((7 * MB as u64 / 2, 256 * 1024)),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(config(dir.path())).unwrap();
    let start = Instant::now();
    let id = engine
        .add(NewDownload {
            connections: Some(8),
            ..new_download(server.url("/f.bin"))
        })
        .await
        .unwrap();
    let mut rx = engine.subscribe();
    let mut most_sections = 0;
    while engine.get(id).unwrap().status != DownloadStatus::Completed {
        if let Ok(Ok(Event::Progress { segments, .. })) =
            tokio::time::timeout(Duration::from_millis(200), rx.recv()).await
        {
            most_sections = most_sections.max(segments.len());
        }
    }
    let d = wait_status(
        &engine,
        id,
        DownloadStatus::Completed,
        Duration::from_secs(30),
    )
    .await;
    let secs = start.elapsed().as_secs_f64();
    assert_eq!(read(&d), data);
    assert!(
        most_sections > 8,
        "no re-splitting: still {most_sections} sections"
    );
    // Alone, the slow 512 KB section would take 2 s; shared out it must finish well before.
    assert!(secs < 1.6, "slow section held up the download: {secs:.2}s");
    println!("slow tail: {most_sections} sections after re-splitting, done in {secs:.2}s");
}
