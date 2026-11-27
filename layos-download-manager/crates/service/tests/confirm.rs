//! IDM-style handling of browser downloads: a file starts in the background while the
//! "add download" window waits, and a link downloaded before is caught as a duplicate.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use ldm_engine::{Download, DownloadStatus, Engine, EngineConfig, NewDownload, RequestInfo};
use ldm_ipc::{BrowserDownload, Request, Response, Source};
use ldm_service::{Frontend, Service};
use ldm_testkit::{ServerOptions, TestServer, test_data};

/// What the window was asked to show: the links and what was started for each.
type Asked = Vec<(Vec<String>, Vec<Option<i64>>)>;

#[derive(Default)]
struct Recorder {
    asks: bool,
    handle_duplicates: bool,
    asked: Mutex<Asked>,
    duplicates: Mutex<Vec<(i64, String)>>,
}

impl Frontend for Recorder {
    fn will_ask(&self, source: Source) -> bool {
        self.asks || source == Source::AllLinks
    }
    fn ask(&self, items: &[BrowserDownload], ids: &[Option<i64>], _source: Source) {
        self.asked
            .lock()
            .unwrap()
            .push((items.iter().map(|i| i.url.clone()).collect(), ids.to_vec()));
    }
    fn duplicate(&self, existing: &Download, item: &BrowserDownload) -> bool {
        self.duplicates
            .lock()
            .unwrap()
            .push((existing.id, item.url.clone()));
        self.handle_duplicates
    }
}

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

async fn setup(
    front: Recorder,
) -> (
    tempfile::TempDir,
    TestServer,
    Engine,
    Service,
    Arc<Recorder>,
) {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(
        test_data(400_000),
        ServerOptions {
            throttle: Some(200_000),
            ..Default::default()
        },
    )
    .await;
    let mut cfg = EngineConfig::new(dir.path().join("Downloads"));
    cfg.min_segment_size = 64 * 1024;
    let engine = Engine::open(cfg).unwrap();
    let front = Arc::new(front);
    let service = Service::new(engine.clone(), front.clone());
    (dir, server, engine, service, front)
}

fn capture(url: String, source: Source) -> Request {
    Request::Download {
        items: vec![BrowserDownload {
            url,
            ..Default::default()
        }],
        source,
        ask: None,
    }
}

#[tokio::test]
async fn a_file_starts_in_the_background_while_the_window_asks() {
    let (_dir, server, engine, service, front) = setup(Recorder {
        asks: true,
        ..Default::default()
    })
    .await;
    let r = service
        .handle(capture(server.url("/files/setup.exe"), Source::Capture))
        .await;
    assert_eq!(r, Response::Accepted { ids: vec![] }, "went to the window");
    let (urls, ids) = front.asked.lock().unwrap()[0].clone();
    assert_eq!(urls, vec![server.url("/files/setup.exe")]);
    let id = ids[0].expect("started in the background");
    let d = engine.get(id).unwrap();
    assert!(d.pending, "not confirmed yet");
    until("downloading", || {
        engine.get(id).unwrap().downloaded > 0
            || engine.get(id).unwrap().status == DownloadStatus::Completed
    })
    .await;
    // The browser's list doesn't show it until it's confirmed.
    let Response::Downloads { items, .. } = service.handle(Request::List).await else {
        panic!()
    };
    assert!(items.iter().all(|i| i.id != id));
    engine.confirm(id).unwrap();
    let Response::Downloads { items, .. } = service.handle(Request::List).await else {
        panic!()
    };
    assert!(items.iter().any(|i| i.id == id));
}

#[tokio::test]
async fn download_all_links_always_lets_you_pick_and_starts_nothing() {
    let (_dir, server, engine, service, front) = setup(Recorder::default()).await;
    let r = service
        .handle(Request::Download {
            items: (0..5)
                .map(|i| BrowserDownload {
                    url: server.url(&format!("/f{i}.zip")),
                    ..Default::default()
                })
                .collect(),
            source: Source::AllLinks,
            ask: None,
        })
        .await;
    assert_eq!(r, Response::Accepted { ids: vec![] });
    assert_eq!(front.asked.lock().unwrap()[0].1, vec![None; 5]);
    assert!(engine.list().is_empty());
}

#[tokio::test]
async fn a_link_downloaded_before_is_caught() {
    let (_dir, server, engine, service, front) = setup(Recorder {
        handle_duplicates: true,
        ..Default::default()
    })
    .await;
    let url = server.url("/files/report.pdf");
    let first = engine
        .add(NewDownload {
            request: RequestInfo::new(url.clone()),
            start: true,
            ..Default::default()
        })
        .await
        .unwrap();
    until("done", || {
        engine.get(first).unwrap().status == DownloadStatus::Completed
    })
    .await;
    // Same link again (the part after # doesn't matter).
    let r = service
        .handle(capture(format!("{url}#page=2"), Source::Capture))
        .await;
    assert_eq!(r, Response::Accepted { ids: vec![first] });
    assert_eq!(front.duplicates.lock().unwrap()[0].0, first);
    assert_eq!(engine.list().len(), 1, "no second download");
    // A different link is not a duplicate.
    service
        .handle(capture(server.url("/files/other.pdf"), Source::Capture))
        .await;
    assert_eq!(engine.list().len(), 2);
    assert_eq!(front.duplicates.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn adding_again_gets_a_numbered_name() {
    let (_dir, server, engine, service, front) = setup(Recorder::default()).await;
    let url = server.url("/files/notes.txt");
    let first = engine
        .add(NewDownload {
            request: RequestInfo::new(url.clone()),
            start: true,
            ..Default::default()
        })
        .await
        .unwrap();
    until("done", || {
        engine.get(first).unwrap().status == DownloadStatus::Completed
    })
    .await;
    let Response::Accepted { ids } = service.handle(capture(url, Source::Capture)).await else {
        panic!()
    };
    assert_eq!(
        front.duplicates.lock().unwrap().len(),
        1,
        "the frontend was asked"
    );
    until("second done", || {
        engine.get(ids[0]).unwrap().status == DownloadStatus::Completed
    })
    .await;
    assert_eq!(engine.get(first).unwrap().file_name, "notes.txt");
    assert_eq!(engine.get(ids[0]).unwrap().file_name, "notes (1).txt");
}

#[tokio::test]
async fn the_same_torrent_is_caught_by_its_hash() {
    let (_dir, _server, engine, service, front) = setup(Recorder {
        handle_duplicates: true,
        ..Default::default()
    })
    .await;
    let t = engine
        .add(NewDownload {
            request: RequestInfo::new(
                "magnet:?xt=urn:btih:ABCDEF0123456789ABCDEF0123456789ABCDEF01&dn=Show",
            ),
            kind: ldm_engine::DownloadKind::Torrent(Default::default()),
            ..Default::default()
        })
        .await
        .unwrap();
    let r = service
        .handle(capture(
            "magnet:?xt=urn:btih:abcdef0123456789abcdef0123456789abcdef01&tr=udp://x".into(),
            Source::Link,
        ))
        .await;
    assert_eq!(r, Response::Accepted { ids: vec![t] });
    assert_eq!(front.duplicates.lock().unwrap()[0].0, t);
}
