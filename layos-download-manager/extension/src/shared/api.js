// One name for the extension API in Chrome (chrome.*) and Firefox (browser.*).
export const api = globalThis.browser ?? globalThis.chrome;
export const isFirefox = typeof globalThis.browser !== 'undefined' && !!globalThis.browser.runtime?.getBrowserInfo;
