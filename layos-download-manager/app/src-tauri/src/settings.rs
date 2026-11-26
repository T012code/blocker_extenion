//! App settings, saved in the download database.

use std::path::PathBuf;

use ldm_torrent::privacy::Privacy;
use serde::{Deserialize, Serialize};

const KEY: &str = "app_settings";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Main download folder.
    pub download_dir: PathBuf,
    /// Save into Video, Music, Documents… sub-folders.
    pub category_folders: bool,
    /// Sections (parallel connections) for new downloads.
    pub default_connections: u8,
    /// Bytes per second for all downloads together, 0 = unlimited.
    pub global_speed_limit: u64,
    /// Show the "add download" window for downloads from the browser.
    pub ask_for_browser_downloads: bool,
    /// Closing the window keeps the app running in the tray.
    pub close_to_tray: bool,
    /// Minimizing the window hides it to the tray.
    pub minimize_to_tray: bool,
    /// Start Layó's Download Manager when you sign in to Windows.
    pub launch_on_startup: bool,
    /// When started with Windows, stay in the tray instead of opening the window.
    pub start_minimized: bool,
    pub notify_on_complete: bool,
    /// Open a small window for each download when it starts (not for queued ones).
    pub show_progress_window: bool,
    /// Open that window again, with Open and Show in folder, when the download finishes.
    pub show_complete_window: bool,
    /// Show the mini window, above other windows, while anything downloads.
    pub show_mini_window: bool,
    /// Start torrents from downloaded .torrent files.
    pub open_torrent_files: bool,
    /// Open magnet links and .torrent files from anywhere in Windows with this app.
    pub handle_torrent_links: bool,
    /// Share finished torrents until uploaded / size reaches this.
    pub seed_ratio: f32,
    pub privacy: Privacy,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            download_dir: PathBuf::new(),
            category_folders: true,
            default_connections: 8,
            global_speed_limit: 0,
            ask_for_browser_downloads: true,
            close_to_tray: true,
            minimize_to_tray: false,
            launch_on_startup: true,
            start_minimized: true,
            notify_on_complete: true,
            show_progress_window: true,
            show_complete_window: true,
            show_mini_window: true,
            open_torrent_files: true,
            handle_torrent_links: true,
            seed_ratio: 1.0,
            privacy: Privacy::default(),
        }
    }
}

impl AppSettings {
    pub fn load(engine: &ldm_engine::Engine, default_dir: PathBuf) -> Self {
        let mut s: Self = engine
            .load_setting(KEY)
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        if s.download_dir.as_os_str().is_empty() {
            s.download_dir = default_dir;
        }
        s.default_connections = s.default_connections.clamp(1, ldm_engine::MAX_CONNECTIONS);
        s
    }

    pub fn save(&self, engine: &ldm_engine::Engine) -> ldm_engine::Result<()> {
        engine.store_setting(KEY, &serde_json::to_string(self)?)?;
        // Read by the service when a .torrent file finishes downloading.
        engine.store_setting(
            "open_torrent_files",
            if self.open_torrent_files {
                "true"
            } else {
                "false"
            },
        )?;
        engine.set_default_connections(self.default_connections)?;
        Ok(())
    }

    /// Applies the settings that live in the engine.
    pub fn apply(&self, engine: &ldm_engine::Engine) {
        let dir = self.download_dir.clone();
        let categories = self.category_folders;
        let limit = self.global_speed_limit;
        let sections = self.default_connections;
        engine.update_config(|c| {
            c.download_dir = dir;
            c.category_folders = categories;
            c.global_speed_limit = limit;
            c.default_connections = sections;
        });
    }
}
