// Builds dist/layos-advanced-blocker-<version>.zip, ready to upload to the
// Chrome Web Store or Microsoft Edge Add-ons. Only the files the extension needs go in.
// Run with: npm run package

import { readFileSync, writeFileSync, readdirSync, statSync, mkdirSync } from 'node:fs';
import { join, relative, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';
import { deflateRawSync, crc32 } from 'node:zlib';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const INCLUDE = ['manifest.json', 'LICENSE', 'icons', 'rules', 'src'];

function walk(path) {
  const full = join(root, path);
  if (statSync(full).isDirectory()) return readdirSync(full).flatMap((f) => walk(join(path, f)));
  return [path];
}

const files = INCLUDE.flatMap(walk).map((f) => relative(root, join(root, f)).split('\\').join('/')).sort();
const version = JSON.parse(readFileSync(join(root, 'manifest.json'), 'utf8')).version;

// A minimal zip writer (deflate, no extras) so the project needs no dependencies.
const local = [];
const central = [];
let offset = 0;
for (const name of files) {
  const data = readFileSync(join(root, name));
  const packed = deflateRawSync(data, { level: 9 });
  const nameBuf = Buffer.from(name, 'utf8');
  const crc = crc32(data);

  const header = Buffer.alloc(30);
  header.writeUInt32LE(0x04034b50, 0);
  header.writeUInt16LE(20, 4);
  header.writeUInt16LE(0x0800, 6);
  header.writeUInt16LE(8, 8);
  header.writeUInt32LE(crc, 14);
  header.writeUInt32LE(packed.length, 18);
  header.writeUInt32LE(data.length, 22);
  header.writeUInt16LE(nameBuf.length, 26);
  local.push(header, nameBuf, packed);

  const entry = Buffer.alloc(46);
  entry.writeUInt32LE(0x02014b50, 0);
  entry.writeUInt16LE(20, 4);
  entry.writeUInt16LE(20, 6);
  entry.writeUInt16LE(0x0800, 8);
  entry.writeUInt16LE(8, 10);
  entry.writeUInt32LE(crc, 16);
  entry.writeUInt32LE(packed.length, 20);
  entry.writeUInt32LE(data.length, 24);
  entry.writeUInt16LE(nameBuf.length, 28);
  entry.writeUInt32LE(offset, 42);
  central.push(entry, nameBuf);

  offset += header.length + nameBuf.length + packed.length;
}
const centralSize = central.reduce((n, b) => n + b.length, 0);
const end = Buffer.alloc(22);
end.writeUInt32LE(0x06054b50, 0);
end.writeUInt16LE(files.length, 8);
end.writeUInt16LE(files.length, 10);
end.writeUInt32LE(centralSize, 12);
end.writeUInt32LE(offset, 16);

mkdirSync(join(root, 'dist'), { recursive: true });
const out = join(root, 'dist', `layos-advanced-blocker-${version}.zip`);
writeFileSync(out, Buffer.concat([...local, ...central, end]));
console.log(`Packed ${files.length} files into ${relative(root, out)}`);
