// End-to-end: a real Chromium with the extension, the real bridge program and the
// headless download manager. Clicks download links on a page and checks who saved them.
//
// Run from the extension folder after building the Rust programs:
//   cargo build -p ldm-native-host -p ldm-service      (in layos-download-manager/)
//   node tools/build.mjs && node test/e2e/capture.e2e.mjs

import { createServer } from 'node:http';
import { execFileSync, spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import assert from 'node:assert/strict';

// Playwright may be installed globally, which only require() finds.
const { chromium } = createRequire(import.meta.url)('playwright');

const EXTENSION_ID = 'klknhohjiaanbbicfdonmcnflkgjofai';
const ext = resolve('dist/chromium');
const target = resolve('../target/debug');
const host = join(target, 'ldm-native-host');
const headless = join(target, 'ldm-headless');
for (const f of [ext, host, headless]) assert.ok(existsSync(f), `missing ${f}`);

const work = mkdtempSync(join(tmpdir(), 'ldm-e2e-'));
const dataDir = join(work, 'data');
const ipc = join(work, 'ldm.sock');
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const saved = (rel) => join(dataDir, 'Downloads', rel);

// ---- A web site with downloads behind a login cookie. Files support byte ranges.
function bytes(n, seed) {
  const b = Buffer.alloc(n);
  let x = seed;
  for (let i = 0; i < n; i++) { x = (x * 1103515245 + 12345) >>> 0; b[i] = x >>> 24; }
  return b;
}
const files = {
  '/files/big.zip': bytes(6 * 1024 * 1024, 1),
  '/files/manual.zip': bytes(300 * 1024, 2),
  '/files/notes.txt': Buffer.from('just text\n'),
  '/get?id=7': bytes(2 * 1024 * 1024, 3),
  '/files/slow.zip': bytes(2 * 1024 * 1024, 4),
  '/files/paced.iso': bytes(12 * 1024 * 1024, 5),
};
// Responses for these trickle out so there's time to pause them.
const PACED = new Set(['/files/paced.iso']);
let rangeRequests = 0;
let pacing = true;
const PAGE = `<!doctype html><title>Files</title>
<a id="big" href="/files/big.zip">big.zip</a>
<a id="video" href="/get?id=7">video via script</a>
<a id="notes" href="/files/notes.txt" download>notes.txt</a>
<a id="manual" href="/files/manual.zip">manual.zip</a>
<a id="slow" href="/files/slow.zip">slow.zip</a>
<a id="paced" href="/files/paced.iso">paced.iso</a>
<script>
  // With ?auto, click the links one after another like a user would.
  if (location.search === '?auto') (async () => {
    const wait = (ms) => new Promise((r) => setTimeout(r, ms));
    await wait(1500);
    for (const id of ['big', 'video', 'notes']) { document.getElementById(id).click(); await wait(1200); }
    // Hold Alt (the bypass key) while clicking.
    const manual = document.getElementById('manual');
    manual.dispatchEvent(new MouseEvent('mousedown', { bubbles: true, altKey: true }));
    manual.click();
  })();
</script>`;

const site = createServer(async (req, res) => {
  if (req.url === '/' || req.url === '/?auto') {
    res.setHeader('content-type', 'text/html');
    res.setHeader('set-cookie', 'session=e2e; Path=/');
    res.end(PAGE);
    return;
  }
  const body = files[req.url];
  if (!body) { res.statusCode = 404; res.end(); return; }
  if (!/(^|; )session=e2e(;|$)/.test(req.headers.cookie ?? '')) { res.statusCode = 403; res.end('login needed'); return; }
  res.setHeader('accept-ranges', 'bytes');
  res.setHeader('content-type', req.url.startsWith('/get') ? 'video/mp4' : req.url.endsWith('.txt') ? 'text/plain' : 'application/zip');
  res.setHeader('content-disposition', req.url.startsWith('/get') ? 'attachment' : `attachment; filename="${req.url.split('/').pop()}"`);
  let a = 0;
  let b = body.length - 1;
  const m = /bytes=(\d+)-(\d*)/.exec(req.headers.range ?? '');
  if (m) {
    rangeRequests++;
    a = Number(m[1]);
    b = m[2] ? Math.min(Number(m[2]), body.length - 1) : b;
    res.statusCode = 206;
    res.setHeader('content-range', `bytes ${a}-${b}/${body.length}`);
  }
  res.setHeader('content-length', b - a + 1);
  const part = body.subarray(a, b + 1);
  if ((req.url === '/files/slow.zip' && !m) || (PACED.has(req.url) && pacing)) {
    // The browser's own request trickles, so it's still running when the backup hook looks.
    for (let i = 0; i < part.length && !res.destroyed; i += 64 * 1024) {
      res.write(part.subarray(i, i + 64 * 1024));
      await sleep(250);
    }
    res.end();
  } else {
    res.end(part);
  }
});
await new Promise((r) => site.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${site.address().port}`;

const env = { ...process.env, LDM_IPC_PATH: ipc, LDM_APP_PATH: headless, LDM_DATA_DIR: dataDir };

async function waitFor(path, expected, ms = 30_000) {
  for (const end = Date.now() + ms; Date.now() < end; await sleep(100)) {
    if (existsSync(path) && statSync(path).size === expected.length && readFileSync(path).equals(expected)) return true;
  }
  return false;
}

/** A profile that saves browser downloads to `downloads` without asking. */
function makeProfile(name, downloads) {
  const dir = join(work, name);
  mkdirSync(join(dir, 'Default'), { recursive: true });
  writeFileSync(join(dir, 'Default', 'Preferences'), JSON.stringify({
    download: { default_directory: downloads, prompt_for_download: false, directory_upgrade: true },
    savefile: { default_directory: downloads },
    // The test page clicks links from script; let it start more than one download.
    profile: { default_content_setting_values: { automatic_downloads: 1 } },
  }));
  execFileSync(host, ['--register', '--user-data-dir', dir]);
  return dir;
}

let failed = false;
const cleanups = [];
try {
  // ---- Part 1: a browser used normally, nothing automating it.
  // The extension's main hook (downloads.onDeterminingFilename) does the work.
  const browserDir = join(work, 'browser-downloads');
  const profile = makeProfile('profile', browserDir);
  const chrome = spawn(chromium.executablePath(), [
    '--headless=new', '--no-sandbox', '--no-first-run', '--no-default-browser-check',
    `--user-data-dir=${profile}`, `--disable-extensions-except=${ext}`, `--load-extension=${ext}`,
    `${base}/?auto`,
  ], { env, stdio: 'ignore' });
  cleanups.push(() => chrome.kill());

  assert.ok(await waitFor(saved('Compressed/big.zip'), files['/files/big.zip']), 'big.zip should be saved by the manager');
  assert.ok(rangeRequests > 1, `expected several section requests, saw ${rangeRequests}`);
  console.log(`ok - big.zip taken over and downloaded in sections (${rangeRequests} range requests) with the login cookie`);

  let video;
  for (const end = Date.now() + 20_000; Date.now() < end && !video; await sleep(100)) {
    video = existsSync(saved('Video')) && readdirSync(saved('Video')).find((f) => !f.endsWith('.ldmpart'));
  }
  assert.ok(video, 'video should be saved by the manager');
  assert.ok(await waitFor(saved(join('Video', video)), files['/get?id=7']));
  console.log(`ok - video/mp4 with no extension in the address taken over as ${video}`);

  assert.ok(await waitFor(join(browserDir, 'notes.txt'), files['/files/notes.txt'], 15_000), 'browser should save notes.txt');
  assert.ok(await waitFor(join(browserDir, 'manual.zip'), files['/files/manual.zip'], 15_000), 'browser should save manual.zip');
  await sleep(1500);
  assert.ok(!existsSync(saved('Documents/notes.txt')) && !existsSync(saved('General/notes.txt')));
  assert.ok(!existsSync(saved('Compressed/manual.zip')), 'Alt+click must not be taken over');
  console.log('ok - notes.txt (not in the type list) and Alt+click manual.zip stayed in the browser');

  const inBrowser = readdirSync(browserDir).filter((f) => !f.endsWith('.crdownload')).sort();
  assert.deepEqual(inBrowser, ['manual.zip', 'notes.txt'], `browser folder holds: ${inBrowser}`);
  console.log('ok - taken-over files never reached the browser\'s download folder');
  chrome.kill();
  await sleep(500);

  // ---- Part 2: a browser whose downloads are controlled by automation. Chrome skips the
  // file-name step there, so the backup hook (downloads.onCreated) must take it over.
  const profile2 = makeProfile('profile2', join(work, 'browser-downloads-2'));
  const managed = await chromium.launchPersistentContext(profile2, {
    channel: 'chromium', headless: true, env, acceptDownloads: true,
    args: [`--disable-extensions-except=${ext}`, `--load-extension=${ext}`],
  });
  cleanups.push(() => managed.close());
  const ours = (w) => w.url().startsWith(`chrome-extension://${EXTENSION_ID}/`);
  const worker = managed.serviceWorkers().find(ours) ?? await managed.waitForEvent('serviceworker', { predicate: ours });
  const page = await managed.newPage();
  await page.goto(base);
  await page.click('#slow');
  assert.ok(await waitFor(saved('Compressed/slow.zip'), files['/files/slow.zip']), 'backup hook should take slow.zip over');
  const list = await worker.evaluate(async () => (await chrome.downloads.search({})).length);
  assert.equal(list, 0, 'the browser\'s copy should be cancelled and erased');
  console.log('ok - backup hook takes over a download when automation controls the browser');

  // ---- Part 3: pause and continue from the toolbar pop-up.
  await page.click('#paced');
  const popup = await managed.newPage();
  await popup.setViewportSize({ width: 360, height: 640 });
  await popup.goto(`chrome-extension://${EXTENSION_ID}/popup/popup.html`);
  const item = popup.locator('.dl', { hasText: 'paced.iso' });
  await item.getByRole('button', { name: 'Pause' }).waitFor({ timeout: 20_000 });
  await popup.waitForFunction(() => /MB\/s|KB\/s/.test(document.querySelector('.dl .meta')?.textContent ?? ''), null, { timeout: 10_000 });
  const shots = process.env.SCREENSHOT_DIR;
  if (shots) await popup.screenshot({ path: join(shots, 'popup-downloading.png') });
  await item.getByRole('button', { name: 'Pause' }).click();
  await item.getByRole('button', { name: 'Continue' }).waitFor({ timeout: 10_000 });
  const pausedText = await item.locator('.meta').textContent();
  assert.match(pausedText, /^Paused · [\d.]+ (KB|MB) of 12 MB/);
  await sleep(1500);
  assert.equal(await item.locator('.meta').textContent(), pausedText, 'progress must not move while paused');
  assert.ok(existsSync(saved('Compressed/paced.iso.ldmpart')), 'the part file is kept while paused');
  if (shots) await popup.screenshot({ path: join(shots, 'popup-paused.png') });
  console.log(`ok - Pause in the pop-up stops the download and keeps its progress (${pausedText})`);

  pacing = false;
  const before = rangeRequests;
  await item.getByRole('button', { name: 'Continue' }).click();
  assert.ok(await waitFor(saved('Compressed/paced.iso'), files['/files/paced.iso']), 'continue should finish the file');
  assert.ok(rangeRequests > before, 'continuing asks only for the missing parts');
  await popup.locator('.dl.completed', { hasText: 'paced.iso' }).waitFor({ timeout: 5_000 });
  console.log('ok - Continue finishes the download from where it stopped, byte-for-byte correct');
} catch (e) {
  failed = true;
  console.error('FAILED:', e);
  console.error(spawnSync('find', [work, '-path', '*/profile*', '-prune', '-o', '-type', 'f', '-print']).stdout.toString());
} finally {
  for (const c of cleanups.reverse()) await Promise.resolve(c()).catch(() => {});
  site.close();
  spawnSync('pkill', ['-f', `${headless} --from-browser`]);
  execFileSync(host, ['--unregister']);
  await sleep(500);
  try { rmSync(work, { recursive: true, force: true, maxRetries: 5 }); } catch { /* browser still closing */ }
}
process.exit(failed ? 1 : 0);
