// Spotify (free): audio ads stream from the same servers as the music and the
// page disables Skip while one plays, so network rules can't stop them.
// While the player says "Advertisement" this script silences the ad and runs
// it at top speed, then gives the user's sound back when music returns.
//
//   page creates <audio>/<video> ──► remembered here (the ad player isn't in the DOM)
//   every 250 ms: ad showing? ──► mute + fast-forward every player ──► restore after
(() => {
  if (location.hostname !== 'open.spotify.com' || window.__glowtuneSpotifyAds) return;
  window.__glowtuneSpotifyAds = true;

  // 1. Remember every media element the page makes, attached to the page or not.
  const players = new Set();
  const keep = (el) => {
    if (el instanceof HTMLMediaElement) players.add(new WeakRef(el));
    return el;
  };
  const create = Document.prototype.createElement;
  Document.prototype.createElement = function (name, ...rest) {
    return keep(create.call(this, name, ...rest));
  };
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (...args) {
    if (![...players].some((r) => r.deref() === this)) keep(this);
    if (adShowing()) hush(this);
    return play.apply(this, args);
  };
  const live = () => {
    const out = [...document.querySelectorAll('audio, video')];
    for (const ref of players) {
      const el = ref.deref();
      if (!el) players.delete(ref);
      else if (!out.includes(el)) out.push(el);
    }
    return out;
  };

  // 2. Is an ad on? The player bar says so in three places; any one is enough.
  const adShowing = () => {
    const bar = document.querySelector('[data-testid="now-playing-bar"]');
    const type = bar?.getAttribute('data-testadtype');
    if (type && type !== 'ad-type-none') return true;
    return !!document.querySelector(
      '[data-testid="context-item-info-ad-subtitle"], [data-testid="ad-controls"], [data-testid="context-link"][data-context-item-type="ad"]',
    );
  };

  // 3. Silence + fast-forward during the ad; undo exactly what we changed after.
  const touched = new Map(); // element -> { muted, rate }
  const hush = (el) => {
    if (!touched.has(el)) touched.set(el, { muted: el.muted, rate: el.playbackRate });
    if (!el.muted) el.muted = true;
    try {
      if (el.playbackRate !== 16) el.playbackRate = 16;
    } catch {}
  };
  const restore = () => {
    for (const [el, was] of touched) {
      el.muted = was.muted;
      try {
        el.playbackRate = was.rate || 1;
      } catch {}
    }
    touched.clear();
  };

  let inAd = false;
  setInterval(() => {
    const ad = adShowing();
    if (ad) {
      inAd = true;
      for (const el of live()) hush(el);
    } else if (inAd) {
      inAd = false;
      restore();
    }
  }, 250);
})();
