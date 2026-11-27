//! App settings, saved in the download database.

use std::path::PathBuf;

use ldm_torrent::privacy::Privacy;
use serde::{Deserialize, Serialize};

const KEY: &str = "app_settings";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct AppSettings {
    /// Which version of these settings was saved (for one-time changes of defaults).
    #[serde(default)]
    pub version: u32,
    /// Main download folder.
    pub download_dir: PathBuf,
    /// Save into Video, Music, Documents… sub-folders.
    pub category_folders: bool,
    /// Folders chosen for some types of file, instead of `<download folder>/<Type>`.
    pub category_dirs: std::collections::BTreeMap<ldm_engine::Category, PathBuf>,
    /// The user's own file-extension lists for some types (Video, Music, Documents,
    /// Compressed, Programs) instead of the built-in ones.
    pub category_extensions: std::collections::BTreeMap<ldm_engine::Category, Vec<String>>,
    /// When a link that was downloaded before comes in: ask, download it again, or
    /// open (or continue) the one already there.
    pub duplicates: Duplicates,
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
    /// Light, dark, or whatever Windows uses.
    pub theme: Theme,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Theme {
    /// Follow Windows (Settings → Personalization → Colors).
    #[default]
    System,
    Light,
    Dark,
}

impl Theme {
    /// For the windows' title bars; `None` follows Windows.
    pub fn window(self) -> Option<tauri::Theme> {
        match self {
            Theme::System => None,
            Theme::Light => Some(tauri::Theme::Light),
            Theme::Dark => Some(tauri::Theme::Dark),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Duplicates {
    #[default]
    Ask,
    /// Add it again under a numbered name ("file (1).zip").
    Again,
    /// Open the finished file, or continue the unfinished download.
    Existing,
}

/// "MP4", ".mkv", " avi " → "mp4", "mkv", "avi", without repeats.
pub fn clean_extensions(list: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for e in list {
        let e = e.trim().trim_start_matches('.').to_ascii_lowercase();
        if !e.is_empty() && e.chars().all(|c| c.is_ascii_alphanumeric()) && !out.contains(&e) {
            out.push(e);
        }
    }
    out
}

/// Version 2: browser downloads no longer ask first; they start with their own window.
const VERSION: u32 = 2;

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            version: VERSION,
            download_dir: PathBuf::new(),
            category_folders: true,
            category_dirs: Default::default(),
            category_extensions: Default::default(),
            duplicates: Duplicates::Ask,
            default_connections: 8,
            global_speed_limit: 0,
            // Browser downloads start at once, each in its own small window.
            ask_for_browser_downloads: false,
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
            theme: Theme::System,
        }
    }
}

impl AppSettings {
    /// One-time changes for settings saved by an older version.
    fn migrate(&mut self) {
        if self.version < 2 {
            self.ask_for_browser_downloads = false;
        }
        self.version = VERSION;
    }

    pub fn load(engine: &ldm_engine::Engine, default_dir: PathBuf) -> Self {
        let mut s: Self = engine
            .load_setting(KEY)
            .ok()
            .flatten()
            .and_then(|j| serde_json::from_str(&j).ok())
            .unwrap_or_default();
        s.migrate();
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
    /// Creates the folder for each type of file (Video, Music, Compressed, Programs,
    /// Documents, Torrents, General) if it isn't there yet; existing ones are used as
    /// they are.
    pub fn ensure_folders(&self, engine: &ldm_engine::Engine) {
        let config = engine.config();
        let _ = std::fs::create_dir_all(&config.download_dir);
        for c in ldm_engine::Category::ALL {
            let dir = config.dir_for(c);
            if let Err(e) = std::fs::create_dir_all(&dir) {
                tracing::warn!("could not create {}: {e}", dir.display());
            }
        }
    }

    pub fn apply(&self, engine: &ldm_engine::Engine) {
        let dir = self.download_dir.clone();
        let categories = self.category_folders;
        let limit = self.global_speed_limit;
        let sections = self.default_connections;
        let chosen: std::collections::HashMap<_, _> = self
            .category_dirs
            .iter()
            .filter(|(_, d)| !d.as_os_str().is_empty())
            .map(|(c, d)| (*c, d.clone()))
            .collect();
        let extensions: std::collections::HashMap<_, _> = self
            .category_extensions
            .iter()
            .map(|(c, list)| (*c, clean_extensions(list)))
            .collect();
        engine.update_config(|c| {
            c.category_extensions = extensions;
            c.download_dir = dir;
            c.category_folders = categories;
            c.category_dirs = chosen;
            c.global_speed_limit = limit;
            c.default_connections = sections;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn older_settings_stop_asking_for_browser_downloads_once() {
        let mut old: AppSettings =
            serde_json::from_str(r#"{"askForBrowserDownloads":true,"closeToTray":true}"#).unwrap();
        assert_eq!(old.version, 0);
        old.migrate();
        assert!(!old.ask_for_browser_downloads);
        assert_eq!(old.version, VERSION);
        // Turned back on after that, it stays on.
        old.ask_for_browser_downloads = true;
        let mut again: AppSettings =
            serde_json::from_str(&serde_json::to_string(&old).unwrap()).unwrap();
        again.migrate();
        assert!(again.ask_for_browser_downloads);
    }
}

#[cfg(test)]
mod folder_tests {
    use super::*;

    #[test]
    fn creates_each_type_folder_and_honours_chosen_ones() {
        let dir = tempfile::tempdir().unwrap();
        let engine = ldm_engine::Engine::open(ldm_engine::EngineConfig::new(dir.path())).unwrap();
        let mut s = AppSettings {
            download_dir: dir.path().join("Downloads"),
            ..Default::default()
        };
        // An existing folder is used as it is.
        std::fs::create_dir_all(s.download_dir.join("Video")).unwrap();
        std::fs::write(s.download_dir.join("Video").join("keep.txt"), b"x").unwrap();
        s.category_dirs
            .insert(ldm_engine::Category::Music, dir.path().join("Songs"));
        s.apply(&engine);
        s.ensure_folders(&engine);
        for name in [
            "Compressed",
            "Documents",
            "Programs",
            "Video",
            "Torrents",
            "General",
        ] {
            assert!(s.download_dir.join(name).is_dir(), "{name}");
        }
        assert!(
            !s.download_dir.join("Music").exists(),
            "Music has its own folder"
        );
        assert!(dir.path().join("Songs").is_dir());
        assert!(s.download_dir.join("Video").join("keep.txt").exists());
        // Saved and read back.
        let back: AppSettings = serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back.category_dirs, s.category_dirs);
    }
}

#[cfg(test)]
mod extension_tests {
    use super::*;

    #[test]
    fn edited_file_types_sort_new_downloads() {
        let engine = ldm_engine::Engine::open(ldm_engine::EngineConfig::new("/tmp/x")).unwrap();
        let mut s = AppSettings::default();
        s.category_extensions.insert(
            ldm_engine::Category::Programs,
            vec!["EXE".into(), ".iso".into(), " msi ".into(), "iso".into()],
        );
        s.apply(&engine);
        let c = engine.config();
        assert_eq!(
            c.category_extensions[&ldm_engine::Category::Programs],
            vec!["exe", "iso", "msi"]
        );
        assert_eq!(c.category_of("ubuntu.iso"), ldm_engine::Category::Programs);
        assert_eq!(c.category_of("a.zip"), ldm_engine::Category::Compressed);
    }
}
