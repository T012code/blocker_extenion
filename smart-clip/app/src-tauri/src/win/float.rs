//! The floating Smart Clip icon that sits in the corner of the text box being typed in,
//! like Grammarly's. Clicking it opens the clip picker; right-clicking offers to hide it
//! in that app or turn it off.
//!
//! How it stays light: Windows tells this thread when the keyboard focus or the front
//! window changes (WinEvent hooks), and only then does it ask UI Automation what kind
//! of control has the focus and where it is. Window-move events are only watched for
//! the app being typed in. The icon itself is a tiny native layered window, not a web view.

use std::cell::RefCell;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};

use image::RgbaImage;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION, CreateCompatibleDC,
    CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW, MONITOR_DEFAULTTONEAREST,
    MONITORINFO, MonitorFromRect, ReleaseDC, SelectObject,
};
use windows::Win32::System::Com::{
    CLSCTX_INPROC_SERVER, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoUninitialize,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Threading::{GetCurrentProcessId, GetCurrentThreadId};
use windows::Win32::UI::Accessibility::{
    CUIAutomation8, HWINEVENTHOOK, IUIAutomation, IUIAutomation2, IUIAutomationElement, IUIAutomationValuePattern,
    SetWinEventHook, UIA_DocumentControlTypeId, UIA_EditControlTypeId, UIA_ValuePatternId, UnhookWinEvent,
};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, CreateWindowExW, DefWindowProcW, DestroyMenu, DestroyWindow, DispatchMessageW,
    EVENT_OBJECT_FOCUS, EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZESTART,
    EVENT_SYSTEM_MOVESIZEEND, EVENT_SYSTEM_MOVESIZESTART, GetForegroundWindow, GetMessageW, GetWindowThreadProcessId,
    HWND_TOPMOST, IDC_HAND, KillTimer, LoadCursorW, MA_NOACTIVATE, MF_SEPARATOR, MF_STRING, MSG, OBJID_CARET,
    OBJID_WINDOW, PostThreadMessageW, RegisterClassExW, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SetCursor, SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenu, TranslateMessage, ULW_ALPHA, UpdateLayeredWindow, WINDOW_EX_STYLE,
    WINEVENT_OUTOFCONTEXT, WINEVENT_SKIPOWNPROCESS, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_QUIT,
    WM_RBUTTONUP, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_POPUP,
};
use windows::core::{Interface, PCWSTR};

use super::wide;

/// What happens on the icon, passed to the app.
pub enum FloatEvent {
    /// Open the picker below this screen rectangle (the icon), to paste into `target`.
    Open { anchor: RECT, target: isize },
    /// Don't show the icon in this app (its program file name) any more.
    HideIn(String),
    /// Turn the floating icon off.
    TurnOff,
}

type Callback = Box<dyn Fn(FloatEvent) + Send + Sync>;

static CALLBACK: OnceLock<Callback> = OnceLock::new();
/// The tracker thread's id while it runs (0 when it doesn't).
static THREAD_ID: AtomicU32 = AtomicU32::new(0);
/// Program file names the icon stays out of.
static HIDDEN_APPS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// The last text box the icon sat on: (front window, its rectangle). The shortcut uses it
/// to open the picker right there.
static LAST_BOX: Mutex<Option<(isize, RECT)>> = Mutex::new(None);

