# Clippy: project brief (paused, nothing built yet)

A cross-device clipboard for **Windows only**, developed by Layó. It works the way Blip does for files: sign in with the same email on several PCs, and anything copied on one PC can be pasted on another.

## What it must do
- Take over the Windows clipboard while signed in, and sync clips across every PC signed in to the same account.
- A small floating Clippy icon beside the focused text box, like Grammarly's. Clicking it opens a list of recent clips from all devices (short excerpts). Clicking a clip pastes it.
- Minimise to the tray, launch on startup, run silently in the background.
- A simple interface with light and dark mode (and a system option).
- Lightweight, power efficient and memory efficient.
- "Developed by Layó" credit, like the other apps.
- Design: no pill or capsule shapes anywhere (matches the brand rule "no capsules" in tokens.css).

## Planned approach (to confirm)
- Stack: Tauri 2 + Rust, the same as Layó's Download Manager. Reuse its brand folder (tokens.css, Archivo font, Layó mark).
- Event-driven, with no polling: clipboard changes through `AddClipboardFormatListener`, the focused text box through `SetWinEventHook` + UI Automation, and new clips through a push connection from the server.
- Folder: `clippy/` at the repository root, next to `adblocker/` and `layos-download-manager/` (that layout is on branch `claude/dazzling-bohr-1mx1tk`).
- CI: a Windows workflow like `.github/workflows/download-manager-windows.yml`.

## Open questions (ask before building)
1. Sync server: Supabase (suggested: free tier, email sign-in, instant push), a small self-hosted Rust relay, or Firebase?
2. Sign-in: email + password (the password also unlocks end-to-end encryption, suggested because people copy passwords) or email + 6-digit code like Blip?
3. Clip types: text only, text + images (suggested), or text + images + small files?
4. Floating icon: in any text box in any app (suggested, plus a Ctrl+Shift+V hotkey), or the hotkey only?
5. The name: "Clippy" is also Microsoft's old Office assistant, so check the name is safe to use before publishing.
