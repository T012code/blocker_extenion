// Backup for when an ad still gets through: skip it, keep it muted, and
// dismiss YouTube's "ad blockers are not allowed" pop-up if it appears.
(() => {
  'use strict';

  if (window.__layoYoutubeSkipper) return;
  window.__layoYoutubeSkipper = true;

  const SKIP_BUTTONS = [
    '.ytp-skip-ad-button',
    '.ytp-ad-skip-button',
    '.ytp-ad-skip-button-modern',
    '.ytp-ad-skip-button-container button',
    '.videoAdUiSkipButton',
  ].join(',');

  let mutedByUs = false;

  function handleAd() {
    const player = document.querySelector('.html5-video-player');
    if (!player) return;
    const video = player.querySelector('video');

    if (player.classList.contains('ad-showing') || player.classList.contains('ad-interrupting')) {
      document.querySelector(SKIP_BUTTONS)?.click();
      if (video) {
        if (!video.muted) {
          video.muted = true;
          mutedByUs = true;
        }
        if (Number.isFinite(video.duration) && video.duration > 0 && video.currentTime < video.duration - 0.2) {
          video.currentTime = video.duration - 0.1;
        }
        if (video.paused) video.play().catch(() => {});
      }
    } else if (mutedByUs && video) {
      video.muted = false;
      mutedByUs = false;
    }
  }

  function handleEnforcement() {
    const dialog = document.querySelector('ytd-enforcement-message-view-model');
    if (!dialog) return;
    dialog.closest('tp-yt-paper-dialog')?.remove();
    dialog.remove();
    document.querySelector('tp-yt-iron-overlay-backdrop')?.remove();
    const video = document.querySelector('.html5-video-player video');
    if (video?.paused) video.play().catch(() => {});
  }

  function tick() {
    handleAd();
    handleEnforcement();
  }

  // YouTube changes the page constantly, so run at most once every 100ms.
  let scheduled = false;
  const observer = new MutationObserver(() => {
    if (scheduled) return;
    scheduled = true;
    setTimeout(() => {
      scheduled = false;
      tick();
    }, 100);
  });
  function start() {
    observer.observe(document.documentElement, {
      subtree: true,
      childList: true,
      attributes: true,
      attributeFilter: ['class'],
    });
    tick();
  }

  if (document.documentElement) start();
  else document.addEventListener('DOMContentLoaded', start, { once: true });
})();
