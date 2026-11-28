//! Tries the video download on real pages, the way the app does it: list the formats,
//! then download one (the smallest video, so it's quick) through the engine.
//!
//!     cargo run -p ldm-media --example try_site -- <page url> [<page url>...]
//!
//! Needs yt-dlp, ffmpeg and Deno on the PATH (or LDM_YTDLP / LDM_FFMPEG / LDM_DENO).
//! Set LDM_TRY_LIST_ONLY=1 to only list the formats.

use std::time::{Duration, Instant};

use ldm_engine::{
    DownloadKind, DownloadStatus, Engine, EngineConfig, MediaSpec, NewDownload, RequestInfo,
};
use ldm_media::{MediaRunner, Tools};

#[tokio::main]
async fn main() {
    let tools = Tools::find(None);
    println!(
        "yt-dlp {:?}, ffmpeg {:?}, deno {:?}",
        tools.ytdlp, tools.ffmpeg, tools.deno
    );
    let list_only = std::env::var_os("LDM_TRY_LIST_ONLY").is_some();
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path().join("Downloads"))).unwrap();
    engine.set_runner(
        "media",
        MediaRunner::new(tools.clone(), dir.path().join("work")),
    );
    for url in std::env::args().skip(1) {
        println!("\n== {url}");
        let started = Instant::now();
        let info =
            match ldm_media::formats(&tools, &RequestInfo::new(&url), Duration::from_secs(120))
                .await
            {
                Ok(i) => i,
                Err(e) => {
                    println!(
                        "FORMATS FAILED ({:.1}s): {e}",
                        started.elapsed().as_secs_f32()
                    );
                    continue;
                }
            };
        println!(
            "title: {:?} ({:.1}s)",
            info.title,
            started.elapsed().as_secs_f32()
        );
        for c in &info.choices {
            println!(
                "  {:<22} {:>12} {}",
                c.label,
                c.size.map(|s| format!("{s} B")).unwrap_or_default(),
                c.format
            );
        }
        if list_only {
            continue;
        }
        // LDM_TRY_BEST=1: the largest video (as people usually pick); otherwise the smallest.
        let best = std::env::var_os("LDM_TRY_BEST").is_some();
        let videos = info.choices.iter().filter(|c| c.kind == "video");
        let pick = if best {
            videos.max_by_key(|c| c.height.unwrap_or(0))
        } else {
            videos.min_by_key(|c| c.height.unwrap_or(u32::MAX))
        };
        let Some(choice) = pick.or(info.choices.first()).cloned() else {
            println!("NO CHOICES");
            continue;
        };
        let id = engine
            .add(NewDownload {
                request: RequestInfo::new(&url),
                kind: DownloadKind::Media(MediaSpec {
                    page_url: url.clone(),
                    format: Some(choice.format.clone()),
                    label: Some(choice.label.clone()),
                    // LDM_TRY_TITLE: the page title, as the browser sends it.
                    item: choice.item,
                    title: Some(
                        std::env::var("LDM_TRY_TITLE").unwrap_or_else(|_| info.title.clone()),
                    ),
                }),
                start: true,
                ..Default::default()
            })
            .await
            .unwrap();
        let end = Instant::now() + Duration::from_secs(240);
        loop {
            let d = engine.get(id).unwrap();
            match d.status {
                DownloadStatus::Completed => {
                    let path = d.save_dir.join(&d.file_name);
                    let ok = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                    println!(
                        "DOWNLOADED {} ({} bytes on disk, size {:?}) in {:.1}s",
                        d.file_name,
                        ok,
                        d.size,
                        started.elapsed().as_secs_f32()
                    );
                    break;
                }
                DownloadStatus::Failed => {
                    println!("DOWNLOAD FAILED: {:?}", d.error);
                    break;
                }
                _ if Instant::now() > end => {
                    println!("TIMED OUT at {:?}, {} bytes", d.status, d.downloaded);
                    let _ = engine.pause(id).await;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(250)).await,
            }
        }
    }
}
