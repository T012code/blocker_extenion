// Turns the filter lists in lists/ into declarativeNetRequest rulesets in rules/.
// Run with: npm run build
//
// We only take the simple, domain-wide entries from each list (the "||example.com^" kind).
// Those map cleanly onto Chrome's requestDomains matching, which is fast and can hold
// thousands of domains in a single rule. Path-based and site-specific filters are skipped.

import { readFileSync, writeFileSync, mkdirSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const upstream = (name) => readFileSync(join(root, 'lists/upstream', name), 'utf8');
const custom = (name) => readFileSync(join(root, 'lists/custom', name), 'utf8');

const DOMAIN = /^(?=.{4,253}$)(?:[a-z0-9](?:[a-z0-9-]{0,61}[a-z0-9])?\.)+(?:[a-z]{2,63}|xn--[a-z0-9-]{2,59})$/;

// Every resource type except the page itself. Blocking main_frame would stop people
// from visiting a company's own website just because it also serves ads.
const SUBRESOURCES = [
  'sub_frame', 'stylesheet', 'script', 'image', 'font', 'object',
  'xmlhttprequest', 'ping', 'media', 'websocket', 'webtransport', 'webbundle', 'other',
];

const neverBlock = new Set(plainList(custom('never-block.txt')));

/** Lines of a plain "one domain per line" file (also understands hosts-file lines). */
function plainList(text) {
  const out = [];
  for (let line of text.split(/\r?\n/)) {
    line = line.replace(/#.*/, '').trim().toLowerCase();
    if (!line) continue;
    const parts = line.split(/\s+/);
    const host = parts.length > 1 ? parts[1] : parts[0];
    if (DOMAIN.test(host)) out.push(host);
  }
  return out;
}

/**
 * Pulls "||domain^" entries out of an Adblock-style list and sorts them into buckets by
 * their options. Anything with options we don't handle is ignored on purpose.
 */
function adblockList(text) {
  const buckets = { all: [], thirdParty: [], doc: [], popup: [] };
  for (const raw of text.split(/\r?\n/)) {
    const line = raw.trim();
    if (!line.startsWith('||') || line.includes('#')) continue;
    const m = line.match(/^\|\|([a-z0-9.-]+)\^(?:\$(.+))?$/i);
    if (!m) continue;
    const host = m[1].toLowerCase();
    if (!DOMAIN.test(host)) continue;
    const opts = (m[2] || '').toLowerCase();
    if (opts === '' || opts === 'all' || opts === 'important' || opts === 'all,important') buckets.all.push(host);
    else if (opts === 'third-party' || opts === '3p') buckets.thirdParty.push(host);
    else if (opts === 'doc' || opts === 'document') buckets.doc.push(host);
    else if (opts.startsWith('popup')) buckets.popup.push(host);
  }
  return buckets;
}

/** Removes duplicates, protected domains and any domain already covered by a parent domain. */
function tidy(domains, alsoCoveredBy = new Set()) {
  const set = new Set(domains.filter((d) => !neverBlock.has(d)));
  const covered = (d, pool) => {
    let i = d.indexOf('.');
    while (i !== -1) {
      const parent = d.slice(i + 1);
      if (!parent.includes('.')) break;
      if (pool.has(parent)) return true;
      i = d.indexOf('.', i + 1);
    }
    return false;
  };
  return [...set]
    .filter((d) => !alsoCoveredBy.has(d) && !covered(d, set) && !covered(d, alsoCoveredBy))
    .sort();
}

function blockRule(id, requestDomains, extra = {}) {
  return {
    id,
    priority: 1,
    action: { type: 'block' },
    condition: { requestDomains, resourceTypes: SUBRESOURCES, ...extra },
  };
}

function write(name, data) {
  const file = join(root, 'rules', name);
  mkdirSync(dirname(file), { recursive: true });
  const pretty = JSON.stringify(data, null, 1);
  writeFileSync(file, (pretty.length > 20000 ? JSON.stringify(data) : pretty) + '\n');
  if (!Array.isArray(data)) return;
  const isRules = typeof data[0] === 'object';
  const domains = isRules ? data.reduce((n, r) => n + (r.condition.requestDomains?.length || 0), 0) : data.length;
  console.log(`  ${name.padEnd(24)} ${isRules ? `${data.length} rules, ` : ''}${domains} domains`);
}

// ---------------------------------------------------------------- ads
const easylistAds = adblockList(upstream('easylist-adservers.txt'));
const easylistPopups = adblockList(upstream('easylist-adservers-popup.txt'));
const adguardAds = adblockList(upstream('adguard-basefilter-adservers.txt'));
const adguardAdsFirst = adblockList(upstream('adguard-basefilter-adservers_firstparty.txt'));

const adsAll = tidy([
  ...easylistAds.all, ...adguardAds.all, ...adguardAdsFirst.all, ...plainList(custom('ads.txt')),
]);
const adsAllSet = new Set(adsAll);
const adsThird = tidy([...easylistAds.thirdParty, ...adguardAds.thirdParty, ...adguardAdsFirst.thirdParty], adsAllSet);
const popupDomains = tidy([...easylistPopups.popup, ...adguardAds.popup]);

// YouTube serves its own ads from youtube.com, so domain lists can't catch them.
// These paths are only used for ads and ad measurement.
const youtubeAdPaths = [
  '||youtube.com/pagead/',
  '||youtube.com/api/stats/ads',
  '||youtube.com/ptracking',
  '||youtube.com/get_midroll_',
  '||youtube.com/pcs/activeview',
  '||youtube.com/youtubei/v1/player/ad_break',
  '||youtube-nocookie.com/pagead/',
];

write('ads.json', [
  blockRule(1, adsAll),
  blockRule(2, adsThird, { domainType: 'thirdParty' }),
  ...youtubeAdPaths.map((urlFilter, i) => ({
    id: 10 + i,
    priority: 1,
    action: { type: 'block' },
    condition: { urlFilter, resourceTypes: SUBRESOURCES },
  })),
]);
write('popup-domains.json', popupDomains);

// ---------------------------------------------------------------- trackers
const epFirst = adblockList(upstream('easyprivacy-trackingservers.txt'));
const epThird = adblockList(upstream('easyprivacy-trackingservers-thirdparty.txt'));
const agTrack = adblockList(upstream('adguard-spywarefilter-tracking_servers.txt'));
const agTrackFirst = adblockList(upstream('adguard-spywarefilter-tracking_servers_firstparty.txt'));

const trackAll = tidy([...epFirst.all, ...agTrack.all, ...agTrackFirst.all], adsAllSet);
const trackAllSet = new Set([...trackAll, ...adsAll]);
const trackThird = tidy([
  ...epFirst.thirdParty, ...epThird.thirdParty, ...epThird.all,
  ...agTrack.thirdParty, ...agTrackFirst.thirdParty, ...plainList(custom('trackers.txt')),
], trackAllSet);

write('trackers.json', [
  blockRule(1, trackAll),
  blockRule(2, trackThird, { domainType: 'thirdParty' }),
]);

// ---------------------------------------------------------------- social widgets
const fanboySocial = adblockList(upstream('fanboy-social.txt'));
const agSocial = adblockList(upstream('adguard-socialfilter-social_trackers.txt'));
const social = tidy([
  ...fanboySocial.thirdParty, ...agSocial.thirdParty, ...agSocial.all, ...plainList(custom('social.txt')),
]);
write('social.json', [blockRule(1, social, { domainType: 'thirdParty' })]);

// ---------------------------------------------------------------- security
const mining = [
  ...adblockList(upstream('easyprivacy-mining.txt')).all,
  ...adblockList(upstream('easyprivacy-mining.txt')).thirdParty,
  ...plainList(upstream('nocoin-hosts.txt')),
];
const badware = adblockList(upstream('ublock-badware.txt'));
const securityAll = tidy([...mining, ...badware.all, ...plainList(custom('security.txt'))]);
// Pages people should be warned about before opening. The warning itself is a dynamic
// rule built by the background script, because it needs the extension's own ID.
const securityPages = tidy([...securityAll, ...badware.doc]);

write('security.json', [blockRule(1, securityAll)]);
write('security-domains.json', securityPages);

// ---------------------------------------------------------------- tracking parameters
const params = new Set([
  'utm_source', 'utm_medium', 'utm_campaign', 'utm_term', 'utm_content', 'utm_id', 'utm_name',
  'utm_brand', 'utm_social', 'utm_social-type', 'fbclid', 'gclid', 'gclsrc', 'dclid', 'gbraid',
  'wbraid', 'msclkid', 'mc_eid', 'mc_cid', '_hsenc', '_hsmi', 'igshid', 'yclid', 'twclid',
  'ttclid', 'li_fat_id', 'oly_anon_id', 'oly_enc_id', 'vero_id', '_openstat', 'mkt_tok',
  'srsltid', 'epik', 'ScCid', 'rb_clickid', 's_cid',
]);
for (const line of upstream('adguard-trackparamfilter-general_url.txt').split(/\r?\n/)) {
  const m = line.trim().match(/^\$removeparam=([A-Za-z0-9_.\-[\]]+)$/);
  if (m) params.add(m[1]);
}
write('strip_params.json', [{
  id: 1,
  priority: 1,
  action: { type: 'redirect', redirect: { transform: { queryTransform: { removeParams: [...params].sort() } } } },
  condition: { urlFilter: '?', resourceTypes: ['main_frame', 'sub_frame'] },
}]);

// ---------------------------------------------------------------- privacy signals
write('privacy_signals.json', [{
  id: 1,
  priority: 1,
  action: {
    type: 'modifyHeaders',
    requestHeaders: [
      { header: 'Sec-GPC', operation: 'set', value: '1' },
      { header: 'DNT', operation: 'set', value: '1' },
    ],
  },
  condition: { resourceTypes: ['main_frame', ...SUBRESOURCES] },
}]);

// ---------------------------------------------------------------- hide referrer from third parties
write('hide_referrer.json', [{
  id: 1,
  priority: 1,
  action: { type: 'modifyHeaders', requestHeaders: [{ header: 'Referer', operation: 'remove' }] },
  condition: { domainType: 'thirdParty', resourceTypes: SUBRESOURCES },
}]);

// ---------------------------------------------------------------- third-party pings / beacons
write('block_pings.json', [{
  id: 1,
  priority: 1,
  action: { type: 'block' },
  condition: { domainType: 'thirdParty', resourceTypes: ['ping'] },
}]);

// ---------------------------------------------------------------- HTTPS upgrade
write('https_upgrade.json', [
  {
    id: 1,
    priority: 1,
    action: { type: 'upgradeScheme' },
    condition: { urlFilter: '|http://', resourceTypes: ['main_frame'] },
  },
  {
    // Local network devices (routers, dev servers) usually have no HTTPS. Leave them alone.
    id: 2,
    priority: 2,
    action: { type: 'allow' },
    condition: {
      regexFilter: '^http://(localhost|127\\.|10\\.|192\\.168\\.|172\\.(1[6-9]|2[0-9]|3[01])\\.|\\[|[^/:]+\\.(local|lan|home|internal|test)[/:]|[^/:.]+[/:])',
      resourceTypes: ['main_frame'],
    },
  },
]);

write('stats.json', {
  ads: adsAll.length + adsThird.length,
  trackers: trackAll.length + trackThird.length,
  social: social.length,
  security: securityPages.length,
  params: params.size,
  popups: popupDomains.length,
});

console.log('\nDone. Reload the extension to pick up the new rules.');
