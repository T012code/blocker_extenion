// Tries the app window in Chromium with the simulated backend (ui/mock.js).
//   node test/ui.e2e.mjs            (SCREENSHOT_DIR=… to save screenshots)

import { createServer } from 'node:http';
import { readFileSync, existsSync } from 'node:fs';
import { createRequire } from 'node:module';
import { extname, join, resolve } from 'node:path';
import assert from 'node:assert/strict';

const { chromium } = createRequire(import.meta.url)('playwright');
const ui = resolve('ui');
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2' };
const server = createServer((req, res) => {
  const path = join(ui, decodeURIComponent(req.url.split('?')[0]).replace(/^\/$/, '/index.html'));
  if (!path.startsWith(ui) || !existsSync(path)) { res.statusCode = 404; return res.end(); }
  res.setHeader('content-type', TYPES[extname(path)] ?? 'application/octet-stream');
  res.end(readFileSync(path));
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const url = `http://127.0.0.1:${server.address().port}/index.html?mock`;
const shots = process.env.SCREENSHOT_DIR;

const browser = await chromium.launch();
let failed = false;
try {
  for (const scheme of ['light', 'dark']) {
    const page = await browser.newPage({ viewport: { width: 1280, height: 800 }, colorScheme: scheme });
    const errors = [];
    page.on('pageerror', (e) => errors.push(e.message));
    page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
    await page.goto(url);
    await page.locator('#rows tr').first().waitFor();
    assert.equal(await page.locator('#rows tr').count(), 5);

    if (scheme === 'light') {
      // Select the running ISO: the details show its live section map.
      await page.locator('#rows tr', { hasText: 'ubuntu' }).click();
      await page.waitForFunction(() => document.querySelectorAll('#d-map .seg').length >= 16);
      const segs = await page.locator('#d-map .seg').count();
      assert.ok(segs >= 16, `16 sections shown, saw ${segs}`);
      console.log(`ok - the section map shows ${segs} sections of the running download`);
      await page.waitForTimeout(600);
      if (shots) await page.screenshot({ path: join(shots, 'app-main-light.png') });

      // Change sections while it runs.
      await page.selectOption('#d-sections', '32');
      await page.waitForFunction(() => document.querySelectorAll('#d-map .seg').length >= 32);
      console.log('ok - changing to 32 sections redraws the map with 32');

      // Pause and continue from the toolbar.
      await page.click('#btn-pause');
      await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.badge', { hasText: 'Paused' }).waitFor();
      await page.click('#btn-resume');
      await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.badge', { hasText: 'Downloading' }).waitFor();
      console.log('ok - Pause and Continue work from the toolbar');

      // Torrent waits for a VPN by default; the banner says so.
      await page.locator('#privacy-banner:not([hidden])').waitFor();
      console.log('ok - the privacy banner shows while a torrent waits for a VPN');

      // Add a download with 16 sections.
      await page.click('#btn-add');
      await page.fill('#add-url', 'https://mirror.example.org/fedora-41.iso');
      await page.click('#add-check');
      await page.locator('#add-info', { hasText: '700 MB' }).waitFor();
      await page.selectOption('#add-sections', '16');
      if (shots) await page.screenshot({ path: join(shots, 'app-add.png') });
      await page.click('#add-start');
      await page.locator('#rows tr', { hasText: 'fedora-41.iso' }).waitFor();
      console.log('ok - Add with 16 sections puts the download in the list');

      // A video page offers formats.
      await page.click('#btn-add');
      await page.fill('#add-url', 'https://video.example.com/watch?v=keynote');
      await page.click('#add-check');
      await page.locator('#add-formats label', { hasText: '1080p60 MP4' }).waitFor();
      await page.locator('#add-formats label', { hasText: 'Audio only' }).click();
      await page.click('#add-start');
      await page.locator('#rows tr', { hasText: 'Keynote 2026' }).waitFor();
      console.log('ok - a video page lists formats and adds the chosen one');

      // Privacy settings: switch to Proxy, test it, save; the torrent starts.
      await page.click('#btn-settings');
      await page.click('.tab[data-tab="privacy"]');
      await page.check('input[name="mode"][value="proxy"]');
      await page.fill('input[name="proxyUrl"]', '10.64.0.1:1080');
      await page.click('#set-test-proxy');
      await page.locator('#privacy-status', { hasText: 'Proxy works' }).waitFor();
      if (shots) await page.screenshot({ path: join(shots, 'app-privacy.png') });
      await page.click('#settings-save');
      await page.locator('#privacy-banner[hidden]').waitFor({ state: 'attached' });
      await page.locator('#rows tr', { hasText: 'Big Buck Bunny' }).locator('.badge', { hasText: 'Downloading' }).waitFor();
      console.log('ok - choosing Proxy mode in Settings lets the waiting torrent start');

      // Downloads sent from the browser ask first.
      await page.evaluate(() => window.__ldmBrowserDownload([
        { url: 'https://example.com/a.zip', fileSize: 1048576 }, { url: 'https://example.com/b.zip' },
      ], 'allLinks'));
      await page.locator('#dlg-browser[open]').waitFor();
      await page.locator('#browser-items input').nth(1).uncheck();
      await page.click('#browser-start');
      await page.locator('#rows tr', { hasText: 'a.zip' }).waitFor();
      assert.equal(await page.locator('#rows tr', { hasText: 'b.zip' }).count(), 0);
      console.log('ok - links from the browser can be picked before downloading');

      // Remove.
      await page.locator('#rows tr', { hasText: 'report-q3.pdf' }).click();
      await page.keyboard.press('Delete');
      await page.click('#remove-ok');
      await page.locator('#rows tr', { hasText: 'report-q3.pdf' }).waitFor({ state: 'detached' });
      console.log('ok - Delete removes the selected download');

      // Categories.
      await page.click('.nav-item[data-filter="torrents"]');
      await page.waitForFunction(() => document.querySelectorAll('#rows tr').length === 1);
      assert.match(await page.locator('#rows tr').textContent(), /Big Buck Bunny/);
      console.log('ok - the Torrents category shows only torrents');
    } else {
      await page.locator('#rows tr', { hasText: 'keynote' }).click();
      await page.waitForTimeout(400);
      if (shots) await page.screenshot({ path: join(shots, 'app-main-dark.png') });
      console.log('ok - dark mode renders');
    }
    assert.deepEqual(errors, [], 'no errors in the page');
    await page.close();
  }
} catch (e) {
  failed = true;
  console.error('FAILED:', e);
} finally {
  await browser.close();
  server.close();
}
process.exit(failed ? 1 : 0);
