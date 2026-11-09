// SoundCloud autoplay: with nothing queued, SoundCloud picks the next song from
// the current song's "station" (related tracks) only when this one ends, so its
// queue shows nothing upcoming. This reads that station so Up next can show
// what autoplay picks from.
//
//   song page ─► its id + the site's API key ─► station page ─► 50 ids ─► API: names, covers
//
// ExecuteScript can't wait for promises: the first call starts the work and
// returns {pending: true}; later calls return the stored result.
(() => {
  const link = document.querySelector('.playbackSoundBadge__titleLink');
  const seed = link?.href?.split('?')[0];
  if (!seed) return { pending: false, items: [] };
  const saved = window.__glowtuneStation;
  if (saved && saved.seed === seed) return saved.done ? { pending: false, items: saved.items } : { pending: true };
  const state = (window.__glowtuneStation = { seed, done: false, items: [] });
  const hydration = (html) => {
    const m = html.match(/window\.__sc_hydration\s*=\s*(\[[\s\S]*?\]);\s*<\/script>/);
    return m ? JSON.parse(m[1]) : [];
  };
  const find = (h, kind) => h.find((x) => x.hydratable === kind)?.data;
  (async () => {
    const page = hydration(await fetch(seed).then((r) => r.text()));
    const sound = find(page, 'sound');
    const key = find(page, 'apiClient')?.id;
    if (!sound?.id || !sound.station_permalink || !key) return;
    const station = find(hydration(await fetch('https://soundcloud.com/discover/sets/' + encodeURIComponent(sound.station_permalink)).then((r) => r.text())), 'systemPlaylist');
    const ids = (station?.tracks || []).map((t) => t.id).filter((id) => Number.isInteger(id) && id !== sound.id).slice(0, 20);
    if (!ids.length) return;
    const tracks = await fetch(`https://api-v2.soundcloud.com/tracks?ids=${ids.join(',')}&client_id=${encodeURIComponent(key)}`).then((r) => r.json());
    const byId = new Map((Array.isArray(tracks) ? tracks : []).map((t) => [t.id, t]));
    state.items = ids.map((id) => byId.get(id)).filter((t) => t?.title && t.permalink_url).map((t) => ({
      service: 'soundcloud',
      kind: 'song',
      title: String(t.title).slice(0, 180),
      artist: String(t.user?.username || '').slice(0, 180),
      artwork: (t.artwork_url || t.user?.avatar_url || '').replace(/-large\./, '-t300x300.') || null,
      url: t.permalink_url,
      section: null,
    }));
  })().catch(() => {}).finally(() => { state.done = true; });
  return { pending: true };
})()
