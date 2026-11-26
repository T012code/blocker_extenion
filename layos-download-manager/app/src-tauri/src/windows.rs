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
}

pub fn label(id: i64) -> String {
    format!("dl-{id}")
}

impl Windows {
    pub fn added(&self, id: i64) {
        self.fresh.lock().unwrap().insert(id);
    }

    pub fn set_mini_dismissed(&self, on: bool) {
        *self.mini_dismissed.lock().unwrap() = on;
    }

    /// A download changed: open its window when it starts or finishes, and the mini
    /// window while anything downloads, as the settings say.
    pub fn updated(&self, app: &AppHandle, d: &Download, s: &AppSettings) {
        let running = matches!(
            d.status,
            DownloadStatus::Downloading | DownloadStatus::Connecting
        );
        if running && s.show_mini_window && !*self.mini_dismissed.lock().unwrap() {
            let _ = show_mini(app);
        }
        // Downloads in a queue run unattended: no windows for them, as in IDM.
        if !self.fresh.lock().unwrap().contains(&d.id) || d.queue_id.is_some() {
            return;
        }
        if running
            && s.show_progress_window
            && self.shown.lock().unwrap().insert(d.id)
            && open_count(app) < MAX_AUTO_OPEN
        {
            let _ = open_download(app, d.id, false);
        }
        let done = matches!(
            d.status,
            DownloadStatus::Completed | DownloadStatus::Seeding
        );
        if done && s.show_complete_window && self.finished.lock().unwrap().insert(d.id) {
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
