# Layó's Download Manager

*Developed by [Layó](https://layothedeveloper.com)*

A free download manager for Windows. It speeds up downloads by fetching each file in many sections at once, resumes broken downloads, catches downloads from your browser, and grabs videos from web pages.

> **Status:** in development. Done and tested: the download engine, the browser extension and bridge, the video download button, torrents with privacy protection, the desktop app with queues, and the Windows installer set-up. Next: code signing and the store release of the extension.

## Installing on Windows

The installer (`Layó's Download Manager_x.y.z_x64-setup.exe`, built by GitHub Actions) installs for the current user, with no admin rights needed. It includes the app, the browser bridge, yt-dlp and ffmpeg, and registers the bridge with Chrome, Edge, Firefox, Opera, Brave and Vivaldi. Uninstalling removes all of that, the magnet and `.torrent` associations, and the start-with-Windows entry. Then add the browser extension from `extension/dist`.

Windows may show "Windows protected your PC" the first time, because the installer isn't code-signed yet: click **More info → Run anyway**. On Windows 10 and 11, if another torrent app is already chosen for magnet links, Windows asks which app to use the first time you open one.

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

## Pause and continue

Any download can be paused and continued. Most servers allow fetching part of a file (byte ranges), so a paused download continues exactly where it stopped, even after closing the app or restarting the computer: the parts already on disk are kept in `name.ext.ldmpart` and their positions in the download database. Before downloading, the app asks the server whether it supports this. If it doesn't, the download is marked **Can't resume**, and continuing it starts the file again from the beginning.

Until the desktop app is ready, the browser extension's toolbar pop-up lists your downloads with live progress, speed and time left, and has **Pause** and **Continue** buttons.


### When the internet goes away

If the network drops while files are downloading, every running download **pauses and keeps what it has**, with the reason in its status ("Paused: the internet connection was lost"). The app notices in two ways: Windows reports no usable network adapter (checked every 2 seconds), or a download's connections keep failing to connect (three times in a row). Nothing restarts by itself when the network is back: a banner says you're back online, and you continue the downloads (one button continues all of those paused this way). Torrents and video downloads pause the same way.

The one exception is a file from a server that can't continue a download (it doesn't support byte ranges). That download can't keep what it has, so it **stops** instead and says so; continuing it starts it again.

### Folders for each type of file

Like IDM, downloads are sorted into folders inside the download folder: **Video, Music, Documents, Compressed, Programs, Torrents and General**. The folders are created when the app starts (and when you change the settings) if they aren't there yet; folders that already exist are used as they are, files included. In Settings → Downloads each type can have a folder of its own anywhere (× goes back to the default), and any single download can still be saved anywhere from the Add window. Turning sorting off saves everything straight into the download folder.

### Downloads start straight away

When the browser hands over a file, it's added and **starts at once** with its own download window, and the main window stays in the tray. If you turned on "Ask before downloading what the browser sends", the Add window shows first, but, as in IDM, the file is **already downloading in the background** while you choose the name, folder and sections. Its part file grows next to where the file would go; when you press Start it carries on, and when it finishes it is saved under the name and in the folder you chose (moved there if needed, even to another drive). Cancel, and the background download and its part file are deleted. The same happens in the app's own Add window once a link has been checked. Downloads started in the background don't show in the list (or the extension's pop-up) until you confirm them, and if the app is closed first they're cleaned up at the next start.

### A link downloaded before

