// Interface tests: opens the main window and the picker in Chromium with made-up data
// (bridge.js uses it when there's no app around) and checks the main flows.
// Run: node test/ui.e2e.mjs   (needs Playwright; SHOTS=<folder> also saves screenshots)
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { extname, join, normalize } from 'node:path';
import { fileURLToPath } from 'node:url';
import assert from 'node:assert/strict';
import { chromium } from 'playwright';

const root = fileURLToPath(new URL('../ui/', import.meta.url));
const types = { '.html': 'text/html', '.js': 'text/javascript', '.css': 'text/css', '.svg': 'image/svg+xml', '.png': 'image/png', '.woff2': 'font/woff2' };
const server = createServer(async (req, res) => {
  const path = normalize(decodeURIComponent(new URL(req.url, 'http://x').pathname)).replace(/^([/\\])+/, '');
  try {
    const body = await readFile(join(root, path || 'index.html'));
    res.writeHead(200, { 'Content-Type': types[extname(path)] ?? 'application/octet-stream' });
    res.end(body);
  } catch {
    res.writeHead(404).end();
  }
});
await new Promise((r) => server.listen(0, '127.0.0.1', r));
const base = `http://127.0.0.1:${server.address().port}`;
const shots = process.env.SHOTS;
const browser = await chromium.launch();
let failed = 0;

async function page(path, { dark = false, width = 440, height = 660 } = {}) {
  const ctx = await browser.newContext({ viewport: { width, height }, colorScheme: dark ? 'dark' : 'light' });
  const p = await ctx.newPage();
  const errors = [];
  p.on('pageerror', (e) => errors.push(e.message));
  p.on('console', (m) => m.type() === 'error' && errors.push(m.text()));
  await p.goto(base + path);
  await p.waitForTimeout(400);
  p.errors = errors;
  return p;
}

async function test(name, fn) {
  try {
    await fn();
    console.log(`ok   ${name}`);
  } catch (e) {
    failed++;
    console.log(`FAIL ${name}\n     ${e.message}`);
  }
}

const snap = async (p, name) => shots && p.screenshot({ path: join(shots, `${name}.png`) });

await test('sign in with an emailed code', async () => {
  const p = await page('/index.html?state=signin');
  assert.equal(await p.isVisible('#view-signin'), true);
  await snap(p, 'signin-email');
  await p.fill('#email', 'not-an-email');
  await p.click('#btn-send');
  await p.waitForSelector('#email-error:not(:empty)');
  await p.fill('#email', 'Layo@Example.com');
  await p.click('#btn-send');
  await p.waitForSelector('#form-code:not([hidden])');
  assert.equal(await p.textContent('#sent-to'), 'layo@example.com');
  assert.match(await p.textContent('#btn-resend'), /\(\d+s\)/);
  await snap(p, 'signin-code');
  await p.fill('#code', '111111');
  await p.waitForSelector('#code-error:not(:empty)');
  await p.fill('#code', '');
  await p.type('#code', '123 456');
  await p.waitForSelector('#view-clips:not([hidden])');
  assert.deepEqual(p.errors, []);
});

await test('clips list: filter, search, copy, delete', async () => {
  const p = await page('/index.html');
  await p.waitForSelector('.clip');
  assert.equal(await p.locator('.clip').count(), 6);
  assert.equal(await p.textContent('#status'), 'Syncing');
  await snap(p, 'clips-light');
  await p.click('[data-filter="image"]');
  assert.equal(await p.locator('.clip').count(), 1);
  await p.click('[data-filter="all"]');
  await p.fill('#search', 'parking');
  assert.equal(await p.locator('.clip').count(), 1);
  await p.fill('#search', 'zzz');
  assert.equal(await p.isVisible('#empty'), true);
  await p.fill('#search', '');
  await p.locator('.clip-main').first().click();
  await p.waitForSelector('.toast.show');
  await p.locator('.clip').first().hover();
  await p.locator('.clip .del').first().click();
  await p.waitForFunction(() => document.querySelectorAll('.clip').length === 5);
  assert.deepEqual(p.errors, []);
});

