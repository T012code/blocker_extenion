// The blocked address arrives after the "#" (see the security rule in the service worker).
const target = location.hash.slice(1);
let url = null;
try {
  url = new URL(target);
  if (!/^https?:$/.test(url.protocol)) url = null;
} catch {
  url = null;
}

document.getElementById('host').textContent = url ? url.hostname : 'Unknown address';

document.getElementById('back').addEventListener('click', () => {
  if (history.length > 1) history.back();
  else location.replace('about:blank');
});

const proceed = document.getElementById('proceed');
proceed.disabled = !url;
proceed.addEventListener('click', async () => {
  proceed.setAttribute('aria-busy', 'true');
  try {
    const res = await chrome.runtime.sendMessage({ type: 'proceed', url: url.href });
    if (!res?.ok) throw new Error(res?.error);
    location.replace(url.href);
  } catch {
    const error = document.getElementById('error');
    error.hidden = false;
    error.textContent = 'Could not open the site. Try turning off the warning in settings.';
    proceed.removeAttribute('aria-busy');
  }
});
