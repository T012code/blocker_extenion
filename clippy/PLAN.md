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

## Decided
- **Sync server: Supabase.** Build and test on the Free plan. Move to Pro ($25/month) before launch, because free projects pause after a week of inactivity.
- **Text clips** are kept for **7 days**, then deleted.
- **Image clips** are kept for **24 hours**, then deleted.

## Database and sync plan
- One `clips` table: id, user_id, device_id, kind (text or image), encrypted payload, content hash, created_at, expires_at. Indexes on `(user_id, created_at desc)` and `expires_at`. Row-level security so each account sees only its own rows.
- Images go to Supabase Storage, not into the table. The table row only points to the file.
- An hourly scheduled job (pg_cron) deletes expired rows. Image files are deleted through the Storage API (an Edge Function on the same schedule), because deleting their database rows directly leaves the files behind.
- New clips reach other devices over a private Realtime **Broadcast** channel for each account, not Postgres Changes, which is slower and heavier.
- On reconnect, a device fetches only the clips newer than the last one it has, never the whole history.

## Optimisations to confirm
- Skip clips that password managers mark as private (Windows' clipboard "don't keep in history" flags).
- Don't upload the same clip twice (compare content hashes), and send only the last of several quick copies.
- Size caps: text up to 100 KB. Images resized to at most 1920 px, saved as WebP, up to 2 MB. Thumbnails are made on the PC, not by Supabase's paid image service.
- Cap each account at about 200 clips, as well as the 7-day and 24-hour limits.
- Disconnect from Realtime while the PC is locked or asleep (saves battery and connections), and catch up on unlock.

## Open questions (ask before building)
1. Sign-in: email + password (the password also unlocks end-to-end encryption, suggested because people copy passwords) or email + 6-digit code like Blip?
2. Floating icon: in any text box in any app (suggested, plus a Ctrl+Shift+V hotkey), or the hotkey only?
3. The name: "Clippy" is also Microsoft's old Office assistant, so check the name is safe to use before publishing.
