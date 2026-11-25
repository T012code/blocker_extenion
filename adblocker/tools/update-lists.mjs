// Downloads fresh copies of the upstream filter lists into lists/upstream/.
// Run with: npm run update-lists   (then npm run build)

import { writeFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = join(dirname(fileURLToPath(import.meta.url)), '..');
const GH = 'https://raw.githubusercontent.com';

const sources = {
  'easylist-adservers.txt': `${GH}/easylist/easylist/master/easylist/easylist_adservers.txt`,
  'easylist-adservers-popup.txt': `${GH}/easylist/easylist/master/easylist/easylist_adservers_popup.txt`,
  'easyprivacy-trackingservers.txt': `${GH}/easylist/easylist/master/easyprivacy/easyprivacy_trackingservers.txt`,
  'easyprivacy-trackingservers-thirdparty.txt': `${GH}/easylist/easylist/master/easyprivacy/easyprivacy_trackingservers_thirdparty.txt`,
  'easyprivacy-mining.txt': `${GH}/easylist/easylist/master/easyprivacy/easyprivacy_trackingservers_mining.txt`,
  'fanboy-social.txt': `${GH}/easylist/easylist/master/fanboy-addon/fanboy_social_thirdparty.txt`,
  'adguard-basefilter-adservers.txt': `${GH}/AdguardTeam/AdguardFilters/master/BaseFilter/sections/adservers.txt`,
  'adguard-basefilter-adservers_firstparty.txt': `${GH}/AdguardTeam/AdguardFilters/master/BaseFilter/sections/adservers_firstparty.txt`,
  'adguard-spywarefilter-tracking_servers.txt': `${GH}/AdguardTeam/AdguardFilters/master/SpywareFilter/sections/tracking_servers.txt`,
  'adguard-spywarefilter-tracking_servers_firstparty.txt': `${GH}/AdguardTeam/AdguardFilters/master/SpywareFilter/sections/tracking_servers_firstparty.txt`,
  'adguard-trackparamfilter-general_url.txt': `${GH}/AdguardTeam/AdguardFilters/master/TrackParamFilter/sections/general_url.txt`,
  'adguard-socialfilter-social_trackers.txt': `${GH}/AdguardTeam/AdguardFilters/master/SocialFilter/sections/social_trackers.txt`,
  'ublock-badware.txt': `${GH}/uBlockOrigin/uAssets/master/filters/badware.txt`,
  'nocoin-hosts.txt': `${GH}/hoshsadiq/adblock-nocoin-list/master/hosts.txt`,
};

let failed = 0;
for (const [name, url] of Object.entries(sources)) {
  try {
    const res = await fetch(url);
    if (!res.ok) throw new Error(`HTTP ${res.status}`);
    const text = await res.text();
    writeFileSync(join(root, 'lists/upstream', name), text);
    console.log(`  ok    ${name} (${text.split('\n').length} lines)`);
  } catch (err) {
    failed++;
    console.log(`  FAIL  ${name}: ${err.message} (kept the old copy)`);
  }
}
process.exit(failed ? 1 : 0);
