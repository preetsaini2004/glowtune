const tracks = [
  ["After the rain", "Night Signal", "#ffb347", "#2ed3b7"],
  ["Soft current", "Night Signal", "#54dedb", "#9983ff"],
  ["Golden hourglass", "Paper Satellites", "#f7ad6e", "#ef6a83"],
  ["Tideglass", "Low Meridian", "#73c9ff", "#5e76ef"],
  ["Still electric", "Night Signal", "#f28ac5", "#a46fff"],
  ["Porchlight", "Paper Satellites", "#f4cb8c", "#8caa65"],
  ["Low clouds", "Low Meridian", "#abcaff", "#af91c9"],
  ["Glass harbor", "Night Signal", "#fdb26e", "#68dacc"],
];

function cover(index, warm, cool) {
  const rings = Array.from(
    { length: 17 },
    (_, i) =>
      '<ellipse cx="256" cy="300" rx="' +
      (79 + i * 12) +
      '" ry="' +
      (18 + i * 4) +
      '" fill="none" stroke="' +
      (i % 3 === 0 ? cool : warm) +
      '" opacity="' +
      (0.7 - i * 0.027) +
      '"/>',
  ).join("");
  const stars = Array.from(
    { length: 80 },
    (_, i) =>
      '<circle cx="' +
      ((i * 263 + index * 37) % 512) +
      '" cy="' +
      ((i * 103 + 77) % 320) +
      '" r="' +
      (i % 9 ? 0.65 : 1.3) +
      '" fill="#fff" opacity="' +
      (0.12 + (i % 5) / 14) +
      '"/>',
  ).join("");
  const svg =
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 512 512"><defs>' +
    '<radialGradient id="sky" cx=".51" cy=".42" r=".62"><stop stop-color="' +
    cool +
    '" stop-opacity=".72"/>' +
    '<stop offset="1" stop-color="#0b1325"/></radialGradient><linearGradient id="sun" x2="0" y2="1">' +
    '<stop stop-color="#fff3cb"/><stop offset=".56" stop-color="' +
    warm +
    '"/><stop offset="1" stop-color="#cf614d"/></linearGradient>' +
    '<clipPath id="horizon"><path d="M0 296h512v216H0z"/></clipPath></defs>' +
    '<path d="M0 0h512v512H0z" fill="url(#sky)"/>' +
    stars +
    '<circle cx="' +
    (256 + ((index % 3) - 1) * 42) +
    '" cy="222" r="91" fill="url(#sun)"/>' +
    '<path d="M0 318 100 253l51 46 36-20 47 35 109-59 67 37 102-61v281H0z" fill="#0b1724"/>' +
    '<g clip-path="url(#horizon)" transform="rotate(-4 256 310)">' +
    rings +
    "</g>" +
    '<path d="M29 29h454v454H29z" fill="none" stroke="#fff" opacity=".25"/>' +
    '<text x="46" y="66" fill="#fff7e9" font-family="sans-serif" font-size="13" letter-spacing="3">NIGHT SIGNAL</text>' +
    '<text x="46" y="454" fill="#fff7e9" font-family="sans-serif" font-size="10" letter-spacing="3">STUDIES IN LIGHT / 0' +
    (index + 1) +
    "</text></svg>";
  return "data:image/svg+xml," + encodeURIComponent(svg);
}

export const songs = tracks.map(([title, artist, warm, cool], index) => ({
  title,
  artist,
  album: "Studies in light",
  source: "Glowtune demo",
  durationMs: 192000,
  palette: [warm, cool],
  artUrl: cover(index, warm, cool),
}));
export const words = [
  "Amber on the windowsill",
  "The city folds into blue",
  "Every little light holds still",
  "The room keeps time with you",
  "Where the water meets the sky",
  "Let the evening settle in",
  "A little color finds the dark",
  "And all the quiet starts to sing",
  "Stay until the sky turns gold",
];
export const lyrics = {
  status: "synced",
  offsetMs: 0,
  lines: words.map((text, index) => {
    const timeMs = index * 2350;
    return {
      timeMs,
      text,
      words: text
        .split(" ")
        .map((word, i) => ({ text: word, timeMs: timeMs + i * 290 })),
    };
  }),
};

