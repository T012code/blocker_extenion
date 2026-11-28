# Smart Clip and your privacy

Smart Clip copies what you copy to your other PCs. This is what that involves.

## What's stored, and for how long

- **Your email address,** for signing in. There's no password.
- **Your clips:** the text you copy (up to 100 KB per clip), and images you copy (shrunk to at most 1920 px and 2 MB, with a small thumbnail). With each clip: the name of the PC it came from, its size, and when it was copied.
- **Text clips are deleted after 7 days, images after 24 hours.** An account keeps at most 200 clips (at most 30 of them images); older ones are deleted sooner when that's reached.
- **You can delete any clip,** or all of them at once (Settings → Delete all), and they're removed from every PC.
- **Deleting your account deletes all its clips.**

Clips are stored with [Supabase](https://supabase.com) in London, encrypted on disk, and sent only over encrypted connections. Each account can only ever read its own clips: the database enforces this, not just the app. Clips are not end-to-end encrypted: whoever runs the server could technically read them, as with most clipboard and notes services.

## What's never read or synced

- **Passwords from password managers,** and anything an app marks as private. Windows apps can flag a copy as "don't keep" (password managers and Windows itself do this); Smart Clip doesn't even read those.
- **Files.** Copying files in File Explorer isn't synced.
- **Anything while syncing is paused.**

The floating icon never appears in password boxes, and you can hide it in any app.

## On your PC

- Your sign-in is saved in `%LOCALAPPDATA%\dev.layo.smartclip`, protected with Windows' own encryption (DPAPI), so only your Windows account on that PC can use it.
- Your settings and this PC's name are kept in the same folder.
- Clips are held in memory while Smart Clip runs, not written to disk.

## What isn't collected

No analytics, no tracking, no ads. Smart Clip only talks to its own server.

## Questions

Contact Layó through [layothedeveloper.com](https://layothedeveloper.com).
