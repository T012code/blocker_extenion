//! Runs the real bridge program the way a browser does and checks the download arrives.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use ldm_engine::{DownloadStatus, Engine, EngineConfig};
use ldm_ipc::Response;
use ldm_service::{NoFrontend, Service};
use ldm_testkit::{ServerOptions, TestServer, test_data};
use serde_json::json;

const HOST: &str = env!("CARGO_BIN_EXE_ldm-native-host");

/// Sends messages like a browser does and returns the replies.
fn talk(ipc_path: &str, env: &[(&str, &Path)], messages: &[serde_json::Value]) -> Vec<Response> {
    let mut cmd = Command::new(HOST);
    cmd.arg("chrome-extension://klknhohjiaanbbicfdonmcnflkgjofai/")
        .env("LDM_IPC_PATH", ipc_path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    for m in messages {
        let body = serde_json::to_vec(m).unwrap();
        stdin.write_all(&(body.len() as u32).to_le_bytes()).unwrap();
        stdin.write_all(&body).unwrap();
    }
    drop(stdin);
    let mut out = Vec::new();
    child.stdout.take().unwrap().read_to_end(&mut out).unwrap();
    child.wait().unwrap();
    let mut replies = Vec::new();
    let mut rest = &out[..];
    while rest.len() >= 4 {
        let len = u32::from_le_bytes(rest[..4].try_into().unwrap()) as usize;
        replies.push(serde_json::from_slice(&rest[4..4 + len]).unwrap());
        rest = &rest[4 + len..];
    }
    replies
}

async fn wait_file(path: &Path, expected: &[u8]) {
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if std::fs::read(path).ok().as_deref() == Some(expected) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("{} never arrived", path.display());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn browser_download_reaches_the_app() {
    let data = test_data(3 * 1024 * 1024);
    let server = TestServer::start(data.clone(), ServerOptions::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let ipc = dir.path().join("ldm.sock").to_string_lossy().into_owned();

    let mut cfg = EngineConfig::new(dir.path().join("Downloads"));
    cfg.min_segment_size = 256 * 1024;
    let engine = Engine::open(cfg).unwrap();
    let service = Service::new(engine.clone(), Arc::new(NoFrontend));
    let p = ipc.clone();
    tokio::spawn(async move {
        ldm_ipc::serve(&p, move |req| {
            let s = service.clone();
            async move { s.handle(req).await }
        })
        .await
        .unwrap()
    });
    while !ldm_ipc::is_running(&ipc).await {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    let ipc2 = ipc.clone();
    let url = server.url("/files/setup.exe");
    let replies = tokio::task::spawn_blocking(move || {
        talk(
            &ipc2,
            &[],
            &[
                json!({"type": "ping"}),
                json!({"type": "download", "source": "capture", "ask": false, "items": [{
                    "url": url, "referer": "https://example.com/page", "cookies": "session=abc",
                    "userAgent": "TestBrowser/1.0", "fileName": "setup.exe", "fileSize": 3145728
                }]}),
            ],
        )
    })
    .await
    .unwrap();

    assert!(
        matches!(
            &replies[0],
            Response::Pong {
                app_running: true,
                host_version: Some(_),
                ..
            }
        ),
        "{replies:?}"
    );
    let Response::Accepted { ids } = &replies[1] else {
        panic!("{replies:?}")
    };
    assert_eq!(ids.len(), 1);
    let d = engine.get(ids[0]).unwrap();
    assert_eq!(d.request.cookies.as_deref(), Some("session=abc"));
    assert_eq!(
        d.request.referer.as_deref(),
        Some("https://example.com/page")
    );
    assert_eq!(d.request.user_agent.as_deref(), Some("TestBrowser/1.0"));

    wait_file(&dir.path().join("Downloads/Programs/setup.exe"), &data).await;
    let deadline = Instant::now() + Duration::from_secs(5);
    while engine.get(ids[0]).unwrap().status != DownloadStatus::Completed
        && Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        engine.get(ids[0]).unwrap().status,
        DownloadStatus::Completed
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn ping_without_app_says_not_running_and_does_not_start_it() {
    let dir = tempfile::tempdir().unwrap();
    let ipc = dir
        .path()
        .join("nobody.sock")
        .to_string_lossy()
        .into_owned();
    let marker = dir.path().join("started");
    let replies = talk(
        &ipc,
        &[("LDM_APP_PATH", &marker)],
        &[json!({"type": "ping"})],
    );
    assert!(
        matches!(
            &replies[0],
            Response::Pong {
                app_running: false,
                ..
            }
        ),
        "{replies:?}"
    );
    assert!(!marker.exists());
}

/// The bridge starts the app when a download comes in and it isn't running.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn starts_the_app_when_needed() {
    let headless = build_headless();
    let data = test_data(500_000);
    let server = TestServer::start(data.clone(), ServerOptions::default()).await;
    let dir = tempfile::tempdir().unwrap();
    let ipc = dir.path().join("ldm.sock").to_string_lossy().into_owned();
    let data_dir = dir.path().join("data");
    let url = server.url("/song.mp3");

    let ipc2 = ipc.clone();
    let (h, dd) = (headless.clone(), data_dir.clone());
    let replies = tokio::task::spawn_blocking(move || {
        talk(
            &ipc2,
            &[("LDM_APP_PATH", &h), ("LDM_DATA_DIR", &dd)],
            &[json!({"type": "download", "ask": false, "items": [{"url": url}]})],
        )
    })
    .await
    .unwrap();
    assert!(
        matches!(&replies[0], Response::Accepted { ids } if ids.len() == 1),
        "{replies:?}"
    );
    wait_file(&data_dir.join("Downloads/Music/song.mp3"), &data).await;

    // Stop the app we started.
    let _ = Command::new("pkill")
        .arg("-f")
        .arg(data_dir.to_string_lossy().as_ref())
        .status();
    let _ = Command::new("pkill")
        .args(["-f", &format!("{} --from-browser", headless.display())])
        .status();
}

fn build_headless() -> PathBuf {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .args(["build", "-q", "-p", "ldm-service", "--bin", "ldm-headless"])
        .status()
        .unwrap();
    assert!(status.success());
    Path::new(HOST).with_file_name(if cfg!(windows) {
        "ldm-headless.exe"
    } else {
        "ldm-headless"
    })
}

/// The extension can list downloads, pause one and continue it, all through the bridge.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pause_and_continue_through_the_bridge() {
    let data = test_data(4 * 1024 * 1024);
    let server = TestServer::start(
        data.clone(),
        ServerOptions {
            throttle: Some(300 * 1024),
            ..Default::default()
        },
    )
    .await;
    let dir = tempfile::tempdir().unwrap();
    let ipc = dir.path().join("ldm.sock").to_string_lossy().into_owned();
    let mut cfg = EngineConfig::new(dir.path().join("Downloads"));
    cfg.min_segment_size = 256 * 1024;
    cfg.progress_interval = Duration::from_millis(100);
    let engine = Engine::open(cfg).unwrap();
    let service = Service::new(engine.clone(), Arc::new(NoFrontend));
    let p = ipc.clone();
    tokio::spawn(async move {
        ldm_ipc::serve(&p, move |req| {
            let s = service.clone();
            async move { s.handle(req).await }
        })
        .await
        .unwrap()
    });
    while !ldm_ipc::is_running(&ipc).await {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let send = |msg: serde_json::Value| {
        let ipc = ipc.clone();
        async move {
            tokio::task::spawn_blocking(move || talk(&ipc, &[], &[msg]).remove(0))
                .await
                .unwrap()
        }
    };
    let list = || async {
        match send(json!({"type": "list"})).await {
            Response::Downloads {
                app_running: true,
                items,
            } => items,
            other => panic!("{other:?}"),
        }
    };

    let url = server.url("/files/disk.iso");
    let Response::Accepted { ids } =
        send(json!({"type": "download", "ask": false, "items": [{"url": url}]})).await
    else {
        panic!()
    };
    let id = ids[0];

    // Running: progress and speed show up, and the server can resume.
    let mut running = None;
    for _ in 0..100 {
        let items = list().await;
        if let Some(d) = items
            .iter()
            .find(|d| d.id == id && d.status == "downloading" && d.downloaded > 200_000)
        {
            running = Some(d.clone());
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let running = running.expect("download should be listed as downloading");
    assert!(running.resumable);
    assert!(running.speed > 0, "{running:?}");
    assert_eq!(running.file_name, "disk.iso");

    // Pause keeps what was fetched.
    assert_eq!(send(json!({"type": "pause", "id": id})).await, Response::Ok);
    let paused = list().await.into_iter().find(|d| d.id == id).unwrap();
    assert_eq!(paused.status, "paused");
    assert!(paused.downloaded > 0 && paused.downloaded < data.len() as u64);
    assert_eq!(paused.speed, 0);
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        list()
            .await
            .into_iter()
            .find(|d| d.id == id)
            .unwrap()
            .downloaded,
        paused.downloaded,
        "nothing moves while paused"
    );

    // Continue finishes from where it stopped.
    server.set_options(|o| o.throttle = None);
    let before = server.state.ranges_seen.lock().unwrap().len();
    assert_eq!(
        send(json!({"type": "resume", "id": id})).await,
        Response::Ok
    );
    wait_file(&dir.path().join("Downloads/Compressed/disk.iso"), &data).await;
    let ranges = server.state.ranges_seen.lock().unwrap()[before..].to_vec();
    assert!(
        ranges
            .iter()
            .any(|r| r != "bytes=0-0" && !r.starts_with("bytes=0-")),
        "should continue mid-file: {ranges:?}"
    );
    let done = list().await.into_iter().find(|d| d.id == id).unwrap();
    assert_eq!(done.status, "completed");
    assert_eq!(done.downloaded, data.len() as u64);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn list_without_app_does_not_start_it() {
    let dir = tempfile::tempdir().unwrap();
    let ipc = dir
        .path()
        .join("nobody.sock")
        .to_string_lossy()
        .into_owned();
    let marker = dir.path().join("started");
    let replies = talk(
        &ipc,
        &[("LDM_APP_PATH", &marker)],
        &[json!({"type": "list"})],
    );
    assert_eq!(
        replies[0],
        Response::Downloads {
            app_running: false,
            items: vec![]
        }
    );
}
