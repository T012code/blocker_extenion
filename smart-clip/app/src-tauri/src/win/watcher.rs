//! A hidden window that Windows tells about clipboard changes, the PC being locked or
//! unlocked, and sleep and wake. Nothing polls: the thread sleeps until a message comes.

use std::sync::OnceLock;
use std::sync::atomic::{AtomicIsize, AtomicU32, Ordering};

use smart_clip_core::Captured;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, GetClipboardSequenceNumber};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::RemoteDesktop::{NOTIFY_FOR_THIS_SESSION, WTSRegisterSessionNotification};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, MSG, PBT_APMRESUMEAUTOMATIC,
    PBT_APMRESUMESUSPEND, PBT_APMSUSPEND, RegisterClassExW, SetTimer, TranslateMessage, WINDOW_EX_STYLE,
    WM_CLIPBOARDUPDATE, WM_POWERBROADCAST, WM_TIMER, WM_WTSSESSION_CHANGE, WNDCLASSEXW, WS_EX_TOOLWINDOW, WS_POPUP,
    WTS_SESSION_LOCK, WTS_SESSION_UNLOCK,
};
use windows::core::PCWSTR;

use super::wide;

pub enum Event {
    Copied(Captured),
    /// Locked or asleep (true), back (false).
    Away(bool),
}

type Callback = Box<dyn Fn(Event) + Send + Sync>;

static CALLBACK: OnceLock<Callback> = OnceLock::new();
static HWND_RAW: AtomicIsize = AtomicIsize::new(0);
static LAST_SEQ: AtomicU32 = AtomicU32::new(0);

/// Copies often come in bursts (an app setting the clipboard several times, or quick
/// repeated Ctrl+C): wait this long after the last change, then read once.
const SETTLE_MS: u32 = 300;
const TIMER_READ: usize = 1;

/// The hidden window, which also owns what Smart Clip puts on the clipboard.
pub fn hwnd() -> Option<HWND> {
    match HWND_RAW.load(Ordering::Acquire) {
        0 => None,
        raw => Some(super::hwnd_from(raw)),
    }
}

unsafe extern "system" fn proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let send = |e: Event| {
        if let Some(cb) = CALLBACK.get() {
            cb(e);
        }
    };
    match msg {
        WM_CLIPBOARDUPDATE => {
            unsafe { SetTimer(Some(hwnd), TIMER_READ, SETTLE_MS, None) };
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_READ => {
            let _ = unsafe { KillTimer(Some(hwnd), TIMER_READ) };
            let seq = unsafe { GetClipboardSequenceNumber() };
            if LAST_SEQ.swap(seq, Ordering::AcqRel) != seq
                && let Some(c) = super::clipboard::read()
            {
                send(Event::Copied(c));
            }
            LRESULT(0)
        }
        WM_WTSSESSION_CHANGE => {
            match wparam.0 as u32 {
                WTS_SESSION_LOCK => send(Event::Away(true)),
                WTS_SESSION_UNLOCK => send(Event::Away(false)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_POWERBROADCAST => {
            match wparam.0 as u32 {
                PBT_APMSUSPEND => send(Event::Away(true)),
                PBT_APMRESUMEAUTOMATIC | PBT_APMRESUMESUSPEND => send(Event::Away(false)),
                _ => {}
            }
            LRESULT(1)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Starts the watcher thread (once).
pub fn start(callback: impl Fn(Event) + Send + Sync + 'static) {
    if CALLBACK.set(Box::new(callback)).is_err() {
        return;
    }
    std::thread::Builder::new()
        .name("watcher".into())
        .spawn(|| unsafe {
            let instance = GetModuleHandleW(None).unwrap_or_default();
            let class = wide("SmartClipWatcher");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(proc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc);
            // A top-level window (never shown): message-only windows don't get the
            // sleep and wake broadcasts.
            let Ok(hwnd) = CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_TOOLWINDOW.0),
                PCWSTR(class.as_ptr()),
                PCWSTR(class.as_ptr()),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            ) else {
                return;
            };
            HWND_RAW.store(hwnd.0 as isize, Ordering::Release);
            LAST_SEQ.store(GetClipboardSequenceNumber(), Ordering::Release);
            let _ = AddClipboardFormatListener(hwnd);
            let _ = WTSRegisterSessionNotification(hwnd, NOTIFY_FOR_THIS_SESSION);
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        })
        .expect("watcher thread");
}
