// Every setting in the Settings window, for its search box: individual
// settings (theme editor included), not just sections, found even with
// typos ("revers moton" → Reverse motion). Pure TypeScript, no imports, so
// `node --test scripts/settings-search.test.ts` can run it.

export type SettingEntry = {
  /** Settings section the setting is in. */
  section: string;
  /** Its name, as the search result shows it. */
  name: string;
  /** The label on the page, to scroll to and highlight (defaults to `name`). */
  label?: string;
  /** Other words people might type. */
  words?: string;
};

export const SECTION_NAMES: Record<string, string> = {
  lyrics: "Lyrics",
  scene: "Desktop scene",
  glow: "Edge glow",
  sound: "Sound",
  services: "Services",
  themes: "Themes",
  store: "Store",
  player: "Player",
  listening: "Listening",
  keys: "Hotkeys",
  feedback: "Feedback",
  about: "About",
};

export const SETTINGS: SettingEntry[] = [
  // Lyrics
  { section: "lyrics", name: "Lyrics on desktop", words: "floating lyrics show hide text words on off" },
  { section: "lyrics", name: "Desktop lyrics look", label: "Desktop look", words: "stars rain shards code melt style floating" },
  { section: "lyrics", name: "Show lyrics over full-screen games", label: "Show over full-screen games", words: "fullscreen game video on top" },
  { section: "lyrics", name: "Hide explicit words", words: "swear clean censor curse profanity explicit" },
  { section: "lyrics", name: "Move lyrics", label: "Position on screen", words: "drag place move reset position location" },
  { section: "lyrics", name: "Lyrics timing", label: "Timing for this song", words: "sync offset early late delay earlier later out of sync" },
  { section: "lyrics", name: "Sync lyrics", label: "Sync", words: "sync reset zero align line up fix timing out of sync" },
  { section: "lyrics", name: "Lyrics animation", label: "Animation", words: "rise karaoke still static motion effect" },
  { section: "lyrics", name: "Lyrics position", label: "Position", words: "top middle bottom center place" },
  { section: "lyrics", name: "Lyrics size", label: "Size", words: "text size bigger smaller font size" },
  { section: "lyrics", name: "Lyrics font", label: "Font", words: "typeface font family georgia segoe" },
  { section: "lyrics", name: "Lyrics style in the desktop scene", label: "Style in the desktop scene", words: "bold flow karaoke outline sticker stars depth rain orbit shards tags code melt" },
  // Desktop scene
  { section: "scene", name: "Desktop scene", words: "wallpaper background live stage full screen f11" },
  { section: "scene", name: "What the scene shows", label: "Show", words: "clock lyrics recent up next dock motion emoji beat pulse" },
  { section: "scene", name: "Scene screen", label: "Screen", words: "monitor display all screens second screen" },
  // Edge glow
  { section: "glow", name: "Edge glow", words: "light rgb ambient glow border on off" },
  { section: "glow", name: "Hide glow over full-screen apps", label: "Hide over fullscreen apps and games", words: "fullscreen game hide glow" },
  { section: "glow", name: "Screens that glow", words: "monitor display screens" },
  // Sound
  { section: "sound", name: "Equalizer", words: "eq bass treble presets sound audio" },
  { section: "sound", name: "Equalizer preamp", label: "Preamp", words: "gain loudness volume eq" },
  { section: "sound", name: "Between songs", words: "crossfade automix dj mix transition fade blend gapless" },
  { section: "sound", name: "Crossfade length", label: "Length", words: "crossfade seconds fade duration" },
  { section: "sound", name: "Volume tip", label: "Between songs", words: "volume quiet loud slider" },
  // Services
  { section: "services", name: "Your music services", words: "spotify youtube music apple music soundcloud sign in login account" },
  { section: "services", name: "Block ads and trackers", words: "adblock ads advertisements skip" },
  // Themes (theme editor)
  { section: "themes", name: "Use the theme's colors", label: "Use the theme’s colors", words: "album art colors adaptive colours" },
  { section: "themes", name: "Themes", words: "theme pick choose look appearance" },
  { section: "themes", name: "Import or export a theme", label: "Import theme", words: "import export share file toml" },
  { section: "themes", name: "Theme name", label: "Name", words: "rename" },
  { section: "themes", name: "Theme colors", label: "Colors", words: "colours palette static gradient" },
  { section: "themes", name: "Glass tint", words: "background window glass colour" },
  { section: "themes", name: "Text color", label: "Text", words: "text colour ink" },
  { section: "themes", name: "Glass tint strength", words: "opacity transparency see through" },
  { section: "themes", name: "Corner radius", words: "rounded corners round" },
  { section: "themes", name: "Light formation", words: "glow shape effect ambient still pulse orbit lava lamp comet aurora bars neon tide" },
  { section: "themes", name: "Formation speed", words: "glow speed fast slow" },
  { section: "themes", name: "Formation strength", words: "glow strength intensity" },
  { section: "themes", name: "Reverse motion", words: "direction backwards reverse glow" },
  { section: "themes", name: "Glow brightness", label: "Brightness", words: "bright dim light" },
  { section: "themes", name: "Glow thickness", label: "Thickness", words: "width thick thin glow size" },
  { section: "themes", name: "Beat reaction", words: "beat pulse bass react music" },
  { section: "themes", name: "Color drift", words: "colour drift shift change" },
  { section: "themes", name: "Rounded glow corners", words: "corners round" },
  { section: "themes", name: "Particle style", words: "particles dots stars dust embers snow fireflies bubbles" },
  { section: "themes", name: "Particle amount", words: "particles count density how many" },
  { section: "themes", name: "Particle size", words: "particles big small" },
  { section: "themes", name: "Particle speed", words: "particles fast slow" },
  { section: "themes", name: "Sparkle", words: "particles twinkle shine" },
  { section: "themes", name: "Interface font", words: "font typeface player ui" },
  // Store
  { section: "store", name: "Store", words: "crate themes scene looks presets install download surprise me" },
  // Player
  { section: "player", name: "Player size", label: "Size", words: "mini player small medium large" },
  { section: "player", name: "Taskbar player", words: "taskbar bar bottom" },
  { section: "player", name: "Player look", label: "Look", words: "glass disc card pill lyric clear mini player style" },
  { section: "player", name: "Keep player on top", words: "always on top pin float" },
  // Listening
  { section: "listening", name: "Listening stats", words: "stats wrapped top songs artists hours played history" },
  { section: "listening", name: "Keep listening history", words: "history log delete privacy" },
  // Hotkeys
  { section: "keys", name: "Hotkeys", words: "shortcuts keyboard keys ctrl alt play pause next volume presets sleep timer open settings lyrics timing earlier later sync reset" },
  // About
  { section: "about", name: "Updates", words: "update new version upgrade check for updates release notes" },
  { section: "about", name: "Check for updates automatically", label: "Check automatically", words: "update auto" },
  { section: "about", name: "Report a bug", label: "Found a bug?", words: "bug report issue help contact" },
  // Feedback
  { section: "feedback", name: "Send feedback", label: "Give us your feedback", words: "feedback message bug report complaint idea feature request review contact support email help" },
  { section: "feedback", name: "Rate Glowtune", label: "Other ways", words: "rate review stars microsoft store github issue" },
  { section: "about", name: "Version", label: "Glowtune", words: "about version" },
];

