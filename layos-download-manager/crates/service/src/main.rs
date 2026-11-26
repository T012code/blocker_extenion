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
    let service = Service::new(engine, Arc::new(NoFrontend));
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