await test('dark mode', async () => {
  const p = await page('/index.html', { dark: true });
  await p.waitForSelector('.clip');
  const bg = await p.evaluate(() => getComputedStyle(document.body).backgroundColor);
  assert.notEqual(bg, 'rgb(255, 255, 255)');
  await snap(p, 'clips-dark');
});

await test('empty, paused and offline states', async () => {
  let p = await page('/index.html?state=empty');
  await p.waitForSelector('#empty:not([hidden])');
  assert.match(await p.textContent('#empty'), /No clips yet/);
  assert.match(await p.textContent('#empty'), /Alt/);
  await snap(p, 'empty');
  p = await page('/index.html?state=paused');
  await p.waitForSelector('#banner:not([hidden])');
  assert.equal(await p.textContent('#status'), 'Paused');
  await p.click('#banner .btn');
  await p.waitForSelector('#banner', { state: 'hidden' });
  p = await page('/index.html?state=offline');
  assert.equal(await p.textContent('#status'), 'Offline');
  assert.match(await p.textContent('#banner'), /internet/);
  p = await page('/index.html?state=setup');
  assert.equal(await p.isVisible('#view-setup'), true);
});

await test('settings: theme, icon apps, shortcut, delete all, sign out', async () => {
  const p = await page('/index.html');
  await p.click('#btn-settings');
  await p.waitForSelector('#view-settings:not([hidden])');
  assert.equal(await p.inputValue('#set-device'), 'LAYO-DESKTOP');
  assert.equal(await p.locator('#hidden-apps .chip').count(), 2);
  await snap(p, 'settings');
  await p.click('#set-theme [data-theme="dark"]');
  await p.waitForFunction(() => document.documentElement.dataset.theme === 'dark');
  await snap(p, 'settings-dark');
  await p.click('#set-theme [data-theme="system"]');
  await p.waitForFunction(() => !('theme' in document.documentElement.dataset));
  await p.locator('#hidden-apps .chip button').first().click();
  await p.waitForFunction(() => document.querySelectorAll('#hidden-apps .chip').length === 1);
  await p.selectOption('#set-hotkey', 'Super+Alt+V');
  await p.waitForTimeout(200);
  assert.equal(await p.locator('#set-hotkey option:checked').textContent(), 'Win+Alt+V');
  await p.click('#btn-clear');
  await p.click('text=Delete everywhere');
  await p.click('#btn-back');
  await p.waitForSelector('#empty:not([hidden])');
  await p.click('#btn-settings');
  await p.click('#btn-signout');
  await p.waitForSelector('#view-signin:not([hidden])');
  assert.deepEqual(p.errors, []);
});

await test('picker: keys and search', async () => {
  const p = await page('/picker.html', { width: 360, height: 440 });
  await p.waitForSelector('.pk-item');
  assert.equal(await p.locator('.pk-item').count(), 6);
  assert.equal(await p.getAttribute('#clip-0', 'aria-selected'), 'true');
  await p.keyboard.press('ArrowDown');
  await p.keyboard.press('ArrowDown');
  assert.equal(await p.getAttribute('#clip-2', 'aria-selected'), 'true');
  await snap(p, 'picker-light');
  await p.keyboard.type('code');
  assert.equal(await p.locator('.pk-item').count(), 1);
  await p.keyboard.press('Enter');
  await p.fill('#q', 'nothing like this');
  assert.equal(await p.isVisible('#empty'), true);
  const d = await page('/picker.html', { width: 360, height: 440, dark: true });
  await d.waitForSelector('.pk-item');
  await snap(d, 'picker-dark');
  assert.deepEqual(p.errors, []);
});

await browser.close();
server.close();
if (failed) {
  console.log(`\n${failed} failed`);
  process.exit(1);
}
console.log('\nall interface tests passed');