const base = {
  meta: { name: "Midnight Glass", author: "Glowtune", version: 1 },
  colors: {
    mode: "adaptive",
    background: "#0b0b10",
    surface: "#ffffff14",
    text: "#f5f5f7",
    accent: "#f0a24a",
    glow: ["#ffb347", "#2ed3b7"],
  },
  glass: { blur: 24, opacity: 0.78, radius: 14 },
  font: {
    family: "Instrument Sans",
    lyrics_family: "Bricolage Grotesque",
    lyrics_size: 50,
  },
  glow: {
    enabled: true,
    thickness: 56,
    intensity: 1,
    beat_reactivity: 0.7,
    drift_speed: 0.6,
    effect: "ambient",
    effect_speed: 1,
    effect_strength: 1,
    reverse_motion: false,
    particles: "off",
    particle_density: 0.4,
    particle_size: 1,
    particle_speed: 1,
    particle_sparkle: 0.2,
    corners: "rounded",
    monitors: "all",
  },
  lyrics: { style: "rise", position: "bottom-center", show_on_desktop: false },
};
export const themes = {
  "midnight-glass": base,
  aurora: {
    ...base,
    meta: { ...base.meta, name: "Aurora" },
    colors: {
      ...base.colors,
      mode: "gradient",
      accent: "#5ddac6",
      glow: ["#5ddac6", "#6799ff", "#ba8bff"],
    },
  },
  ember: {
    ...base,
    meta: { ...base.meta, name: "Ember" },
    colors: {
      ...base.colors,
      mode: "static",
      accent: "#ff965d",
      glow: ["#ff965d"],
    },
    glow: { ...base.glow, corners: "square" },
  },
};

