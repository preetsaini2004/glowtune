export const FPS = 30;
export const HERO_SECONDS = 70;
export const CLIP_SECONDS = 7;
export const BEAT_SECONDS = 0.5;

export const features = {
  glow: {
    label: "Edge glow",
    caption: "Your screen glows with the album’s colors.",
  },
  scene: {
    label: "Desktop scene",
    caption: "Play the music video behind your desktop.",
  },
  lyrics: { label: "Lyrics view", caption: "Lyrics in time, word by word." },
  layout: { label: "Edit layout", caption: "Move and resize your scene." },
  search: { label: "Dock search", caption: "Find something to play." },
  services: { label: "Music services", caption: "Choose where to play." },
  upnext: { label: "Up next", caption: "See what’s coming next." },
  controls: {
    label: "Playback controls",
    caption: "Play, pause, or skip from the keyboard.",
  },
  themes: { label: "Themes", caption: "Change the mood with a theme." },
  screens: {
    label: "All screens",
    caption: "Bring the scene to every display.",
  },
} as const;

export type Feature = keyof typeof features;

export const timeline = [
  { id: "opening", start: 0, end: 4 },
  { id: "promise", start: 4, end: 9 },
  { id: "glow", start: 9, end: 18 },
  { id: "scene", start: 18, end: 28, trim: 14 },
  { id: "cinema", start: 28, end: 36 },
  { id: "layout", start: 36, end: 44 },
  { id: "search", start: 44, end: 48, trim: 2.3 },
  { id: "services", start: 48, end: 52, trim: 2.8 },
  { id: "controls", start: 52, end: 58 },
  { id: "privacy", start: 58, end: 62 },
  { id: "end", start: 62, end: HERO_SECONDS },
] as const;
