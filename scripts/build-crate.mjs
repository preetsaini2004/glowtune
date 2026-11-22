// Builds app/src-tauri/store/catalog.json (the Crate: themes and scene looks).
//
//   node scripts/build-crate.mjs
//
// Edit the packs here, not the JSON: themes are written out as TOML for you.
// `cargo test -p glowtune store` checks that every pack is accepted (a pack
// the app rejects would quietly disappear from the Crate).
//
// Sample lines are original. Never use real song lyrics.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

const BASE = {
  glass: { blur: 24, opacity: 0.6, radius: 18 },
  font: { family: "Instrument Sans", lyrics_family: "Bricolage Grotesque", lyrics_size: 50 },
  glow: {
    enabled: true, thickness: 56, intensity: 1, beat_reactivity: 0.6, drift_speed: 0.6,
    effect: "ambient", effect_speed: 1, effect_strength: 1, reverse_motion: false,
    particles: "off", particle_density: 0.35, particle_size: 1, particle_speed: 1, particle_sparkle: 0.5,
    corners: "rounded", monitors: "all",
  },
  lyrics: { style: "rise", position: "bottom-center", show_on_desktop: true },
};

// ── Themes ──────────────────────────────────────────────
// colors: [mode, background, text, glow colours…]; the accent is the first glow colour.
const THEMES = [
  {
    id: "after-hours", name: "After hours", tags: ["night", "neon"],
    description: "Violet neon and taxi-light blue, with two comets racing round the edge. One more track before dawn.",
    sample: "One more song, then home",
    colors: ["gradient", "#0d091b", "#f5edff", "#b58aff", "#55cfff", "#ff63b5"],
    glass: { blur: 36, opacity: 0.5, radius: 24 },
    glow: { effect: "comet", effect_speed: 0.9, thickness: 64, intensity: 1.1, beat_reactivity: 0.8, drift_speed: 1.6 },
  },
  {
    id: "needle-dust", name: "Needle dust", tags: ["warm", "vinyl"],
    description: "Honeyed glass, paper lyrics and a little dust drifting in the light.",
    sample: "Drop the needle and stay",
    colors: ["static", "#20150f", "#fae9cd", "#e2ac6a", "#b96c46", "#f1dbb6"],
    glass: { blur: 16, opacity: 0.76, radius: 10 },
    font: { lyrics_family: "Georgia", lyrics_size: 46 },
    glow: { effect: "ambient", effect_speed: 0.4, thickness: 50, intensity: 0.8, beat_reactivity: 0.35, drift_speed: 0.2, particles: "dust", particle_density: 0.3, particle_speed: 0.5 },
    lyrics: { style: "static", position: "center" },
  },
  {
    id: "control-room", name: "Control room", tags: ["mono", "studio"],
    description: "Silver light, smoked glass and a meter bridge that moves with the mix.",
    sample: "Take it from the top",
    colors: ["adaptive", "#101419", "#f5f7fa", "#dce5eb", "#8295a5"],
    glass: { blur: 24, opacity: 0.72, radius: 4 },
    font: { lyrics_family: "Instrument Sans", lyrics_size: 40 },
    glow: { effect: "bars", effect_speed: 1, thickness: 40, intensity: 0.7, beat_reactivity: 0.7, drift_speed: 0.1, corners: "square" },
    lyrics: { style: "static", position: "top-center" },
  },
  {
    id: "last-light", name: "Last light", tags: ["sunset", "soft"],
    description: "Apricot melts into rose, and a few embers drift up. Keep the golden hour playing.",
    sample: "Hold the gold a little longer",
    colors: ["gradient", "#21121b", "#fff0e8", "#ffad79", "#ef739e", "#9b85e4"],
    glass: { blur: 40, opacity: 0.48, radius: 28 },
    font: { lyrics_size: 56 },
    glow: { effect: "lava-lamp", effect_speed: 0.6, thickness: 82, intensity: 1, beat_reactivity: 0.5, drift_speed: 0.65, particles: "embers", particle_density: 0.25, particle_speed: 0.6 },
  },
  {
    id: "blue-hour", name: "Blue hour", tags: ["ocean", "calm"],
    description: "Deep-water glass. Slow tides roll up the sides and bubbles rise with the beat.",
    sample: "Down where the light goes slow",
    colors: ["gradient", "#051923", "#e2fcff", "#3dd9c4", "#3989ee", "#6670d6"],
    glass: { blur: 48, opacity: 0.6, radius: 18 },
    font: { lyrics_family: "Segoe UI", lyrics_size: 48 },
    glow: { effect: "tide", effect_speed: 0.7, thickness: 74, intensity: 0.95, beat_reactivity: 0.4, drift_speed: 0.45, particles: "bubbles", particle_density: 0.3, particle_size: 1.4, particle_speed: 0.7 },
    lyrics: { style: "karaoke" },
  },
  {
    id: "sugar-static", name: "Sugar static", tags: ["pastel", "playful"],
    description: "Lilac candy, pistachio light and sprinkles that pop on every beat.",
    sample: "Sweet on the radio",
    colors: ["adaptive", "#231d30", "#fff5ff", "#d8b4f4", "#a9e3cb", "#ffc5d3"],
    glass: { blur: 32, opacity: 0.55, radius: 32 },
    font: { lyrics_size: 54 },
    glow: { effect: "pulse", thickness: 46, intensity: 0.8, beat_reactivity: 0.7, drift_speed: 0.4, particles: "dots", particle_density: 0.4, particle_sparkle: 0.8 },
    lyrics: { position: "center" },
  },
  {
    id: "acid-test", name: "Acid test", tags: ["club", "contrast"],
    description: "Hard lime, hot pink, sharp corners and a strobe of sparks. Built for the drop.",
    sample: "Wait for it, wait for the drop",
    colors: ["gradient", "#090c08", "#f7ffe8", "#d5ff46", "#ff4aa2", "#f5fff0"],
    glass: { blur: 8, opacity: 0.85, radius: 0 },
    font: { lyrics_family: "Instrument Sans", lyrics_size: 64 },
    glow: { effect: "distortion", effect_speed: 1.4, thickness: 100, intensity: 1.4, beat_reactivity: 1, drift_speed: 2.2, corners: "square", particles: "stars", particle_density: 0.45, particle_sparkle: 1, particle_speed: 1.6 },
    lyrics: { style: "karaoke", position: "center" },
  },
  {
    id: "moss-tape", name: "Moss tape", tags: ["forest", "earth"],
    description: "Fern green over dark bark, with fireflies after sundown. A slow walk with headphones.",
    sample: "Green all the way down",
    colors: ["static", "#121b15", "#eef2dc", "#9dc58b", "#b7a16d", "#537c68"],
    glass: { blur: 28, opacity: 0.68, radius: 16 },
    font: { lyrics_family: "Georgia", lyrics_size: 44 },
    glow: { effect: "ambient", effect_speed: 0.5, thickness: 52, intensity: 0.7, beat_reactivity: 0.2, drift_speed: 0.25, particles: "fireflies", particle_density: 0.3, particle_speed: 0.6 },
    lyrics: { style: "static" },
  },
  {
    id: "first-snow", name: "First snow", tags: ["winter", "quiet"], fresh: true,
    description: "Northern lights fold along the edge while snow falls through them. The street gone quiet.",
    sample: "Quiet as the first snow",
    colors: ["gradient", "#0a1220", "#eef6ff", "#a9e8ff", "#7fa8ff", "#c6b8ff"],
    glass: { blur: 44, opacity: 0.55, radius: 22 },
    font: { lyrics_size: 52 },
    glow: { effect: "aurora", effect_speed: 0.6, thickness: 80, intensity: 1, beat_reactivity: 0.35, drift_speed: 0.5, particles: "snow", particle_density: 0.45, particle_size: 1.2, particle_speed: 0.6 },
  },
  {
    id: "wet-asphalt", name: "Wet asphalt", tags: ["city", "neon"], fresh: true,
    description: "A thin neon tube inside the edge, pink and cyan, like a sign after the rain.",
    sample: "Neon in the puddles",
    colors: ["gradient", "#0b0d12", "#f4f7ff", "#ff4f9a", "#3fe0ff"],
    glass: { blur: 20, opacity: 0.7, radius: 8 },
    font: { lyrics_family: "Instrument Sans", lyrics_size: 54 },
    glow: { effect: "neon", effect_speed: 1, thickness: 60, intensity: 1.25, beat_reactivity: 0.8, drift_speed: 0.8 },
    lyrics: { style: "karaoke" },
  },
  {
    id: "low-orbit", name: "Low orbit", tags: ["space", "dreamy"], fresh: true,
    description: "A swell of light travels round the screen through a field of stars. Headphones on, gravity off.",
    sample: "Circling the dark side",
    colors: ["gradient", "#05060d", "#eef0ff", "#8e7dff", "#4fd2ff", "#ffffff"],
    glass: { blur: 40, opacity: 0.45, radius: 26 },
    font: { lyrics_size: 50 },
    glow: { effect: "orbit", effect_speed: 0.6, thickness: 70, intensity: 1, beat_reactivity: 0.6, drift_speed: 0.9, particles: "stars", particle_density: 0.5, particle_size: 0.8, particle_speed: 0.4, particle_sparkle: 0.9 },
    lyrics: { position: "center" },
  },
  {
    id: "campfire", name: "Campfire", tags: ["warm", "cosy"], fresh: true,
    description: "Flickering amber with sparks climbing off the edge. Pull a chair closer.",
    sample: "Gather round the sparks",
    colors: ["gradient", "#170b06", "#fff1e0", "#ff8a3d", "#ffcf6b", "#d6452f"],
    glass: { blur: 30, opacity: 0.65, radius: 20 },
    font: { lyrics_family: "Georgia", lyrics_size: 50 },
    glow: { effect: "lava-lamp", effect_speed: 1.2, thickness: 76, intensity: 1.15, beat_reactivity: 0.6, drift_speed: 1, particles: "embers", particle_density: 0.55, particle_speed: 1.3, particle_sparkle: 0.7 },
  },
];

