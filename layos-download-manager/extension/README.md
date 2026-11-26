# Layó's Download Manager: browser extension

Takes downloads over from the browser and sends them to Layó's Download Manager, which fetches them in several sections at once. Works in Chrome, Edge, Opera, Brave, Vivaldi and Firefox.

## What it does

**Takes over downloads automatically.** When you click a download link, the extension decides whether the download manager should handle it. If yes, it cancels the browser's download and sends the link to the app, together with the cookies, referrer and user agent the browser used, so downloads that need a login still work.

It takes a download over when **all** of these are true:

| Check | Why |
| --- | --- |
| Taking over is switched on | You can switch it off from the toolbar pop-up |
| The download manager is installed | Otherwise the browser keeps handling downloads, so nothing is lost |
| The address starts with `http` or `https` | `blob:` and `data:` downloads only exist inside the browser |
| It wasn't a form submission (POST) | Another program can't repeat a form submission |
| You weren't holding the bypass key | **Alt** by default; Shift, Ctrl or none in settings |
| The site isn't on your skip list | `example.com` also covers its subdomains |
| It's at least the minimum size | Off by default |
| The file type is in your list | 80 common types by default. A file with no extension counts when the server says it's video, audio, an archive or a program. |

If the app can't be reached when a download arrives, the extension gives the download back to the browser and tells you.

**How it hooks in.** In Chrome-family browsers it uses `downloads.onDeterminingFilename`, which runs before the browser shows a "Save as" window. If that step is skipped (some enterprise policies and automation tools do this), a backup on `downloads.onCreated` takes the download over within a second. Firefox uses `downloads.onCreated`.

**Right-click menu**
- **Download with Layó's DM** on a link
- **Download this video/audio with Layó's DM** on a video or audio player
- **Download all links with Layó's DM** on a page or a text selection. Only links to file types in your list are sent, and the app shows them in a list to choose from.

**Toolbar pop-up.** Shows whether the app is connected, has an on/off switch, and lists recent downloads, each marked *Sent* or *In browser*.

## How it talks to the app

```
extension ──Native Messaging──▶ ldm-native-host.exe ──named pipe──▶ Layó's Download Manager
```

`ldm-native-host` (in `../crates/native-host`) is a small program the browser starts on demand. It starts the app if it isn't running. The Windows installer registers it for every supported browser under `HKEY_CURRENT_USER`, so no admin rights are needed. To register it by hand:

```bash
ldm-native-host --register
```

```bash
ldm-native-host --unregister
```

Chrome only lets listed extension ids use the bridge. The development id is `klknhohjiaanbbicfdonmcnflkgjofai`, fixed by the `key` in `src/manifest.json`. When the extension is published, add the store ids to `CHROMIUM_EXTENSION_IDS` in `../crates/native-host/src/register.rs`, or pass `--extension-id <id>` when registering. Firefox uses the id `downloadmanager@layo.dev`.

## Building

You need Node.js 22 or newer. There are no packages to install.

```bash
npm run build
```

Writes `dist/chromium` (Chrome, Edge, Opera, Brave, Vivaldi) and `dist/firefox`.

```bash
npm run package
```

Writes store-ready zips to `dist/`. It leaves out the development `key`, because the stores give the extension its own id.

To try it, open `chrome://extensions` (or `edge://extensions`), turn on Developer mode, click **Load unpacked** and pick `dist/chromium`. In Firefox, open `about:debugging`, choose **This Firefox**, then **Load Temporary Add-on**, and pick `dist/firefox/manifest.json`. Firefox asks you to allow access to all sites the first time.

## Tests

```bash
npm test
```

Unit tests for the decision rules.

```bash
npm run test:e2e
```

End-to-end test in a real Chromium. Build the Rust programs first: run `cargo build -p ldm-native-host -p ldm-service` in `layos-download-manager/`. The test registers the real bridge, starts a local web site with downloads behind a login cookie, and checks that:

1. A clicked `.zip` is taken over, and the app downloads it in several sections using the login cookie
2. A link with no extension that the server sends as `video/mp4` is taken over
3. A `.txt` file (not in the list) stays in the browser
4. Alt+click stays in the browser
5. Taken-over files never reach the browser's download folder
6. The backup hook takes a download over when automation controls the browser's downloads

## Files

```
src/manifest.json           Chrome manifest (the build makes the Firefox one from it)
src/background/main.js      Starts everything; messages from the pop-up and pages
src/background/rules.js     Which downloads to take over (pure functions, unit tested)
src/background/capture.js   Download hooks: cancel in the browser, send to the app
src/background/requests.js  Remembers request method, cookies, referrer, user agent
src/background/menus.js     Right-click menu
src/background/native.js    Talks to the bridge program
src/content/bypass.js       Notices the bypass key on link clicks
src/popup/, src/options/    Toolbar pop-up and settings page
tools/build.mjs             Builds dist/chromium and dist/firefox
tools/make-icons.py         Draws the icons
```
