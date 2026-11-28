// Small wording helpers shared by the windows.

/** "now", "4 min", "2 h", "yesterday", "3 days" */
export function ago(iso, now = Date.now()) {
  const t = Date.parse(iso);
  if (Number.isNaN(t)) return '';
  const s = Math.max(0, Math.round((now - t) / 1000));
  if (s < 45) return 'now';
  const m = Math.round(s / 60);
  if (m < 60) return `${m} min`;
  const h = Math.round(m / 60);
  if (h < 24) return `${h} h`;
  const d = Math.round(h / 24);
  return d === 1 ? 'yesterday' : `${d} days`;
}

/** Which PC a clip came from, as people say it. */
export function from(clip) {
  return clip.this_device ? 'This PC' : clip.device_name;
}

/** The shortcut as shown to people ("Super" is the Windows key). */
export function hotkeyLabel(h) {
  return h === 'Off' ? 'Off' : h.replace('Super', 'Win');
}

export function plural(n, one, many) {
  return `${n} ${n === 1 ? one : many}`;
}
