// Applies the elements you chose to hide with "Hide an element" on this site.
(() => {
  'use strict';

  if (window.__layoCustomHides) return;
  window.__layoCustomHides = true;

  const host = location.hostname.replace(/^www\./, '');
  if (!host) return;

  chrome.storage.local.get('settings').then(({ settings }) => {
    const hides = settings?.customHides || {};
    const selectors = [];
    for (const [site, list] of Object.entries(hides)) {
      if (host === site || host.endsWith(`.${site}`)) selectors.push(...list);
    }
    // A selector that the browser can't parse would throw away the whole style rule,
    // so each one gets its own rule.
    const valid = selectors.filter((s) => {
      if (/[{}<]/.test(s)) return false;
      try {
        document.createDocumentFragment().querySelector(s);
        return true;
      } catch {
        return false;
      }
    });
    if (!valid.length) return;

    const style = document.createElement('style');
    style.dataset.layoBlocker = 'custom';
    style.textContent = valid.map((s) => `${s} { display: none !important; }`).join('\n');
    (document.head || document.documentElement).appendChild(style);
  });
})();