const ICON_PNG: &[u8] = include_bytes!("../../icons/float.png");
/// The icon's size at 100% scaling, and its gap from the text box's edge.
const ICON_SIZE: f64 = 22.0;
const MARGIN: f64 = 5.0;
const IDLE_ALPHA: u8 = 215;
const TIMER_CHECK: usize = 1;
/// Sent after TrackMouseEvent when the pointer leaves the icon (from Win32 Controls).
const WM_MOUSELEAVE: u32 = 0x02A3;
/// Apps whose text boxes are mostly not for writing (Start, search, File Explorer's
/// rename box) or are password managers.
pub const DEFAULT_HIDDEN: &[&str] = &[
    "explorer.exe",
    "searchhost.exe",
    "searchapp.exe",
    "startmenuexperiencehost.exe",
    "shellexperiencehost.exe",
    "lockapp.exe",
    "textinputhost.exe",
    "keepass.exe",
    "keepassxc.exe",
    "1password.exe",
    "bitwarden.exe",
    "smartclip.exe",
];
/// Document-type controls that are real writing areas in these apps.
const DOCUMENT_APPS: &[&str] = &["winword.exe", "onenote.exe", "notepad.exe", "wordpad.exe", "outlook.exe", "olk.exe"];

struct Tracker {
    uia: IUIAutomation,
    overlay: HWND,
    source: RgbaImage,
    /// The drawn icon, premultiplied BGRA, at `size` px.
    pixels: Vec<u8>,
    size: i32,
    pos: POINT,
    visible: bool,
    hovered: bool,
    /// The front window the icon belongs to.
    target: HWND,
    target_name: String,
    /// Window-move events of the app being typed in: (hook, process id).
    moves: Option<(HWINEVENTHOOK, u32)>,
    hooks: Vec<HWINEVENTHOOK>,
}

thread_local! {
    static TRACKER: RefCell<Option<Tracker>> = const { RefCell::new(None) };
}

pub fn set_callback(callback: impl Fn(FloatEvent) + Send + Sync + 'static) {
    let _ = CALLBACK.set(Box::new(callback));
}

pub fn set_hidden_apps(apps: &[String]) {
    *HIDDEN_APPS.lock().unwrap_or_else(|e| e.into_inner()) = apps.iter().map(|a| a.to_lowercase()).collect();
}

/// The text box the icon last sat on, if it's in `front` (the front window).
pub fn last_box(front: isize) -> Option<RECT> {
    LAST_BOX.lock().unwrap_or_else(|e| e.into_inner()).filter(|(w, _)| *w == front).map(|(_, r)| r)
}

pub fn is_running() -> bool {
    THREAD_ID.load(Ordering::Acquire) != 0
}

fn emit(e: FloatEvent) {
    if let Some(cb) = CALLBACK.get() {
        cb(e);
    }
}

/// Runs `f` on the tracker. Re-entrant calls (Windows can deliver a message while the
/// tracker is in use) get None instead of a panic.
fn with<R>(f: impl FnOnce(&mut Tracker) -> R) -> Option<R> {
    TRACKER.with(|t| t.try_borrow_mut().ok()?.as_mut().map(f))
}

/// Checks again after things settle (focus changes come in bursts).
fn check_soon(ms: u32) {
    with(|t| unsafe {
        SetTimer(Some(t.overlay), TIMER_CHECK, ms, None);
    });
}

fn hide() {
    with(|t| {
        if t.visible {
            unsafe {
                let _ = ShowWindow(t.overlay, SW_HIDE);
            }
            t.visible = false;
        }
    });
}

unsafe extern "system" fn on_event(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    object: i32,
    _child: i32,
    _thread: u32,
    _time: u32,
) {
    match event {
        EVENT_SYSTEM_FOREGROUND | EVENT_SYSTEM_MINIMIZESTART | EVENT_SYSTEM_MOVESIZESTART => {
            hide();
            check_soon(150);
        }
        EVENT_OBJECT_FOCUS | EVENT_SYSTEM_MOVESIZEEND => check_soon(120),
        EVENT_OBJECT_LOCATIONCHANGE => {
            // The front window moved or resized, or the caret moved (typing, scrolling).
            let relevant =
                with(|t| (object == OBJID_WINDOW.0 && hwnd == t.target) || object == OBJID_CARET.0).unwrap_or(false);
            if relevant {
                check_soon(200);
            }
        }
        _ => {}
    }
}

