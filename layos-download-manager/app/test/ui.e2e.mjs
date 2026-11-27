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
      assert.equal(await page.isHidden('#add-start'), true, 'in a queue, only "Add to queue" is offered');
      await page.locator('#add-later.primary', { hasText: 'Add to queue' }).click();
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
      const finishedIds = await page.$$eval('#rows tr.completed', (rows) => rows.map((r) => r.dataset.id));
      const finishedBefore = finishedIds.length;
      await page.click('#btn-delete');
      await page.check('input[name="scope"][value="finished"]');
      if (shots) await page.screenshot({ path: join(shots, 'app-remove.png') });
      await page.click('#remove-ok');
      // Those finished downloads are gone (another may finish meanwhile; it stays).
      await page.waitForFunction((ids) => ids.every((id) => !document.querySelector(`#rows tr[data-id="${id}"]`)), finishedIds);
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
      await page.waitForTimeout(300);
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
    for (const [open, dlg] of [[null], ['#btn-settings', '#dlg-settings'], ['#btn-add', '#dlg-add']]) {
      if (open) { await page.click(open); await page.locator(`${dlg}[open]`).waitFor(); }
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
      if (open) { await page.keyboard.press('Escape'); await page.locator(`${dlg}[open]`).waitFor({ state: 'detached' }); }
    }
    console.log(`ok - no pill shapes anywhere (${scheme})`);
    assert.deepEqual(errors, [], 'no errors in the page');
    await page.close();
  }
  await layoutAndWindowTests();
} catch (e) {
  failed = true;
  console.error('FAILED:', e);
} finally {
  await browser.close();
  server.close();
}
process.exit(failed ? 1 : 0);