When a link comes in that is already in the list (the same address, or the same torrent), you're asked, as in IDM, in that download's own window: **Open it** (the finished file) or **Continue it** (if it isn't finished), or **Download again**, which saves a second copy with a numbered name ("report (1).pdf"). "Do this every time" remembers the answer; Settings → Downloads changes it (ask, download again, or open/continue the one you have). The Add window says so too, before anything starts.

### From a magnet link to a finished torrent

1. You click a magnet link on a web page. The extension catches the click before the browser does anything and sends the link to the app (starting the app if it isn't running).
2. The app checks whether that torrent is already in your list (by its info hash). If it is, you're asked what to do, as above.
3. Otherwise it's added and starts at once, into the **Torrents** folder. Its download window opens and shows its progress.
4. The torrent first fetches its metadata (the list of files) from peers, then downloads its pieces from many peers at once, the torrent's own way of fetching in parts. Pieces are checked as they arrive and kept on disk, so pausing and continuing loses nothing.
5. With the default privacy settings, a torrent only connects to peers through your VPN or proxy. Until one is set up, its window and the list say "Waiting for VPN" instead of downloading.
6. When it's done it's shared with others until the share ratio in Settings is reached, then stops. Its window shows "Download complete" with Open and Show in folder.

### When a link expires

Many sites give out download links that only work for a while. When the server refuses a link part-way (401, 403, 404 or 410), the download is marked **Link expired** and keeps everything it has. **Refresh link** (in the details, the right-click menu, the download's window, the extension pop-up, or simply Continue) then:

1. tries the same link again, since some servers refuse only for a while; if it works, the download just carries on;
2. otherwise opens the page the file came from (or the link itself) in your browser, and waits up to 15 minutes. Download the file there again: the extension sends it to the app, which recognises it by name or size and gives the **fresh link to the old download**, continuing from where it stopped instead of starting a second one. Any expired download is also matched automatically when the same file (same name and size) comes in from the browser;
3. or you paste a new link yourself.

If the file behind the new link turns out to be a different size, the download starts over rather than mixing two files.
## The app

The main window lists your downloads with their progress, speed and time left. Categories on the left (Video, Music, Documents, Compressed, Programs, Torrents) and a search box narrow the list. Select a download to see its **live section map**: one strip per section, filled as it downloads, with a lime marker where each connection is working. You can change its **number of sections while it runs**.

- **The list:** a **Type** column (icon and extension), then name, size, progress, speed, time left and status. Add more from the **Columns** button or by right-clicking the header: Downloaded, Added, Finished, Sections, Queue, Folder, Address. Click a header to sort (again to reverse, a third time for newest first), drag a header to move its column (or Alt+←/→), and drag a header's edge to make the column wider or narrower, down to a sliver (double-click it to reset). Wider than the window, the list scrolls sideways; the header stays at the top. Rows are updated in place, so hundreds of downloads scroll smoothly.
- **Status icons:** green when done or sharing, amber when it's waiting on you (paused, link expired, waiting for a VPN), red when it failed; everything else is neutral. Every status has an icon and a word, never colour alone.
- **Side bar and details:** fold the side bar to an icon rail (Ctrl+B, or **Hide side bar** at its foot) and the details down to one line (Ctrl+J, its Details heading, or the button at the top right). Every icon-only button names itself (and its shortcut) in a tooltip. Drag the line between them and the list to resize either one, or use the arrow keys on it. Sizes, columns and sorting are remembered.
- **Exit and close:** closing the window keeps the app running in the tray; **Exit** (at the foot of the side bar, Ctrl+Q, or the tray menu) closes it, after asking if downloads are running. They're paused first, keeping everything fetched.
- **Download windows:** each download you start (including ones the browser hands over, which start at once without opening the main window) gets a small window of its own with its progress, speed, time left, section map and Pause/Continue. **Less** shrinks it to one line; **Hide** closes it while the download carries on. When the download finishes it turns into a "Download complete" window with **Open** and **Show in folder** (and a box to stop showing it). At most three open by themselves; queued downloads don't open any. Open one any time from the right-click menu or the details.
- **Mini window:** while anything downloads, a small window above the others (bottom right) sums it up: how many, overall percentage, speed and time left. Expand it to see each download and click one to open its window. It hides itself a few seconds after the last download finishes. Settings → General turns each of these windows off.
- **Add** (or Ctrl+N, or paste a link anywhere): checks the address first, showing name, size and whether it can be paused and continued. A video page lists its formats; magnet links and `.torrent` files become torrents. You choose the name, folder and sections.
- **Continue, Pause, Remove** (Ctrl+R, Ctrl+P, Delete) work on one or many selected downloads; double-click opens a finished file.
- **From the browser:** downloads the extension sends can be confirmed first, and "Download all links" lets you tick which files to get.
- **Settings:** download folder and category folders, default sections, overall speed limit, browser behaviour, the tray, notifications, and **Torrents & privacy** (mode, proxy with a Test button, VPN adapter, sharing ratio, blocklist).
- **Queues:** create queues in the sidebar, each running a set number of downloads at once, oldest first. **Start queue** and **Stop queue** run or pause it; downloads can be added straight into a queue or moved between queues; **Delete queue** keeps its downloads in the list or removes them (and their files, if you want).
- **Scheduler:** each queue can start and stop by itself at set times on chosen days, and then do nothing, close the app, put the computer to sleep, or shut it down (with a minute to cancel, from a banner in the window). The schedule dialog says what will happen in plain words, e.g. "Starts at 22:30 on weekdays, stops at 06:00."
- **Site grabber** (*All downloads → Grab files from a website*): reads a site's pages to a depth you choose, stays on that site, honours its robots.txt, and lists the files of the types you want, to tick before downloading.
- **Right-click menu and keyboard:** open, show in folder, show the download window, continue, pause, move to a queue, copy the address, remove. In the list, ↑/↓ move, Shift extends the selection, Enter opens, Space pauses or continues.
- **Removing in groups:** **Remove** clears the selection, every finished download, every failed or expired one, or everything in the current view, optionally deleting the files too.
- **Starts with Windows** (in the tray, without opening the window) and can **minimize to the tray**; both are settings. Closing the window keeps it running in the tray, where you can pause or continue everything.
- **Magnet links and .torrent files** anywhere in Windows open in the app (a setting; turning it off removes the association).
- Light and dark follow Windows. There are no pill-shaped controls: buttons, labels and toggles are rectangles, and a test checks every element on screen for it. Icons are from [Lucide](https://lucide.dev); motion is short and only uses transform and opacity (rows slide in, status icons pop when they change, bars carry a moving sheen while downloading), and it stops when Windows asks for reduced motion. The interface follows a written design audit ([docs/design-audit.md](docs/design-audit.md)) with [before and after screenshots](docs/screenshots/README.md), including [the icons, columns and download windows](docs/screenshots/README.md#icons-columns-and-download-windows). *Developed by Layó* in the corner links to [layothedeveloper.com](https://layothedeveloper.com).

## Download button on videos

Point at a video on any web page and a **Download** button appears in its corner. Click it for a menu of what can be saved:

- **On this page:** the video file the page plays, and the qualities of streaming players (HLS), e.g. *720p stream*, *360p stream*, read straight from the stream's playlist.
- **From the site:** for sites like YouTube, whose players stream in small pieces, the download manager asks [yt-dlp](https://github.com/yt-dlp/yt-dlp) for the formats: one entry per picture size (1080p60 MP4, 720p MP4…) plus *Audio only*, with sizes.

It works the same on social sites and feeds (Facebook, X, Instagram, TikTok, Reddit…), in embedded players (iframes), and in players built with shadow DOM. The button finds the video even under the overlays these sites put on top, and in a feed it looks up the video's own post (its permalink) rather than the feed page, so the formats are for the right video. Sites that need you signed in get your browser's cookies for the lookup.

Pick one and it downloads in the app, named after the page's title. Streams are fetched several pieces at a time, and separate picture and sound tracks are joined with ffmpeg. Video downloads pause and continue like any other. Protected (DRM) videos, such as on paid streaming services, show a message instead: they can't be downloaded.

yt-dlp and ffmpeg ship with the Windows installer. For development, put them on your PATH, or set `LDM_YTDLP` and `LDM_FFMPEG`.

## Torrents

Magnet links clicked in the browser, `magnet:` links added by hand, and `.torrent` files (including ones you download, which open automatically) are downloaded with the built-in BitTorrent engine ([librqbit](https://github.com/ikatson/rqbit)). They're ordinary items in the download list: they show progress and speed, and pause and continue, keeping every verified piece. A torrent with several files gets its own folder.

When a torrent finishes, it keeps sharing (*Sharing* in the list) until you've uploaded as much as you downloaded (a ratio of 1.0), then stops. **Stop sharing** ends it early; the download stays complete. Peers are found through trackers, the DHT and the local network, and the router's port is opened with UPnP when it allows.

### Torrent privacy

By default **torrents wait until a VPN or proxy protects them**, so your IP address is never exposed by accident. In *Proxy* mode everything goes through your VPN provider's SOCKS5 proxy and whatever can't (DHT, UDP trackers, incoming connections, UPnP, local discovery) is switched off. A **kill switch** pauses every torrent the moment the VPN or proxy drops and carries on when it's back. See [PRIVACY.md](PRIVACY.md) for what each mode protects, what it can't, and how it's tested.

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
  crates/torrent/        Torrents (librqbit), as a runner for the engine
  crates/media/          Video downloads (yt-dlp + ffmpeg): format menus and a runner
  crates/testkit/        Test web servers used by the Rust tests
  vendor/                A patched librqbit part (IPv4 fallback); see its NOTICE.md
  app/src-tauri/         The Windows app (Tauri 2): window, tray, settings, commands
  app/ui/                Its interface (HTML, CSS, JavaScript; mock.js simulates the backend for tests):
                         index.html main window, progress.html a download's window, mini.html the mini window;
                         table.js the list, layout.js folding and resizing, icons.js the Lucide icons
  app/test/              Interface tests in Chromium
  extension/             Browser extension (Chrome-family and Firefox builds)
  brand/                 Colours, fonts and Layó's logo from layothedeveloper.com
  app-icon.png           App icon (drawn by extension/tools/make-icons.py)
```

## Working on it

You need [Rust](https://rustup.rs/) 1.85 or newer, and Node.js 22 or newer for the extension.

```bash
cargo test --workspace
```

Runs the engine, torrent, video, bridge and pipe tests. The torrent tests share real files between two BitTorrent sessions on your computer. The video tests make HLS streams with ffmpeg and download them with yt-dlp; they're skipped if those aren't installed. The engine tests use a small web server that can go slow, drop connections, refuse byte ranges, hide the file size and refuse links. They check every download is byte-for-byte correct, including with 8, 16, 32 and 64 parallel sections. The bridge tests run the real `ldm-native-host` program the way a browser does.

```bash
cd extension && npm test && npm run test:e2e
```

Unit tests for the capture rules, then an end-to-end test in a real Chromium with the extension, the bridge and the download manager.

```bash
cd app && node test/ui.e2e.mjs
```

Tries the app window in Chromium against a simulated backend: section map, changing sections, pause and continue, adding downloads and videos, privacy settings, browser downloads, removing, categories, light and dark.

On Windows, `.github/workflows/download-manager-windows.yml` builds and tests everything and produces the installer. To build it yourself (in `app/src-tauri`):

```bash
npx @tauri-apps/cli@2 build
```

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
