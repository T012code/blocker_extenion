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
      await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.status', { hasText: 'Paused' }).waitFor();
      await page.click('#btn-resume');
      await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.status', { hasText: 'Downloading' }).waitFor();
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
      await page.locator('#rows tr', { hasText: 'Big Buck Bunny' }).locator('.status', { hasText: 'Downloading' }).waitFor();
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

      // Queues: create one, add a download to it, start it, then delete it keeping the download.
      await page.click('.nav-item[data-filter="all"]');
      await page.click('#btn-new-queue');
      await page.fill('#queue-name', 'Tonight');
      await page.selectOption('#queue-max', '1');
      await page.click('#queue-ok');
      const qnav = page.locator('#queue-nav .nav-item', { hasText: 'Tonight' });
      await qnav.click();
      await page.locator('#queuebar:not([hidden])').waitFor();
      await page.click('#btn-add');
      assert.equal(await page.locator('#add-queue option:checked').textContent(), 'Tonight', 'adding from a queue picks that queue');
      await page.fill('#add-url', 'https://mirror.example.org/archive.zip');
      await page.click('#add-start');
      const row = page.locator('#rows tr', { hasText: 'archive.zip' });
      await row.locator('.status', { hasText: 'Waiting' }).waitFor();
      console.log('ok - a download added to a stopped queue waits');
      await page.click('#qb-start');
      await row.locator('.status', { hasText: 'Downloading' }).waitFor();
      await page.locator('#qb-state', { hasText: 'Running' }).waitFor();
      // The bars above the list keep their own height; the list gets the rest.
      const bannerH = await page.locator('#privacy-banner').evaluate((e) => e.getBoundingClientRect().height);
      assert.ok(bannerH < 80, `privacy banner is ${bannerH}px tall`);
      if (shots) await page.screenshot({ path: join(shots, 'app-queue.png') });
      console.log('ok - Start queue starts its downloads');
      await page.click('#qb-stop');
      await row.locator('.status', { hasText: 'Paused' }).waitFor();
      console.log('ok - Stop queue pauses them');
      await page.click('#qb-delete');
      await page.click('#delqueue-ok');
      await page.locator('#queue-nav .nav-item', { hasText: 'Tonight' }).waitFor({ state: 'detached' });
      await page.locator('#rows tr', { hasText: 'archive.zip' }).waitFor();
      console.log('ok - Delete queue (keeping downloads) removes the queue and keeps its download');

      // Group remove: everything finished at once.
      await page.click('.nav-item[data-filter="all"]');
      await page.locator('#rows tr.completed').first().waitFor({ timeout: 60_000 });
      const finishedBefore = await page.locator('#rows tr.completed').count();
      await page.click('#btn-delete');
      await page.check('input[name="scope"][value="finished"]');
      if (shots) await page.screenshot({ path: join(shots, 'app-remove.png') });
      await page.click('#remove-ok');
      await page.waitForFunction(() => document.querySelectorAll('#rows tr.completed').length === 0);
      console.log(`ok - "All finished" removes ${finishedBefore || 'the'} finished downloads in one go`);

      // Start with Windows and minimize to tray are settings.
      await page.click('#btn-settings');
      assert.equal(await page.isChecked('input[name="launchOnStartup"]'), true);
      await page.check('input[name="minimizeToTray"]');
      await page.uncheck('input[name="launchOnStartup"]');
      assert.equal(await page.isDisabled('input[name="startMinimized"]'), true);
      await page.click('#settings-save');
      const saved = await page.evaluate(() => import('./backend.js').then((b) => b.invoke('get_settings')));
      assert.equal(saved.minimizeToTray, true);
      assert.equal(saved.launchOnStartup, false);
      console.log('ok - "Start with Windows" and "Minimize to the tray" are saved');

      // Keyboard: the list moves with the arrow keys; Space pauses or continues.
      await page.click('.nav-item[data-filter="all"]');
      await page.locator('#rows tr').first().click();
      await page.keyboard.press('ArrowDown');
      await page.waitForFunction(() => document.querySelector('#rows tr:nth-child(2)')?.classList.contains('selected'));
      console.log('ok - arrow keys move the selection in the list');

      // Right-click menu.
      await page.locator('#rows tr').first().click({ button: 'right' });
      await page.locator('#menu-row:not([hidden])').waitFor();
      const items = await page.locator('#menu-row button').allTextContents();
      assert.ok(items.some((t) => t.startsWith('Remove')) && items.some((t) => t.startsWith('Copy address')), items.join(' | '));
      if (shots) await page.screenshot({ path: join(shots, 'app-context-menu.png') });
      await page.keyboard.press('Escape');
      await page.locator('#menu-row[hidden]').waitFor({ state: 'attached' });
      console.log(`ok - right-click shows: ${items.map((t) => t.replace(/(Ctrl\+.|Del|Enter)$/, '')).join(' | ')}`);

      // Scheduler.
      await page.click('#btn-new-queue');
      await page.fill('#queue-name', 'Overnight');
      await page.click('#queue-ok');
      await page.locator('#queue-nav .nav-item', { hasText: 'Overnight' }).click();
      await page.click('#qb-schedule');
      await page.check('#sch-on');
      await page.fill('#sch-start', '22:30');
      await page.fill('#sch-stop', '06:00');
      for (const d of ['Saturday', 'Sunday']) await page.getByRole('checkbox', { name: d }).uncheck();
      await page.selectOption('#sch-done', 'sleep');
      await page.locator('#sch-summary', { hasText: 'Starts at 22:30 on weekdays, stops at 06:00. When it\'s done, the computer sleeps.' }).waitFor();
      if (shots) await page.screenshot({ path: join(shots, 'app-schedule.png') });
      await page.click('#sch-save');
      await page.locator('#qb-state', { hasText: 'starts at 22:30' }).waitFor();
      console.log('ok - a queue can be scheduled, and the summary says what will happen in plain words');

      // Site grabber.
      await page.click('#btn-more');
      await page.click('#btn-grab');
      await page.fill('#grab-url', 'https://example.org/downloads/');
      await page.fill('#grab-types', 'pdf zip');
      await page.click('#grab-go');
      await page.locator('#dlg-browser[open]').waitFor();
      await page.locator('#browser-count', { hasText: '3 of 3 selected · 12 pages read · 1 left out at the site\'s request' }).waitFor();
      if (shots) await page.screenshot({ path: join(shots, 'app-grab-results.png') });
      await page.click('#browser-toggle');
      await page.locator('#browser-items input').first().check();
      await page.click('#browser-start');
      await page.locator('#rows tr', { hasText: 'annual-report-2026.pdf' }).waitFor();
      console.log('ok - the site grabber lists matching files and downloads the ones picked');
    } else {
      await page.locator('#rows tr', { hasText: 'keynote' }).click();
      await page.waitForTimeout(400);
      if (shots) await page.screenshot({ path: join(shots, 'app-main-dark.png') });
      console.log('ok - dark mode renders');
    }
    // No pills: nothing may be capsule- or circle-shaped (corners of half the height or more).
    for (const open of [null, '#btn-settings', '#btn-add']) {
      if (open) await page.click(open);
      const pills = await page.evaluate(() => {
        const out = [];
        for (const el of document.querySelectorAll('body *')) {
          const r = el.getBoundingClientRect();
          if (r.width < 4 || r.height < 4) continue;
          const cs = getComputedStyle(el);
          if (cs.display === 'none' || cs.visibility === 'hidden') continue;
          const radius = Math.max(...['TopLeft', 'TopRight', 'BottomLeft', 'BottomRight'].map((c) => parseFloat(cs[`border${c}Radius`]) || 0));
          if (radius > 0 && radius >= Math.min(r.width, r.height) / 2 - 0.5) out.push(`${el.tagName.toLowerCase()}.${el.className} (${Math.round(r.width)}x${Math.round(r.height)}, radius ${radius})`);
        }
        return out;
      });
      assert.deepEqual(pills, [], `pill-shaped elements: ${pills.join(', ')}`);
      if (open) await page.keyboard.press('Escape');
    }
    console.log(`ok - no pill shapes anywhere (${scheme})`);
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
