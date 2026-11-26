// Links the user clicked while holding the bypass key go to the browser as usual.

const WINDOW_MS = 15_000;
const bypassed = new Map();
const handedBack = new Map();

export function noteBypass(url) {
  if (url) bypassed.set(url, Date.now());
}

export function isBypassed(...urls) {
  return urls.some((u) => u && Date.now() - (bypassed.get(u) ?? 0) < WINDOW_MS);
}

/** Marks a download the extension gives back to the browser, so it isn't taken over again. */
export function noteHandedBack(url) {
  if (url) handedBack.set(url, Date.now());
}

export function isHandedBack(...urls) {
  return urls.some((u) => u && Date.now() - (handedBack.get(u) ?? 0) < 60_000);
}