// ── Scene looks ─────────────────────────────────────────
// Desktop scene settings, plus (optionally) the edge glow's shape and particles.
const LOOKS = [
  {
    id: "cinema", name: "Cinema", tags: ["video", "immersive"],
    description: "Let the music video fill the room. Floating words, a quiet clock and a thin line of a dock.",
    sample: "Roll the picture",
    colors: ["#b98bdf", "#79adcc"],
    settings: { stageWallpaper: "video", stageLyricsStyle: "flow", stageClockStyle: "minimal", stageArtStyle: "tilt", stageDockStyle: "line", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true },
  },
  {
    id: "poster", name: "Poster", tags: ["graphic", "bold"],
    description: "Big cut-out words on a printed cover, in ink and coral. Pasted up on the street.",
    sample: "Say it loud",
    colors: ["#ff936f", "#818eff"],
    settings: { stageWallpaper: "cover", stageLyricsStyle: "sticker", stageClockStyle: "solid", stageArtStyle: "print", stageDockStyle: "islands", stageMotion: false, stageClock: true, stageRecent: true, artworkColors: false, customColors: ["#ff936f", "#818eff"] },
  },
  {
    id: "slow-sunday", name: "Slow Sunday", tags: ["still", "warm"],
    description: "Soft silk, gentle karaoke and nowhere to rush.",
    sample: "Nowhere to be today",
    colors: ["#dab58b", "#a3b49a"],
    settings: { stageWallpaper: "silk", stageLyricsStyle: "karaoke", stageClockStyle: "minimal", stageArtStyle: "print", stageDockStyle: "pill", stageMotion: false, stageClock: true, stageRecent: false, artworkColors: false, customColors: ["#dab58b", "#a3b49a"] },
  },
  {
    id: "night-drive", name: "Night drive", tags: ["night", "motion"],
    description: "Outline lettering over drifting cobalt and cherry light, with headlights racing round the edge.",
    sample: "Headlights on the hill",
    colors: ["#558aff", "#fa668a"],
    settings: { stageWallpaper: "art", stageLyricsStyle: "outline", stageClockStyle: "outline", stageArtStyle: "glow", stageDockStyle: "line", stageMotion: true, stageClock: true, stageRecent: true, artworkColors: false, customColors: ["#558aff", "#fa668a"], glowEffect: "comet" },
  },
  {
    id: "liner-notes", name: "Liner notes", tags: ["quiet", "lyrics"],
    description: "Just the words and the cover. No clock, no distractions.",
    sample: "Read along with me",
    colors: ["#b9cad7", "#8493a6"],
    settings: { stageWallpaper: "cover", stageLyricsStyle: "flow", stageClockStyle: "minimal", stageArtStyle: "tilt", stageDockStyle: "pill", stageMotion: false, stageClock: false, stageRecent: false, artworkColors: true },
  },
  {
    id: "front-row", name: "Front row", tags: ["live", "video"],
    description: "Bold words over the music video, with emoji bursting off the big lines. Bring the show home.",
    sample: "Hands up in the front row",
    colors: ["#e6e88b", "#ea7fa9"],
    settings: { stageWallpaper: "video", stageLyricsStyle: "bold", stageClockStyle: "solid", stageArtStyle: "glow", stageDockStyle: "bar", stageEmoji: true, stageMotion: true, stageClock: true, stageRecent: true, artworkColors: true },
  },
  {
    id: "constellation", name: "Constellation", tags: ["stars", "lyrics"], fresh: true,
    description: "Every sung word lights up as a star and joins the last, until the line draws its own constellation. The record spins and the edge fills with stars.",
    sample: "Join the dots back to you",
    colors: ["#9fb4ff", "#f3d9a4"],
    settings: { stageWallpaper: "art", stageLyricsStyle: "stars", desktopLyricsStyle: "stars", stageClockStyle: "outline", stageArtStyle: "vinyl", stageDockStyle: "line", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true, glowParticles: "stars" },
  },
  {
    id: "rainy-window", name: "Rainy window", tags: ["rain", "calm"], fresh: true,
    description: "Water runs down over the cover and the words fall like rain. Wipe the glass clear with your mouse.",
    sample: "Rain on the window again",
    colors: ["#8fb3c9", "#5d7f9a"],
    settings: { stageWallpaper: "water", stageLyricsStyle: "rain", desktopLyricsStyle: "rain", stageBeat: true, stageClockStyle: "minimal", stageArtStyle: "glow", stageDockStyle: "pill", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true, glowEffect: "tide" },
  },
  {
    id: "frosted", name: "Frosted", tags: ["winter", "beat"], fresh: true,
    description: "The cover fogs over like a cold window and flashes on the kick. Snow drifts round the edge.",
    sample: "Draw a heart in the glass",
    colors: ["#cfe6ff", "#9db7e8"],
    settings: { stageWallpaper: "frost", stageLyricsStyle: "depth", stageBeat: true, stageClockStyle: "solid", stageArtStyle: "tilt", stageDockStyle: "islands", stageMotion: true, stageClock: true, stageRecent: true, artworkColors: true, glowEffect: "aurora", glowParticles: "snow" },
  },
  {
    id: "record-store", name: "Record store", tags: ["vinyl", "warm"], fresh: true,
    description: "The cover spins as the record on a tiled wall of colour, with dust in the light. Flip through the crates.",
    sample: "Flip it over to side B",
    colors: ["#e7b46a", "#c4553e"],
    settings: { stageWallpaper: "mosaic", stageLyricsStyle: "karaoke", stageBeat: true, stageClockStyle: "solid", stageArtStyle: "vinyl", stageDockStyle: "bar", stageMotion: true, stageClock: true, stageRecent: true, artworkColors: true, glowParticles: "dust" },
  },
  {
    id: "planetarium", name: "Planetarium", tags: ["space", "dreamy"], fresh: true,
    description: "Words orbit the middle of the screen while a swell of light circles the edge. Lights out, look up.",
    sample: "Lights out, look up",
    colors: ["#7f8cff", "#47d6c7"],
    settings: { stageWallpaper: "silk", stageLyricsStyle: "orbit", stageClockStyle: "minimal", stageArtStyle: "glow", stageDockStyle: "line", stageMotion: true, stageClock: false, stageRecent: false, artworkColors: false, customColors: ["#7f8cff", "#47d6c7", "#c48bff"], glowEffect: "orbit", glowParticles: "fireflies" },
  },
  {
    id: "basement-show", name: "Basement show", tags: ["club", "beat"], fresh: true,
    description: "Printed dots that swell on every kick, loud words, emoji and an equalizer round the edge. Turn it up.",
    sample: "Louder in the basement",
    colors: ["#d5ff46", "#ff4aa2"],
    settings: { stageWallpaper: "halftone", stageLyricsStyle: "bold", stageEmoji: true, stageBeat: true, stageClockStyle: "solid", stageArtStyle: "print", stageDockStyle: "islands", stageMotion: true, stageClock: true, stageRecent: true, artworkColors: false, customColors: ["#d5ff46", "#ff4aa2"], glowEffect: "bars" },
  },
  {
    id: "after-credits", name: "After credits", tags: ["dots", "beat"], fresh: true,
    description: "A field of dots in the song's colours, breathing slowly. Every kick drum sends a ring out across it, like the Glowtune film.",
    sample: "Stay through the credits",
    colors: ["#e2ac6a", "#3dd9c4"],
    settings: { stageWallpaper: "dots", stageLyricsStyle: "flow", stageBeat: true, stageClockStyle: "outline", stageArtStyle: "glow", stageDockStyle: "pill", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true },
  },
  {
    id: "broken-glass", name: "Broken glass", tags: ["shards", "bold"], fresh: true,
    description: "Every letter flies in as sharp slivers and snaps together on the word, over a cover printed in dots. Glass breaking backwards.",
    sample: "Pick up the pieces",
    colors: ["#ff5fa2", "#b6ff3d"],
    settings: { stageWallpaper: "halftone", stageLyricsStyle: "shards", desktopLyricsStyle: "shards", stageBeat: true, stageClockStyle: "solid", stageArtStyle: "print", stageDockStyle: "line", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true, glowEffect: "distortion" },
  },
  {
    id: "price-tag", name: "Price tag", tags: ["tags", "playful"], fresh: true,
    description: "The words hang on swinging white tags and roll like a counter onto every word. A shop window for your songs.",
    sample: "Hang on to this one",
    colors: ["#ff4fa3", "#ffd0e6"],
    settings: { stageWallpaper: "cover", stageLyricsStyle: "tags", stageClockStyle: "minimal", stageArtStyle: "tilt", stageDockStyle: "islands", stageMotion: true, stageClock: true, stageRecent: true, artworkColors: true, glowEffect: "pulse", glowParticles: "dots" },
  },
  {
    id: "decoder", name: "Decoder", tags: ["code", "dots"], fresh: true,
    description: "Each line waits as a row of glowing dots and decodes letter by letter as it's sung, over a field of dots that ripples on the beat.",
    sample: "Read me like a signal",
    colors: ["#ffd23d", "#5ee7ff"],
    settings: { stageWallpaper: "dots", stageLyricsStyle: "code", desktopLyricsStyle: "code", stageBeat: true, stageClockStyle: "outline", stageArtStyle: "glow", stageDockStyle: "line", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true, glowEffect: "neon" },
  },
  {
    id: "wet-paint", name: "Wet paint", tags: ["melt", "rain"], fresh: true,
    description: "Words are brushed on in streaks of colour and run down the screen like wet paint, while water runs over the cover.",
    sample: "Still drying on the wall",
    colors: ["#b6ff3d", "#ff5fa2"],
    settings: { stageWallpaper: "water", stageLyricsStyle: "melt", desktopLyricsStyle: "melt", stageBeat: true, stageClockStyle: "minimal", stageArtStyle: "glow", stageDockStyle: "pill", stageMotion: true, stageClock: true, stageRecent: false, artworkColors: true, glowEffect: "tide" },
  },
];

