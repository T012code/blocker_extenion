// Right-click menu: download a link, all links on a page, or a video/audio element.

import { api } from '../shared/api.js';
import { getSettings } from '../shared/settings.js';
import { baseName, pickLinks } from './rules.js';
import { askFlag, describe } from './capture.js';
import * as native from './native.js';
import { addHistory } from './history.js';

const TITLE = 'Layó\'s DM';

export function create() {
  api.contextMenus.removeAll(() => {
    api.contextMenus.create({ id: 'ldm-link', title: `Download with ${TITLE}`, contexts: ['link'] });
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
    const item = await describe({
      url: info.linkUrl, referrer: info.pageUrl, incognito, pageUrl: info.pageUrl,
      description: info.linkText || info.selectionText,
    });
    return sendItems([item], 'link', settings);
  }
  if (info.menuItemId === 'ldm-media') {
    const url = info.srcUrl;
    if (!/^https?:/i.test(url ?? '')) {
      return notify('Can\'t download this one directly',
        'This video is streamed in pieces. The video grabber in Layó\'s Download Manager will handle streams like this.');
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
}
