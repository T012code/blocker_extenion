# Layó's Download Manager

A free download manager for Windows. It speeds up downloads by fetching each file in many sections at once, resumes broken downloads, catches downloads from your browser, and grabs videos from web pages.

> **Status:** in development. The download engine, the browser extension and the browser bridge are done and tested. The desktop app, video grabber, queues and installer come next.

## How the multi-section download works

Most servers send a single connection's data at a limited speed. Layó's Download Manager asks the server for different parts of the file at the same time, over several connections, and writes each part straight into its place in the file.

1. **Check the file.** It asks the server for the first byte. The reply gives the file size, the name, and whether the server lets clients ask for byte ranges.
2. **Cut it into sections.** The file is split into equal sections, **8 by default**, up to **64**. Each section gets its own connection. Sections are never smaller than 1 MB, so a 3 MB file only uses 3.
3. **Download all sections at once.** Each connection asks for its own byte range (`Range: bytes=…`) and writes into a shared part file (`name.ext.ldmpart`) at the right offset.
4. **Keep every connection busy.** When a section finishes, its connection takes over the second half of the section with the most left to fetch. That repeats until the end, so one slow connection can't hold up the whole file.
5. **Finish.** Once every byte is on disk, the part file is flushed and renamed to the real name. It never overwrites an existing file: you get `name (1).ext` instead.

```
8 sections at the start:
[■■■■■■■■|■■■■■■■■|■■■■■■■■|■■■■■■■■|■■■■■■■■|■■■■■■■■|■■■■■■■■|■■■■■■■■]
  conn 1    conn 2   conn 3   conn 4   conn 5   conn 6   conn 7   conn 8

conn 1 is done, conn 8 is slow: conn 1 takes over half of what's left of section 8
[████████|██████■■|█████■■■|██████■■|███████■|████■■■■|██████■■|██■■▌■■■]
                                                                     ^ conn 1 starts here
```

If the server doesn't support byte ranges, it downloads over one connection, the same way a browser does.

### Choosing the number of sections

You choose how many sections a file is split into: **1, 2, 4, 8, 16, 24, 32, 48 or 64**, or type any number from 1 to 64.

| Where | What it changes |
| --- | --- |
| **Settings → Connection → Default sections** | Every new download. Saved, and kept after restarting the app. Default: 8. |
| **Add download dialog → Sections** | Just this download. |
| **Right-click a download → Sections**, or the download's progress window | A download that's already in the list, **even while it's downloading**. Raising the number opens new connections at once, which take over halves of the biggest sections. Lowering it closes the extra connections after they save what they've fetched, and the remaining connections finish their sections. |

More sections help most on fast connections and servers that limit speed per connection. Some servers limit how many connections one computer may open and refuse extra ones, so 8 or 16 is a good default and 32–64 is for servers that allow it.

## What the engine does

- Multi-section downloads with 1–64 connections, chosen by you and changeable while downloading, with dynamic re-splitting (above)
- Pause and resume, also after closing the app or a crash. Progress is saved every 2 seconds to a local SQLite database.
- Automatic retry with back-off when a connection drops. Only the missing bytes are fetched again.
- Detects when a link has expired (HTTP 401/403/404/410) and lets you give the download a fresh link without losing progress
- Detects when the file on the server changed, and starts over instead of making a corrupt file
- Speed limits per download and for all downloads together, adjustable while they run
- Uses the browser's cookies, referrer and user agent, so downloads that need a login work
- Reads the file name from `Content-Disposition` (including UTF-8 names), then from the address, and makes it safe for Windows
- Sorts files into Video, Music, Documents, Compressed, Programs and General folders

## Taking over browser downloads

The browser extension (in [`extension/`](extension/)) watches downloads in Chrome, Edge, Opera, Brave, Vivaldi and Firefox. When you download a file of a type on its list (archives, programs, video, audio, documents), it cancels the browser's download and hands the link to the download manager, including the cookies, referrer and user agent, so downloads behind a login keep working. It leaves alone:

- files not on the list (you can edit it, or switch to "take over everything")
- links you click while holding **Alt**
- sites on your skip list
- form submissions and `blob:`/`data:` downloads, which no other program can repeat

If the app can't be reached, the browser gets the download back and nothing is lost. The extension also adds right-click menu items: **Download with Layó's DM**, **Download this video/audio**, and **Download all links**.

```
browser extension ──Native Messaging──▶ ldm-native-host ──named pipe──▶ download manager ──▶ engine
```

The bridge program starts the download manager if it isn't running. See [extension/README.md](extension/README.md) for details.

## Project layout

```
layos-download-manager/
  Cargo.toml             Rust workspace
  crates/engine/         Download engine (Rust library)
    src/transfer.rs      Multi-section download, re-splitting, live section count
    src/engine.rs        Add, start, pause, resume, remove; progress events
    src/http.rs          Requests and the first-byte server check
    src/store.rs         SQLite storage
    src/filename.rs      File names
    src/category.rs      Categories
    src/limiter.rs       Speed limiter
    tests/               Integration tests against a local test server
  crates/ipc/            Messages and the local pipe between the bridge and the app
  crates/service/        Turns browser requests into downloads (shared with the app);
                         ldm-headless runs it without a window, for development
  crates/native-host/    ldm-native-host: the browser bridge, and --register/--unregister
  crates/testkit/        Test web server used by the Rust tests
  extension/             Browser extension (Chrome-family and Firefox builds)
  app-icon.png           App icon (drawn by extension/tools/make-icons.py)
```

Still to come: `app/` (Tauri desktop app), the video grabber, queues and scheduler, and the Windows installer.

## Working on it

You need [Rust](https://rustup.rs/) 1.85 or newer, and Node.js 22 or newer for the extension.

```bash
cargo test --workspace
```

Runs the engine, bridge and pipe tests. The engine tests use a small web server that can go slow, drop connections, refuse byte ranges, hide the file size and refuse links. They check every download is byte-for-byte correct, including with 8, 16, 32 and 64 parallel sections. The bridge tests run the real `ldm-native-host` program the way a browser does.

```bash
cd extension && npm test && npm run test:e2e
```

Unit tests for the capture rules, then an end-to-end test in a real Chromium with the extension, the bridge and the download manager.

To try the extension against a running download manager without the desktop app:

```bash
cargo build -p ldm-native-host -p ldm-service
```

```bash
target/debug/ldm-native-host --register
```

```bash
target/debug/ldm-headless
```

Then load `extension/dist/chromium` as an unpacked extension (run `npm run build` in `extension/` first). Downloads are saved to a `layos-download-manager` folder in your temp folder.

## Licence

GNU GPL v3 or later. See [LICENSE](../LICENSE).
