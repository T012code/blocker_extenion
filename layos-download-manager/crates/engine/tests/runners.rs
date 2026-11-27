//! Non-web downloads (torrents, video streams) go through runners; the engine still
//! handles their list entry, pause and continue.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use futures_util::future::BoxFuture;
use ldm_engine::{
    Category, DownloadKind, DownloadStatus, Engine, EngineConfig, Error, Job, MediaSpec,
    NewDownload, RequestInfo, Runner, TorrentSpec,
};

/// Pretends to download 1000 units, 50 per tick, keeping its place across pauses.
struct FakeRunner {
    done: Arc<AtomicU64>,
}

impl Runner for FakeRunner {
    fn run(&self, job: Job) -> BoxFuture<'static, ldm_engine::Result<()>> {
        let done = self.done.clone();
        Box::pin(async move {
            job.report.set_status(DownloadStatus::Downloading);
            loop {
                let now = done.load(Ordering::SeqCst);
                if now >= 1000 {
                    return Ok(());
                }
                tokio::select! {
                    _ = job.cancel.cancelled() => return Err(Error::Cancelled),
                    _ = tokio::time::sleep(Duration::from_millis(20)) => {}
                }
                done.store(now + 50, Ordering::SeqCst);
                job.report.progress(now + 50, Some(1000), 2500);
            }
        })
    }
}

async fn wait(engine: &Engine, id: i64, status: DownloadStatus) {
    let end = Instant::now() + Duration::from_secs(10);
    while Instant::now() < end {
        if engine.get(id).unwrap().status == status && !engine.is_running(id) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("never reached {status:?}: {:?}", engine.get(id).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runner_downloads_pause_and_continue() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path())).unwrap();
    let done = Arc::new(AtomicU64::new(0));
    engine.set_runner("torrent", Arc::new(FakeRunner { done: done.clone() }));

    let id = engine
        .add(NewDownload {
            request: RequestInfo::new(
                "magnet:?xt=urn:btih:0123456789abcdef0123456789abcdef01234567&dn=Big+Linux+ISO",
            ),
            kind: DownloadKind::Torrent(TorrentSpec::default()),
            start: true,
            ..Default::default()
        })
        .await
        .unwrap();
    let d = engine.get(id).unwrap();
    assert_eq!(
        d.file_name, "Big Linux ISO",
        "name comes from the magnet link"
    );
    assert_eq!(
        d.category,
        Category::Torrents,
        "torrents have their own folder"
    );

    while done.load(Ordering::SeqCst) < 300 {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    engine.pause(id).await.unwrap();
    let paused = engine.get(id).unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert!(paused.downloaded >= 300 && paused.downloaded < 1000);

    engine.start(id).unwrap();
    wait(&engine, id, DownloadStatus::Completed).await;
    let d = engine.get(id).unwrap();
    assert_eq!(d.downloaded, 1000);
    assert!(d.completed_at.is_some());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn media_downloads_are_sorted_and_missing_runner_fails_clearly() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path())).unwrap();
    let id = engine
        .add(NewDownload {
            request: RequestInfo::new("https://video.example/watch?v=1"),
            kind: DownloadKind::Media(MediaSpec {
                page_url: "https://video.example/watch?v=1".into(),
                title: Some("Cat: the movie?".into()),
                label: Some("Audio only (M4A)".into()),
                ..Default::default()
            }),
            ..Default::default()
        })
        .await
        .unwrap();
    let d = engine.get(id).unwrap();
    assert_eq!(d.file_name, "Cat_ the movie_");
    assert_eq!(
        d.category,
        Category::Music,
        "an audio-only pick goes to Music"
    );
    assert!(engine.start(id).is_err());
    assert_eq!(engine.get(id).unwrap().status, DownloadStatus::Failed);
    // Magnets are only accepted for torrents.
    assert!(
        engine
            .add(NewDownload {
                request: RequestInfo::new("magnet:?xt=urn:btih:00"),
                ..Default::default()
            })
            .await
            .is_err()
    );
}