export function installFixture({ songs, lyrics, themes, shot, wallpaper }) {
  if (window !== window.top) return;
  const serviceNames = {
    spotify: "Spotify",
    "apple-music": "Apple Music",
    "youtube-music": "YouTube Music",
    soundcloud: "SoundCloud",
  };
  const items = Object.keys(serviceNames).flatMap((service, shelf) =>
    songs.slice(shelf, shelf + 5).map((song, i) => ({
      service,
      title: song.title,
      artist: song.artist,
      artwork: song.artUrl,
      kind: "song",
      section: shelf === 0 ? "Evening rotation" : "Discovery mix",
      url: "demo:" + service + "/" + i,
    })),
  );
  let active = "midnight-glass";
  let settings = {
    artworkColors: true,
    customColors: ["#ffb347", "#2ed3b7"],
    glowEnabled: true,
    glowMonitors: [],
    glowHideFullscreen: true,
    stageEnabled: true,
    stageClock: true,
    stageClockStyle: "outline",
    stageLyrics: true,
    stageLyricsStyle: "flow",
    stageWallpaper: wallpaper ? "video" : "art",
    stageRecent: true,
    stageUpcoming: true,
    stageMotion: true,
    stageDock: true,
    stageMonitor: "",
    blockAds: true,
    useTheme: true,
    disabledThemes: [],
  };
  let current = 0;
  let playback = {
    status: wallpaper || shot === "glow" ? "paused" : "playing",
    positionMs: wallpaper?.startMs ?? 0,
    rate: 1,
    reportedAtMs: Date.now(),
    shuffle: false,
    repeat: "off",
  };
  const callbacks = new Map();
  const listeners = new Map();
  const emit = (event, payload) => {
    for (const [id, name] of listeners) {
      if (name === event) callbacks.get(id)?.({ event, id, payload });
    }
  };
  const selectSong = (index) => {
    current = (songs.length + index) % songs.length;
    playback = {
      ...playback,
      positionMs: 0,
      reportedAtMs: Date.now(),
      status: "playing",
    };
    emit("np-track", songs[current]);
    emit("np-playback", playback);
  };
  // Freeze the app clock to a fictional evening. Relative timers still advance.
  const RealDate = Date;
  const shift = RealDate.parse("2026-01-16T21:24:00Z") - RealDate.now();
  window.Date = class extends RealDate {
    constructor(...args) {
      super(...(args.length ? args : [RealDate.now() + shift]));
    }
    static now() {
      return RealDate.now() + shift;
    }
  };
  playback.reportedAtMs = Date.now();
  window.capturePlayback = () => ({
    ...playback,
    palette: songs[current].palette,
    energy:
      wallpaper?.pulse[
        Math.max(
          0,
          Math.min(
            wallpaper.pulse.length - 1,
            Math.floor(
              (playback.positionMs +
                (playback.status === "playing"
                  ? Date.now() - playback.reportedAtMs
                  : 0) -
                wallpaper.musicStartMs) *
                0.03,
            ),
          ),
        )
      ],
  });

  let nextCallback = 1;
  window.__TAURI_EVENT_PLUGIN_INTERNALS__ = {
    unregisterListener: (_event, id) => listeners.delete(id),
  };
  window.__TAURI_INTERNALS__ = {
    metadata: {
      currentWindow: { label: "stage" },
      currentWebview: { label: "stage" },
    },
    transformCallback: (fn) => {
      callbacks.set(nextCallback, fn);
      return nextCallback++;
    },
    unregisterCallback: (id) => callbacks.delete(id),
    async invoke(command, args = {}) {
      switch (command) {
        case "plugin:event|listen":
          listeners.set(args.handler, args.event);
          return args.handler;
        case "plugin:event|unlisten":
          listeners.delete(args.eventId);
          return;
        case "plugin:app|version":
          return "0.1.0";
        case "get_stage_state":
          return [false, false];
        case "get_settings":
          return settings;
        case "set_settings":
          settings = args.new;
          emit("settings", settings);
          return settings;
        case "get_state":
          return { track: songs[current], playback };
        case "get_history":
          return songs.slice(3, 8);
        case "music_video":
          return wallpaper && current === 0 ? "hjr4wfnVmpE" : null;
        case "get_lyrics":
          return wallpaper
            ? { status: "none", offsetMs: 0, lines: [] }
            : lyrics;
        case "get_volume":
          return 0.72;
        case "get_theme":
          return themes[active];
        case "active_theme_id":
          return active;
        case "list_themes":
          return Object.entries(themes).map(([id, theme]) => ({
            id,
            name: theme.meta.name,
            author: "Glowtune",
            bundled: true,
            swatch: theme.colors.glow,
          }));
        case "select_theme":
          active = args.id;
          emit("theme", themes[active]);
          return themes[active];
        case "list_monitors":
          return [
            {
              name: "Landscape display",
              primary: true,
              width: 1920,
              height: 1080,
            },
            {
              name: "Portrait display",
              primary: false,
              width: 1080,
              height: 1920,
            },
          ];
        case "get_music_queue":
          return {
            items: items.filter((item) => item.service === "spotify").slice(1),
            message: "",
          };
        case "refresh_music_library":
        case "get_music_library":
          return Object.entries(serviceNames).map(([service, name]) => ({
            service,
            name,
            status: "ready",
            items: items.filter((item) => item.service === service),
            updatedAt: Date.now(),
          }));
        case "search_catalog":
          return {
            songs: songs
              .filter((song) =>
                song.title
                  .toLowerCase()
                  .includes(args.query?.toLowerCase() ?? ""),
              )
              .map((song) => ({
                title: song.title,
                subtitle: song.artist,
                artwork: song.artUrl,
                query: song.title,
                kind: "song",
              })),
            albums: [],
            artists: [],
          };
        case "play_on":
        case "open_music_item":
          selectSong(
            songs.findIndex(
              (song) => song.title === (args.item?.title ?? args.query),
            ),
          );
          return;
        case "adblock_stats":
          return 0;
        case "control":
          if (args.action === "next") selectSong(current + 1);
          if (args.action === "previous") selectSong(current - 1);
          if (args.action === "playPause") {
            const playing = playback.status === "playing";
            const now = Date.now();
            playback = {
              ...playback,
              status: playing ? "paused" : "playing",
              reportedAtMs: now,
              positionMs:
                playback.positionMs +
                (playing ? now - playback.reportedAtMs : 0),
            };
            emit("np-playback", playback);
          }
          return;
        default:
          throw new Error("Unexpected capture command " + command);
      }
    },
  };

  // A visible capture pointer follows actual browser input.
  document.addEventListener("DOMContentLoaded", () => {
    const pointer = document.createElement("div");
    Object.assign(pointer.style, {
      width: "17px",
      height: "17px",
      border: "2px solid #fff",
      borderRadius: "50%",
      position: "fixed",
      pointerEvents: "none",
      display: "none",
      zIndex: 200000,
      background: "#10171db0",
      boxShadow: "0 1px 8px #000b",
      transform: "translate(-50%, -50%)",
    });
    document.body.append(pointer);
    window.addEventListener("pointermove", (e) => {
      Object.assign(pointer.style, {
        display: "block",
        left: e.clientX + "px",
        top: e.clientY + "px",
      });
    });
    window.addEventListener("pointerdown", () => {
      pointer.style.background = "#f0a24a";
    });
    window.addEventListener("pointerup", () => {
      pointer.style.background = "#10171db0";
    });
  });
}