fn hook(min: u32, max: u32, pid: u32) -> Option<HWINEVENTHOOK> {
    let h = unsafe {
        SetWinEventHook(min, max, None, Some(on_event), pid, 0, WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS)
    };
    (!h.is_invalid()).then_some(h)
}

fn monitor_scale_and_bounds(r: &RECT) -> (f64, RECT) {
    unsafe {
        let mon = MonitorFromRect(r, MONITOR_DEFAULTTONEAREST);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut info);
        (dx.max(96) as f64 / 96.0, info.rcMonitor)
    }
}

/// Is the focused control something to paste text into?
fn writable(el: &IUIAutomationElement, app: &str) -> bool {
    unsafe {
        if el.CurrentIsPassword().map(|b| b.as_bool()).unwrap_or(true) {
            return false;
        }
        if !el.CurrentIsEnabled().map(|b| b.as_bool()).unwrap_or(false) {
            return false;
        }
        let Ok(kind) = el.CurrentControlType() else { return false };
        let read_only = el
            .GetCurrentPatternAs::<IUIAutomationValuePattern>(UIA_ValuePatternId)
            .ok()
            .map(|p| p.CurrentIsReadOnly().map(|b| b.as_bool()).unwrap_or(true));
        if kind == UIA_EditControlTypeId {
            read_only != Some(true)
        } else if kind == UIA_DocumentControlTypeId {
            // Web pages are documents too: only writable ones, or real editors.
            read_only == Some(false) || DOCUMENT_APPS.contains(&app)
        } else {
            false
        }
    }
}

