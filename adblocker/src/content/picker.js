// "Hide an element": point at something on the page, click it, and it stays hidden
// on this site from now on. Press Esc or "Done" to stop.
(() => {
  'use strict';

  if (window.__layoPickerActive) return;
  window.__layoPickerActive = true;

  const host = document.createElement('layo-blocker-picker');
  host.style.cssText = 'all: initial; position: fixed; inset: 0; z-index: 2147483647; pointer-events: none;';
  const root = host.attachShadow({ mode: 'closed' });
  root.innerHTML = `
    <style>
      :host {
        --ink: oklch(0.22 0.02 165);
        --paper: oklch(0.98 0.01 165);
        --accent: oklch(0.52 0.11 165);
        --accent-soft: oklch(0.52 0.11 165 / 0.18);
        --ease-out: cubic-bezier(0.22, 1, 0.36, 1);
      }
      .box {
        position: fixed;
        border: 2px solid var(--accent);
        background: var(--accent-soft);
        border-radius: 4px;
        pointer-events: none;
        display: none;
      }
      .bar {
        position: fixed;
        left: 50%;
        bottom: 16px;
        transform: translateX(-50%);
        display: flex;
        gap: 12px;
        align-items: center;
        padding: 8px 8px 8px 16px;
        max-width: calc(100vw - 32px);
        box-sizing: border-box;
        background: var(--ink);
        color: var(--paper);
        font: 500 13px/1.4 system-ui, -apple-system, "Segoe UI", sans-serif;
        border-radius: 12px;
        box-shadow: 0 8px 24px oklch(0 0 0 / 0.3);
        pointer-events: auto;
      }
      button {
        font: inherit;
        color: var(--ink);
        background: var(--paper);
        border: 0;
        border-radius: 8px;
        padding: 6px 12px;
        cursor: pointer;
        transition: opacity 150ms var(--ease-out);
      }
      button:hover { opacity: 0.85; }
      button:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }
    </style>
    <div class="box"></div>
    <div class="bar" role="status">
      <span class="msg">Click the thing you want to hide. Press Esc when you're done.</span>
      <button type="button">Done</button>
    </div>
  `;
  const box = root.querySelector('.box');
  const bar = root.querySelector('.bar');
  const msg = root.querySelector('.msg');
  const doneButton = root.querySelector('button');
  document.documentElement.appendChild(host);

  let target = null;

  const isSimpleName = (s) => /^[A-Za-z_][\w-]*$/.test(s) && s.length < 48;

  function selectorFor(el) {
    if (el.id && isSimpleName(el.id) && document.querySelectorAll(`#${CSS.escape(el.id)}`).length === 1) {
      return `#${CSS.escape(el.id)}`;
    }
    const parts = [];
    let node = el;
    while (node && node.nodeType === 1 && node !== document.body && node !== document.documentElement) {
      if (node.id && isSimpleName(node.id)) {
        parts.unshift(`#${CSS.escape(node.id)}`);
        break;
      }
      let part = node.localName;
      const classes = [...node.classList].filter(isSimpleName).slice(0, 3);
      if (classes.length) part += `.${classes.map((c) => CSS.escape(c)).join('.')}`;
      const parent = node.parentElement;
      if (parent) {
        const siblings = [...parent.children].filter((c) => c.localName === node.localName);
        if (siblings.length > 1) part += `:nth-of-type(${siblings.indexOf(node) + 1})`;
      }
      parts.unshift(part);
      const candidate = parts.join(' > ');
      try {
        if (document.querySelectorAll(candidate).length === 1) return candidate;
      } catch {
        // Keep walking up.
      }
      node = parent;
    }
    const sel = parts.join(' > ');
    return node === document.body ? `body > ${sel}` : sel;
  }

  function elementAt(x, y) {
    host.style.display = 'none';
    const el = document.elementFromPoint(x, y);
    host.style.display = '';
    if (!el || el === document.documentElement || el === document.body) return null;
    return el;
  }

  function onMove(event) {
    if (event.composedPath().includes(bar)) return;
    target = elementAt(event.clientX, event.clientY);
    if (!target) {
      box.style.display = 'none';
      return;
    }
    const r = target.getBoundingClientRect();
    box.style.display = 'block';
    box.style.left = `${r.left}px`;
    box.style.top = `${r.top}px`;
    box.style.width = `${r.width}px`;
    box.style.height = `${r.height}px`;
  }

  function swallow(event) {
    if (event.composedPath().includes(bar)) return;
    event.preventDefault();
    event.stopPropagation();
    event.stopImmediatePropagation();
  }

  async function onClick(event) {
    if (event.composedPath().includes(bar)) return;
    swallow(event);
    const el = target || elementAt(event.clientX, event.clientY);
    if (!el) return;
    const selector = selectorFor(el);
    el.style.setProperty('display', 'none', 'important');
    box.style.display = 'none';
    target = null;
    try {
      const res = await chrome.runtime.sendMessage({ type: 'saveHide', selector });
      msg.textContent = res?.ok
        ? 'Hidden. Click something else, or press Esc to finish.'
        : 'Hidden for now, but it could not be saved.';
    } catch {
      msg.textContent = 'Hidden for now, but it could not be saved.';
    }
  }

  function onKey(event) {
    if (event.key === 'Escape') {
      swallow(event);
      stop();
    }
  }

  function stop() {
    document.removeEventListener('mousemove', onMove, true);
    document.removeEventListener('click', onClick, true);
    document.removeEventListener('mousedown', swallow, true);
    document.removeEventListener('mouseup', swallow, true);
    document.removeEventListener('keydown', onKey, true);
    host.remove();
    window.__layoPickerActive = false;
  }

  doneButton.addEventListener('click', stop);
  document.addEventListener('mousemove', onMove, true);
  document.addEventListener('click', onClick, true);
  document.addEventListener('mousedown', swallow, true);
  document.addEventListener('mouseup', swallow, true);
  document.addEventListener('keydown', onKey, true);
})();
