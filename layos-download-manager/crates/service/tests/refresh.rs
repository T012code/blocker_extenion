//! Refreshing an expired link: the same link again, or a fresh one from the browser,
//! continuing from where the download stopped.

use std::sync::Arc;
use std::time::{Duration, Instant};

use ldm_engine::{DownloadStatus, Engine, EngineConfig, NewDownload, RequestInfo};
use ldm_ipc::{BrowserDownload, Request, Response, Source};
use ldm_service::{NoFrontend, Refresh, Service};
use ldm_testkit::{ServerOptions, TestServer, test_data};

const SIZE: usize = 800_000;

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

/// A download stopped part-way whose link the server now refuses (403).
async fn expired_download() -> (tempfile::TempDir, TestServer, Engine, Service, i64) {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(
        test_data(SIZE),
        ServerOptions {
            throttle: Some(60_000),
            ..Default::default()
        },
    )
    .await;
    let mut cfg = EngineConfig::new(dir.path().join("Downloads"));
    cfg.min_segment_size = 64 * 1024;
    cfg.progress_interval = Duration::from_millis(50);
    let engine = Engine::open(cfg).unwrap();
    let service = Service::new(engine.clone(), Arc::new(NoFrontend));
    let id = engine
        .add(NewDownload {
            request: RequestInfo {
                url: server.url("/files/report.bin?token=old"),
                referer: Some(server.url("/downloads/")),
                ..Default::default()
            },
            connections: Some(2),
            start: true,
            ..Default::default()
        })
        .await
        .unwrap();
    // About a quarter of the file at 2 × 60 KB/s.
    tokio::time::sleep(Duration::from_millis(1500)).await;
    engine.pause(id).await.unwrap();
    server.set_options(|o| o.refuse = true);
    engine.start(id).unwrap();
    until("link expired", || {
        engine.get(id).unwrap().status == DownloadStatus::NeedsRefresh
    })
    .await;
    assert!(
        engine.get(id).unwrap().downloaded > 50_000,
        "kept what it had"
    );
    server.set_options(|o| {
        o.refuse = false;
        o.throttle = None;
    });
    (dir, server, engine, service, id)
}

fn file_of(engine: &Engine, id: i64) -> Vec<u8> {
    std::fs::read(engine.get(id).unwrap().final_path()).unwrap()
}

#[tokio::test]
async fn same_link_works_again_so_it_just_continues() {
    let (_dir, _server, engine, service, id) = expired_download().await;
    let before = engine.get(id).unwrap().downloaded;
    assert_eq!(service.refresh_link(id).await, Ok(Refresh::Resumed));
    until("done", || {
        engine.get(id).unwrap().status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(file_of(&engine, id), test_data(SIZE));
    assert!(before > 0);
}

#[tokio::test]
async fn fresh_link_from_the_browser_continues_the_old_download() {
    let (_dir, server, engine, service, id) = expired_download().await;
    server.set_options(|o| o.refuse = true);
    let page = match service.refresh_link(id).await.unwrap() {
        Refresh::OpenPage(p) => p,
        other => panic!("expected to wait for a fresh link, got {other:?}"),
    };
    assert_eq!(
        page,
        server.url("/downloads/"),
        "opens the page the file came from"
    );
    assert_eq!(service.waiting_for_link(), vec![id]);

    // The same file from a new address, with the server's fresh token.
    let fresh = TestServer::start(test_data(SIZE), ServerOptions::default()).await;
    let ranges_before = fresh.state.ranges_seen.lock().unwrap().len();
    let r = service
        .handle(Request::Download {
            items: vec![BrowserDownload {
                url: fresh.url("/cdn/report.bin?token=new"),
                file_name: Some("report.bin".into()),
                cookies: Some("session=2".into()),
                ..Default::default()
            }],
            source: Source::Capture,
            ask: Some(false),
        })
        .await;
    assert_eq!(r, Response::Accepted { ids: vec![id] });
    assert_eq!(engine.list().len(), 1, "no second download was added");
    until("done", || {
        engine.get(id).unwrap().status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(file_of(&engine, id), test_data(SIZE));
    let d = engine.get(id).unwrap();
    assert!(d.request.url.contains("token=new"));
    assert_eq!(d.request.cookies.as_deref(), Some("session=2"));
    // It only fetched what was missing: apart from the one-byte check of the new link,
    // every range starts where a section had stopped.
    let ranges = fresh.state.ranges_seen.lock().unwrap()[ranges_before..].to_vec();
    let data: Vec<_> = ranges.iter().filter(|r| *r != "bytes=0-0").collect();
    assert!(
        !data.is_empty() && data.iter().all(|r| !r.starts_with("bytes=0-")),
        "re-downloaded from the start: {ranges:?}"
    );
    assert!(service.waiting_for_link().is_empty());
}

#[tokio::test]
async fn an_unrelated_download_is_added_as_usual() {
    let (_dir, server, engine, service, id) = expired_download().await;
    server.set_options(|o| o.refuse = true);
    service.refresh_link(id).await.unwrap();
    let other = TestServer::start(test_data(1000), ServerOptions::default()).await;
    let r = service
        .handle(Request::Download {
            items: vec![BrowserDownload {
                url: other.url("/notes.txt"),
                file_name: Some("notes.txt".into()),
                file_size: Some(1000),
                ..Default::default()
            }],
            source: Source::Capture,
            ask: Some(false),
        })
        .await;
    let Response::Accepted { ids } = r else {
        panic!("{r:?}")
    };
    assert_eq!(ids.len(), 1);
    assert_ne!(ids[0], id);
    assert_eq!(engine.get(id).unwrap().status, DownloadStatus::NeedsRefresh);
    assert_eq!(
        service.waiting_for_link(),
        vec![id],
        "still waiting for its own link"
    );
}

#[tokio::test]
async fn without_asking_only_same_name_and_size_counts() {
    let (_dir, server, engine, service, id) = expired_download().await;
    server.set_options(|o| o.refuse = true);
    let fresh = TestServer::start(test_data(SIZE), ServerOptions::default()).await;
    let send = |size: Option<u64>| {
        service.handle(Request::Download {
            items: vec![BrowserDownload {
                url: fresh.url("/x/report.bin"),
                file_size: size,
                ..Default::default()
            }],
            source: Source::Capture,
            ask: Some(false),
        })
    };
    // Same name, size unknown: a new download.
    let Response::Accepted { ids } = send(None).await else {
        panic!()
    };
    assert_ne!(ids, vec![id]);
    // Same name and size: the expired one continues.
    assert_eq!(
        send(Some(SIZE as u64)).await,
        Response::Accepted { ids: vec![id] }
    );
    until("done", || {
        engine.get(id).unwrap().status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(file_of(&engine, id), test_data(SIZE));
}

#[tokio::test]
async fn refresh_over_the_browser_bridge() {
    let (_dir, server, _engine, service, id) = expired_download().await;
    server.set_options(|o| o.refuse = true);
    let r = service.handle(Request::RefreshLink { id }).await;
    assert_eq!(
        r,
        Response::Refresh {
            resumed: false,
            page_url: Some(server.url("/downloads/"))
        }
    );
    let json = serde_json::to_string(&r).unwrap();
    assert_eq!(
        json,
        format!(
            r#"{{"type":"refresh","resumed":false,"pageUrl":"{}"}}"#,
            server.url("/downloads/")
        )
    );
}
