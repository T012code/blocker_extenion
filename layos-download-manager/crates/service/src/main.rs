//! Runs the download manager without a window: listens for the browser and downloads.
//! Useful for development and testing before the desktop app is installed.
//!
//! Usage: ldm-headless [data folder]   (or set LDM_DATA_DIR)

use std::path::PathBuf;
use std::sync::Arc;

use ldm_engine::{Engine, EngineConfig, Event};
use ldm_service::{NoFrontend, Service};

#[tokio::main]
async fn main() {
    let dir = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with("--"))
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LDM_DATA_DIR").map(PathBuf::from))
        .unwrap_or_else(|| std::env::temp_dir().join("layos-download-manager"));
    let mut config = EngineConfig::new(dir.join("Downloads"));
    config.db_path = Some(dir.join("downloads.db"));
    let engine = match Engine::open(config) {
        Ok(e) => e,
        Err(e) => {
            eprintln!("could not open the download database: {e}");
            std::process::exit(1);
        }
    };
    let mut events = engine.subscribe();
    tokio::spawn(async move {
        while let Ok(ev) = events.recv().await {
            if let Event::Updated { download } | Event::Added { download } = ev {
                eprintln!(
                    "[{}] {:?} {} ({:?} bytes)",
                    download.id, download.status, download.file_name, download.size
                );
            }
        }
    });
    // Torrents and video streams are handled by their own runners.
    let app_dir = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.to_path_buf()));
    let tools = ldm_media::Tools::find(app_dir.as_deref());
    if tools.ytdlp.is_none() {
        eprintln!("yt-dlp not found: the video button will not be able to list formats");
    }
    engine.set_runner(
        "media",
        ldm_media::MediaRunner::new(tools.clone(), dir.join("media-work")),
    );
    // Torrent privacy (VPN / proxy, kill switch). Stored as JSON under "torrent_privacy";
    // without one, torrents wait for a VPN or proxy rather than expose your IP.
    let privacy: ldm_torrent::privacy::Privacy = engine
        .load_setting("torrent_privacy")
        .ok()
        .flatten()
        .and_then(|j| serde_json::from_str(&j).ok())
        .unwrap_or_default();
    eprintln!("torrent privacy: {:?}", privacy.mode);
    let mut torrent_opts = ldm_torrent::TorrentOptions::new(dir.join("torrents"));
    torrent_opts.privacy = privacy;
    match ldm_torrent::TorrentRunner::start(torrent_opts).await {
        Ok(t) => engine.set_runner("torrent", t),
        Err(e) => eprintln!("torrents are unavailable: {e:#}"),
    }
    let service = Service::new(engine, Arc::new(NoFrontend)).with_media_tools(tools);
    let path = ldm_ipc::default_path();
    eprintln!(
        "Layó's Download Manager (headless) listening on {path}, saving to {}",
        dir.display()
    );
    if let Err(e) = ldm_ipc::serve(&path, move |req| {
        let s = service.clone();
        async move { s.handle(req).await }
    })
    .await
    {
        eprintln!("{e}");
        std::process::exit(1);
    }
}
