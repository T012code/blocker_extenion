//! The clip picker: a small window that opens below the floating icon (or at the text
//! cursor for the shortcut), lists recent clips, and pastes the one picked.
//!
//! It's made the first time it's needed and kept hidden for quick reopening; after
//! three minutes unused it's closed to give its memory back.

use std::sync::atomic::{AtomicIsize, AtomicU64, Ordering};
use std::time::Duration;

use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, WebviewUrl, WebviewWindow, WebviewWindowBuilder,
    WindowEvent,
};
use windows::Win32::Foundation::RECT;

const LABEL: &str = "picker";
const WIDTH: f64 = 360.0;
const HEIGHT: f64 = 440.0;
const GAP: f64 = 6.0;
const KEEP_HIDDEN: Duration = Duration::from_secs(180);

/// The window to paste into.
static TARGET: AtomicIsize = AtomicIsize::new(0);
/// Changes on every open and hide; a pending close only goes ahead if nothing happened since.
static GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn target() -> isize {
    TARGET.load(Ordering::Acquire)
}

fn window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(w) = app.get_webview_window(LABEL) {
        return Some(w);
    }
    let w = WebviewWindowBuilder::new(app, LABEL, WebviewUrl::App("picker.html".into()))
        .title("Smart Clip")
        .inner_size(WIDTH, HEIGHT)
        .decorations(false)
        .resizable(false)
        .skip_taskbar(true)
        .always_on_top(true)
        .shadow(true)
        .visible(false)
        .focused(true)
        .build()
        .ok()?;
    let handle = app.clone();
    w.on_window_event(move |e| {
        if let WindowEvent::Focused(false) = e {
            hide(&handle);
        }
    });
    Some(w)
}

/// Opens the picker next to `anchor` (screen pixels), to paste into `target`.
pub fn open(app: &AppHandle, anchor: RECT, target: isize) {
    let Some(w) = window(app) else { return };
    TARGET.store(target, Ordering::Release);
    GENERATION.fetch_add(1, Ordering::AcqRel);

    let monitor = app
        .monitor_from_point(anchor.left as f64, anchor.bottom as f64)
        .ok()
        .flatten()
        .or_else(|| app.primary_monitor().ok().flatten());
    let (scale, area) = match &monitor {
        Some(m) => (m.scale_factor(), Some(*m.work_area())),
        None => (1.0, None),
    };
    let (width, height, gap) = (WIDTH * scale, HEIGHT * scale, GAP * scale);
    // Below the anchor, its right edge lined up with the anchor's; above it if there's no
    // room below; always inside the screen.
    let mut x = anchor.right as f64 - width;
    let mut y = anchor.bottom as f64 + gap;
    if let Some(a) = area {
        let (left, top) = (a.position.x as f64, a.position.y as f64);
        let (right, bottom) = (left + a.size.width as f64, top + a.size.height as f64);
        if y + height > bottom {
            y = anchor.top as f64 - gap - height;
        }
        x = x.clamp(left, (right - width).max(left));
        y = y.clamp(top, (bottom - height).max(top));
    }
    let _ = w.set_size(PhysicalSize::new(width.round() as u32, height.round() as u32));
    let _ = w.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    let _ = w.emit("picker-open", ());
    let _ = w.show();
    let _ = w.set_focus();
}

pub fn hide(app: &AppHandle) {
    let Some(w) = app.get_webview_window(LABEL) else { return };
    if !w.is_visible().unwrap_or(false) {
        return;
    }
    let _ = w.hide();
    let generation = GENERATION.fetch_add(1, Ordering::AcqRel) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(KEEP_HIDDEN).await;
        if GENERATION.load(Ordering::Acquire) == generation
            && let Some(w) = app.get_webview_window(LABEL)
            && !w.is_visible().unwrap_or(true)
        {
            let _ = w.destroy();
        }
    });
}

pub fn is_open(app: &AppHandle) -> bool {
    app.get_webview_window(LABEL).is_some_and(|w| w.is_visible().unwrap_or(false))
}
