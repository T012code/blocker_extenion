// Takes screenshots of every screen of the app and the extension.
//   node tools/screenshots.mjs <out-dir>
// The app runs against its simulated backend (app/ui/mock.js); the extension is loaded
// into Chromium without the desktop app, plus a demo page with a video for the button.

import { createServer } from 'node:http';
import { execFileSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { extname, join, resolve } from 'node:path';

const { chromium } = createRequire(import.meta.url)('playwright');
const out = resolve(process.argv[2] ?? 'docs/screenshots/after');
mkdirSync(out, { recursive: true });
const root = resolve('.');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2', '.mp4': 'video/mp4' };

// A short clip for the video button.
const media = mkdtempSync(join(tmpdir(), 'ldm-shots-'));
execFileSync('ffmpeg', ['-hide_banner', '-loglevel', 'error', '-y', '-f', 'lavfi', '-i', 'testsrc2=duration=4:size=960x540:rate=25',
  '-c:v', 'libx264', '-preset', 'ultrafast', '-pix_fmt', 'yuv420p', join(media, 'clip.mp4')]);

const server = createServer((req, res) => {
  const url = decodeURIComponent(req.url.split('?')[0]);
  if (url === '/video.html') {
    res.setHeader('content-type', 'text/html');
    return res.end(`<!doctype html><title>Product launch keynote</title><body style="margin:0;background:#0b0b0b;display:grid;place-items:center;height:100vh">
      <video id="v" src="/clip.mp4" muted autoplay loop playsinline width="960" height="540"></video></body>`);
  }
  if (url === '/clip.mp4') { res.setHeader('content-type', 'video/mp4'); return res.end(readFileSync(join(media, 'clip.mp4'))); }
  const path = join(root, 'app/ui', url === '/' ? 'index.html' : url);
  if (!path.startsWith(join(root, 'app/ui')) || !existsSync(path)) { res.statusCode = 404; return res.end(); }
  res.setHeader('content-type', TYPES[extname(path)] ?? 'application/octet-stream');
  res.end(readFileSync(path));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;
const shot = (page, name) => page.screenshot({ path: join(out, `${name}.png`) });

// ---------------- desktop app
const browser = await chromium.launch();
for (const scheme of ['light', 'dark']) {
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, colorScheme: scheme, deviceScaleFactor: 2 });
  await page.goto(`${base}/index.html?mock`);
  await page.locator('#rows tr').first().waitFor();
  await page.locator('#rows tr', { hasText: 'ubuntu' }).click();
  await page.waitForTimeout(1800);
  await shot(page, `app-main-${scheme}`);
  if (scheme === 'dark') { await page.close(); continue; }
  await page.click('#btn-add');
  await page.fill('#add-url', 'https://mirror.example.org/fedora-41-workstation.iso');
  await page.click('#add-check');
  await page.locator('#add-info:not([hidden])').waitFor();
  await page.waitForTimeout(300);
  await shot(page, 'app-add');
  await page.keyboard.press('Escape');
  await page.click('#btn-add');
  await page.fill('#add-url', 'https://video.example.com/watch?v=keynote');
  await page.click('#add-check');
  await page.locator('#add-formats:not([hidden])').waitFor();
  await page.waitForTimeout(300);
  await shot(page, 'app-add-video');
  await page.keyboard.press('Escape');
  for (const tab of ['general', 'connection', 'privacy', 'about']) {
    await page.click('#btn-settings');
    await page.click(`.tab[data-tab="${tab}"]`);
    await page.waitForTimeout(400);
    await shot(page, `app-settings-${tab}`);
    await page.keyboard.press('Escape');
  }
  if (await page.locator('#btn-new-queue').count()) {
    await page.click('#btn-new-queue');
    await page.fill('#queue-name', 'Tonight');
    await page.waitForTimeout(200);
    await shot(page, 'app-queue-new');
    await page.click('#queue-ok');
    await page.locator('#queue-nav .nav-item').first().click();
    await page.waitForTimeout(500);
    await shot(page, 'app-queue');
    await page.click('.nav-item[data-filter="all"]');
  }
  await page.click('#btn-delete');
  await page.waitForTimeout(300);
  await shot(page, 'app-remove');
  await page.keyboard.press('Escape');
  await page.evaluate(() => window.__ldmBrowserDownload?.([
    { url: 'https://example.com/files/report-2026.pdf', fileSize: 2_400_000 },
    { url: 'https://example.com/files/dataset.zip', fileSize: 380_000_000 },
    { url: 'https://example.com/files/installer.exe' },
  ], 'allLinks'));
  await page.waitForTimeout(400);
  await shot(page, 'app-browser-downloads');
  await page.keyboard.press('Escape');
  // Screens added in this release (no "before").
  if (await page.locator('#menu-row').count()) {
    await page.click('.nav-item[data-filter="all"]');
    await page.locator('#rows tr').first().click({ button: 'right' });
    await page.waitForTimeout(300);
    await shot(page, 'app-context-menu');
    await page.keyboard.press('Escape');
    await page.locator('#queue-nav .nav-item').first().click();
    await page.click('#qb-schedule');
    await page.check('#sch-on');
    await page.fill('#sch-start', '22:30');
    await page.fill('#sch-stop', '06:00');
    for (const d of ['Saturday', 'Sunday']) await page.getByRole('checkbox', { name: d }).uncheck();
    await page.selectOption('#sch-done', 'sleep');
    await page.waitForTimeout(300);
    await shot(page, 'app-schedule');
    await page.keyboard.press('Escape');
    await page.click('.nav-item[data-filter="all"]');
    await page.click('#btn-more');
    await page.waitForTimeout(250);
    await shot(page, 'app-more-menu');
    await page.click('#btn-grab');
    await page.fill('#grab-url', 'https://example.org/downloads/');
    await page.fill('#grab-types', 'pdf zip mp4');
    await page.click('#grab-go');
    await page.locator('#dlg-browser[open]').waitFor();
    await page.waitForTimeout(300);
    await shot(page, 'app-grab-results');
    await page.keyboard.press('Escape');
  }
  await page.click('.nav-item[data-filter="music"]');
  await page.waitForTimeout(400);
  await shot(page, 'app-empty');
  await page.close();
}

// Columns, folding, a long list, and the small windows.
{
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2 });
  await page.goto(`${base}/index.html?mock&many=150`);
  await page.locator('#rows tr').nth(150).waitFor({ state: 'attached' });
  await page.waitForTimeout(1200);
  await shot(page, 'app-long-list');
  await page.click('#btn-columns');
  await page.waitForTimeout(300);
  await shot(page, 'app-columns-menu');
  for (const label of ['Downloaded', 'Added', 'Finished', 'Sections', 'Queue', 'Address']) {
    await page.locator('#menu-columns button', { hasText: label }).click();
  }
  await page.keyboard.press('Escape');
  await page.click('th[data-key="size"] .th-button');
  await page.click('th[data-key="size"] .th-button');
  await page.evaluate(() => { document.getElementById('list').scrollLeft = 520; });
  await page.waitForTimeout(500);
  await shot(page, 'app-columns-sorted');
  await page.keyboard.press('Control+b');
  await page.keyboard.press('Control+j');
  await page.evaluate(() => { document.getElementById('list').scrollLeft = 0; });
  await page.waitForTimeout(600);
  await shot(page, 'app-folded');
  await page.close();
}
for (const scheme of ['light', 'dark']) {
  const win = await browser.newPage({ viewport: { width: 460, height: 302 }, colorScheme: scheme, deviceScaleFactor: 2 });
  await win.goto(`${base}/progress.html?mock&id=1`);
  await win.locator('#sections', { hasText: 'of 16' }).waitFor();
  await win.waitForTimeout(1500);
  const fit = async () => win.setViewportSize({ width: 460, height: Math.ceil(await win.evaluate(() => document.getElementById('win').getBoundingClientRect().height)) });
  await fit();
  await shot(win, `window-progress-${scheme}`);
  await win.click('#toggle');
  await fit();
  await win.waitForTimeout(400);
  await shot(win, `window-progress-compact-${scheme}`);
  await win.click('#toggle');
  await win.goto(`${base}/progress.html?mock&id=4`);
  await win.locator('#done:not([hidden])').waitFor();
  await fit();
  await win.waitForTimeout(900);
  await shot(win, `window-done-${scheme}`);
  await win.goto(`${base}/progress.html?mock&id=5`);
  await win.waitForTimeout(800);
  await fit();
  await shot(win, `window-expired-${scheme}`);
  await win.close();
  const mini = await browser.newPage({ viewport: { width: 320, height: 96 }, colorScheme: scheme, deviceScaleFactor: 2 });
  await mini.goto(`${base}/mini.html?mock`);
  await mini.locator('#summary', { hasText: 'downloading' }).waitFor();
  await mini.waitForTimeout(1200);
  const fitMini = async () => mini.setViewportSize({ width: 320, height: Math.ceil(await mini.evaluate(() => document.getElementById('win').getBoundingClientRect().height)) });
  await fitMini();
  await shot(mini, `window-mini-${scheme}`);
  await mini.click('#expand');
  await fitMini();
  await mini.waitForTimeout(500);
  await shot(mini, `window-mini-open-${scheme}`);
  await mini.close();
}
await browser.close();

