//! Smart Clip's settings, kept as JSON in the app's local data folder (local, not
//! roaming: this PC's id and name must stay on this PC).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The shortcuts on offer (as the shortcut plugin reads them), and "Off".
pub const HOTKEYS: &[&str] = &["Alt+Shift+V", "Ctrl+Shift+V", "Super+Alt+V", "Off"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// "system", "light" or "dark".
    pub theme: String,
    /// The floating icon beside text boxes.
    pub float_icon: bool,
    /// Apps the floating icon stays out of (program file names).
    pub hidden_apps: Vec<String>,
    /// The shortcut that opens the picker, one of HOTKEYS.
    pub hotkey: String,
    pub sync_images: bool,
    pub paused: bool,
    pub device_id: String,
    pub device_name: String,
    /// Set once the first-run setup (launch at startup) has been done.
    pub first_run_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: "system".into(),
            float_icon: true,
            hidden_apps: crate::win::float::DEFAULT_HIDDEN.iter().map(|s| s.to_string()).collect(),
            hotkey: HOTKEYS[0].into(),
            sync_images: true,
            paused: false,
            device_id: String::new(),
            device_name: String::new(),
            first_run_done: false,
        }
    }
}

fn file(dir: &Path) -> PathBuf {
    dir.join("settings.json")
}

impl Settings {
    pub fn load(dir: &Path) -> Settings {
        let mut s: Settings =
            std::fs::read(file(dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default();
        if !matches!(s.theme.as_str(), "system" | "light" | "dark") {
            s.theme = "system".into();
        }
        if !HOTKEYS.contains(&s.hotkey.as_str()) {
            s.hotkey = HOTKEYS[0].into();
        }
        if uuid::Uuid::parse_str(&s.device_id).is_err() {
            s.device_id = uuid::Uuid::new_v4().to_string();
        }
        if s.device_name.trim().is_empty() {
            s.device_name = crate::win::computer_name();
        }
        s
    }

    /// Writes the settings (to a temporary file first, so a crash can't leave half a file).
    pub fn save(&self, dir: &Path) {
        let _ = std::fs::create_dir_all(dir);
        let tmp = dir.join("settings.json.tmp");
        if let Ok(json) = serde_json::to_vec_pretty(self)
            && std::fs::write(&tmp, json).is_ok()
        {
            let _ = std::fs::rename(&tmp, file(dir));
        }
    }
}

/// The saved sign-in, protected with DPAPI.
pub mod session {
    use super::*;
    use smart_clip_core::StoredSession;

    fn file(dir: &Path) -> PathBuf {
        dir.join("session.bin")
    }

    pub fn load(dir: &Path) -> Option<StoredSession> {
        let sealed = std::fs::read(file(dir)).ok()?;
        let plain = crate::win::dpapi::unprotect(&sealed)?;
        serde_json::from_slice(&plain).ok()
    }

    pub fn save(dir: &Path, session: Option<&StoredSession>) {
        match session {
            Some(s) => {
                let _ = std::fs::create_dir_all(dir);
                if let Some(sealed) = serde_json::to_vec(s).ok().and_then(|j| crate::win::dpapi::protect(&j)) {
                    let tmp = dir.join("session.bin.tmp");
                    if std::fs::write(&tmp, sealed).is_ok() {
                        let _ = std::fs::rename(&tmp, file(dir));
                    }
                }
            }
            None => {
                let _ = std::fs::remove_file(file(dir));
            }
        }
    }
}
