//! "Whole playlist" from the video button: each video becomes its own download, in a
//! queue named after the playlist. yt-dlp is stood in for by a small script that lists
//! a known playlist, so this doesn't depend on YouTube.

#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use ldm_engine::{DownloadKind, Engine, EngineConfig};
use ldm_ipc::{BrowserDownload, MediaPick, Request, Response, Source};
use ldm_media::Tools;
use ldm_service::queues::Queues;
use ldm_service::{Frontend, Service};

struct Quiet;
impl Frontend for Quiet {
    fn will_ask(&self, _: Source) -> bool {
        false
    }
    fn ask(&self, _: &[BrowserDownload], _: &[Option<i64>], _: Source) {}
}

const LIST: &str = r#"{"_type": "playlist", "title": "Google Search Stories", "entries": [
  {"_type": "url", "url": "https://www.youtube.com/watch?v=GvgqDSnpRQM", "title": "Andrew Willis, Skatepark Engineer", "duration": 90},
  {"_type": "url", "url": "https://www.youtube.com/watch?v=PRIVATE", "title": "[Private video]"},
  {"_type": "url", "url": "https://www.youtube.com/watch?v=V4DDt30Aat4", "title": "Mark Lesek: A New/Old Prosthetic", "duration": 159},
  {"_type": "url", "url": "https://www.youtube.com/watch?v=fIc_VEQ7Vo0", "title": null}
]}"#;

fn fake_ytdlp(dir: &std::path::Path) -> Tools {
    let script = dir.join("yt-dlp");
    std::fs::write(&script, format!("#!/bin/sh\ncat <<'JSON'\n{LIST}\nJSON\n")).unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    Tools {
        ytdlp: Some(script),
        ffmpeg: None,
        deno: None,
    }
}

#[tokio::test]
async fn a_whole_playlist_becomes_a_queue_of_downloads() {
    let dir = tempfile::tempdir().unwrap();
    let engine = Engine::open(EngineConfig::new(dir.path().join("Downloads"))).unwrap();
    let queues = Queues::start(engine.clone());
    let service = Service::new(engine.clone(), Arc::new(Quiet))
        .with_media_tools(fake_ytdlp(dir.path()))
        .with_queues(queues.clone());

    // The menu offers the playlist next to the video's own formats.
    let r = service
        .handle(Request::MediaFormats {
            item: Box::new(BrowserDownload {
                url: "https://www.youtube.com/watch?v=GvgqDSnpRQM".into(),
                ..Default::default()
            }),
            playlist_url: Some("https://www.youtube.com/playlist?list=PLBCF2DAC6FFB574DE".into()),
        })
        .await;
    let Response::MediaFormats {
        playlist: Some(list),
        ..
    } = r
    else {
        panic!("{r:?}")
    };
    assert_eq!(list.title, "Google Search Stories");
    assert_eq!(list.count, 3, "the private video isn't counted");
    assert_eq!(
        list.url,
        "https://www.youtube.com/playlist?list=PLBCF2DAC6FFB574DE"
    );
    let p720 = list
        .choices
        .iter()
        .find(|c| c.label == "720p")
        .unwrap()
        .clone();

    // Picking "720p" adds one download per video, in the playlist's own queue.
    let r = service
        .handle(Request::Download {
            items: vec![BrowserDownload {
                url: list.url.clone(),
                media: Some(MediaPick {
                    page_url: list.url.clone(),
                    format: Some(p720.format.clone()),
                    label: Some("720p".into()),
                    title: Some(list.title.clone()),
                    item: None,
                    playlist: true,
                }),
                ..Default::default()
            }],
            source: Source::Media,
            ask: Some(false),
        })
        .await;
    let Response::Accepted { ids } = r else {
        panic!("{r:?}")
    };
    assert_eq!(ids.len(), 3);
    let q = queues
        .list()
        .into_iter()
        .find(|q| q.name == "Google Search Stories")
        .expect("a queue for the playlist");
    assert_eq!(q.max_active, 2, "two videos at a time");
    let titles: Vec<_> = ids
        .iter()
        .map(|id| {
            let d = engine.get(*id).unwrap();
            assert_eq!(d.queue_id, Some(q.id));
            let DownloadKind::Media(m) = d.kind else {
                panic!("not a video download")
            };
            assert_eq!(m.format.as_deref(), Some(p720.format.as_str()));
            assert!(m.page_url.starts_with("https://www.youtube.com/watch?v="));
            m.title
        })
        .collect();
    assert_eq!(
        titles,
        [
            Some("Andrew Willis, Skatepark Engineer".to_string()),
            Some("Mark Lesek: A New/Old Prosthetic".into()),
            None
        ]
    );
}
