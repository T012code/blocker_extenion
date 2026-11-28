//! Everything that talks to Windows directly.

pub mod clipboard;
pub mod dpapi;
pub mod float;
pub mod paste;
pub mod watcher;

use windows::Win32::Foundation::{CloseHandle, HWND};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
};
use windows::core::PWSTR;

/// A UTF-16, NUL-terminated copy of a string, for Windows calls.
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Window handles cross threads as plain numbers.
pub fn hwnd_from(raw: isize) -> HWND {
    HWND(raw as *mut _)
}

/// The program file name of a process ("chrome.exe"), lowercase.
pub fn process_name(pid: u32) -> Option<String> {
    unsafe {
        let process = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(process);
        ok.ok()?;
        let path = String::from_utf16_lossy(&buf[..len as usize]);
        path.rsplit(['\\', '/']).next().map(|n| n.to_lowercase())
    }
}

/// This PC's name, for labelling its clips on the other PCs.
pub fn computer_name() -> String {
    use windows::Win32::System::SystemInformation::{ComputerNamePhysicalDnsHostname, GetComputerNameExW};
    unsafe {
        let mut len = 0u32;
        let _ = GetComputerNameExW(ComputerNamePhysicalDnsHostname, None, &mut len);
        if len > 0 {
            let mut buf = vec![0u16; len as usize];
            if GetComputerNameExW(ComputerNamePhysicalDnsHostname, Some(PWSTR(buf.as_mut_ptr())), &mut len).is_ok() {
                let name = String::from_utf16_lossy(&buf[..len as usize]);
                if !name.trim().is_empty() {
                    return name;
                }
            }
        }
    }
    std::env::var("COMPUTERNAME").unwrap_or_else(|_| "This PC".into())
}

/// Where to open the picker for the keyboard shortcut, and which window to paste into:
/// at the text cursor if the app shows one, else at the text box the floating icon is
/// on, else at the mouse pointer.
pub fn shortcut_anchor() -> (windows::Win32::Foundation::RECT, isize) {
    use windows::Win32::Foundation::{POINT, RECT};
    use windows::Win32::Graphics::Gdi::ClientToScreen;
    use windows::Win32::UI::WindowsAndMessaging::{
        GUITHREADINFO, GetCursorPos, GetForegroundWindow, GetGUIThreadInfo, GetWindowThreadProcessId,
    };
    unsafe {
        let front = GetForegroundWindow();
        let target = front.0 as isize;
        let thread = GetWindowThreadProcessId(front, None);
        let mut info = GUITHREADINFO { cbSize: size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if thread != 0 && GetGUIThreadInfo(thread, &mut info).is_ok() && !info.hwndCaret.is_invalid() {
            let c = info.rcCaret;
            if c.bottom > c.top {
                let mut a = POINT { x: c.left, y: c.top };
                let mut b = POINT { x: c.right.max(c.left + 1), y: c.bottom };
                if ClientToScreen(info.hwndCaret, &mut a).as_bool() && ClientToScreen(info.hwndCaret, &mut b).as_bool()
                {
                    return (RECT { left: a.x, top: a.y, right: b.x, bottom: b.y }, target);
                }
            }
        }
        if let Some(r) = float::last_box(target) {
            return (RECT { left: r.right - 1, top: r.top, right: r.right, bottom: r.bottom }, target);
        }
        let mut p = POINT::default();
        let _ = GetCursorPos(&mut p);
        (RECT { left: p.x, top: p.y, right: p.x + 1, bottom: p.y + 1 }, target)
    }
}
