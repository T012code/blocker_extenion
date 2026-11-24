# Layo's Advanced Blocker

An ad blocker for Google Chrome and Microsoft Edge. It blocks ads, trackers and YouTube ads, warns you before you open malware sites, and gives you a set of privacy and security switches in one place.

It runs on Manifest V3 and uses the browser's built-in request filtering (`declarativeNetRequest`). The browser checks every request against the block lists itself, which is fast, and nothing you browse ever leaves your computer. The extension has no servers and collects no data.

## What it does

**Blocking**
- Blocks requests to about 44,700 ad servers and 11,200 tracking and analytics domains
- Hides the empty boxes and ad containers that stay on the page after an ad is blocked
- Closes pop-up tabs that pages open to known ad networks
- Optional: blocks social media widgets (like buttons, share bars, embedded feeds)
- Optional: hides cookie consent banners from the common providers (OneTrust, Cookiebot, Quantcast, Didomi and others). It only hides them and never clicks "accept" for you.

**YouTube**
- Strips the ad schedule out of YouTube's player data before the player reads it, so video ads never start
- If an ad still gets through, it gets muted, skipped and hidden
- Hides ads in the home feed, search results, sidebar and Shorts
- Dismisses the "ad blockers are not allowed" pop-up if it appears

**Privacy**
- Removes tracking codes from links (`utm_*`, `fbclid`, `gclid`, `msclkid` and 300+ more)
- Sends the Global Privacy Control and Do Not Track signals
- Blocks third-party pings and beacons
- Turns off hyperlink auditing
- Stops WebRTC from leaking your real IP address (useful with a VPN)
- Optional: removes the Referer header on requests to other sites
- Optional: blocks third-party cookies
- Optional: turns off network prediction

**Security**
- Shows a warning page before opening about 3,600 sites known for malware, scams, bundled adware or hidden crypto mining. You can still choose to open the site.
- Blocks cryptocurrency miners running inside pages
- Optional: always opens sites over HTTPS (local network addresses are left alone)

**Control**
- Turn protection off for a single site from the toolbar pop-up, or manage your trusted sites in settings
- "Hide an element": click anything on a page to hide it on that site for good (also on the right-click menu)
- Pause everything with one switch
- Export, import and reset your settings

## Install it (developer mode)

Until it's published on the stores, load it straight from this folder.

**Chrome**
1. Download or clone this repository.
2. Open `chrome://extensions`.
3. Turn on **Developer mode** (top right).
4. Click **Load unpacked** and pick the repository folder (the one with `manifest.json` in it).

**Edge**
1. Download or clone this repository.
2. Open `edge://extensions`.
3. Turn on **Developer mode** (left sidebar).
4. Click **Load unpacked** and pick the repository folder.

Pin the shield icon to your toolbar so you can see how much gets blocked on each page.

Needs Chrome or Edge 121 or newer.

## Working on it

You need [Node.js](https://nodejs.org/) 22 or newer. There are no packages to install.

```bash
npm run update-lists
```
Downloads the newest copies of the block lists into `lists/upstream/`.

```bash
npm run build
```
Turns the lists into browser rules in `rules/`. Run this after updating lists or editing anything in `lists/custom/`, then hit reload on the extension.

```bash
npm run package
```
Creates `dist/layos-advanced-blocker-<version>.zip`, which is what you upload to the Chrome Web Store or Edge Add-ons.

### Folder layout

```
manifest.json          Extension manifest
src/background/        Service worker: applies settings, trusted sites, pop-up closing, warnings
src/content/           Scripts and styles that run on web pages (ad hiding, YouTube, element picker)
src/popup/             Toolbar pop-up
src/options/           Settings page
src/blocked/           Warning page for dangerous sites
src/shared/            Settings helpers and shared styles
lists/custom/          Hand-picked domains (edit these)
lists/upstream/        Downloaded block lists (don't edit, they get overwritten)
rules/                 Generated rules (don't edit, run npm run build)
tools/                 Build, list update and packaging scripts
```

### Adding your own domains

Add one domain per line to the matching file in `lists/custom/`:

- `ads.txt` for ad servers (blocked everywhere)
- `trackers.txt` for trackers (blocked when another site loads them)
- `social.txt` for social widgets
- `security.txt` for malware and miner sites
- `never-block.txt` for domains that must never be blocked, even if a list adds them

Then run `npm run build` and reload the extension.

## Block list sources

The domain lists come from these projects. Thanks to everyone who maintains them.

- [EasyList and EasyPrivacy](https://easylist.to/) (GPLv3 / CC BY-SA 3.0)
- [AdGuard filters](https://github.com/AdguardTeam/AdguardFilters) (GPLv3)
- [uBlock Origin badware list](https://github.com/uBlockOrigin/uAssets) (GPLv3)
- [NoCoin list](https://github.com/hoshsadiq/adblock-nocoin-list) (MIT)

## Limits worth knowing

- Chrome and Edge extensions can't run on browser pages (`chrome://`, `edge://`) or the extension stores, so nothing is blocked there.
- The "blocked on this page" number covers the last few minutes of activity on that tab. The browser keeps that history only for a short time.
- YouTube changes its site often. If ads start showing again, update the extension. Sometimes the fix is a single line in `src/content/youtube-main.js` or `src/content/youtube.css`.
- Some privacy switches use the browser's own settings. If your workplace or another extension controls those settings, the settings page tells you, and those switches may have no effect.

## License

GPL-3.0-or-later. See [LICENSE](LICENSE).
