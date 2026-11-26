//! Real video downloads through yt-dlp from a local HLS stream made with ffmpeg.
//! Skipped (with a message) when yt-dlp or ffmpeg isn't installed.

use std::path::Path;
use std::process::Command;
use std::time::{Duration, Instant};

use ldm_engine::{
    DownloadKind, DownloadStatus, Engine, EngineConfig, Event, MediaSpec, NewDownload, RequestInfo,
};
use ldm_media::{MediaRunner, Tools};
use ldm_testkit::StaticServer;

fn tools() -> Option<Tools> {
    let t = Tools::find(None);
    if t.ytdlp.is_none() || t.ffmpeg.is_none() {
        eprintln!(
            "skipping: yt-dlp and ffmpeg are needed (set LDM_YTDLP / LDM_FFMPEG or put them on PATH)"
        );
        return None;
    }
    Some(t)
}

/// A 2-quality HLS stream (360p and 180p) with sound, `seconds` long.
fn make_hls(dir: &Path, ffmpeg: &Path, seconds: u32) {
    let status = Command::new(ffmpeg)
        .current_dir(dir)
        .args(["-hide_banner", "-loglevel", "error", "-y"])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("testsrc2=duration={seconds}:size=640x360:rate=25"),
        ])
        .args([
            "-f",
            "lavfi",
            "-i",
            &format!("sine=frequency=440:duration={seconds}"),
        ])
        .args(["-filter_complex", "[0:v]split=2[a][b];[b]scale=320:180[b2]"])
        .args(["-map", "[a]", "-map", "[b2]", "-map", "1:a", "-map", "1:a"])
        .args([
            "-c:v",
            "libx264",
            "-preset",
            "ultrafast",
            "-b:v:0",
            "1500k",
            "-b:v:1",
            "400k",
            "-c:a",
            "aac",
            "-b:a",
            "96k",
        ])
        .args(["-f", "hls", "-hls_time", "1", "-hls_playlist_type", "vod"])
        .args([
            "-var_stream_map",
            "v:0,a:0 v:1,a:1",
            "-master_pl_name",
            "master.m3u8",
        ])
        .args(["-hls_segment_filename", "v%v_%03d.ts", "v%v.m3u8"])
        .status()
        .unwrap();
    assert!(status.success(), "ffmpeg could not make the test stream");
}

fn video_height(ffmpeg: &Path, file: &Path) -> Option<u32> {
    let ffprobe = ffmpeg.with_file_name(if cfg!(windows) {
        "ffprobe.exe"
    } else {
        "ffprobe"
    });
    let out = Command::new(ffprobe)
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=height",
            "-of",
            "csv=p=0",
        ])
        .arg(file)
        .output()
        .ok()?;
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

async fn wait_status(engine: &Engine, id: i64, status: DownloadStatus, secs: u64) {
    let end = Instant::now() + Duration::from_secs(secs);
    while Instant::now() < end {
        let d = engine.get(id).unwrap();
        if d.status == status && !engine.is_running(id) {
            return;
        }
        if d.status == DownloadStatus::Failed {
            panic!("failed: {:?}", d.error);
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("never reached {status:?}: {:?}", engine.get(id).unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn lists_stream_qualities() {
    let Some(tools) = tools() else { return };
    let dir = tempfile::tempdir().unwrap();
    make_hls(dir.path(), tools.ffmpeg.as_deref().unwrap(), 4);
    let server = StaticServer::start(dir.path().to_path_buf()).await;

    let info = ldm_media::formats(
        &tools,
        &RequestInfo::new(server.url("master.m3u8")),
        Duration::from_secs(60),
    )
    .await
    .unwrap();
    let labels: Vec<_> = info.choices.iter().map(|c| c.label.as_str()).collect();
    assert!(
        labels.contains(&"360p MP4") && labels.contains(&"180p MP4"),
        "{labels:?}"
    );
    assert_eq!(info.choices[0].height, Some(360), "best first");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn downloads_a_chosen_quality_with_pause_and_continue() {
    let Some(tools) = tools() else { return };
    let ffmpeg = tools.ffmpeg.clone().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let stream = dir.path().join("stream");
    std::fs::create_dir_all(&stream).unwrap();
    make_hls(&stream, &ffmpeg, 30);
    let server = StaticServer::start(stream.clone()).await;
    let master = server.url("master.m3u8");
    let info = ldm_media::formats(&tools, &RequestInfo::new(&master), Duration::from_secs(60))
        .await
        .unwrap();
    let small = info
        .choices
        .iter()
        .find(|c| c.height == Some(180))
        .unwrap()
        .clone();

    let engine = Engine::open(EngineConfig::new(dir.path().join("Downloads"))).unwrap();
    engine.set_runner(
        "media",
        MediaRunner::new(tools.clone(), dir.path().join("media-work")),
    );
    let mut events = engine.subscribe();
    let id = engine
        .add(NewDownload {
            request: RequestInfo::new(&master),
            kind: DownloadKind::Media(MediaSpec {
                page_url: master.clone(),
                format: Some(small.format.clone()),
                label: Some(small.label.clone()),
                title: Some("Test stream".into()),
            }),
            // Slow enough to pause part-way.
            speed_limit: Some(150 * 1024),
            start: true,
            ..Default::default()
        })
        .await
        .unwrap();
    assert_eq!(
        engine.get(id).unwrap().save_dir,
        dir.path().join("Downloads/Video")
    );

    let end = Instant::now() + Duration::from_secs(60);
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
            && downloaded > 200 * 1024
        {
            break;
        }
    }
    engine.pause(id).await.unwrap();
    let paused = engine.get(id).unwrap();
    assert_eq!(paused.status, DownloadStatus::Paused);
    assert!(paused.downloaded > 0);
    let work = dir.path().join("media-work").join(id.to_string());
    assert!(
        std::fs::read_dir(&work).map(|d| d.count()).unwrap_or(0) > 0,
        "partial pieces are kept while paused"
    );

    engine.set_speed_limit(id, 0).unwrap();
    engine.start(id).unwrap();
    wait_status(&engine, id, DownloadStatus::Completed, 120).await;
    let d = engine.get(id).unwrap();
    let file = d.final_path();
    assert!(file.exists(), "{file:?}");
    assert!(d.file_name.ends_with(".mp4"), "{}", d.file_name);
    assert_eq!(d.size, Some(std::fs::metadata(&file).unwrap().len()));
    if let Some(h) = video_height(&ffmpeg, &file) {
        assert_eq!(h, 180, "the chosen quality was downloaded");
    }
    assert!(!work.exists(), "work folder is cleaned up");
}