// ---------------- extension (no desktop app installed)
execFileSync('node', ['tools/build.mjs'], { cwd: join(root, 'extension'), stdio: 'ignore' });
const ext = join(root, 'extension/dist/chromium');
for (const scheme of ['light', 'dark']) {
  const ctx = await chromium.launchPersistentContext(mkdtempSync(join(tmpdir(), 'ldm-prof-')), {
    channel: 'chromium', headless: true, colorScheme: scheme, deviceScaleFactor: 2,
    args: [`--disable-extensions-except=${ext}`, `--load-extension=${ext}`],
  });
  const ours = (w) => w.url().includes('klknhohjiaanbbicfdonmcnflkgjofai');
  ctx.serviceWorkers().find(ours) ?? await ctx.waitForEvent('serviceworker', { predicate: ours });
  const id = 'chrome-extension://klknhohjiaanbbicfdonmcnflkgjofai';
  const popup = await ctx.newPage();
  await popup.setViewportSize({ width: 380, height: 560 });
  await popup.goto(`${id}/popup/popup.html`);
  await popup.waitForTimeout(1500);
  await shot(popup, `ext-popup-${scheme}`);
  const opts = await ctx.newPage();
  await opts.setViewportSize({ width: 960, height: 1400 });
  await opts.goto(`${id}/options/options.html`);
  await opts.waitForTimeout(1500);
  await opts.screenshot({ path: join(out, `ext-options-${scheme}.png`), fullPage: true });
  if (scheme === 'light') {
    const vp = await ctx.newPage();
    await vp.setViewportSize({ width: 1100, height: 640 });
    await vp.goto(`${base}/video.html`);
    await vp.waitForFunction(() => document.getElementById('v').readyState >= 2);
    await vp.mouse.move(550, 300);
    const btn = vp.locator('ldm-video-download').getByRole('button', { name: 'Download' });
    await btn.waitFor();
    await vp.waitForTimeout(300);
    await shot(vp, 'ext-video-button');
    await btn.click();
    await vp.waitForTimeout(2500);
    await shot(vp, 'ext-video-menu');
  }
  await ctx.close();
}
server.close();
console.log(`screenshots in ${out}`);
