//! The small windows: one per download (progress while it runs, then a "done" view),
//! and the mini window that sums up everything downloading, above other windows.

use std::collections::HashSet;
use std::sync::Mutex;

use ldm_engine::{Download, DownloadStatus};
use tauri::{
    AppHandle, Manager, PhysicalPosition, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
};

use crate::settings::AppSettings;

/// At most this many download windows open by themselves at once.
const MAX_AUTO_OPEN: usize = 3;
const PROGRESS_SIZE: (f64, f64) = (460.0, 356.0);
const MINI_SIZE: (f64, f64) = (320.0, 96.0);
pub const MINI: &str = "mini";

#[derive(Default)]
pub struct Windows {
    /// Downloads added while the app runs: only these open windows by themselves,
    /// not ones that carry on from last time.
    fresh: Mutex<HashSet<i64>>,
    /// Downloads whose progress window has already opened by itself.
    shown: Mutex<HashSet<i64>>,
    /// Downloads whose "done" window has already opened.
    finished: Mutex<HashSet<i64>>,
    /// The mini window was closed: it stays away until asked for.
    mini_dismissed: Mutex<bool>,
    /// Links that came in again, by the download they duplicate, waiting for an answer
    /// in that download's window.
    duplicates: Mutex<std::collections::HashMap<i64, ldm_ipc::BrowserDownload>>,
}

/// Which windows a download's change calls for.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Plan {
    /// The mini window (anything downloading).
    pub mini: bool,
    /// The download's own window, with its progress (it started).
    pub progress: bool,
    /// The download's own window, saying it's complete.
    pub done: bool,
}

/// `fresh`: added while the app runs (not carried over from last time).
pub fn plan(d: &Download, s: &AppSettings, fresh: bool) -> Plan {
    // Started in the background for an "add download" question: that question's own
    // window is open already; nothing else shows until it's answered.
    if d.pending {
        return Plan::default();
    }
    let running = matches!(
        d.status,
        DownloadStatus::Downloading | DownloadStatus::Connecting
    );
    let mut p = Plan {
        mini: running && s.show_mini_window,
        ..Default::default()
    };
    // Downloads in a queue run unattended (no windows for them, as in IDM), and so do
    // ones carried over from last time.
    if !fresh || d.queue_id.is_some() {
        return p;
    }
    // A torrent waiting for the VPN gets its window too, saying so.
    p.progress = (running || d.status == DownloadStatus::Blocked) && s.show_progress_window;
    p.done = matches!(
        d.status,
        DownloadStatus::Completed | DownloadStatus::Seeding
    ) && s.show_complete_window;
    p
}

pub fn label(id: i64) -> String {
    format!("dl-{id}")
}

impl Windows {
    pub fn added(&self, id: i64) {
        self.fresh.lock().unwrap().insert(id);
    }

    pub fn set_duplicate(&self, id: i64, item: ldm_ipc::BrowserDownload) {
        self.duplicates.lock().unwrap().insert(id, item);
    }

    pub fn duplicate(&self, id: i64) -> Option<ldm_ipc::BrowserDownload> {
        self.duplicates.lock().unwrap().get(&id).cloned()
    }

    pub fn take_duplicate(&self, id: i64) -> Option<ldm_ipc::BrowserDownload> {
        self.duplicates.lock().unwrap().remove(&id)
    }

    pub fn set_mini_dismissed(&self, on: bool) {
        *self.mini_dismissed.lock().unwrap() = on;
    }

    /// A download changed: open its window when it starts or finishes, and the mini
    /// window while anything downloads, as the settings say. None of these needs the
    /// main window: they open on their own while the app sits in the tray.
    pub fn updated(&self, app: &AppHandle, d: &Download, s: &AppSettings) {
        let fresh = self.fresh.lock().unwrap().contains(&d.id);
        let p = plan(d, s, fresh);
        if p.mini && !*self.mini_dismissed.lock().unwrap() {
            let _ = show_mini(app);
        }
        if p.progress && self.shown.lock().unwrap().insert(d.id) && open_count(app) < MAX_AUTO_OPEN
        {
            let _ = open_download(app, d.id, false);
        }
        if p.done && self.finished.lock().unwrap().insert(d.id) {
            let _ = open_download(app, d.id, true);
        }
    }

    pub fn removed(&self, app: &AppHandle, id: i64) {
        if let Some(w) = app.get_webview_window(&label(id)) {
            let _ = w.close();
        }
    }
}

fn open_count(app: &AppHandle) -> usize {
    app.webview_windows()
        .keys()
        .filter(|l| l.starts_with("dl-"))
        .count()
}

