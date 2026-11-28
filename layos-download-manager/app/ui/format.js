// Human-readable sizes, speeds and times.

const UNITS = ['B', 'KB', 'MB', 'GB', 'TB'];

export function bytes(n) {
  if (n == null || !Number.isFinite(n) || n < 0) return '?';
  let i = 0;
  while (n >= 1024 && i < UNITS.length - 1) { n /= 1024; i++; }
  const shown = i === 0 ? String(n) : n.toFixed(n >= 100 ? 0 : 1).replace(/\.0$/, '');
  return `${shown} ${UNITS[i]}`;
}

export function speed(bps) {
  return bps > 0 ? `${bytes(bps)}/s` : '';
}

export function eta(remaining, bps) {
  if (!(bps > 0) || !(remaining > 0)) return '';
  const s = Math.ceil(remaining / bps);
  if (s < 60) return `${s} s left`;
  if (s < 3600) return `${Math.ceil(s / 60)} min left`;
  const h = Math.floor(s / 3600);
  return `${h} h ${Math.ceil((s % 3600) / 60)} min left`;
}

export function percent(done, size) {
  return size > 0 ? Math.min(100, Math.floor((done / size) * 100)) : null;
}

/** What a download can do now, as the pop-up shows it. */
export function action(status) {
  if (status === 'downloading' || status === 'connecting' || status === 'seeding' || status === 'blocked') return 'pause';
  if (status === 'paused' || status === 'failed' || status === 'queued' || status === 'needsRefresh' || status === 'stopped') return 'continue';
  return null;
}

/** Whether a download can be stopped: anything not finished or already stopped. */
export function canStop(status) {
  return status !== 'completed' && status !== 'stopped';
}

export const STATUS_TEXT = {
  queued: 'Waiting',
  connecting: 'Connecting',
  downloading: 'Downloading',
  paused: 'Paused',
  stopped: 'Stopped',
  completed: 'Done',
  failed: 'Failed',
  needsRefresh: 'Link expired',
  seeding: 'Sharing',
  blocked: 'Waiting for VPN',
};
