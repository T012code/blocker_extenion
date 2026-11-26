// The "Download" button that appears over videos, with a menu of formats.
// Runs in every page and frame. Everything it draws lives in a shadow root, so
// the page's styles can't change it and it can't change the page.
(() => {
  if (window.__ldmVideoButton) return;
  window.__ldmVideoButton = true;
  const api = globalThis.browser ?? globalThis.chrome;

  const MIN_W = 200;
  const MIN_H = 112;
  const HIDE_AFTER_MS = 2500;
  let enabled = true;

  api.storage.local.get('settings').then((s) => { enabled = s.settings?.videoButton !== false; }).catch(() => {});
  api.storage.onChanged.addListener((c) => {
    if (c.settings?.newValue) {
      enabled = c.settings.newValue.videoButton !== false;
      if (!enabled) hide(true);
    }
  });

  // ---- Drawing

  const host = document.createElement('ldm-video-download');
  host.style.cssText = 'all: initial; position: fixed; z-index: 2147483647; top: 0; left: 0; width: 0; height: 0;';
  const root = host.attachShadow({ mode: 'open' });
  root.innerHTML = `
<style>
:host {
    all: initial;
    --accent: oklch(55.5% 0.215 263); --accent-strong: oklch(47% 0.22 265); --accent-ink: oklch(99% 0.004 250);
    --card: oklch(100% 0 0); --ink: oklch(22% 0.045 266); --ink-muted: oklch(45% 0.03 262);
    --surface: oklch(95.6% 0.016 250); --rule: oklch(22% 0.045 266 / 0.1); --focus: oklch(89% 0.19 125);
    --success: oklch(50% 0.13 155); --danger: oklch(52% 0.19 25); --tag-bg: oklch(93.5% 0.038 255);
    --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
  }
  @media (prefers-color-scheme: dark) {
    :host {
      --card: oklch(19.5% 0.035 266); --ink: oklch(96% 0.01 250); --ink-muted: oklch(76% 0.03 255);
      --surface: oklch(25.5% 0.045 266); --rule: oklch(96% 0.01 250 / 0.1);
      --success: oklch(76% 0.14 155); --danger: oklch(72% 0.16 25); --tag-bg: oklch(28% 0.075 263);
    }
  }
  * { box-sizing: border-box; font-family: "Segoe UI Variable Text", "Segoe UI", system-ui, -apple-system, sans-serif; }
  .wrap { position: fixed; display: none; flex-direction: column; align-items: flex-end; gap: 6px; pointer-events: auto; }
  .wrap.show { display: flex; }
  .btn {
    display: inline-flex; align-items: center; gap: 6px; height: 32px; padding: 0 14px 0 10px;
    border: 0; border-radius: 6px; cursor: pointer; font-size: 13px; font-weight: 600; letter-spacing: .01em;
    color: var(--accent-ink); background: var(--accent); box-shadow: 0 1px 2px rgb(0 0 0 / .25), 0 4px 12px rgb(0 0 0 / .2);
    transition: background-color 120ms var(--ease-out), transform 120ms var(--ease-out);
  }
  .btn:hover { background: var(--accent-strong); }
  .btn:active { transform: scale(.97); }
  .btn:focus-visible, .item:focus-visible { outline: 2px solid var(--focus); outline-offset: 2px; }
  .btn svg { width: 16px; height: 16px; }
  .menu {
    display: none; width: 300px; max-height: 360px; overflow: auto; padding: 6px;
    border-radius: 10px; background: var(--card); color: var(--ink); border: 1px solid var(--rule);
    box-shadow: 0 2px 6px rgb(0 0 0 / .1), 0 16px 40px -12px rgb(0 0 0 / .35); font-size: 13px; transform-origin: top right; animation: menu-in 160ms var(--ease-out);
  }
  .menu.open { display: block; }
  .head { padding: 8px 10px 6px; font-weight: 650; font-size: 13px; }
  .title { padding: 0 10px 8px; color: var(--ink-muted); font-size: 12px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .group { padding: 8px 10px 4px; color: var(--ink-muted); font-size: 11px; font-weight: 650; text-transform: uppercase; letter-spacing: .06em; }
  .item {
    width: 100%; display: flex; align-items: center; justify-content: space-between; gap: 10px;
    padding: 8px 10px; border: 0; border-radius: 6px; background: transparent; color: inherit;
    font-size: 13px; text-align: left; cursor: pointer;
  }
  .item:hover:not(:disabled) { background: var(--surface); }
  .item:disabled { cursor: default; color: var(--ink-muted); }
  .item .size { color: var(--ink-muted); font-size: 12px; white-space: nowrap; }
  .tag { font-size: 10px; font-weight: 700; padding: 1px 6px; border-radius: 3px; background: var(--tag-bg); color: var(--accent); margin-left: 6px; }
  .note { padding: 8px 10px; color: var(--ink-muted); font-size: 12px; }
  .note.ok { color: var(--success); font-weight: 600; }
  .note.bad { color: var(--danger); }
  .foot { padding: 6px 10px 4px; color: var(--ink-muted); font-size: 11px; border-top: 1px solid var(--rule); margin-top: 4px; }
  @keyframes menu-in { from { opacity: 0; transform: scale(.97); } to { opacity: 1; transform: none; } }
  /* Small players: just the icon, so it doesn't cover the player's own controls. */
  .wrap.compact .btn { width: 32px; padding: 0; justify-content: center; }
  .wrap.compact .btn span { position: absolute; width: 1px; height: 1px; overflow: hidden; clip-path: inset(50%); }
  @media (prefers-reduced-motion: reduce) { .btn, .menu { transition: none; animation: none; } }
</style>
<div class="wrap" part="wrap">
  <button class="btn" type="button" aria-haspopup="menu" aria-expanded="false">
    <svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M8 2.5v8M4.5 7.5 8 11l3.5-3.5M3 13.5h10"/>
    </svg>
    <span>Download</span>
  </button>
  <div class="menu" role="menu" aria-label="Download this video"></div>
</div>`;
  const wrap = root.querySelector('.wrap');
  const button = root.querySelector('.btn');
  const menu = root.querySelector('.menu');

  function attach() {
    const parent = document.fullscreenElement ?? document.documentElement;
    if (host.parentNode !== parent) parent.appendChild(host);
  }
  document.addEventListener('fullscreenchange', () => { attach(); place(); });

  // ---- Which video the pointer is on

  let current = null;
  let hideTimer = 0;
  let menuOpen = false;

  function videoAt(x, y) {
    for (const v of document.querySelectorAll('video')) {
      const r = v.getBoundingClientRect();
      if (r.width < MIN_W || r.height < MIN_H) continue;
      if (x >= r.left && x <= r.right && y >= r.top && y <= r.bottom) return v;
    }
    return null;
  }

  function place() {
    if (!current) return;
    const r = current.getBoundingClientRect();
    if (r.width < MIN_W || r.height < MIN_H) { hide(true); return; }
    // Top right corner of the video, kept on screen.
    const top = Math.max(8, Math.min(r.top + 10, innerHeight - 44));
    const right = Math.max(8, innerWidth - Math.min(r.right, innerWidth) + 10);
    wrap.style.top = `${top}px`;
    wrap.style.right = `${right}px`;
    wrap.style.left = 'auto';
    wrap.classList.toggle('compact', r.width < 360);
  }

  function show(video) {
    clearTimeout(hideTimer);
    if (current !== video) closeMenu();
    current = video;
    attach();
    place();
    wrap.classList.add('show');
  }

  function hide(now) {
    clearTimeout(hideTimer);
    const go = () => {
      if (menuOpen) return;
      wrap.classList.remove('show');
      current = null;
    };
    if (now) { menuOpen = false; closeMenu(); go(); } else hideTimer = setTimeout(go, HIDE_AFTER_MS);
  }

  let frame = 0;
  addEventListener('mousemove', (e) => {
    if (!enabled || frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (e.composedPath().includes(host)) { clearTimeout(hideTimer); return; }
      const v = videoAt(e.clientX, e.clientY);
      if (v) show(v);
      else if (current) hide(false);
    });
  }, { passive: true, capture: true });
  addEventListener('scroll', place, { passive: true, capture: true });
  addEventListener('resize', place, { passive: true });

  // ---- Menu

  const fmt = (n) => {
    if (!(n > 0)) return '';
    const u = ['B', 'KB', 'MB', 'GB'];
    let i = 0;
    while (n >= 1024 && i < u.length - 1) { n /= 1024; i++; }
    return `${n >= 100 || i === 0 ? Math.round(n) : n.toFixed(1).replace(/\.0$/, '')} ${u[i]}`;
  };

  function el(tag, cls, text) {
    const e = document.createElement(tag);
    if (cls) e.className = cls;
    if (text != null) e.textContent = text;
    return e;
  }

  function closeMenu() {
    menuOpen = false;
    menu.classList.remove('open');
    button.setAttribute('aria-expanded', 'false');
  }

  function itemFor(choice, title) {
    const b = el('button', 'item');
    b.type = 'button';
    b.setAttribute('role', 'menuitem');
    const name = el('span', null, choice.label);
    if (choice.kind === 'audio') name.append(el('span', 'tag', 'AUDIO'));
    const size = el('span', 'size', choice.size ? `${choice.estimated ? '~' : ''}${fmt(choice.size)}` : '');
    b.append(name, size);
    b.disabled = !!choice.disabled;
    b.addEventListener('click', async () => {
      for (const x of menu.querySelectorAll('.item')) x.disabled = true;
      setStatus('Sending to Layó\'s Download Manager…');
      try {
        const r = await api.runtime.sendMessage({ type: 'videoDownload', choice, title, pageUrl: location.href });
        if (!r?.ok) throw new Error(r?.error || 'it could not be sent');
        setStatus('Downloading in Layó\'s Download Manager', 'ok');
        setTimeout(() => { closeMenu(); hide(true); }, 1400);
      } catch (e) {
        setStatus(`Couldn't download: ${e.message}`, 'bad');
        for (const x of menu.querySelectorAll('.item')) x.disabled = !!x.dataset.disabled;
      }
    });
    if (choice.disabled) b.dataset.disabled = '1';
    return b;
  }

  let statusEl = null;
  function setStatus(text, kind = '') {
    statusEl ??= el('div', 'note');
    statusEl.className = `note ${kind}`;
    statusEl.textContent = text;
    menu.insertBefore(statusEl, menu.querySelector('.foot'));
  }

  async function openMenu() {
    const video = current;
    if (!video) return;
    menuOpen = true;
    statusEl = null;
    button.setAttribute('aria-expanded', 'true');
    menu.replaceChildren();
    const title = (document.title || '').trim();
    menu.append(el('div', 'head', 'Download this video'));
    if (title) menu.append(el('div', 'title', title));
    menu.classList.add('open');

    if (video.mediaKeys) {
      menu.append(el('div', 'note bad', 'This video is protected (DRM) and can\'t be downloaded.'));
      menu.append(el('div', 'foot', 'Layó\'s Download Manager'));
      return;
    }

    const localGroup = el('div');
    const siteGroup = el('div');
    menu.append(localGroup, siteGroup, el('div', 'foot', 'Layó\'s Download Manager'));
    const info = { src: video.currentSrc || video.src, duration: video.duration, pageUrl: location.href, title };

    // What the page itself played: instant.
    api.runtime.sendMessage({ type: 'videoChoices', ...info }).then((r) => {
      const list = r?.choices ?? [];
      if (!list.length) return;
      localGroup.append(el('div', 'group', 'On this page'));
      for (const c of list) localGroup.append(itemFor(c, title));
    }).catch(() => {});

    // What the site offers, via the app: slower.
    const waiting = el('div', 'note', 'Looking for more formats…');
    siteGroup.append(waiting);
    try {
      const r = await api.runtime.sendMessage({ type: 'siteFormats', ...info });
      waiting.remove();
      if (r?.choices?.length) {
        siteGroup.append(el('div', 'group', r.live ? 'From the site (live)' : 'From the site'));
        for (const c of r.choices) siteGroup.append(itemFor(c, r.title || title));
      } else if (!localGroup.childElementCount) {
        siteGroup.append(el('div', 'note', r?.error || 'No downloadable formats found for this video.'));
      }
    } catch (e) {
      waiting.remove();
      if (!localGroup.childElementCount) siteGroup.append(el('div', 'note bad', e.message));
    }
  }

  button.addEventListener('click', (e) => {
    e.preventDefault();
    e.stopPropagation();
    if (menu.classList.contains('open')) closeMenu();
    else openMenu();
  });
  // Clicks on the button must not pause or play the video underneath.
  for (const t of ['mousedown', 'mouseup', 'pointerdown', 'pointerup', 'dblclick']) {
    wrap.addEventListener(t, (e) => e.stopPropagation());
  }
  addEventListener('keydown', (e) => { if (e.key === 'Escape' && menuOpen) { closeMenu(); button.focus(); } }, true);
  addEventListener('mousedown', (e) => { if (menuOpen && !e.composedPath().includes(host)) { closeMenu(); hide(false); } }, true);
})();
