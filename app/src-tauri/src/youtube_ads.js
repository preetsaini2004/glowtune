// YouTube Music: video ads come from the same servers as the music, so network
// blocking can't stop them. Two layers, injected before the page's own scripts:
//   1. strip ad slots from the player data (JSON.parse, fetch, initial response)
//   2. backup: if an ad still starts, mute it, jump to its end and press Skip,
//      then restore the user's sound
(() => {
  if (location.hostname !== 'music.youtube.com' || window.__glowtuneYtAds) return;
  window.__glowtuneYtAds = true;

  const AD_KEYS = ['adPlacements', 'playerAds', 'adSlots', 'adBreakHeartbeatParams'];
  const prune = (o) => {
    if (!o || typeof o !== 'object') return o;
    for (const k of AD_KEYS) if (k in o) delete o[k];
    if (o.playerResponse && typeof o.playerResponse === 'object') prune(o.playerResponse);
    return o;
  };

  // 1a. Anything the page parses.
  const parse = JSON.parse;
  JSON.parse = function (...args) {
    return prune(parse.apply(this, args));
  };

  // 1b. Player requests made with fetch.
  const fetch0 = window.fetch;
  window.fetch = async function (input, init) {
    const response = await fetch0.call(this, input, init);
    try {
      const url = typeof input === 'string' ? input : input?.url || '';
      if (!/\/youtubei\/v1\/(player|next)/.test(url)) return response;
      const data = prune(await response.clone().json());
      return new Response(JSON.stringify(data), {
        status: response.status,
        statusText: response.statusText,
        headers: response.headers,
      });
    } catch {
      return response;
    }
  };

  // 1c. The response embedded in the first page load.
  let initial;
  try {
    Object.defineProperty(window, 'ytInitialPlayerResponse', {
      configurable: true,
      get: () => initial,
      set: (v) => (initial = prune(v)),
    });
  } catch {}

  // 2. Backup skipper (cheap: one check every 400 ms, only while a video exists).
  let mutedByUs = false;
  setInterval(() => {
    const video = document.querySelector('video');
    if (!video) return;
    const ad = document.querySelector('.ad-showing, .ad-interrupting');
    if (ad) {
      if (!video.muted) {
        video.muted = true;
        mutedByUs = true;
      }
      if (Number.isFinite(video.duration) && video.duration > 0 && video.currentTime < video.duration - 0.2) {
        video.currentTime = video.duration;
      }
      document.querySelector('.ytp-ad-skip-button, .ytp-ad-skip-button-modern, .ytp-skip-ad-button, .ytp-ad-overlay-close-button')?.click();
    } else if (mutedByUs) {
      video.muted = false;
      mutedByUs = false;
    }
  }, 400);
})();