// ---- Matching --------------------------------------------------------------

function words(s: string): string[] {
  return s
    .toLowerCase()
    .replace(/[’']/g, "")
    .split(/[^\p{L}\p{N}]+/u)
    .filter(Boolean);
}

/** Damerau–Levenshtein distance (adjacent swaps count once), capped at `max + 1`. */
export function distance(a: string, b: string, max = 3): number {
  if (Math.abs(a.length - b.length) > max) return max + 1;
  const d: number[][] = Array.from({ length: a.length + 1 }, (_, i) => [i, ...Array(b.length).fill(0)]);
  for (let j = 1; j <= b.length; j++) d[0][j] = j;
  for (let i = 1; i <= a.length; i++) {
    let best = Infinity;
    for (let j = 1; j <= b.length; j++) {
      const cost = a[i - 1] === b[j - 1] ? 0 : 1;
      d[i][j] = Math.min(d[i - 1][j] + 1, d[i][j - 1] + 1, d[i - 1][j - 1] + cost);
      if (i > 1 && j > 1 && a[i - 1] === b[j - 2] && a[i - 2] === b[j - 1]) d[i][j] = Math.min(d[i][j], d[i - 2][j - 2] + 1);
      best = Math.min(best, d[i][j]);
    }
    if (best > max) return max + 1;
  }
  return d[a.length][b.length];
}

/** Typos allowed for a typed word of this length. */
function allowed(len: number, loose: boolean): number {
  const strict = len <= 3 ? 0 : len <= 6 ? 1 : 2;
  return loose && len >= 3 ? strict + 1 : strict;
}

/** "particles" and "particle" are the same word to a search. */
const singular = (w: string) => (w.length > 3 && w.endsWith("s") && !w.endsWith("ss") ? w.slice(0, -1) : w);

/** How well one typed word matches one word of a setting, 0..1. */
function wordScore(q: string, w: string, loose: boolean): number {
  if (q === w || singular(q) === singular(w)) return 1;
  if (w.startsWith(q) && q.length >= 2) return 0.9;
  q = singular(q);
  w = singular(w);
  const max = allowed(q.length, loose);
  if (max === 0) return 0;
  const whole = distance(q, w, max);
  // Still typing: compare with the start of the word ("partcl" → "particle").
  const start = w.length > q.length ? distance(q, w.slice(0, q.length), max) : max + 1;
  const d = Math.min(whole, start + 0.5);
  return d <= max ? 0.75 - 0.12 * d : 0;
}

export type SettingHit = SettingEntry & { score: number };

function score(entry: SettingEntry, typed: string[], phrase: string, loose: boolean): number {
  const name = words(entry.name);
  const extra = [...words(entry.words ?? ""), ...words(SECTION_NAMES[entry.section] ?? "")];
  let total = 0;
  let matched = 0;
  for (const q of typed) {
    let best = 0;
    for (const w of name) best = Math.max(best, wordScore(q, w, loose));
    for (const w of extra) best = Math.max(best, 0.85 * wordScore(q, w, loose));
    if (best > 0) matched++;
    total += best;
  }
  // Every typed word must match, or (did-you-mean) at least half of them.
  if (matched < (loose ? Math.ceil(typed.length / 2) : typed.length)) return 0;
  let s = total / typed.length;
  if (entry.name.toLowerCase().includes(phrase)) s += 0.5;
  else if (name.join(" ").startsWith(typed.join(" "))) s += 0.3;
  return s;
}

function rank(query: string, loose: boolean, limit: number): SettingHit[] {
  const typed = words(query);
  if (!typed.length) return [];
  const phrase = typed.join(" ");
  return SETTINGS.map((e) => ({ ...e, score: score(e, typed, phrase, loose) }))
    .filter((h) => h.score > (loose ? 0.2 : 0.35))
    // Ties: the shorter, more general setting first ("Updates" before "Check for updates automatically").
    .sort((a, b) => b.score - a.score || a.name.length - b.name.length || a.name.localeCompare(b.name))
    .slice(0, limit);
}

/** Settings matching what was typed, best first. */
export function searchSettings(query: string, limit = 8): SettingHit[] {
  return rank(query, false, limit);
}

/** Close guesses for "Did you mean…?" when `searchSettings` finds nothing. */
export function didYouMean(query: string, limit = 3): SettingHit[] {
  return rank(query, true, limit);
}