/// Finds the focused text box and puts the icon in its corner, or hides the icon.
fn check() {
    let found = (|| unsafe {
        let front = GetForegroundWindow();
        if front.is_invalid() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(front, Some(&mut pid));
        if pid == 0 || pid == GetCurrentProcessId() {
            return None;
        }
        let app = super::process_name(pid).unwrap_or_default();
        if HIDDEN_APPS.lock().unwrap_or_else(|e| e.into_inner()).contains(&app) {
            return None;
        }
        let el = with(|t| t.uia.GetFocusedElement().ok()).flatten()?;
        if el.CurrentProcessId().ok()? as u32 == GetCurrentProcessId() || !writable(&el, &app) {
            return None;
        }
        let rect = el.CurrentBoundingRectangle().ok()?;
        Some((front, pid, app, rect))
    })();

    let Some((front, pid, app, rect)) = found else {
        hide();
        return;
    };
    let (scale, screen) = monitor_scale_and_bounds(&rect);
    let w = rect.right - rect.left;
    let h = rect.bottom - rect.top;
    // Too small to be a text box worth decorating, or off the screen.
    if w < (60.0 * scale) as i32 || h < (14.0 * scale) as i32 || rect.right <= screen.left || rect.left >= screen.right
    {
        hide();
        return;
    }
    let size = (ICON_SIZE * scale).round() as i32;
    let margin = (MARGIN * scale).round() as i32;
    // One-line boxes: at the right end, centred. Bigger boxes: the bottom-right corner.
    let x = rect.right.min(screen.right) - size - margin;
    let y = if h < (44.0 * scale) as i32 {
        rect.top + (h - size) / 2
    } else {
        rect.bottom.min(screen.bottom) - size - margin
    };
    *LAST_BOX.lock().unwrap_or_else(|e| e.into_inner()) = Some((front.0 as isize, rect));
    with(|t| {
        t.target = front;
        t.target_name = app;
        // Follow this app's window moves (and nothing else's).
        if t.moves.map(|(_, p)| p) != Some(pid) {
            if let Some((old, _)) = t.moves.take() {
                unsafe {
                    let _ = UnhookWinEvent(old);
                }
            }
            t.moves = hook(EVENT_OBJECT_LOCATIONCHANGE, EVENT_OBJECT_LOCATIONCHANGE, pid).map(|h| (h, pid));
        }
        if t.size != size {
            t.size = size;
            t.pixels = render(&t.source, size);
        }
        t.pos = POINT { x, y };
        draw(t);
        if !t.visible {
            unsafe {
                let _ = ShowWindow(t.overlay, SW_SHOWNOACTIVATE);
                let _ =
                    SetWindowPos(t.overlay, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
            }
            t.visible = true;
        }
    });
}

/// The icon at `size` px, as premultiplied BGRA rows (what layered windows draw).
fn render(source: &RgbaImage, size: i32) -> Vec<u8> {
    let small = image::imageops::resize(source, size as u32, size as u32, image::imageops::FilterType::Lanczos3);
    let mut out = Vec::with_capacity((size * size * 4) as usize);
    for p in small.pixels() {
        let a = p[3] as u32;
        let pm = |c: u8| ((c as u32 * a + 127) / 255) as u8;
        out.extend_from_slice(&[pm(p[2]), pm(p[1]), pm(p[0]), p[3]]);
    }
    out
}

fn draw(t: &Tracker) {
    unsafe {
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: t.size,
                biHeight: -t.size, // top-down rows
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits = std::ptr::null_mut();
        if let Ok(bitmap) = CreateDIBSection(Some(mem), &info, DIB_RGB_COLORS, &mut bits, None, 0) {
            if !bits.is_null() {
                std::ptr::copy_nonoverlapping(t.pixels.as_ptr(), bits as *mut u8, t.pixels.len());
            }
            let old = SelectObject(mem, bitmap.into());
            let size = SIZE { cx: t.size, cy: t.size };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: if t.hovered { 255 } else { IDLE_ALPHA },
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            let _ = UpdateLayeredWindow(
                t.overlay,
                Some(screen),
                Some(&t.pos),
                Some(&size),
                Some(mem),
                Some(&POINT { x: 0, y: 0 }),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            );
            SelectObject(mem, old);
            let _ = DeleteObject(bitmap.into());
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
    }
}

fn right_click_menu() {
    let Some((overlay, target, name)) = with(|t| (t.overlay, t.target, t.target_name.clone())) else { return };
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let label = if name.is_empty() { "Hide in this app".to_owned() } else { format!("Hide in {name}") };
        let hide_in = wide(&label);
        let off = wide("Turn off the floating icon");
        let _ = AppendMenuW(menu, MF_STRING, 1, PCWSTR(hide_in.as_ptr()));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, 2, PCWSTR(off.as_ptr()));
        let mut at = POINT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut at);
        // A menu needs its window in front to close properly when clicking elsewhere.
        let _ = SetForegroundWindow(overlay);
        let choice =
            TrackPopupMenu(menu, TPM_RETURNCMD | TPM_NONOTIFY | TPM_RIGHTBUTTON, at.x, at.y, Some(0), overlay, None);
        let _ = DestroyMenu(menu);
        match choice.0 {
            1 if !name.is_empty() => {
                hide();
                emit(FloatEvent::HideIn(name));
            }
            2 => {
                hide();
                emit(FloatEvent::TurnOff);
            }
            _ => {}
        }
        let _ = SetForegroundWindow(target);
    }
}

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match msg {
        // Clicking the icon must not take the focus from the text box.
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        WM_SETCURSOR => {
            unsafe {
                if let Ok(hand) = LoadCursorW(None, IDC_HAND) {
                    SetCursor(Some(hand));
                }
            }
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let start = with(|t| {
                let start = !t.hovered;
                if start {
                    t.hovered = true;
                    draw(t);
                }
                start
            });
            if start == Some(true) {
                let mut tme = TRACKMOUSEEVENT {
                    cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
                    dwFlags: TME_LEAVE,
                    hwndTrack: hwnd,
                    dwHoverTime: 0,
                };
                unsafe {
                    let _ = TrackMouseEvent(&mut tme);
                }
            }
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|t| {
                t.hovered = false;
                draw(t);
            });
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            if let Some((anchor, target)) = with(|t| {
                let r = RECT { left: t.pos.x, top: t.pos.y, right: t.pos.x + t.size, bottom: t.pos.y + t.size };
                (r, t.target.0 as isize)
            }) {
                hide();
                emit(FloatEvent::Open { anchor, target });
            }
            LRESULT(0)
        }
        WM_RBUTTONUP => {
            right_click_menu();
            LRESULT(0)
        }
        WM_TIMER if wparam.0 == TIMER_CHECK => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_CHECK);
            }
            check();
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// Starts the icon (it shows up the next time a text box gets the focus).
pub fn start() {
    if is_running() {
        return;
    }
    let _ = std::thread::Builder::new().name("float".into()).spawn(|| unsafe {
        if CoInitializeEx(None, COINIT_MULTITHREADED).is_err() {
            return;
        }
        let setup = (|| -> Option<Tracker> {
            let uia: IUIAutomation = CoCreateInstance(&CUIAutomation8, None, CLSCTX_INPROC_SERVER).ok()?;
            // Never wait long on an app that's busy.
            if let Ok(uia2) = uia.cast::<IUIAutomation2>() {
                let _ = uia2.SetConnectionTimeout(1000);
                let _ = uia2.SetTransactionTimeout(500);
            }
            let instance = GetModuleHandleW(None).ok()?;
            let class = wide("SmartClipFloat");
            let wc = WNDCLASSEXW {
                cbSize: size_of::<WNDCLASSEXW>() as u32,
                lpfnWndProc: Some(overlay_proc),
                hInstance: instance.into(),
                lpszClassName: PCWSTR(class.as_ptr()),
                ..Default::default()
            };
            RegisterClassExW(&wc); // 0 when already registered by an earlier start: fine
            let title = wide("Smart Clip");
            let overlay = CreateWindowExW(
                WINDOW_EX_STYLE(WS_EX_LAYERED.0 | WS_EX_TOPMOST.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0),
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(instance.into()),
                None,
            )
            .ok()?;
            let source = smart_clip_core::image::decode(ICON_PNG)?;
            Some(Tracker {
                uia,
                overlay,
                source,
                pixels: Vec::new(),
                size: 0,
                pos: POINT::default(),
                visible: false,
                hovered: false,
                target: HWND::default(),
                target_name: String::new(),
                moves: None,
                hooks: Vec::new(),
            })
        })();
        let Some(mut tracker) = setup else {
            CoUninitialize();
            return;
        };
        for (min, max) in [
            (EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_FOREGROUND),
            (EVENT_SYSTEM_MOVESIZESTART, EVENT_SYSTEM_MOVESIZEEND),
            (EVENT_SYSTEM_MINIMIZESTART, EVENT_SYSTEM_MINIMIZESTART),
            (EVENT_OBJECT_FOCUS, EVENT_OBJECT_FOCUS),
        ] {
            tracker.hooks.extend(hook(min, max, 0));
        }
        let overlay = tracker.overlay;
        TRACKER.with(|t| *t.borrow_mut() = Some(tracker));
        THREAD_ID.store(GetCurrentThreadId(), Ordering::Release);
        // Place the icon for whatever has the focus right now.
        SetTimer(Some(overlay), TIMER_CHECK, 50, None);

        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).0 > 0 {
            if msg.message == WM_QUIT {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }

        if let Some(t) = TRACKER.with(|t| t.borrow_mut().take()) {
            for h in t.hooks.into_iter().chain(t.moves.map(|(h, _)| h)) {
                let _ = UnhookWinEvent(h);
            }
            let _ = DestroyWindow(t.overlay);
            drop(t.uia);
        }
        *LAST_BOX.lock().unwrap_or_else(|e| e.into_inner()) = None;
        CoUninitialize();
    });
}

/// Stops the icon and everything it listens to.
pub fn stop() {
    let id = THREAD_ID.swap(0, Ordering::AcqRel);
    if id != 0 {
        unsafe {
            let _ = PostThreadMessageW(id, WM_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}
