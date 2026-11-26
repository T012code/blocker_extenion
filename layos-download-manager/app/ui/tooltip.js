// Tooltips: the name of every icon-only control (and its shortcut) on hover or keyboard
// focus, quicker and clearer than the browser's own. They also show text that's cut off
// ("…"). Where the text on screen already says it all, nothing shows.
// Any element with a title gets one; the title moves to data-tip so the browser's own
// tooltip doesn't show as well.

const SHOW_AFTER = 450;
const WARM_FOR = 700;

let tip = null;
let target = null;
let timer = 0;
let lastHidden = 0;

function tipText(el) {
  if (el.hasAttribute('title')) {
    el.dataset.tip = el.getAttribute('title');
    el.removeAttribute('title');
    // Keep it for screen readers.
    if (!el.hasAttribute('aria-label') && !el.textContent.trim()) el.setAttribute('aria-label', el.dataset.tip.replace(/\s*\([^)]*\)\s*$/, ''));
    else if (!el.hasAttribute('aria-description')) el.setAttribute('aria-description', el.dataset.tip);
  }
  return el.dataset.tip ?? '';
}

/** Whether the tooltip adds anything to what's on screen. */
function worthShowing(el, text) {
  const visible = (el.innerText ?? '').replace(/\s+/g, ' ').trim();
  if (!visible) return true; // icon only, or its words are hidden (folded side bar)
  if (text.replace(/\s+/g, ' ').trim() !== visible) return true; // more to say, e.g. a shortcut
  // Cut off with "…" somewhere inside?
  for (const n of [el, ...el.querySelectorAll('*')]) if (n.scrollWidth > n.clientWidth + 1 && getComputedStyle(n).textOverflow === 'ellipsis') return true;
  return false;
}

function render(text) {
  tip.replaceChildren();
  const m = /^(.*?)\s*\(((?:Ctrl|Shift|Alt|Del|Enter|Esc|F\d)[^)]*)\)\s*$/.exec(text);
  tip.append(m ? m[1] : text);
  if (m) tip.append(Object.assign(document.createElement('kbd'), { textContent: m[2] }));
}

function place(el) {
  const r = el.getBoundingClientRect();
  const t = tip.getBoundingClientRect();
  const gap = 8;
  let x;
  let y;
  // In the folded side bar, to the right; elsewhere below (above near the bottom).
  if (el.closest('.sidebar-collapsed .sidebar')) {
    x = r.right + gap;
    y = r.top + r.height / 2 - t.height / 2;
    tip.dataset.side = 'right';
  } else {
    x = r.left + r.width / 2 - t.width / 2;
    y = r.bottom + gap;
    tip.dataset.side = 'below';
    if (y + t.height > innerHeight - 4) { y = r.top - gap - t.height; tip.dataset.side = 'above'; }
  }
  tip.style.left = `${Math.round(Math.max(4, Math.min(x, innerWidth - t.width - 4)))}px`;
  tip.style.top = `${Math.round(Math.max(4, Math.min(y, innerHeight - t.height - 4)))}px`;
}

function show(el) {
  const text = tipText(el);
  if (!text || !el.isConnected || !worthShowing(el, text)) return;
  render(text);
  tip.hidden = false;
  tip.classList.remove('in');
  place(el);
  void tip.offsetWidth;
  tip.classList.add('in');
  el.setAttribute('aria-describedby', 'tooltip');
}

function hide() {
  clearTimeout(timer);
  if (target) target.removeAttribute('aria-describedby');
  if (tip && !tip.hidden) { tip.hidden = true; lastHidden = performance.now(); }
  target = null;
}

function arm(el, now = false) {
  if (el === target) return;
  hide();
  target = el;
  tipText(el);
  const warm = performance.now() - lastHidden < WARM_FOR;
  timer = setTimeout(() => { if (target === el) show(el); }, now || warm ? 0 : SHOW_AFTER);
}

export function initTooltips() {
  tip = document.createElement('div');
  tip.id = 'tooltip';
  tip.className = 'tooltip';
  tip.setAttribute('role', 'tooltip');
  tip.hidden = true;
  document.body.append(tip);
  const find = (e) => e.target.closest?.('[title], [data-tip]');
  addEventListener('pointerover', (e) => {
    const el = find(e);
    if (el) arm(el); else if (target) hide();
  }, true);
  addEventListener('pointerout', (e) => {
    if (target && !target.contains(e.relatedTarget)) hide();
  }, true);
  addEventListener('focusin', (e) => {
    const el = find(e);
    if (el && el.matches(':focus-visible')) arm(el, true);
  });
  addEventListener('focusout', hide);
  for (const ev of ['pointerdown', 'keydown', 'wheel', 'blur']) addEventListener(ev, hide, true);
  addEventListener('scroll', hide, true);
}