/// Opens (or brings forward) a download's window. `focus` is false for windows that
/// open by themselves, so typing elsewhere isn't interrupted.
pub fn open_download(app: &AppHandle, id: i64, focus: bool) -> tauri::Result<()> {
    let label = label(id);
    if let Some(w) = app.get_webview_window(&label) {
        let _ = w.unminimize();
        w.show()?;
        if focus {
            w.set_focus()?;
        }
        return Ok(());
    }
    let others = open_count(app);
    let w = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("progress.html".into()))
        .title("Layó's Download Manager")
        .inner_size(PROGRESS_SIZE.0, PROGRESS_SIZE.1)
        .resizable(false)
        .maximizable(false)
        .center()
        .focused(focus)
        .visible(false)
        .build()?;
    // Several at once: step each one down and right so none hides another.
    if others > 0
        && let Ok(p) = w.outer_position()
    {
        let step = (28.0 * w.scale_factor().unwrap_or(1.0)) as i32 * others as i32;
        let _ = w.set_position(PhysicalPosition::new(p.x + step, p.y + step));
    }
    w.show()?;
    Ok(())
}

/// Shows the mini window at the bottom right of the screen, creating it if needed.
pub fn show_mini(app: &AppHandle) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window(MINI) {
        if !w.is_visible().unwrap_or(false) {
            w.show()?;
        }
        return Ok(());
    }
    let w = WebviewWindowBuilder::new(app, MINI, WebviewUrl::App("mini.html".into()))
        .title("Downloads")
        .inner_size(MINI_SIZE.0, MINI_SIZE.1)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .focused(false)
        .visible(false)
        .build()?;
    place_bottom_right(&w);
    w.show()?;
    Ok(())
}

/// Bottom right of the screen's work area (above the taskbar), 16 px in.
fn place_bottom_right(w: &WebviewWindow) {
    let (Ok(Some(m)), Ok(size)) = (w.current_monitor(), w.outer_size()) else {
        return;
    };
    let area = m.work_area();
    let gap = (16.0 * m.scale_factor()) as i32;
    let x = area.position.x + area.size.width as i32 - size.width as i32 - gap;
    let y = area.position.y + area.size.height as i32 - size.height as i32 - gap;
    let _ = w.set_position(PhysicalPosition::new(x, y));
}

/// Resizes a small window. The mini window grows upwards, keeping its bottom edge.
pub fn resize(w: &WebviewWindow, width: f64, height: f64) -> tauri::Result<()> {
    let before = w.outer_position()?;
    let old = w.outer_size()?;
    w.set_size(tauri::LogicalSize::new(width, height))?;
    if w.label() == MINI {
        let new = w.outer_size()?;
        let x = before.x + old.width as i32 - new.width as i32;
        let y = before.y + old.height as i32 - new.height as i32;
        w.set_position(PhysicalPosition::new(x, y))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dl(status: DownloadStatus) -> Download {
        let mut d: Download = serde_json::from_value(serde_json::json!({
            "id": 1, "request": { "url": "https://example.com/a.zip" }, "finalUrl": null,
            "fileName": "a.zip", "saveDir": "/d", "category": "compressed", "size": null,
            "resumable": true, "contentType": null, "status": "queued", "error": null,
            "downloaded": 0, "segments": [], "connections": 8, "speedLimit": 0, "queueId": null,
            "tempPath": null, "createdAt": 0, "completedAt": null
        }))
        .unwrap();
        d.status = status;
        d
    }

    #[test]
    fn a_new_download_shows_its_window_and_the_mini_window() {
        let s = AppSettings::default();
        let p = plan(&dl(DownloadStatus::Downloading), &s, true);
        assert_eq!(
            p,
            Plan {
                mini: true,
                progress: true,
                done: false
            }
        );
        assert!(plan(&dl(DownloadStatus::Connecting), &s, true).progress);
    }

    #[test]
    fn a_finished_download_shows_the_complete_window() {
        let s = AppSettings::default();
        assert_eq!(
            plan(&dl(DownloadStatus::Completed), &s, true),
            Plan {
                mini: false,
                progress: false,
                done: true
            }
        );
        assert!(
            plan(&dl(DownloadStatus::Seeding), &s, true).done,
            "a finished torrent too"
        );
    }

    #[test]
    fn a_torrent_waiting_for_the_vpn_shows_its_window() {
        assert!(plan(&dl(DownloadStatus::Blocked), &AppSettings::default(), true).progress);
    }

    #[test]
    fn queued_carried_over_and_unconfirmed_downloads_open_no_window() {
        let s = AppSettings::default();
        let mut queued = dl(DownloadStatus::Completed);
        queued.queue_id = Some(3);
        assert!(!plan(&queued, &s, true).done);
        assert!(
            !plan(&dl(DownloadStatus::Downloading), &s, false).progress,
            "from last time"
        );
        let mut pending = dl(DownloadStatus::Downloading);
        pending.pending = true;
        assert_eq!(plan(&pending, &s, true), Plan::default());
    }

    #[test]
    fn each_window_can_be_turned_off() {
        let s = AppSettings {
            show_progress_window: false,
            show_complete_window: false,
            show_mini_window: false,
            ..Default::default()
        };
        assert_eq!(
            plan(&dl(DownloadStatus::Downloading), &s, true),
            Plan::default()
        );
        assert_eq!(
            plan(&dl(DownloadStatus::Completed), &s, true),
            Plan::default()
        );
    }
}
