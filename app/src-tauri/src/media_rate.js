// Glowtune: the one owner of the service player's speed (fast fill).
//
//   Glowtune ──fill(1.25, 1500 ms)──► every player plays 1.25× with the pitch
//                                     raised; the sound engine undoes both, and
//                                     the extra sound fills the blend's delay
//   no renewal within 1.5 s, a new song loading, or the page wanting another
//   speed itself (an ad running at 16×) ──► back to what the page asked for
//
// The page keeps seeing the speed it set itself, so its own logic is unchanged.
(() => {
  if (window.top !== window || window.__glowtuneRate) return;
  const RATE = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'playbackRate');
  const PITCH = Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype, 'preservesPitch');
  const refs = new Set(); // WeakRefs to every player the page made
  const wish = new WeakMap(); // player -> { rate, pitch } as the page set them
  let fill = 1;

  const want = (el) => wish.get(el) || { rate: 1, pitch: true };
  const apply = (el) => {
    const w = want(el);
    const fast = fill > 1 && w.rate === 1;
    const rate = fast ? fill : w.rate;
    const pitch = fast ? false : w.pitch;
    try {
      if (PITCH.get.call(el) !== pitch) PITCH.set.call(el, pitch);
      if (RATE.get.call(el) !== rate) RATE.set.call(el, rate);
    } catch {}
  };
  const adopt = (el) => {
    if (!(el instanceof HTMLMediaElement) || wish.has(el)) return el;
    wish.set(el, { rate: RATE.get.call(el) || 1, pitch: PITCH.get.call(el) });
    refs.add(new WeakRef(el));
    Object.defineProperty(el, 'playbackRate', {
      configurable: true,
      get: () => want(el).rate,
      set: (v) => {
        want(el).rate = Number(v);
        apply(el);
      },
    });
    Object.defineProperty(el, 'preservesPitch', {
      configurable: true,
      get: () => want(el).pitch,
      set: (v) => {
        want(el).pitch = !!v;
        apply(el);
      },
    });
    // A new song resets the speed without telling anyone: end the fill there
    // (Glowtune sees the real speed in `state()` and stops correcting).
    el.addEventListener('loadstart', () => stop());
    el.addEventListener('ratechange', () => apply(el));
    return el;
  };
  const create = Document.prototype.createElement;
  Document.prototype.createElement = function (...args) {
    return adopt(create.apply(this, args));
  };
  const play = HTMLMediaElement.prototype.play;
  HTMLMediaElement.prototype.play = function (...args) {
    adopt(this);
    apply(this);
    return play.apply(this, args);
  };
  const live = () => {
    for (const el of document.querySelectorAll('audio, video')) adopt(el);
    const out = [];
    for (const r of refs) {
      const el = r.deref();
      if (el) out.push(el);
      else refs.delete(r);
    }
    return out;
  };
  // The player you hear: not muted, playing before paused, then the longest
  // (Spotify also loops a short, silent cover video). Paused counts too, so
  // the speed it will resume at is known.
  const heard = () =>
    live()
      .filter((el) => !el.muted && el.volume > 0 && (el.duration || 0) > 0)
      .sort((a, b) => a.paused - b.paused || (b.duration || 0) - (a.duration || 0))[0];

  let timer = 0;
  const stop = () => {
    if (fill === 1) return;
    fill = 1;
    clearTimeout(timer);
    for (const el of live()) apply(el);
  };
  const state = () => {
    const el = heard();
    return el
      ? { speed: RATE.get.call(el), pitch: PITCH.get.call(el), page: want(el).rate, playing: !el.paused }
      : { speed: 1, pitch: true, page: 1, playing: false };
  };

  window.__glowtuneRate = {
    /** Play `speed`× (pitch raised) for `ms`, unless renewed. */
    fill(speed, ms) {
      const s = Math.min(1.5, Math.max(1, Number(speed) || 1));
      if (s === 1) return stop(), state();
      fill = s;
      clearTimeout(timer);
      timer = setTimeout(stop, ms);
      for (const el of live()) apply(el);
      return state();
    },
    stop() {
      stop();
      return state();
    },
    state,
  };
})();
