# Smart Clip

**Your clipboard on all your PCs.** Copy on one Windows PC, paste on another. Sign in with the same email on each PC (with a code sent by email, no password), and everything you copy shows up on the others in a moment.

Developed by [Layó](https://layothedeveloper.com). Windows 10 and 11.

## What it does

- **Syncs what you copy.** Text, links and images (screenshots and pictures) go to your other PCs as you copy them.
- **A floating icon in text boxes.** Like Grammarly's, a small Smart Clip icon sits in the corner of the text box you're typing in, in any app. Click it to see your recent clips from every PC, and click one to paste it. Right-click the icon to hide it in that app or turn it off.
- **A shortcut that works anywhere.** <kbd>Alt</kbd>+<kbd>Shift</kbd>+<kbd>V</kbd> opens the same list at your text cursor (you can pick another shortcut in Settings). Use <kbd>↑</kbd>/<kbd>↓</kbd> and <kbd>Enter</kbd>, or press <kbd>1</kbd>–<kbd>9</kbd>.
- **Clips expire.** Text is kept for 7 days and images for 24 hours, then deleted everywhere. Each account keeps at most 200 clips, of which at most 30 are images.
- **Private copies stay private.** Passwords copied from password managers (and anything else an app marks "don't keep") are never read, let alone synced. The floating icon never appears in password boxes.
- **Runs quietly.** It starts with Windows, lives in the tray, and closing the window keeps it running. Light and dark mode, or follow Windows.

### Light on power and memory

- **Nothing polls.** Windows tells Smart Clip when something is copied, when the focus moves to a text box, and when the PC locks or sleeps. In between, its threads sleep.
- **One connection.** While signed in, unlocked and not paused, there's one connection to the server with a heartbeat every 30 seconds. It closes while the PC is locked or asleep and catches up when you're back.
- **Only what changed.** Catching up asks for the list of clip ids first, then downloads only the clips this PC doesn't have. Full images download only when you paste them; lists show small thumbnails.
- **No hidden browser.** In the background Smart Clip is a small native process. The floating icon is a tiny native window. The main window and the picker are web views that only exist while open (the picker is kept for three minutes after use so it reopens instantly).
- **Small uploads.** Images are shrunk to at most 1920 px and 2 MB (lossless WebP for screenshots, JPEG for photos). Copying the same thing twice doesn't upload it twice, and quick bursts of copies send only the last one.

## How it's built

| Folder | What's in it |
| --- | --- |
| [`crates/core`](crates/core) | The sync engine in plain Rust (tested on any OS): sign-in with email codes, the clips database, image shrinking and clipboard formats, and the live channel (Supabase Realtime). |
| [`app/src-tauri`](app/src-tauri) | The Windows app (Tauri 2): clipboard watching, the floating icon (UI Automation and a layered window), pasting, the tray, the shortcut, saved sign-in (protected with Windows DPAPI). |
| [`app/ui`](app/ui) | The main window and the clip picker (plain HTML, CSS and JavaScript). |
| [`supabase`](supabase) | The server: the database migration, the image clean-up function, the sign-in emails, and database tests. |
| [`config/supabase.json`](config/supabase.json) | The server's address and public key, built into the app. |
| [`brand`](brand) | Layó's colours, font and marks. |

The server is [Supabase](https://supabase.com): email sign-in, a Postgres database with row-level security (each account can only ever see its own clips), Storage for images, and Realtime for instant updates.

## Setting up the server (once)

1. **Create the project.** In Supabase, create a project named `smart-clip` in **Europe West (London)** (`eu-west-2`).
2. **Create the database.** Run [`supabase/migrations/20261128000000_smart_clip.sql`](supabase/migrations/20261128000000_smart_clip.sql) in the SQL editor (or `supabase db push`). It creates the `clips` table, its security rules, the private image bucket, the live updates and the hourly clean-up of expired clips.
3. **Remove expired image files hourly.**
   1. Deploy the Edge Function in [`supabase/functions/purge-images`](supabase/functions/purge-images) with JWT verification **off** (it checks its own secret): `supabase functions deploy purge-images --no-verify-jwt`.
   2. Make a long random secret (at least 32 characters) and set it as the function's `PURGE_SECRET`: `supabase secrets set PURGE_SECRET=...`.
   3. Put the same secret and your project ref into [`supabase/schedule-images.sql`](supabase/schedule-images.sql), and run it in the SQL editor.
4. **Set up sign-in** (Authentication in the dashboard):
   - **Sign In / Providers → Email:** on, with **Confirm email** on. Set the **email OTP expiration** to `900` seconds (the emails say 15 minutes) and the **OTP length** to `6`.
   - **Emails → SMTP settings:** connect your own email sender (Resend, Postmark, Amazon SES, Brevo, etc.). Supabase's built-in sender only emails the project's team members and only a few emails an hour, so real users won't get their codes without it. Then raise **Rate limits → emails per hour** to suit.
   - **Emails → Templates:**
     - **Magic link** (existing users signing in): subject `Your Smart Clip code: {{ .Token }}`, body: [`supabase/email/sign-in-code.html`](supabase/email/sign-in-code.html).
     - **Confirm signup** (new users): subject `Welcome to Smart Clip: your code is {{ .Token }}`, body: [`supabase/email/welcome-code.html`](supabase/email/welcome-code.html).
5. **Connect the app.** Put the project URL and its **publishable** key (Project Settings → API Keys) in [`config/supabase.json`](config/supabase.json). Both are safe to ship inside the app: the security rules decide what each signed-in account can do. Never put the secret or service-role key there.

Check the dashboard's Security and Performance advisors after setting up.

## Building

On Windows, with [Rust](https://rustup.rs) and [Node.js](https://nodejs.org) installed:

```bash
cd smart-clip/app/src-tauri
npx --yes @tauri-apps/cli@2 build
```

The installer is written to `smart-clip/target/release/bundle/nsis/`. It installs for the current user, with no admin rights needed.

The [GitHub workflow](../.github/workflows/smart-clip-windows.yml) runs every test and builds the installer on each push. Pushing a tag like `smart-clip-v1.0.0` also publishes a release (the notes come from `docs/releases/<version>.md`).

## Testing

```bash
cd smart-clip
cargo test -p smart-clip-core            # the sync engine (any OS)
bash supabase/tests/run-tests.sh         # the database, on a throwaway local PostgreSQL
cd app && npm install --no-save playwright@1 && node test/ui.e2e.mjs   # the windows, in Chromium
```

The interface tests use made-up data. Opening `app/ui/index.html` or `app/ui/picker.html` in a browser through a local web server shows the same: add `?state=signin`, `empty`, `paused`, `offline` or `setup` to see the other screens.

## Icons

`python3 tools/make-icons.py` (needs Pillow) draws the app, tray and floating icons.

## Licence

GNU GPL v3 or later. The Archivo font is under the SIL Open Font License (`brand/fonts/OFL.txt`).
