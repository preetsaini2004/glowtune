// Capture-only backend. No service profiles, native hotkeys, or personal history.
export const newFeatures = [
  "presets",
  "hotkeys",
  "stars-lyrics",
  "beat-walls",
  "vinyl-dock",
  "lyric-card",
  "stats",
  "mini-player",
  "glow-shapes",
];

export const captureSettings = {
  activeTheme: "midnight-glass",
  stageLyricsStyle: "stars",
  stageArtStyle: "vinyl",
  stageDockStyle: "line",
  stageEmoji: false,
  stageBeat: true,
  stageClock: false,
  stageRecent: false,
  stageUpcoming: false,
  cleanLyrics: false,
  playerStyle: "glass",
  playerSize: "m",
  taskbarPlayer: "left",
  hotkeys: true,
  listeningHistory: true,
  lyricsOnDesktop: false,
  desktopLyricsStyle: "stars",
  scenePresets: [],
  eqEnabled: false,
  eqBands: [0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
  crossfadeEnabled: false,
  automixEnabled: false,
};

export function installProduction({ shot, pulse }) {
  const internal = window.__TAURI_INTERNALS__;
  const original = internal.invoke.bind(internal);
  const channel = new BroadcastChannel("glowtune-video-" + shot);
  const label =
    location.pathname === "/osd"
      ? "osd"
      : location.pathname === "/taskbar"
        ? "taskbar"
        : location.pathname === "/"
          ? "player"
          : "stage";
  internal.metadata.currentWindow.label = label;
  internal.metadata.currentWebview.label = label;
  let lastToast = null;
  let volume = 0.4;
  const receive = ({ event, payload }) => {
    if (event === "settings") original("set_settings", { new: payload });
    else {
      if (event === "osd") lastToast = payload;
      window.captureEmit(event, payload);
    }
  };
  channel.onmessage = ({ data }) => receive(data);
  const send = (event, payload) => {
    receive({ event, payload });
    channel.postMessage({ event, payload });
  };
  window.capturePatch = async (patch) => {
    const current = await original("get_settings");
    send("settings", { ...current, ...patch });
  };
  const toast = (icon, text, level = null) =>
    send("osd", { icon, text, level });
  window.captureStart = async () => {
    await original("control", { action: "playPause" });
    const state = await original("get_state");
    send("np-playback", state.playback);
    window.captureBeatStart = performance.now();
  };
  const style = new URL(location.href).searchParams.get("captureStyle");
  internal.invoke = async (command, args = {}) => {
    switch (command) {
      case "plugin:app|version":
        return "0.2.0";
      case "get_sleep_timer":
        return { endsAtMs: null, endOfSong: false };
      case "last_toast":
        return lastToast;
      case "hires_art":
        return (
          window.capturePlayback && (await original("get_state")).track.artUrl
        );
      case "adblock_rules":
        return 138000;
      case "hotkey_list":
        return [];
      case "want_kicks":
      case "taskbar_visible":
      case "show_player_window":
        return;
      case "get_volume":
        return volume;
      case "get_settings": {
        const settings = await original(command, args);
        return style ? { ...settings, playerStyle: style } : settings;
      }
      case "set_settings":
        send("settings", args.new);
        return args.new;
      case "toast":
        toast(args.icon, args.text);
        return;
      case "save_theme": {
        const theme = await original("get_theme");
        Object.assign(theme, args.theme);
        send("theme", theme);
        return theme;
      }
      case "save_scene_preset": {
        const settings = await original("get_settings");
        const { scenePresets, ...look } = settings;
        const preset = { name: args.name, look, layout: args.layout };
        await window.capturePatch({
          scenePresets: [
            ...scenePresets.filter((p) => p.name !== args.name),
            preset,
          ],
        });
        return;
      }
      case "apply_scene_preset": {
        const settings = await original("get_settings");
        const preset = settings.scenePresets.find((p) => p.name === args.name);
        if (!preset) throw new Error("Missing capture preset");
        await window.capturePatch(preset.look);
        send("scene-preset", { layout: preset.layout });
        toast("preset", preset.name);
        return;
      }
      case "listening_stats": {
        const { songs } = window.captureData;
        const days = args.range === "week" ? 7 : 30;
        const bars = Array.from({ length: days }, (_, i) => ({
          start: Date.UTC(2026, 0, 17 - days + i) / 1000,
          ms: (24 + ((i * 23 + 17) % 100)) * 60000,
        }));
        const totalMs = bars.reduce((n, b) => n + b.ms, 0);
        return {
          totalMs,
          plays: Math.round(totalMs / 192000),
          bars,
          barDays: 1,
          busiestHour: 21,
          first: bars[0].start,
          songs: songs.slice(0, 5).map((song, i) => ({
            ...song,
            plays: 24 - i * 3,
            ms: (24 - i * 3) * 192000,
          })),
          artists: [...new Set(songs.map((s) => s.artist))].map(
            (artist, i) => ({
              title: artist,
              artist: "",
              plays: 53 - i * 11,
              ms: (53 - i * 11) * 192000,
            }),
          ),
        };
      }
      case "search_catalog": {
        const { songs } = window.captureData;
        return {
          songs: songs
            .filter((s) => args.query.includes(s.title))
            .map((s) => ({ artwork: s.artUrl })),
          albums: [],
          artists: [],
        };
      }
      case "save_lyric_card": {
        const bytes = new Uint8Array(args);
        await window.captureSaveCard(Array.from(bytes));
        toast("card", "Lyric card saved");
        return "Pictures/Glowtune/After the rain.png";
      }
      default:
        return original(command, args);
    }
  };
  const playback = window.capturePlayback;
  const energy = () => {
    const t =
      Math.max(
        0,
        performance.now() - (window.captureBeatStart ?? performance.now()),
      ) / 1000;
    const offset = shot === "glow-shapes" ? 64 : 34;
    return pulse[Math.min(pulse.length - 1, Math.floor((offset + t) * 30))];
  };
  window.capturePlayback = () => ({ ...playback(), energy: energy() });
  if (shot === "beat-walls") {
    let previous = 0;
    let last = -1000;
    setInterval(() => {
      const strength = energy();
      if (
        strength > 0.35 &&
        strength - previous > 0.08 &&
        performance.now() - last > 180
      ) {
        send("kick", strength);
        last = performance.now();
      }
      previous = strength;
    }, 1000 / 30);
  }
  // Rust owns these keys in the product; the isolated browser supplies its events.
  window.addEventListener(
    "keydown",
    async (event) => {
      if (!event.ctrlKey || !event.altKey) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      if (event.code === "Digit1" || event.code === "Digit2") {
        const settings = await original("get_settings");
        const preset = settings.scenePresets[Number(event.code.slice(-1)) - 1];
        if (preset)
          await internal.invoke("apply_scene_preset", { name: preset.name });
      } else if (event.code === "ArrowRight") {
        window.captureSelectSong(1);
        const state = await original("get_state");
        send("np-track", state.track);
        send("np-playback", state.playback);
        toast("next", "Next song");
      } else if (event.code === "ArrowUp") {
        volume = 0.45;
        send("volume-changed", volume);
        toast("volume", "Volume 45 %", volume);
      } else if (event.code === "KeyC") send("make-lyric-card", "stage");
    },
    true,
  );
}

export function desktopHTML(mini = false) {
  const frame = (src, style, title) =>
    `<iframe title="${title}" src="${src}" style="${style}"></iframe>`;
  const player = (look, x, y, w, h) =>
    `<div class="look" style="left:${x}px;top:${y}px"><p>${look[0].toUpperCase() + look.slice(1)}</p>${frame("/?captureStyle=" + look, `width:${w}px;height:${h}px`, look)}</div>`;
  return `<!doctype html><html><head><style>
  @font-face{font-family:Instrument;src:url('/capture-brand.woff2')} :root{color-scheme:dark}
  *{box-sizing:border-box}body{margin:0;background:radial-gradient(ellipse at 20% 10%,#3d4946,#151820 75%);color:#eee9df;font-family:Instrument,sans-serif;height:1080px;overflow:hidden}
  iframe{border:0;background:transparent} .look{position:absolute}.look p{font-size:23px;color:#dfdac7aa;letter-spacing:.15em;text-transform:uppercase;margin:0 0 28px}
  .notes{position:absolute;left:390px;top:130px;width:1140px;height:690px;border:1px solid #ffffff25;border-radius:18px;background:#20232af5;box-shadow:0 35px 100px #0008}
  .title{padding:20px 30px;border-bottom:1px solid #ffffff15;color:#cbc6b7;font-size:19px}.paper{padding:55px 95px}.paper h1{font-size:48px;font-weight:550;margin:0 0 36px}.paper p{font-size:25px;line-height:1.8;color:#c5c4c1;margin:0 0 22px}
  .taskbar{position:absolute;bottom:0;left:0;right:0;height:64px;background:#15171eeb;border-top:1px solid #ffffff16;display:flex;align-items:center;justify-content:center;gap:20px}.tile{width:25px;height:25px;border-radius:6px;background:#ffffff22}.time{position:absolute;right:35px;color:#bbb;font-size:15px}
  </style></head><body>${
    mini
      ? player("glass", 150, 205, 480, 132) +
        player("disc", 820, 135, 220, 248) +
        player("card", 1330, 130, 280, 380) +
        player("pill", 300, 630, 400, 96) +
        player("lyric", 1000, 645, 520, 156)
      : `<div class="notes"><div class="title">Field notes &nbsp; / &nbsp; Evening ideas</div><div class="paper"><h1>Room for a little quiet.</h1><p>Let the windows catch the last light.<br>Leave a space for a song.</p><p>Finish the sketch. Make a cup of tea.<br>Take the long way home.</p></div></div>`
  }
  <div class="taskbar"><i class="tile"></i><i class="tile"></i><i class="tile"></i><i class="tile"></i><span class="time">21:24 &nbsp; 16 Jan</span>${mini ? frame("/taskbar", "position:absolute;left:18px;bottom:8px;width:300px;height:48px", "Taskbar player") : ""}</div>
  ${frame("/osd", "position:absolute;bottom:72px;left:calc(50% - 180px);width:360px;height:84px;pointer-events:none", "Glowtune toast")}
  </body></html>`;
}
