//! Reading and writing the Windows clipboard.
//!
//! Copies that apps mark as private are never read: password managers (and Windows
//! itself) set "ExcludeClipboardContentFromMonitorProcessing", "Clipboard Viewer Ignore",
//! or "CanIncludeInClipboardHistory"/"CanUploadToCloudClipboard" to 0 for passwords.
//! What Smart Clip puts on the clipboard itself carries a marker so it isn't sent again.

use std::time::Duration;

use smart_clip_core::Captured;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL, HWND};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIB, CF_DIBV5, CF_UNICODETEXT};
use windows::core::PCWSTR;

use super::wide;

/// Marks what Smart Clip itself put on the clipboard.
const MARKER: &str = "SmartClip.Internal";
/// The biggest clipboard item read (bytes); anything larger is left alone.
const MAX_READ: usize = 64 * 1024 * 1024;
/// The biggest image read, in pixels (8K is 33 million).
const MAX_PIXELS: u64 = 40_000_000;

fn format(name: &str) -> u32 {
    let w = wide(name);
    unsafe { RegisterClipboardFormatW(PCWSTR(w.as_ptr())) }
}

fn available(fmt: u32) -> bool {
    fmt != 0 && unsafe { IsClipboardFormatAvailable(fmt).is_ok() }
}

/// Opens the clipboard, retrying while another app holds it.
struct Open;

impl Open {
    fn new(owner: Option<HWND>) -> Option<Open> {
        for attempt in 0..10 {
            if unsafe { OpenClipboard(owner) }.is_ok() {
                return Some(Open);
            }
            std::thread::sleep(Duration::from_millis(15 + attempt * 10));
        }
        None
    }
}

impl Drop for Open {
    fn drop(&mut self) {
        let _ = unsafe { CloseClipboard() };
    }
}

/// The bytes of one clipboard format (the clipboard must be open).
fn bytes(fmt: u32) -> Option<Vec<u8>> {
    unsafe {
        let handle = GetClipboardData(fmt).ok()?;
        let global = HGLOBAL(handle.0);
        let size = GlobalSize(global);
        if size == 0 || size > MAX_READ {
            return None;
        }
        let ptr = GlobalLock(global) as *const u8;
        if ptr.is_null() {
            return None;
        }
        let out = std::slice::from_raw_parts(ptr, size).to_vec();
        let _ = GlobalUnlock(global);
        Some(out)
    }
}

/// A DWORD format set to 0 means "don't keep this".
fn says_no(name: &str) -> bool {
    let fmt = format(name);
    available(fmt) && bytes(fmt).is_some_and(|b| b.len() >= 4 && u32::from_le_bytes([b[0], b[1], b[2], b[3]]) == 0)
}

/// Reads what was just copied, unless it's private, Smart Clip's own, or nothing it syncs.
pub fn read() -> Option<Captured> {
    let _open = Open::new(None)?;
    if available(format(MARKER))
        || available(format("ExcludeClipboardContentFromMonitorProcessing"))
        || available(format("Clipboard Viewer Ignore"))
        || says_no("CanIncludeInClipboardHistory")
        || says_no("CanUploadToCloudClipboard")
    {
        return None;
    }
    // Text wins when both are there (a spreadsheet's cells also come as a picture).
    if available(CF_UNICODETEXT.0 as u32) {
        let raw = bytes(CF_UNICODETEXT.0 as u32)?;
        let units: Vec<u16> =
            raw.as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&u| u != 0).collect();
        return Some(Captured::Text(String::from_utf16_lossy(&units)));
    }
    let image = if available(format("PNG")) {
        bytes(format("PNG")).and_then(|b| smart_clip_core::image::decode(&b))
    } else if available(CF_DIBV5.0 as u32) {
        bytes(CF_DIBV5.0 as u32).and_then(|b| smart_clip_core::image::from_dib(&b))
    } else if available(CF_DIB.0 as u32) {
        bytes(CF_DIB.0 as u32).and_then(|b| smart_clip_core::image::from_dib(&b))
    } else {
        None
    }?;
    let (width, height) = image.dimensions();
    if width as u64 * height as u64 > MAX_PIXELS {
        return None;
    }
    Some(Captured::Image { rgba: image.into_raw(), width, height })
}

/// Puts one format on the open clipboard.
fn put(fmt: u32, data: &[u8]) -> bool {
    unsafe {
        let Ok(global) = GlobalAlloc(GMEM_MOVEABLE, data.len().max(1)) else { return false };
        let ptr = GlobalLock(global) as *mut u8;
        if ptr.is_null() {
            let _ = GlobalFree(Some(global));
            return false;
        }
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        let _ = GlobalUnlock(global);
        if SetClipboardData(fmt, Some(HANDLE(global.0))).is_err() {
            // Still ours when Windows didn't take it.
            let _ = GlobalFree(Some(global));
            return false;
        }
        true
    }
}

fn write(owner: HWND, items: &[(u32, &[u8])]) -> bool {
    let Some(_open) = Open::new(Some(owner)) else { return false };
    if unsafe { EmptyClipboard() }.is_err() {
        return false;
    }
    let mut ok = true;
    for (fmt, data) in items {
        ok &= put(*fmt, data);
    }
    ok && put(format(MARKER), &[1])
}

/// Puts text on the clipboard. `owner` is one of the app's windows (Windows requires one).
pub fn write_text(owner: HWND, text: &str) -> bool {
    let units: Vec<u8> = text.encode_utf16().chain(std::iter::once(0)).flat_map(u16::to_le_bytes).collect();
    write(owner, &[(CF_UNICODETEXT.0 as u32, &units)])
}

/// Puts a picture on the clipboard: PNG (keeps transparency) and CF_DIB (every app).
pub fn write_image(owner: HWND, png: &[u8], dib: &[u8]) -> bool {
    write(owner, &[(format("PNG"), png), (CF_DIB.0 as u32, dib)])
}
