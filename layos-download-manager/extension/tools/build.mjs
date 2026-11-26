// Builds the extension for each browser family into dist/.
//   dist/chromium  Chrome, Edge, Opera, Brave, Vivaldi
//   dist/firefox   Firefox 128+
// With --zip, also writes store-ready zips next to them.
// With --store, leaves out the development "key" (the stores give their own id).

import { cpSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { execFileSync } from 'node:child_process';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const src = join(root, 'src');
const dist = join(root, 'dist');
const zip = process.argv.includes('--zip');
const store = process.argv.includes('--store');

const base = JSON.parse(readFileSync(join(src, 'manifest.json'), 'utf8'));

// The colours are kept in one place (../brand/tokens.css); make sure the copy here matches.
const tokens = readFileSync(join(root, '..', 'brand', 'tokens.css'), 'utf8');
if (readFileSync(join(src, 'brand', 'tokens.css'), 'utf8') !== tokens) {
  writeFileSync(join(src, 'brand', 'tokens.css'), tokens);
  console.log('updated src/brand/tokens.css from ../brand/tokens.css');
}

function chromium() {
  const m = structuredClone(base);
  if (store) delete m.key;
  return m;
}

function firefox() {
  const m = structuredClone(base);
  delete m.key;
  delete m.minimum_chrome_version;
  // Firefox runs background scripts as an event page rather than a service worker.
  m.background = { scripts: ['background/main.js'], type: 'module' };
  m.browser_specific_settings = { gecko: { id: 'downloadmanager@layo.dev', strict_min_version: '128.0' } };
  return m;
}

rmSync(dist, { recursive: true, force: true });
for (const [name, manifest] of [['chromium', chromium()], ['firefox', firefox()]]) {
  const out = join(dist, name);
  mkdirSync(out, { recursive: true });
  cpSync(src, out, { recursive: true, filter: (p) => !p.endsWith('manifest.json') });
  writeFileSync(join(out, 'manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
  if (zip) {
    const file = join(dist, `layos-download-manager-${name}-${base.version}.zip`);
    if (process.platform === 'win32') {
      // Windows has no zip command; its own tar (bsdtar) writes zips with -a. List the
      // top-level entries so paths inside the zip don't start with "./".
      const tar = join(process.env.SystemRoot ?? 'C:\\Windows', 'System32', 'tar.exe');
      execFileSync(tar, ['-a', '-c', '-f', file, ...readdirSync(out)], { cwd: out });
    } else {
      execFileSync('zip', ['-qr', '-X', file, '.'], { cwd: out });
    }
    console.log(`wrote ${file}`);
  } else {
    console.log(`built ${out}`);
  }
}
