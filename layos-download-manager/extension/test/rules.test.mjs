import { test } from 'node:test';
import assert from 'node:assert/strict';

import {
  DEFAULT_SETTINGS, baseName, cookieHeader, decide, extensionOf, hostMatches, parseFileTypes, pickLinks,
} from '../src/background/rules.js';

const ready = { hostReady: true, ownExtensionId: 'me' };
const item = (over = {}) => ({
  url: 'https://dl.example.com/files/setup.exe',
  filename: 'setup.exe',
  mime: 'application/octet-stream',
  fileSize: 5_000_000,
  incognito: false,
  ...over,
});

test('takes over listed file types', () => {
  assert.equal(decide(item(), {}, ready).capture, true);
  assert.equal(decide(item({ filename: 'movie.MKV' }), {}, ready).capture, true);
  assert.equal(decide(item({ filename: 'notes.txt', url: 'https://x.com/notes.txt' }), {}, ready).capture, false);
});

test('uses the URL when the browser has no name yet', () => {
  assert.equal(decide(item({ filename: '' }), {}, ready).capture, true);
});

test('uses the content type when there is no extension', () => {
  const r = decide(item({ filename: 'download', url: 'https://x.com/get?id=3', mime: 'video/mp4' }), {}, ready);
  assert.equal(r.capture, true);
  const php = decide(item({ filename: 'get.php', url: 'https://x.com/get.php', mime: 'application/zip' }), {}, ready);
  assert.equal(php.capture, true);
  const html = decide(item({ filename: 'page', url: 'https://x.com/page', mime: 'text/html' }), {}, ready);
  assert.equal(html.capture, false);
});

test('never takes over what it cannot fetch', () => {
  assert.equal(decide(item({ url: 'blob:https://x.com/123' }), {}, ready).capture, false);
  assert.equal(decide(item({ url: 'data:text/plain,hi', filename: 'a.zip' }), {}, ready).capture, false);
  assert.equal(decide(item(), {}, { ...ready, method: 'POST' }).capture, false);
  assert.equal(decide(item(), {}, { hostReady: false }).capture, false);
});

test('respects the user', () => {
  assert.equal(decide(item(), { enabled: false }, ready).capture, false);
  assert.equal(decide(item(), {}, { ...ready, bypassed: true }).reason, 'bypass key held');
  assert.equal(decide(item({ incognito: true }), { captureIncognito: false }, ready).capture, false);
  assert.equal(decide(item({ fileSize: 10_000 }), { minSizeKB: 100 }, ready).capture, false);
  assert.equal(decide(item({ fileSize: -1 }), { minSizeKB: 100 }, ready).capture, true, 'unknown size is not too small');
  assert.equal(decide(item({ filename: 'a.txt', url: 'https://x/a.txt' }), { captureAll: true }, ready).capture, true);
});

test('skip list matches the site and its subdomains only', () => {
  assert.equal(decide(item(), { skipSites: 'example.com' }, ready).capture, false);
  assert.equal(decide(item(), { skipSites: 'https://*.example.com/path' }, ready).capture, false);
  assert.equal(decide(item(), { skipSites: 'other.com\nfoo.org' }, ready).capture, true);
  assert.equal(decide(item({ referrer: 'https://mail.google.com/x' }), { skipSites: 'google.com' }, ready).capture, false);
  assert.equal(hostMatches('badexample.com', 'example.com'), false);
});

test('does not loop on downloads it handed back', () => {
  assert.equal(decide(item({ byExtensionId: 'me' }), {}, ready).capture, false);
  assert.equal(decide(item({ byExtensionId: 'someone-else' }), {}, ready).capture, false);
  assert.equal(decide(item(), {}, { ...ready, letThrough: true }).capture, false);
});

test('helpers', () => {
  assert.equal(baseName('https://x.com/a/My%20File.zip?x=1#y'), 'My File.zip');
  assert.equal(baseName('C:\\Users\\me\\Downloads\\a.iso'), 'a.iso');
  assert.equal(extensionOf('archive.tar.GZ'), 'gz');
  assert.equal(extensionOf('.hidden'), '');
  assert.deepEqual([...parseFileTypes('ZIP, *.rar .7z\nexe')], ['zip', 'rar', '7z', 'exe']);
  assert.equal(cookieHeader([{ name: 'a', value: '1', path: '/' }, { name: 'b', value: '2', path: '/dl' }]), 'b=2; a=1');
  assert.ok(parseFileTypes(DEFAULT_SETTINGS.fileTypes).has('mp4'));
});

test('picks only file links, once each', () => {
  const links = [
    { url: 'https://x.com/a.zip' }, { url: 'https://x.com/a.zip#frag' }, { url: 'https://x.com/about' },
    { url: 'javascript:void(0)' }, { url: 'https://x.com/b.mp3', text: 'Song' },
  ];
  assert.deepEqual(pickLinks(links, {}).map((l) => l.url), ['https://x.com/a.zip', 'https://x.com/b.mp3']);
});
