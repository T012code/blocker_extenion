# Smart Clip: project notes

Smart Clip (first called Clippy; renamed because that name is taken) is a cross-device clipboard for **Windows only**, developed by Layó. It works the way Blip does for files: sign in with the same email on several PCs, and anything copied on one can be pasted on another. See [README.md](README.md) for what it does and how to set it up.

## Decisions

- **Server:** Supabase, in **Europe West (London)**. Build and test on the Free plan; move to Pro ($25/month) before launch (free projects pause after a week without activity).
- **Sign-in:** email + emailed code, like Blip. No password. Branded code emails in `supabase/email/`.
- **Clips:** text kept 7 days, images 24 hours. At most 200 clips per account, at most 30 images.
- **Optimisations (all in):** password-manager copies skipped; no duplicate uploads; text up to 100 KB; images ≤ 1920 px, ≤ 2 MB, thumbnails made on the PC; per-account caps; the connection closes while locked or asleep; catch-up fetches only what changed; expired-clip clean-up hourly.
- **Floating icon** in any app's text box, plus a shortcut (Alt+Shift+V by default; Ctrl+Shift+V would break "paste as plain text" in browsers and Office, and Ctrl+Alt+V would break typing on keyboards where AltGr+V types a character).
- **Stack:** Tauri 2 + Rust, like Layó's Download Manager, with the same brand.
- **Privacy trade-off:** with email-code sign-in there's no password to derive a key from, so clips aren't end-to-end encrypted (they're encrypted in transit and at rest, and row-level security keeps accounts apart). End-to-end encryption could be added later with a "pair this PC" step.

## Status

- Built: sync engine, Windows app, interface, database migration, image clean-up function, emails, tests, CI.
- **Next:** create the Supabase project (the free plan's 2 active projects are in use: pause one, or upgrade), then follow README → "Setting up the server", fill in `config/supabase.json`, and build.
- Then: try it on two PCs; check the floating icon in the apps you use most (browsers, Word, Outlook, Teams, Slack, Notepad).

## Capacity (Supabase, estimated)

- **Free:** about 200 people online at once (200 live connections).
- **Pro, $25/month:** about 500 included; up to ~10,000 at once with pay-as-you-go (about $0.02 per extra user a month).
- Storage is never the limit thanks to the expiry rules and caps.