const value = (v) => (typeof v === "string" ? JSON.stringify(v) : Array.isArray(v) ? `[${v.map(value).join(", ")}]` : String(v));
const section = (name, fields) => `[${name}]\n${Object.entries(fields).map(([k, v]) => `${k} = ${value(v)}`).join("\n")}\n`;

function themePack(t) {
  const [mode, background, text, ...glow] = t.colors;
  const toml = [
    section("meta", { name: t.name, author: "Glowtune", version: 1 }),
    section("colors", { mode, background, surface: "#ffffff18", text, accent: glow[0], glow }),
    section("glass", { ...BASE.glass, ...t.glass }),
    section("font", { ...BASE.font, ...t.font }),
    section("glow", { ...BASE.glow, ...t.glow }),
    section("lyrics", { ...BASE.lyrics, ...t.lyrics }),
  ].join("\n");
  return pack(t, "theme", glow.slice(0, 4), { toml });
}

function pack(p, kind, colors, extra) {
  return {
    id: p.id, kind, name: p.name, author: "Glowtune", description: p.description,
    colors, tags: p.tags, ...extra, sample: p.sample, ...(p.fresh ? { fresh: true } : {}),
  };
}

const catalog = {
  version: 1,
  packs: [...THEMES.map(themePack), ...LOOKS.map((l) => pack(l, "preset", l.colors, { settings: l.settings }))],
};
const out = fileURLToPath(new URL("../app/src-tauri/store/catalog.json", import.meta.url));
writeFileSync(out, JSON.stringify(catalog, null, 2) + "\n");
console.log(`${catalog.packs.length} packs → ${out}`);
