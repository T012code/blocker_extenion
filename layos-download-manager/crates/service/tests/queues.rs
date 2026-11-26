use std::time::{Duration, Instant};

use ldm_engine::{DownloadStatus, Engine, EngineConfig, NewDownload, RequestInfo};
use ldm_service::queues::Queues;
use ldm_testkit::{ServerOptions, TestServer, test_data};

async fn until(what: &str, mut f: impl FnMut() -> bool) {
    let end = Instant::now() + Duration::from_secs(20);
    while !f() {
        assert!(Instant::now() < end, "timed out waiting for: {what}");
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
}

async fn setup(throttle: u64) -> (tempfile::TempDir, TestServer, Engine) {
    let dir = tempfile::tempdir().unwrap();
    let server = TestServer::start(
        test_data(600_000),
        ServerOptions {
            throttle: Some(throttle),
            ..Default::default()
        },
    )
    .await;
    let mut cfg = EngineConfig::new(dir.path().join("Downloads"));
    cfg.min_segment_size = 64 * 1024;
    cfg.progress_interval = Duration::from_millis(50);
    let engine = Engine::open(cfg).unwrap();
    (dir, server, engine)
}

async fn add_waiting(engine: &Engine, url: String) -> i64 {
    engine
        .add(NewDownload {
            request: RequestInfo::new(url),
            connections: Some(2),
            ..Default::default()
        })
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_queue_runs_its_downloads_a_few_at_a_time_in_order() {
    let (_dir, server, engine) = setup(400_000).await;
    let queues = Queues::start(engine.clone());
    let q = queues.create("Night", 1);
    let ids: Vec<i64> = {
        let mut v = Vec::new();
        for i in 0..3 {
            v.push(add_waiting(&engine, server.url(&format!("/f{i}.bin"))).await);
        }
        v
    };
    queues.assign(&ids, Some(q.id)).unwrap();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(engine.running_count(), 0, "a stopped queue starts nothing");

    queues.start_queue(q.id).unwrap();
    let mut max_running = 0;
    let mut order = Vec::new();
    until("all three finish", || {
        max_running = max_running.max(engine.running_count());
        for id in &ids {
            if engine.get(*id).unwrap().status == DownloadStatus::Completed && !order.contains(id) {
                order.push(*id);
            }
        }
        order.len() == 3
    })
    .await;
    assert_eq!(max_running, 1, "never more than the queue's limit");
    assert_eq!(order, ids, "oldest first");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn stopping_pauses_and_a_manual_pause_sticks() {
    let (_dir, server, engine) = setup(100_000).await;
    let queues = Queues::start(engine.clone());
    let q = queues.create("Big files", 2);
    let a = add_waiting(&engine, server.url("/a.bin")).await;
    let b = add_waiting(&engine, server.url("/b.bin")).await;
    queues.assign(&[a, b], Some(q.id)).unwrap();
    queues.start_queue(q.id).unwrap();
    until("both running", || {
        engine.is_running(a) && engine.is_running(b)
    })
    .await;

    // A download paused by hand inside a running queue stays paused.
    engine.pause(a).await.unwrap();
    tokio::time::sleep(Duration::from_millis(1200)).await;
    assert_eq!(engine.get(a).unwrap().status, DownloadStatus::Paused);

    queues.stop_queue(q.id).await.unwrap();
    assert!(!engine.is_running(b));
    assert_eq!(engine.get(b).unwrap().status, DownloadStatus::Paused);
    assert!(!queues.list()[0].running);

    // Starting it again picks both up.
    queues.start_queue(q.id).unwrap();
    until("both running again", || {
        engine.is_running(a) && engine.is_running(b)
    })
    .await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deleting_a_queue_keeps_or_removes_its_downloads() {
    let (_dir, server, engine) = setup(50_000).await;
    let queues = Queues::start(engine.clone());
    let keep = queues.create("Keep", 1);
    let drop = queues.create("Drop", 1);
    let a = add_waiting(&engine, server.url("/a.bin")).await;
    let b = add_waiting(&engine, server.url("/b.bin")).await;
    queues.assign(&[a], Some(keep.id)).unwrap();
    queues.assign(&[b], Some(drop.id)).unwrap();
    queues.start_queue(keep.id).unwrap();
    until("a running", || engine.is_running(a)).await;

    queues.delete_queue(keep.id, false, false).await.unwrap();
    let da = engine.get(a).unwrap();
    assert_eq!(da.queue_id, None, "kept, out of the queue");
    assert!(!engine.is_running(a));

    queues.delete_queue(drop.id, true, true).await.unwrap();
    assert!(engine.get(b).is_err(), "removed with its queue");
    assert_eq!(
        queues.list().iter().map(|q| q.id).collect::<Vec<_>>(),
        Vec::<i64>::new()
    );
    assert!(queues.delete_queue(drop.id, false, false).await.is_err());
    assert!(queues.assign(&[a], Some(99)).is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn queues_are_saved() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = EngineConfig::new(dir.path().join("Downloads"));
    cfg.db_path = Some(dir.path().join("db"));
    {
        let engine = Engine::open(cfg.clone()).unwrap();
        let q = Queues::start(engine);
        let n = q.create("  ", 0);
        assert_eq!(n.name, "Queue 1");
        assert_eq!(n.max_active, 1);
        q.update(n.id, Some("Evening"), Some(4)).unwrap();
    }
    let engine = Engine::open(cfg).unwrap();
    let q = Queues::start(engine);
    let list = q.list();
    assert_eq!(list.len(), 1);
    assert_eq!((list[0].name.as_str(), list[0].max_active), ("Evening", 4));
}