// The list's columns, sorting and scrolling; folding and resizing the side bar and details;
// a download's own window and the mini window.
async function layoutAndWindowTests() {
  const base = url.replace('index.html?mock', '');
  const page = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  const errors = [];
  page.on('pageerror', (e) => errors.push(e.message));
  page.on('console', (m) => { if (m.type() === 'error') errors.push(m.text()); });
  await page.goto(url);
  await page.locator('#rows tr.blocked').waitFor();

  // Status colours: red failed, green done, amber waiting on you, neutral otherwise.
  const tones = await page.evaluate(() => {
    const css = getComputedStyle(document.documentElement);
    const probe = (v) => { const e = document.createElement('i'); e.style.color = `var(${v})`; document.body.append(e); const c = getComputedStyle(e).color; e.remove(); return c; };
    const want = { good: probe('--success'), held: probe('--warning'), bad: probe('--danger'), neutral: probe('--ink') };
    void css;
    return [...document.querySelectorAll('#rows tr')].map((tr) => {
      const s = tr.querySelector('.status');
      const tone = [...s.classList].find((c) => c.startsWith('tone-')).slice(5);
      return { status: tr.className.split(' ')[0], tone, ok: getComputedStyle(s).color === want[tone], icon: !!s.querySelector('svg') };
    });
  });
  const toneOf = Object.fromEntries(tones.map((t) => [t.status, t.tone]));
  assert.equal(toneOf.completed, 'good');
  assert.equal(toneOf.paused, 'held');
  assert.equal(toneOf.needsRefresh, 'held');
  assert.equal(toneOf.blocked, 'held');
  assert.equal(toneOf.downloading, 'neutral');
  assert.ok(tones.every((t) => t.ok && t.icon), JSON.stringify(tones));
  console.log('ok - status icons are green when done, amber for paused, expired or waiting for a VPN, neutral otherwise');

  // Type has its own column; the name cell is only the name.
  const pdf = page.locator('#rows tr', { hasText: 'report-q3.pdf' });
  assert.equal((await pdf.locator('td').nth(0).innerText()).trim(), 'PDF');
  assert.equal((await pdf.locator('td').nth(1).innerText()).trim(), 'report-q3.pdf');
  console.log('ok - file type is a column of its own');

  // Sorting by size, both ways.
  const sortedBySize = (dir) => page.waitForFunction((d) => {
    const toBytes = (t) => { const [n, u] = t.split(' '); return Number(n) * 1024 ** ['B', 'KB', 'MB', 'GB', 'TB'].indexOf(u); };
    const v = [...document.querySelectorAll('#rows tr')].map((r) => toBytes(r.children[2].textContent));
    return v.length === 5 && v.every((x, i) => i === 0 || (x - v[i - 1]) * d >= 0);
  }, dir);
  await page.click('th[data-key="size"] .th-button');
  assert.equal(await page.getAttribute('th[data-key="size"]', 'aria-sort'), 'ascending');
  await sortedBySize(1);
  await page.click('th[data-key="size"] .th-button');
  assert.equal(await page.getAttribute('th[data-key="size"]', 'aria-sort'), 'descending');
  await sortedBySize(-1);
  console.log('ok - clicking a column header sorts by it, then the other way');

  // Columns: add all of them, then the list scrolls sideways.
  await page.click('#btn-columns');
  for (const label of ['Downloaded', 'Added', 'Finished', 'Sections', 'Queue', 'Folder', 'Address']) {
    await page.locator('#menu-columns button', { hasText: label }).click();
    await page.locator('#menu-columns:not([hidden])').waitFor();
  }
  await page.keyboard.press('Escape');
  for (const key of ['downloaded', 'added', 'finished', 'sections', 'queue', 'folder', 'address']) await page.locator(`th[data-key="${key}"]`).waitFor();
  const wide = await page.evaluate(() => { const l = document.getElementById('list'); l.scrollLeft = 400; return { sw: l.scrollWidth, cw: l.clientWidth, left: l.scrollLeft }; });
  assert.ok(wide.sw > wide.cw && wide.left > 0, JSON.stringify(wide));
  const addressAt = await page.$$eval('thead th[data-key]', (ths) => ths.findIndex((t) => t.dataset.key === 'address'));
  assert.equal((await page.locator('#rows tr', { hasText: 'report-q3.pdf' }).locator('td').nth(addressAt).innerText()).trim(), 'files.example.net');
  console.log(`ok - all 14 columns can be shown, and the list scrolls sideways (${wide.sw}px in ${wide.cw}px)`);

  // Resize a column by dragging its edge.
  const before = (await page.locator('th[data-key="size"]').boundingBox()).width;
  const grip = await page.locator('th[data-key="size"] .col-resize').boundingBox();
  await page.mouse.move(grip.x + grip.width / 2, grip.y + grip.height / 2);
  await page.mouse.down();
  await page.mouse.move(grip.x + 64, grip.y + grip.height / 2, { steps: 4 });
  await page.mouse.up();
  const after = (await page.locator('th[data-key="size"]').boundingBox()).width;
  assert.ok(after - before > 50, `${before} -> ${after}`);
  console.log(`ok - dragging a column's edge resizes it (${Math.round(before)} -> ${Math.round(after)}px)`);

  // Folding and resizing the side bar.
  const sidebarW = () => page.evaluate(() => document.getElementById('sidebar').getBoundingClientRect().width);
  await page.click('#btn-sidebar');
  await page.waitForFunction(() => document.getElementById('sidebar').getBoundingClientRect().width < 60);
  assert.equal(await page.getAttribute('#btn-sidebar', 'aria-expanded'), 'false');
  await page.keyboard.press('Control+b');
  await page.waitForFunction(() => document.getElementById('sidebar').getBoundingClientRect().width === 232);
  const sp = await page.locator('#split-sidebar').boundingBox();
  await page.mouse.move(sp.x + 1, 300);
  await page.mouse.down();
  await page.mouse.move(sp.x + 81, 300, { steps: 5 });
  await page.mouse.up();
  const w1 = await sidebarW();
  assert.ok(Math.abs(w1 - 312) <= 3, `side bar is ${w1}px`);
  console.log(`ok - the side bar folds to icons (button and Ctrl+B) and drags wider (${Math.round(w1)}px)`);

  // Folding and resizing the details panel.
  const detailsH = () => page.evaluate(() => document.getElementById('details').getBoundingClientRect().height);
  await page.keyboard.press('Control+j');
  await page.waitForFunction(() => document.getElementById('details').getBoundingClientRect().height <= 49);
  await page.click('#details-toggle');
  await page.waitForFunction(() => document.getElementById('details').getBoundingClientRect().height === 248);
  const dsp = await page.locator('#split-details').boundingBox();
  await page.mouse.move(600, dsp.y);
  await page.mouse.down();
  await page.mouse.move(600, dsp.y - 100, { steps: 5 });
  await page.mouse.up();
  const h1 = await detailsH();
  assert.ok(Math.abs(h1 - 348) <= 3, `details are ${h1}px`);
  console.log(`ok - the details fold to one line (Ctrl+J and its own button) and drag taller (${Math.round(h1)}px)`);

  // All of it is remembered.
  await page.reload();
  await page.locator('#rows tr').first().waitFor();
  assert.ok(Math.abs((await sidebarW()) - w1) <= 3);
  assert.ok(Math.abs((await detailsH()) - h1) <= 3);
  await page.locator('th[data-key="address"]').waitFor();
  assert.equal(await page.getAttribute('th[data-key="size"]', 'aria-sort'), 'descending');
  console.log('ok - layout, columns and sorting are remembered');

  // The download's window, from the right-click menu.
  await page.evaluate(() => { window.__ldmCalls.length = 0; });
  await page.locator('#rows tr', { hasText: 'ubuntu' }).click({ button: 'right' });
  await page.locator('#menu-row button', { hasText: 'Show download window' }).click();
  await page.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'open_download_window' && c.args.id === 1));
  console.log('ok - "Show download window" opens the download\'s own window');

  // Window settings.
  await page.click('#btn-settings');
  await page.locator('#dlg-settings[open]').waitFor();
  for (const n of ['showProgressWindow', 'showCompleteWindow', 'showMiniWindow']) assert.equal(await page.isChecked(`input[name="${n}"]`), true);
  await page.uncheck('input[name="showMiniWindow"]');
  await page.click('#settings-save');
  const saved = await page.evaluate(() => import('./backend.js').then((b) => b.invoke('get_settings')));
  assert.equal(saved.showMiniWindow, false);
  assert.equal(saved.showProgressWindow, true);
  console.log('ok - download windows, the "done" window and the mini window can each be turned off');

  // The full name shows in the side bar, at its default width and at its narrowest.
  const brandFits = () => page.evaluate(() => [...document.querySelectorAll('.brand .label > span')].map((sp) => sp.scrollWidth <= sp.clientWidth && sp.getBoundingClientRect().width > 0));
  assert.deepEqual(await brandFits(), [true, true]);
  await page.evaluate(() => { localStorage.setItem('ldm.layout.v1', JSON.stringify({ sidebar: 150 })); });
  await page.reload();
  await page.locator('#rows tr').first().waitFor();
  assert.deepEqual(await brandFits(), [true, true], 'name fits at the narrowest side bar');
  assert.deepEqual(await page.$$eval('.brand .label > span', (s) => s.map((x) => x.textContent)), ['Layó\'s', 'Download Manager']);
  console.log('ok - "Layó\'s Download Manager" shows in full in the side bar, even at its narrowest');

  // Tooltips name the icon buttons, with their shortcuts; plain labelled items get none.
  const tipFor = async (sel) => {
    await page.mouse.move(5, 790);
    await page.locator('#tooltip').waitFor({ state: 'hidden' }).catch(() => {});
    await page.hover(sel);
    await page.locator('#tooltip:not([hidden])').waitFor({ timeout: 3000 });
    return page.locator('#tooltip').innerText();
  };
  assert.equal(await tipFor('#btn-columns'), 'Choose columns');
  assert.match(await tipFor('#btn-details'), /^Hide the details panel\s*Ctrl\+J$/);
  assert.match(await tipFor('#btn-settings'), /^Settings\s*Ctrl\+,$/);
  await page.locator('#rows tr', { hasText: 'ubuntu' }).click();
  assert.match(await tipFor('#d-window'), /Show this download's window/);
  assert.match(await tipFor('#d-copy'), /Copy the download address/);
  await page.mouse.move(5, 790);
  await page.hover('.nav-item[data-filter="music"]');
  await page.waitForTimeout(800);
  assert.equal(await page.isHidden('#tooltip'), true, 'no tooltip repeating a visible label');
  if (shots) { await page.hover('#btn-columns'); await page.locator('#tooltip:not([hidden])').waitFor(); await page.screenshot({ path: join(shots, 'app-tooltip.png') }); }
  console.log('ok - tooltips name the toolbar and details icons (with shortcuts), and stay away from visible labels');

  // The side bar button is at the bottom of the side bar, says what it does, and shows names as tooltips when folded.
  assert.equal((await page.locator('#btn-sidebar').innerText()).trim(), 'Hide side bar');
  await page.click('#btn-sidebar');
  await page.waitForFunction(() => document.getElementById('sidebar').getBoundingClientRect().width < 60);
  assert.equal(await page.getAttribute('#btn-sidebar', 'aria-expanded'), 'false');
  assert.match(await tipFor('.nav-item[data-filter="music"]'), /^Music$/);
  assert.equal(await page.getAttribute('#tooltip', 'data-side'), 'right');
  assert.match(await tipFor('#btn-sidebar'), /Show the side bar/);
  await page.click('#btn-sidebar');
  await page.waitForFunction(() => document.getElementById('sidebar').getBoundingClientRect().width > 190);
  console.log('ok - "Hide side bar" sits at the bottom of the side bar; folded, its items show their names on hover');

  // Columns: Name can be made narrower; any column can be dragged to a new place.
  await page.evaluate(() => localStorage.removeItem('ldm.table.v1'));
  await page.reload();
  await page.locator('#rows tr').first().waitFor();
  // Let the page settle (fonts, first layout) before dragging.
  await page.waitForFunction(() => document.fonts.status === 'loaded' && !document.body.classList.contains('layout-init'));
  await page.waitForTimeout(300);
  const nameW = () => page.evaluate(() => document.querySelector('th[data-key="name"]').getBoundingClientRect().width);
  const n0 = await nameW();
  const ng = await page.locator('th[data-key="name"] .col-resize').boundingBox();
  await page.mouse.move(ng.x + ng.width / 2, ng.y + ng.height / 2);
  await page.mouse.down();
  await page.mouse.move(ng.x - 150, ng.y + ng.height / 2, { steps: 5 });
  await page.mouse.up();
  const n1 = await nameW();
  assert.ok(n0 - n1 > 140, `Name ${n0} -> ${n1}`);
  console.log(`ok - every column can be made narrower, Name too (${Math.round(n0)} -> ${Math.round(n1)}px)`);
  const order = () => page.$$eval('thead th[data-key]', (ths) => ths.map((t) => t.dataset.key));
  const sizeH = await page.locator('th[data-key="size"] .th-button').boundingBox();
  const typeH = await page.locator('th[data-key="type"] .th-button').boundingBox();
  await page.mouse.move(sizeH.x + sizeH.width / 2, sizeH.y + sizeH.height / 2);
  await page.mouse.down();
  await page.mouse.move(sizeH.x - 40, sizeH.y + 10, { steps: 4 });
  await page.mouse.move(typeH.x + 4, typeH.y + 10, { steps: 6 });
  await page.locator('.col-drop').waitFor();
  if (shots) await page.screenshot({ path: join(shots, 'app-column-move.png') });
  await page.mouse.up();
  assert.deepEqual((await order()).slice(0, 3), ['size', 'type', 'name']);
  assert.equal(await page.getAttribute('th[data-key="size"]', 'aria-sort'), 'none', 'dragging a header does not sort');
  assert.equal(await page.locator('#rows tr', { hasText: 'report-q3.pdf' }).locator('td').nth(0).innerText(), '3.2 MB', 'cells moved with their header');
  await page.focus('th[data-key="size"] .th-button');
  await page.keyboard.press('Alt+ArrowRight');
  assert.deepEqual((await order()).slice(0, 3), ['type', 'size', 'name']);
  await page.reload();
  await page.locator('#rows tr').first().waitFor();
  assert.deepEqual((await order()).slice(0, 3), ['type', 'size', 'name'], 'the order is remembered');
  console.log('ok - columns move by dragging their header (or Alt+←/→), and the order is remembered');

  // Exit: the Exit button (or Ctrl+Q) closes the app; with downloads running it asks first.
  await page.evaluate(() => { window.__ldmCalls.length = 0; });
  await page.click('#btn-exit');
  await page.locator('#dlg-exit[open]').waitFor();
  assert.match(await page.locator('#exit-text').innerText(), /running/);
  await page.getByRole('button', { name: 'Keep running' }).click();
  await page.locator('#dlg-exit[open]').waitFor({ state: 'detached' });
  assert.equal(await page.evaluate(() => window.__ldmCalls.some((c) => c.cmd === 'exit_app')), false);
  await page.keyboard.press('Control+q');
  await page.locator('#dlg-exit[open]').waitFor();
  await page.click('#exit-ok');
  await page.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'exit_app'));
  console.log('ok - Exit (and Ctrl+Q) exits, asking first while downloads run');
  await page.reload();
  await page.locator('#rows tr').first().waitFor();
  await page.evaluate(() => { window.__ldmCalls.length = 0; });

  // IDM-style: a checked file link starts in the background, hidden from the list,
  // and Start confirms it with the chosen name and folder.
  await page.evaluate(() => { window.__ldmCalls.length = 0; });
  const rowsBefore = await page.locator('#rows tr').count();
  await page.click('#btn-add');
  await page.fill('#add-url', 'https://mirror.example.org/fedora-41.iso');
  await page.click('#add-check');
  await page.locator('#add-info .early', { hasText: 'Already downloading in the background' }).waitFor();
  const bgId = await page.evaluate(() => window.__ldmCalls.find((c) => c.cmd === 'start_background')?.args.url);
  assert.equal(bgId, 'https://mirror.example.org/fedora-41.iso');
  await page.waitForTimeout(300);
  assert.equal(await page.locator('#rows tr').count(), rowsBefore, 'not in the list before it is confirmed');
  await page.fill('#add-name', 'Fedora Workstation.iso');
  await page.click('#add-start');
  await page.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'confirm_download' && c.args.args.fileName === 'Fedora Workstation.iso' && c.args.args.start));
  await page.locator('#rows tr', { hasText: 'Fedora Workstation.iso' }).waitFor();
  assert.equal(await page.evaluate(() => window.__ldmCalls.some((c) => c.cmd === 'discard_download')), false);
  console.log('ok - a checked link starts downloading in the background; Start confirms it with the chosen name');
  // Cancelling throws the background download away.
  await page.click('#btn-add');
  await page.fill('#add-url', 'https://mirror.example.org/debian-13.iso');
  await page.click('#add-check');
  await page.locator('#add-info .early').waitFor();
  await page.keyboard.press('Escape');
  await page.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'discard_download'));
  console.log('ok - cancelling the Add window discards what it started');

  // Downloaded before: the Add window says so and offers to open it instead.
  await page.evaluate(() => { window.__ldmCalls.length = 0; });
  await page.click('#btn-add');
  await page.fill('#add-url', 'https://files.example.net/report-q3.pdf');
  await page.click('#add-check');
  await page.locator('#add-dup:not([hidden])', { hasText: 'You\'ve downloaded this before' }).waitFor();
  assert.equal(await page.locator('#add-start').innerText(), 'Download again');
  assert.equal(await page.locator('#add-dup-use').innerText(), 'Open it');
  assert.equal(await page.evaluate(() => window.__ldmCalls.some((c) => c.cmd === 'start_background')), false, 'nothing starts until you decide');
  await page.waitForTimeout(400);
  if (shots) await page.screenshot({ path: join(shots, 'app-add-duplicate.png') });
  await page.click('#add-dup-use');
  await page.locator('#dlg-add[open]').waitFor({ state: 'detached' });
  console.log('ok - a link downloaded before is caught in the Add window: open it, or download it again');

  // From the browser with "ask first": items already started are confirmed or discarded.
  const started = await page.evaluate(() => import('./backend.js').then((b) => Promise.all([
    b.invoke('start_background', { url: 'https://cdn.example.com/a.zip' }),
    b.invoke('start_background', { url: 'https://cdn.example.com/b.zip' }),
  ])));
  await page.evaluate((ids) => window.__ldmBrowserDownload([{ url: 'https://cdn.example.com/a.zip' }, { url: 'https://cdn.example.com/b.zip' }], 'capture', ids), started);
  await page.locator('#dlg-browser[open]').waitFor();
  await page.locator('#browser-items input').nth(1).uncheck();
  await page.click('#browser-start');
  await page.waitForFunction((ids) => window.__ldmCalls.some((c) => c.cmd === 'confirm_download' && c.args.id === ids[0])
    && window.__ldmCalls.some((c) => c.cmd === 'discard_download' && c.args.id === ids[1]), started);
  console.log('ok - downloads the browser sent (already started) are confirmed when picked, discarded when not');

  // The network goes away: downloads pause and stay paused; back online, one click continues them.
  await page.evaluate(() => window.__ldmNetwork(false));
  await page.locator('#network-banner:not([hidden])', { hasText: 'No internet connection' }).waitFor();
  await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.status', { hasText: 'Paused' }).waitFor();
  await page.waitForTimeout(400);
  if (shots) await page.screenshot({ path: join(shots, 'app-offline.png') });
  await page.waitForTimeout(600);
  assert.equal(await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.status').innerText(), 'Paused', 'nothing restarts by itself');
  await page.evaluate(() => window.__ldmNetwork(true));
  await page.locator('#network-banner:not([hidden])', { hasText: 'You\'re back online' }).waitFor();
  await page.click('#network-continue');
  await page.locator('#rows tr', { hasText: 'ubuntu' }).locator('.status', { hasText: 'Downloading' }).waitFor();
  console.log('ok - offline, downloads pause and say why; back online, they continue only when asked');

  // Settings: a folder for each type of file.
  await page.click('#btn-settings');
  await page.locator('#dlg-settings[open]').waitFor();
  await page.click('.tab[data-tab="connection"]');
  assert.equal(await page.locator('#type-folders input').count(), 7);
  const music = page.locator('#type-folders input[data-category="music"]');
  assert.match(await music.getAttribute('placeholder'), /Download Manager\\Music$/);
  await page.locator('#type-folders .type-folder', { hasText: 'Music' }).locator('.pathfield-btn').click();
  await page.waitForFunction(() => document.querySelector('#type-folders input[data-category="music"]').value === 'D:\\Media');
  await page.selectOption('select[name="duplicates"]', 'existing');
  if (shots) await page.screenshot({ path: join(shots, 'app-settings-folders.png') });
  await page.click('#settings-save');
  const saved2 = await page.evaluate(() => import('./backend.js').then((b) => b.invoke('get_settings')));
  assert.deepEqual(saved2.categoryDirs, { music: 'D:\\Media' });
  assert.equal(saved2.duplicates, 'existing');
  console.log('ok - each type of file (Video, Music, Documents, Compressed, Programs, Torrents, General) can have its own folder');

  // The folder field: automatic by default, Choose… picks a folder, × goes back.
  await page.click('#btn-add');
  await page.locator('#dlg-add[open]').waitFor();
  await page.waitForFunction(() => document.getElementById('add-dir').placeholder.startsWith('Automatic'));
  assert.equal(await page.isHidden('#add-dir-clear'), true);
  const fit = await page.evaluate(() => {
    const box = document.querySelector('#add-dir').closest('.pathfield').getBoundingClientRect();
    const b = document.getElementById('add-browse').getBoundingClientRect();
    return { inside: b.top >= box.top && b.bottom <= box.bottom && b.right <= box.right + 0.5, full: b.height >= box.height - 2.5 };
  });
  assert.deepEqual(fit, { inside: true, full: true }, 'Choose sits inside the folder field, full height');
  await page.click('#add-browse');
  await page.waitForFunction(() => document.getElementById('add-dir').value === 'D:\\Media');
  await page.locator('#add-dir-clear:not([hidden])').click();
  assert.equal(await page.inputValue('#add-dir'), '');
  if (shots) await page.screenshot({ path: join(shots, 'app-add-folder.png') });
  await page.keyboard.press('Escape');
  console.log('ok - the folder field picks a folder with Choose…, and × goes back to automatic');

  // Refresh an expired link: Continue on it tries the link, opens its page and waits.
  await page.locator('#rows tr', { hasText: 'setup.exe' }).click();
  await page.locator('#d-refresh:not([hidden])').waitFor();
  assert.equal(await page.locator('#d-open').count(), 0, 'no Open button in the details');
  await page.click('#btn-resume');
  await page.locator('#dlg-refresh[open]').waitFor();
  await page.locator('#refresh-waiting:not([hidden])', { hasText: 'dl.example.com' }).waitFor();
  await page.locator('#refresh-pct', { hasText: '43%' }).waitFor();
  await page.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'refresh_link' && c.args.open === true));
  if (shots) await page.screenshot({ path: join(shots, 'app-refresh-link.png') });
  const expiredId = Number(await page.locator('#rows tr', { hasText: 'setup.exe' }).getAttribute('data-id'));
  await page.evaluate((id) => window.__ldmFreshLink(id), expiredId);
  await page.locator('#dlg-refresh[open]').waitFor({ state: 'detached' });
  await page.locator('#toast', { hasText: 'Got a fresh link. Continuing setup.exe' }).waitFor();
  await page.locator('#rows tr', { hasText: 'setup.exe' }).locator('.status', { hasText: 'Downloading' }).waitFor();
  console.log('ok - "Refresh link" opens the download\'s page, and a fresh link from the browser continues it');

  // Or paste a new link.
  await page.evaluate(() => import('./backend.js').then((b) => b.invoke('pause', { ids: [] })));
  await page.goto(url);
  await page.locator('#rows tr', { hasText: 'setup.exe' }).click({ button: 'right' });
  await page.locator('#menu-row button', { hasText: 'Refresh link…' }).click();
  await page.locator('#refresh-waiting:not([hidden])').waitFor();
  await page.click('#refresh-paste summary');
  await page.fill('#refresh-url', 'not a link');
  await page.click('#refresh-use');
  await page.locator('#refresh-error:not([hidden])').waitFor();
  await page.fill('#refresh-url', 'https://mirror.example.com/setup.exe?token=new');
  await page.keyboard.press('Enter');
  await page.locator('#dlg-refresh[open]').waitFor({ state: 'detached' });
  await page.locator('#rows tr', { hasText: 'setup.exe' }).locator('.status', { hasText: 'Downloading' }).waitFor();
  console.log('ok - a pasted link continues an expired download (and a non-link is refused)');
  assert.deepEqual(errors, [], 'no errors in the page');
  await page.close();

  // 150 downloads: the list scrolls, the header stays, rows are updated in place.
  const long = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  long.on('pageerror', (e) => errors.push(e.message));
  await long.goto(`${url}&many=150`);
  await long.locator('#rows tr').nth(154).waitFor({ state: 'attached' });
  assert.equal(await long.locator('#rows tr').count(), 155);
  const running = await long.locator('#rows tr', { hasText: 'ubuntu' }).elementHandle();
  await long.waitForTimeout(800);
  assert.equal(await running.evaluate((tr) => tr.isConnected), true, 'the running row was replaced instead of updated');
  const scroll = await long.evaluate(async () => {
    const l = document.getElementById('list');
    l.scrollTop = l.scrollHeight;
    await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
    const last = document.querySelector('#rows tr:last-child').getBoundingClientRect();
    const box = l.getBoundingClientRect();
    const head = document.querySelector('thead th').getBoundingClientRect();
    return { scrolled: l.scrollTop > 0, lastInside: last.bottom <= box.bottom + 1 && last.top >= box.top, headAtTop: Math.abs(head.top - box.top) < 1, pageFits: document.documentElement.scrollHeight <= innerHeight };
  });
  assert.deepEqual(scroll, { scrolled: true, lastInside: true, headAtTop: true, pageFits: true });
  await long.locator('#rows tr').last().click();
  await long.keyboard.press('ArrowUp');
  await long.waitForFunction(() => document.querySelector('#rows tr:nth-last-child(2)').classList.contains('selected'));
  if (shots) await long.screenshot({ path: join(shots, 'app-long-list.png') });
  console.log('ok - 155 downloads scroll smoothly to the last one, the header stays put, rows update in place');
  await long.close();

  // Everything that can be taller than the window scrolls, at the smallest window size:
  // dialogs scroll their middle and keep their buttons in view; menus scroll; lists scroll.
  const small = await browser.newPage({ viewport: { width: 880, height: 540 } });
  small.on('pageerror', (e) => errors.push(e.message));
  await small.goto(url);
  await small.locator('#rows tr').first().waitFor();
  const scrollCheck = (name) => small.evaluate(async (name) => {
    const d = [...document.querySelectorAll('dialog[open]')].pop();
    const form = d.querySelector('.dialog');
    const vis = (el) => { const r = el.getBoundingClientRect(); return r.top >= 0 && r.bottom <= innerHeight && r.height > 0; };
    const fits = d.getBoundingClientRect().bottom <= innerHeight && d.getBoundingClientRect().top >= 0;
    const buttons = [...form.querySelectorAll(':scope > .actions .btn')].filter((b) => b.offsetParent).every(vis);
    form.scrollTop = form.scrollHeight;
    await new Promise((r) => requestAnimationFrame(r));
    // The last thing in the dialog body can be scrolled into view, above the buttons.
    const body = [...form.children].filter((c) => !c.matches('.actions, h2, .dialog-head') && c.offsetParent && c.getBoundingClientRect().height > 0);
    const last = body.at(-1);
    const actions = form.querySelector(':scope > .actions');
    const lastVisible = !last || last.getBoundingClientRect().bottom <= (actions?.getBoundingClientRect().top ?? innerHeight) + 1;
    form.scrollTop = 0;
    return { name, fits, buttons, lastVisible };
  }, name);
  const dialogs = [
    ['settings: general', async () => { await small.click('#btn-settings'); await small.locator('#dlg-settings[open]').waitFor(); }],
    ['settings: downloads', async () => { await small.click('#btn-settings'); await small.locator('#dlg-settings[open]').waitFor(); await small.click('.tab[data-tab="connection"]'); }],
    ['settings: privacy', async () => { await small.click('#btn-settings'); await small.locator('#dlg-settings[open]').waitFor(); await small.click('.tab[data-tab="privacy"]'); }],
    ['add: video formats', async () => { await small.click('#btn-add'); await small.fill('#add-url', 'https://video.example.com/watch?v=x'); await small.click('#add-check'); await small.locator('#add-formats:not([hidden])').waitFor(); }],
    ['browser: 30 links', async () => { await small.evaluate(() => window.__ldmBrowserDownload(Array.from({ length: 30 }, (_, i) => ({ url: `https://x.example/f${i}.zip` })), 'allLinks')); await small.locator('#dlg-browser[open]').waitFor(); }],
    ['grab', async () => { await small.click('#btn-more'); await small.click('#btn-grab'); }],
  ];
  for (const [name, open] of dialogs) {
    await open();
    await small.waitForTimeout(350);
    assert.deepEqual(await scrollCheck(name), { name, fits: true, buttons: true, lastVisible: true });
    await small.keyboard.press('Escape');
    await small.locator('dialog[open]').waitFor({ state: 'detached' });
  }
  // The settings sections can be reached by scrolling, and the type lists are there.
  await small.click('#btn-settings');
  await small.locator('#dlg-settings[open]').waitFor();
  await small.click('.tab[data-tab="connection"]');
  await small.locator('#exts-programs').scrollIntoViewIfNeeded();
  assert.ok(await small.locator('#exts-programs').isVisible());
  await small.keyboard.press('Escape');
  // A long right-click menu scrolls and stays inside the window.
  await small.evaluate(async () => { const bk = await import('./backend.js'); for (let i = 1; i <= 14; i++) await bk.invoke('queue_create', { name: `Queue ${i}`, maxActive: 2 }); });
  await small.waitForTimeout(200);
  await small.locator('#rows tr').first().click({ button: 'right' });
  await small.locator('#menu-row:not([hidden])').waitFor();
  await small.waitForTimeout(250);
  const menu = await small.evaluate(() => { const m = document.getElementById('menu-row'); const r = m.getBoundingClientRect(); m.scrollTop = m.scrollHeight; const last = m.lastElementChild.getBoundingClientRect(); return { inside: r.top >= 0 && r.bottom <= innerHeight, scrolls: m.scrollHeight > m.clientHeight, lastReachable: last.bottom <= r.bottom + 1 }; });
  assert.deepEqual(menu, { inside: true, scrolls: true, lastReachable: true });
  await small.keyboard.press('Escape');
  // The side bar (with many queues) and the details panel scroll.
  assert.ok(await small.evaluate(() => { const n = document.getElementById('nav'); n.scrollTop = n.scrollHeight; return n.scrollTop > 0; }), 'side bar scrolls');
  await small.locator('#rows tr').first().click();
  assert.ok(await small.evaluate(() => { const b = document.getElementById('details-body'); b.scrollTop = b.scrollHeight; return b.scrollTop > 0; }), 'details scroll');
  await small.close();
  console.log(`ok - at the smallest window size every dialog, menu and panel that's taller than the window scrolls, with its buttons in view (${dialogs.length} dialogs)`);

  // File types: each type's extensions can be edited; only changed lists are saved.
  const ft = await browser.newPage({ viewport: { width: 1280, height: 800 } });
  await ft.goto(url);
  await ft.locator('#rows tr').first().waitFor();
  await ft.click('#btn-settings');
  await ft.locator('#dlg-settings[open]').waitFor();
  await ft.click('.tab[data-tab="connection"]');
  assert.equal(await ft.locator('#type-exts textarea').count(), 5);
  assert.match(await ft.inputValue('#exts-compressed'), /\biso\b/);
  await ft.fill('#exts-compressed', 'zip rar 7z tar gz img dmg');
  await ft.fill('#exts-programs', 'exe msi msix apk jar deb rpm .ISO');
  await ft.locator('#exts-programs').scrollIntoViewIfNeeded();
  if (shots) await ft.screenshot({ path: join(shots, 'app-settings-file-types.png') });
  await ft.click('#settings-save');
  const s3 = await ft.evaluate(() => import('./backend.js').then((b) => b.invoke('get_settings')));
  assert.deepEqual(s3.categoryExtensions, { compressed: ['zip', 'rar', '7z', 'tar', 'gz', 'img', 'dmg'], programs: ['exe', 'msi', 'msix', 'apk', 'jar', 'deb', 'rpm', 'iso'] });
  await ft.click('#btn-settings');
  await ft.locator('#dlg-settings[open]').waitFor();
  await ft.click('.tab[data-tab="connection"]');
  await ft.locator('#type-exts .type-ext', { hasText: 'Programs' }).getByRole('button', { name: 'Reset' }).click();
  await ft.click('#settings-save');
  const s4 = await ft.evaluate(() => import('./backend.js').then((b) => b.invoke('get_settings')));
  assert.deepEqual(Object.keys(s4.categoryExtensions), ['compressed'], 'a list put back to the built-in one is not saved');
  await ft.close();
  console.log('ok - the file extensions of each type can be edited (e.g. .iso moved to Programs) and reset');

  // A download's window: live progress, one-line form, Pause; and the "done" form.
  const pw = await browser.newPage({ viewport: { width: 460, height: 360 } });
  pw.on('pageerror', (e) => errors.push(e.message));
  await pw.goto(`${base}progress.html?mock&id=1`);
  await pw.locator('#name', { hasText: 'ubuntu' }).waitFor();
  const p1 = await pw.locator('#pct').innerText();
  await pw.waitForFunction((p) => document.getElementById('pct').textContent !== p, p1);
  await pw.locator('#sections', { hasText: 'of 16' }).waitFor();
  if (shots) await pw.screenshot({ path: join(shots, 'window-progress.png') });
  const fullH = await pw.evaluate(() => document.getElementById('win').getBoundingClientRect().height);
  await pw.click('#toggle');
  const smallH = await pw.evaluate(() => document.getElementById('win').getBoundingClientRect().height);
  assert.ok(smallH < fullH / 2, `${fullH} -> ${smallH}`);
  await pw.waitForFunction((h) => window.__ldmCalls.some((c) => c.cmd === 'window_size' && Math.abs(c.args.height - h) < 2), smallH);
  if (shots) await pw.screenshot({ path: join(shots, 'window-progress-compact.png'), clip: { x: 0, y: 0, width: 460, height: Math.ceil(smallH) } });
  await pw.click('#pause');
  await pw.locator('#status', { hasText: 'Paused' }).waitFor();
  await pw.locator('#pause', { hasText: 'Continue' }).waitFor();
  await pw.click('#toggle');
  // The same link came in again: the existing download's window asks what to do.
  await pw.goto(`${base}progress.html?mock&id=4`);
  await pw.locator('#done:not([hidden])').waitFor();
  await pw.evaluate(() => window.__ldmDuplicate(4, { url: 'https://files.example.net/report-q3.pdf' }));
  await pw.locator('#dup:not([hidden])', { hasText: 'You\'ve downloaded this before' }).waitFor();
  await pw.waitForTimeout(900);
  if (shots) await pw.screenshot({ path: join(shots, 'window-duplicate.png') });
  await pw.check('#dup-remember');
  await pw.click('#dup-again');
  await pw.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'resolve_duplicate' && c.args.choice === 'again' && c.args.remember));
  await pw.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'window_close'));
  console.log('ok - when a link comes in again, its download\'s window offers Open it / Download again (and can remember)');

  // "Ask first" on: a browser download's own small window asks (name, folder, Start) while
  // it already downloads, as IDM's "Download File Info"; the app itself stays in the tray.
  await pw.goto(`${base}progress.html?mock&pending&id=6`);
  await pw.locator('#confirm:not([hidden])').waitFor();
  assert.equal(await pw.inputValue('#c-name'), 'studio-setup-2.4.exe');
  await pw.locator('#c-line', { hasText: 'Already downloading in the background' }).waitFor();
  await pw.waitForTimeout(700);
  if (shots) await pw.screenshot({ path: join(shots, 'window-confirm.png') });
  await pw.fill('#c-name', 'Studio 2.4 setup.exe');
  await pw.click('#c-start');
  await pw.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'confirm_download' && c.args.id === 6 && c.args.args.fileName === 'Studio 2.4 setup.exe' && c.args.args.start));
  await pw.locator('#running:not([hidden])').waitFor();
  await pw.locator('#name', { hasText: 'Studio 2.4 setup.exe' }).waitFor();
  for (const [button, check] of [['#c-later', (c) => c.cmd === 'confirm_download' && c.args.args.start === false], ['#c-cancel', (c) => c.cmd === 'discard_download']]) {
    await pw.goto(`${base}progress.html?mock&pending&id=6`);
    await pw.locator('#confirm:not([hidden])').waitFor();
    await pw.click(button);
    await pw.waitForFunction((src) => window.__ldmCalls.some(eval(src)), check.toString());
    await pw.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'window_close'));
  }
  console.log('ok - a new download\'s own window asks Start / Download later / Cancel while it already downloads, without the app');

  // An expired download's window offers Refresh link, handled by the main window.
  await pw.goto(`${base}progress.html?mock&id=5`);
  await pw.locator('#pause', { hasText: 'Refresh link' }).click();
  await pw.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'request_refresh' && c.args.id === 5));
  await pw.goto(`${base}progress.html?mock&id=1`);
  await pw.locator('#name', { hasText: 'ubuntu' }).waitFor();
  console.log(`ok - a download's window shows live progress, shrinks to one line (${Math.round(fullH)} -> ${Math.round(smallH)}px) and pauses`);
  await pw.goto(`${base}progress.html?mock&id=4`);
  await pw.locator('#done:not([hidden])', { hasText: 'Download complete' }).waitFor();
  await pw.locator('#done-name', { hasText: 'report-q3.pdf' }).waitFor();
  await pw.waitForTimeout(600);
  if (shots) await pw.screenshot({ path: join(shots, 'window-done.png') });
  await pw.check('#dont-show');
  await pw.waitForFunction(() => import('./backend.js').then((b) => b.invoke('get_settings')).then((s) => s.showCompleteWindow === false));
  console.log('ok - a finished download\'s window offers Open and Show in folder, and can be turned off from there');
  await pw.close();

  // The mini window.
  const mini = await browser.newPage({ viewport: { width: 320, height: 240 } });
  mini.on('pageerror', (e) => errors.push(e.message));
  await mini.goto(`${base}mini.html?mock`);
  await mini.locator('#summary', { hasText: '1 downloading' }).waitFor();
  await mini.click('#expand');
  await mini.locator('#items li', { hasText: 'ubuntu' }).waitFor();
  await mini.waitForTimeout(500);
  if (shots) await mini.screenshot({ path: join(shots, 'window-mini.png'), clip: { x: 0, y: 0, width: 320, height: Math.ceil(await mini.evaluate(() => document.getElementById('win').getBoundingClientRect().height)) } });
  await mini.locator('#items li button').first().click();
  await mini.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'open_download_window' && c.args.id === 1));
  await mini.click('#dismiss');
  await mini.waitForFunction(() => window.__ldmCalls.some((c) => c.cmd === 'mini_dismiss'));
  console.log('ok - the mini window sums up what\'s downloading, lists each one, and opens its window');

  // No pills in the small windows either.
  for (const [p, path] of [[mini, 'mini.html?mock'], [mini, 'progress.html?mock&id=1']]) {
    await p.goto(base + path);
    await p.waitForTimeout(400);
    const pills = await p.evaluate(() => [...document.querySelectorAll('body *')].filter((el) => {
      const r = el.getBoundingClientRect();
      if (r.width < 4 || r.height < 4) return false;
      const cs = getComputedStyle(el);
      const radius = Math.max(...['TopLeft', 'TopRight', 'BottomLeft', 'BottomRight'].map((c) => parseFloat(cs[`border${c}Radius`]) || 0));
      return radius > 0 && radius >= Math.min(r.width, r.height) / 2 - 0.5;
    }).map((el) => el.className));
    assert.deepEqual(pills, [], `pill-shaped elements in ${path}`);
  }
  console.log('ok - no pill shapes in the download and mini windows');
  await mini.close();
  assert.deepEqual(errors, [], 'no errors in the pages');
}
