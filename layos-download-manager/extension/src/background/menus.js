// Right-click menu, as in IDM: "Download with LDM" on links and images, "Download this
// video/audio with LDM" on players (streamed ones go through the video grabber), and
// "Download all links with LDM" on pages and selections.

import { api } from '../shared/api.js';
import { getSettings } from '../shared/settings.js';
import { baseName, pickLinks } from './rules.js';
import { askFlag, describe } from './capture.js';
import * as native from './native.js';
import { addHistory } from './history.js';
import * as media from './media.js';

const TITLE = 'LDM';

export function create() {
  api.contextMenus.removeAll(() => {
    api.contextMenus.create({ id: 'ldm-link', title: `Download with ${TITLE}`, contexts: ['link', 'image'] });
    api.contextMenus.create({ id: 'ldm-media', title: `Download this video/audio with ${TITLE}`, contexts: ['video', 'audio'] });
    api.contextMenus.create({ id: 'ldm-all', title: `Download all links with ${TITLE}`, contexts: ['page', 'selection'] });
  });
}

function notify(title, message) {
  api.notifications?.create({ type: 'basic', iconUrl: api.runtime.getURL('icons/icon-128.png'), title, message })?.catch?.(() => {});
}

async function sendItems(items, source, settings) {
  try {
    await native.send({ type: 'download', source, ask: source === 'allLinks' ? true : askFlag(settings), items });
    for (const i of items.slice(0, 5)) await addHistory({ name: baseName(i.url), url: i.url, status: 'sent' });
  } catch (e) {
    native.markStale();
    notify('Layó\'s Download Manager', `Couldn't send the download: ${e.message ?? e}`);
  }
}

/** Runs in the page: collects links, from the selection when there is one. */
function collectLinks() {
  const sel = window.getSelection();
  const inSelection = (a) => sel && !sel.isCollapsed && sel.containsNode(a, true);
  const anchors = [...document.querySelectorAll('a[href], area[href]')];
  const chosen = sel && !sel.isCollapsed ? anchors.filter(inSelection) : anchors;
  return chosen.map((a) => ({ url: a.href, text: (a.textContent || a.title || '').trim().slice(0, 200) }));
}

async function onClicked(info, tab) {
  const settings = await getSettings();
  const incognito = !!tab?.incognito;
  if (info.menuItemId === 'ldm-link') {
    // A link, or an image (a linked image downloads what the link points to).
    const url = info.linkUrl || info.srcUrl;
    if (!/^(https?:|magnet:)/i.test(url ?? '')) {
      return notify('Can\'t download this one', 'Only web links (and magnet links) can be downloaded.');
    }
    const item = await describe({
      url, referrer: info.pageUrl, incognito, pageUrl: info.pageUrl,
      description: info.linkText || info.selectionText,
    });
    return sendItems([item], 'link', settings);
  }
  if (info.menuItemId === 'ldm-media') {
    const url = info.srcUrl;
    if (!/^https?:/i.test(url ?? '')) {
      // Streamed in pieces (blob: or MSE): the video grabber (yt-dlp) fetches it from
      // the page, in the best quality.
      try {
        await media.download({
          action: { type: 'media', pageUrl: info.pageUrl, label: 'Best quality' },
          title: tab?.title ?? '', pageUrl: info.pageUrl, incognito,
        });
      } catch (e) {
        native.markStale();
        notify('Layó\'s Download Manager', `Couldn't send the video: ${e.message ?? e}`);
      }
      return;
    }
    const item = await describe({ url, referrer: info.pageUrl, incognito, pageUrl: info.pageUrl });
    return sendItems([item], 'media', settings);
  }
  if (info.menuItemId === 'ldm-all' && tab?.id != null) {
    let links = [];
    try {
      const results = await api.scripting.executeScript({
        target: { tabId: tab.id, frameIds: [info.frameId ?? 0] },
        func: collectLinks,
      });
      links = results.flatMap((r) => r.result ?? []);
    } catch (e) {
      return notify('Layó\'s Download Manager', `Couldn't read the links on this page: ${e.message ?? e}`);
    }
    const picked = pickLinks(links, settings);
    if (!picked.length) {
      return notify('No files found', 'There are no links to files of the types in your list on this page.');
    }
    const items = [];
    for (const l of picked) {
      items.push(await describe({ url: l.url, referrer: info.pageUrl, incognito, pageUrl: info.pageUrl, description: l.text }));
    }
    return sendItems(items, 'allLinks', settings);
  }
}

export function start() {
  api.contextMenus.onClicked.addListener((info, tab) => {
    onClicked(info, tab);
  });
  // For the end-to-end tests: a browser can't be made to click its own menu.
  globalThis.__ldmMenuClick = onClicked;
}
