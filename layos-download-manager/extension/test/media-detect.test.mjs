import { test } from 'node:test';
import assert from 'node:assert/strict';
import { classify, estimateSize, parseHls, sizeFromHeaders, variantLabel } from '../src/background/media-detect.js';

test('spots streams and whole media files', () => {
  assert.deepEqual(classify({ url: 'https://cdn.x/live/master.m3u8?token=1' }), { kind: 'hls', ext: 'm3u8' });
  assert.deepEqual(classify({ url: 'https://cdn.x/p', mime: 'application/vnd.apple.mpegURL' }), { kind: 'hls', ext: 'm3u8' });
  assert.deepEqual(classify({ url: 'https://cdn.x/manifest.mpd' }), { kind: 'dash', ext: 'mpd' });
  assert.deepEqual(classify({ url: 'https://cdn.x/clip.mp4', size: 5e6 }), { kind: 'file', ext: 'mp4' });
  assert.deepEqual(classify({ url: 'https://cdn.x/get?id=1', mime: 'video/webm', size: 9e6 }), { kind: 'file', ext: 'webm' });
  assert.deepEqual(classify({ url: 'https://cdn.x/a', mime: 'audio/mpeg' }), { kind: 'file', ext: 'mp3' });
});

test('ignores pieces of streams, previews and non-media', () => {
  assert.equal(classify({ url: 'https://cdn.x/seg_001.ts', mime: 'video/mp2t' }), null);
  assert.equal(classify({ url: 'https://cdn.x/chunk.m4s' }), null);
  assert.equal(classify({ url: 'https://rr1.googlevideo.com/videoplayback?itag=22&range=0-9999', mime: 'video/mp4' }), null);
  // Facebook and Instagram stream pieces.
  assert.equal(classify({ url: 'https://video.fxyz1-1.fna.fbcdn.net/o1/v/t2/f2/m69/abc.mp4?efg=x&bytestart=0&byteend=8191', mime: 'video/mp4' }), null);
  assert.equal(classify({ url: 'https://scontent.cdninstagram.com/v/t50/abc.mp4?_nc_ht=x&bytestart=966&byteend=200000', mime: 'video/mp4' }), null);
  assert.equal(classify({ url: 'https://cdn.x/preview.mp4', size: 50_000 }), null);
  assert.equal(classify({ url: 'https://cdn.x/page.html', mime: 'text/html' }), null);
  assert.equal(classify({ url: 'blob:https://x/123', mime: 'video/mp4' }), null);
});

test('reads sizes from headers', () => {
  assert.equal(sizeFromHeaders([{ name: 'Content-Range', value: 'bytes 0-99/123456' }, { name: 'content-length', value: '100' }]), 123456);
  assert.equal(sizeFromHeaders([{ name: 'Content-Length', value: '4096' }]), 4096);
  assert.equal(sizeFromHeaders([]), undefined);
});

test('reads HLS qualities, best first, one per height', () => {
  const master = `#EXTM3U
#EXT-X-STREAM-INF:BANDWIDTH=800000,RESOLUTION=640x360,CODECS="avc1.4d401e,mp4a.40.2"
low/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=2400000,RESOLUTION=1280x720
https://other.cdn/hd/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=700000,RESOLUTION=640x360
low-alt/index.m3u8
#EXT-X-STREAM-INF:BANDWIDTH=64000,CODECS="mp4a.40.2"
audio.m3u8`;
  const r = parseHls(master, 'https://cdn.x/show/master.m3u8');
  assert.equal(r.isMaster, true);
  assert.equal(r.protected, false);
  assert.deepEqual(r.variants.map((v) => [v.height, v.url]), [
    [720, 'https://other.cdn/hd/index.m3u8'],
    [360, 'https://cdn.x/show/low/index.m3u8'],
    [null, 'https://cdn.x/show/audio.m3u8'],
  ]);
  assert.equal(variantLabel(r.variants[0]), '720p');
  assert.equal(variantLabel(r.variants[2]), '64 kbit/s');
});

test('media playlists and protection', () => {
  const media = '#EXTM3U\n#EXT-X-KEY:METHOD=AES-128,URI="k.bin"\n#EXTINF:4,\nseg1.ts\n#EXT-X-ENDLIST';
  assert.deepEqual(parseHls(media, 'https://x/a.m3u8'), { isMaster: false, variants: [], protected: false });
  const drm = '#EXTM3U\n#EXT-X-KEY:METHOD=SAMPLE-AES,URI="skd://key",KEYFORMAT="com.apple.streamingkeydelivery"\nseg1.ts';
  assert.equal(parseHls(drm, 'https://x/a.m3u8').protected, true);
  assert.equal(parseHls('<html>', 'https://x'), null);
  assert.equal(estimateSize(800_000, 60), 6_000_000);
  assert.equal(estimateSize(0, 60), null);
});
