//! Pasting into the app that was in use: bring its window back to the front, then
//! press Ctrl+V for the person.

use std::time::Duration;

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBD_EVENT_FLAGS, KEYBDINPUT, KEYEVENTF_KEYUP, SendInput,
    VIRTUAL_KEY, VK_CONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SHIFT, VK_V,
};
use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, IsIconic, IsWindow, SetForegroundWindow};

fn key(vk: VIRTUAL_KEY, up: bool) -> INPUT {
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: if up { KEYEVENTF_KEYUP } else { KEYBD_EVENT_FLAGS(0) },
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

/// Brings `target` back and pastes. Runs on its own thread: it waits briefly for the
/// window to come to the front.
pub fn paste_into(target_raw: isize) {
    std::thread::spawn(move || unsafe {
        let target: HWND = super::hwnd_from(target_raw);
        if target_raw == 0 || !IsWindow(Some(target)).as_bool() || IsIconic(target).as_bool() {
            return;
        }
        let _ = SetForegroundWindow(target);
        for _ in 0..25 {
            if GetForegroundWindow() == target {
                break;
            }
            std::thread::sleep(Duration::from_millis(12));
        }
        if GetForegroundWindow() != target {
            // The window didn't come back; the clip is on the clipboard for a manual paste.
            return;
        }
        // Let the app put the focus back in its text box.
        std::thread::sleep(Duration::from_millis(40));
        let mut inputs = Vec::new();
        // Release modifiers still held from the shortcut, so Ctrl+V isn't Ctrl+Alt+V.
        for vk in [VK_SHIFT, VK_LSHIFT, VK_RSHIFT, VK_MENU, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN] {
            if GetAsyncKeyState(vk.0 as i32) as u16 & 0x8000 != 0 {
                inputs.push(key(vk, true));
            }
        }
        inputs.extend([key(VK_CONTROL, false), key(VK_V, false), key(VK_V, true), key(VK_CONTROL, true)]);
        SendInput(&inputs, size_of::<INPUT>() as i32);
    });
}
