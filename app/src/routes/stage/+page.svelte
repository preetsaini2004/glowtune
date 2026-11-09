<script lang="ts">
  // Desktop scene: a live wallpaper behind every window, lit by the song.
  //
  //   wall   album art washed across the screen + two drifting color fields
  //   clock  huge outlined time in the album's colors
  //   now    tilted art card + big lyrics (or the title when there are none)
  //   recent recently played (click = find it again in a service)
  //   dock   now playing, controls, search everywhere, quick settings
  //
  // Cost control: everything that moves is a compositor transform stepped at
  // ~12 fps, and all animation freezes while paused or buried under windows.
  import { onMount, untrack } from "svelte";
  import { fade, fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { np, formatTime } from "$lib/nowPlaying.svelte";
  import { LyricsFollower } from "$lib/lyrics.svelte";
  import StageLyrics from "$lib/StageLyrics.svelte";
  import SceneMenu, { type SceneSettings } from "$lib/SceneMenu.svelte";
  import PlayerControls from "$lib/PlayerControls.svelte";
  import MusicBrowser from "$lib/MusicBrowser.svelte";
  import UpNext from "$lib/UpNext.svelte";

  type LyricsStyle = "flow" | "bold" | "karaoke" | "outline" | "sticker";
  type Settings = SceneSettings & { stageEnabled: boolean; stageLyricsStyle: LyricsStyle };
  type HistoryItem = { title: string; artist: string; source: string; artUrl: string | null };
  type ServiceId = "spotify" | "apple-music" | "youtube-music" | "soundcloud";

  const SERVICES: { id: ServiceId; name: string }[] = [
    { id: "spotify", name: "Spotify" },
    { id: "apple-music", name: "Apple Music" },
    { id: "youtube-music", name: "YouTube Music" },
    { id: "soundcloud", name: "SoundCloud" },
  ];

  const primary = new URLSearchParams(location.search).get("primary") !== "0";
  const lyr = new LyricsFollower();
  let settings = $state<Settings | null>(null);
  let covered = $state(false);
  let full = $state(false);
  // Lyrics view: lyrics on the left, video/art on the right, info in the corner.
  // Independent of full screen (that one only lifts the scene over everything).
  let cinema = $state(loadCinema());
  // The lyrics view's own layout (separate from the scene's blocks): which side
  // the lyrics sit on and how big they are.
  type CineSide = "left" | "center" | "right";
  type CineSize = "compact" | "normal" | "large";
  const cineSaved = (() => {
    try {
      return JSON.parse(localStorage.getItem("glowtune.cinemaLayout") ?? "{}");
    } catch {
      return {};
    }
  })();
  let cineSide = $state<CineSide>(["left", "center", "right"].includes(cineSaved.side) ? cineSaved.side : "left");
  let cineSize = $state<CineSize>(["compact", "normal", "large"].includes(cineSaved.size) ? cineSaved.size : "normal");
  function saveCine() {
    try {
      localStorage.setItem("glowtune.cinemaLayout", JSON.stringify({ side: cineSide, size: cineSize }));
    } catch {}
  }
  const SIDES: { id: CineSide; label: string; icon: string }[] = [
    { id: "left", label: "Lyrics on the left", icon: "M4 6h10M4 10h8M4 14h10M4 18h7" },
    { id: "center", label: "Lyrics in the middle", icon: "M7 6h10M8 10h8M7 14h10M9 18h6" },
    { id: "right", label: "Lyrics on the right", icon: "M10 6h10M12 10h8M10 14h10M13 18h7" },
  ];
  const SIZES: { id: CineSize; label: string }[] = [
    { id: "compact", label: "Compact" },
    { id: "normal", label: "Normal" },
    { id: "large", label: "Large" },
  ];
  function loadCinema(): boolean {
    try {
      return localStorage.getItem("glowtune.cinema") === "1";
    } catch {
      return false;
    }
  }
  // ── Your layout ───────────────────────────────────────────────────────
  // Every block (clock, album card, lyrics, recently played, dock) can be
  // dragged anywhere and resized while "Edit layout" is on. Stored as an offset
  // from its default spot (in % of the screen) + a scale, so it survives
  // resolution changes. Kept per screen setup in this browser profile.
  type Block = "clock" | "card" | "lyrics" | "recent" | "upcoming" | "dock";
  // x/y: offset (vw/vh); s: scale; w/h: your own width (vw) / height (vh).
  type Pos = { x: number; y: number; s: number; w?: number; h?: number };
  const BLOCK_NAMES: Record<Block, string> = {
    clock: "Clock",
    card: "Album",
    lyrics: "Lyrics",
    recent: "Recently played",
    upcoming: "Up next",
    dock: "Dock",
  };
  let layout = $state<Partial<Record<Block, Pos>>>(loadLayout());
  let editing = $state(false);
  // The block you last grabbed sits above the others (its handles stay reachable
  // even where blocks overlap).
  let selected = $state<Block | null>(null);
  let drag: { id: Block; x: number; y: number; from: Pos } | null = null;

  function loadLayout(): Partial<Record<Block, Pos>> {
    try {
      const saved: Partial<Record<Block, Pos>> = JSON.parse(localStorage.getItem("glowtune.layout") ?? "{}");
      // Older saves could hold only a width or only a height: drop those half
      // sizes (positions stay) so blocks never get stuck half-sized.
      for (const p of Object.values(saved)) {
        if (p && !(p.w && p.h)) {
          delete p.w;
          delete p.h;
        }
      }
      return saved;
    } catch {
      return {};
    }
  }
  function saveLayout() {
    try {
      localStorage.setItem("glowtune.layout", JSON.stringify(layout));
    } catch {}
  }
  function pos(id: Block): Pos {
    return layout[id] ?? { x: 0, y: 0, s: 1 };
  }
  /** CSS `translate` / `scale` for a block (separate from its own transforms). */
  function moveOf(id: Block): string | undefined {
    if (cinema) return undefined; // the lyrics view has its own layout
    const p = layout[id];
    return p && (p.x || p.y) ? `${p.x}vw ${p.y}vh` : undefined;
  }
  function scaleOf(id: Block): string | undefined {
    if (cinema) return undefined; // the lyrics view has its own layout
    const p = layout[id];
    return p && p.s !== 1 ? String(p.s) : undefined;
  }
  function widthOf(id: Block): string | undefined {
    if (cinema) return undefined; // the lyrics view has its own layout
    const w = layout[id]?.w;
    return w ? `${w}vw` : undefined;
  }
  function heightOf(id: Block): string | undefined {
    if (cinema) return undefined; // the lyrics view has its own layout
    const h = layout[id]?.h;
    return h ? `${h}vh` : undefined;
  }
  // Free resizing from the right edge, bottom edge or corner. Sizes are the
  // block's own (before scale) and are stored relative to the screen.
  const MIN: Record<Block, [number, number]> = {
    clock: [180, 70],
    card: [130, 150],
    lyrics: [200, 110],
    recent: [160, 120],
    upcoming: [160, 120],
    dock: [320, 56],
  };
  let sizing: { id: Block; edge: "e" | "s" | "se"; x: number; y: number; w: number; h: number; s: number } | null = null;
  function startResize(id: Block, edge: "e" | "s" | "se", e: PointerEvent) {
    e.stopPropagation();
    selected = id;
    const handle = e.currentTarget as HTMLElement;
    const block = handle.closest(".grip")?.parentElement;
    if (!block) return;
    handle.setPointerCapture(e.pointerId);
    const r = block.getBoundingClientRect();
    const s = pos(id).s || 1;
    sizing = { id, edge, x: e.clientX, y: e.clientY, w: r.width / s, h: r.height / s, s };
  }
  function resizeMove(e: PointerEvent) {
    if (!sizing) return;
    const { id, edge, s } = sizing;
    const [minW, minH] = MIN[id];
    const p = { ...pos(id) };
    // Both sizes are always stored, so content can adapt to width and height.
    const dx = edge === "s" ? 0 : (e.clientX - sizing.x) / s;
    const dy = edge === "e" ? 0 : (e.clientY - sizing.y) / s;
    p.w = (Math.max(minW, sizing.w + dx) / innerWidth) * 100;
    p.h = (Math.max(minH, sizing.h + dy) / innerHeight) * 100;
    layout[id] = p;
  }
  function resizeEnd() {
    if (sizing) saveLayout();
    sizing = null;
  }

  function grab(id: Block, e: PointerEvent) {
    selected = id;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    drag = { id, x: e.clientX, y: e.clientY, from: pos(id) };
  }
  function dragMove(e: PointerEvent) {
    if (!drag) return;
    const { id, from } = drag;
    layout[id] = {
      ...from,
      x: from.x + ((e.clientX - drag.x) / innerWidth) * 100,
      y: from.y + ((e.clientY - drag.y) / innerHeight) * 100,
    };
  }
  function drop() {
    if (drag) saveLayout();
    drag = null;
  }
  function resize(id: Block, e: WheelEvent) {
    e.preventDefault();
    const p = pos(id);
    layout[id] = { ...p, s: Math.round(Math.min(1.8, Math.max(0.5, p.s - Math.sign(e.deltaY) * 0.05)) * 100) / 100 };
    saveLayout();
  }
  function resetLayout() {
    layout = {};
    saveLayout();
  }

  function toggleCinema() {
    cinema = !cinema;
    panelOpen = false;
    try {
      localStorage.setItem("glowtune.cinema", cinema ? "1" : "0");
    } catch {}
  }
  let history = $state<HistoryItem[]>([]);
  let clock = $state(new Date());
  let now = $state(Date.now());

  let searchOpen = $state(false);
  let searchBtn: HTMLButtonElement | undefined = $state();
  let panelOpen = $state(false);
  let target = $state<ServiceId>(loadTarget());
  $effect(() => {
    try { localStorage.setItem("glowtune.searchTarget", target); } catch {}
  });

  const track = $derived(np.track);
  const palette = $derived(np.palette);
  const live = $derived(np.playing && !covered);
  const motion = $derived((settings?.stageMotion ?? true) && live);
  const duration = $derived(track?.durationMs ?? null);
  const position = $derived(np.positionAt(now));
  const progress = $derived(duration ? Math.min(1, position / duration) : 0);
  const showLyrics = $derived((settings?.stageLyrics ?? true) && lyr.synced);
  const lyricsStyle = $derived<LyricsStyle>(settings?.stageLyricsStyle ?? "bold");

  const timeParts = $derived(splitTime(clock));
  const dateText = $derived(
    clock.toLocaleDateString(undefined, { weekday: "long", month: "long", day: "numeric" }),
  );

  function splitTime(d: Date): { time: string; suffix: string } {
    const parts = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" }).formatToParts(d);
    const time = parts
      .filter((p) => p.type === "hour" || p.type === "minute" || (p.type === "literal" && p.value.trim() === ":"))
      .map((p) => p.value)
      .join("");
    const suffix = parts.find((p) => p.type === "dayPeriod")?.value ?? "";
    return { time, suffix };
  }

  function loadTarget(): ServiceId {
    try {
      const v = localStorage.getItem("glowtune.searchTarget");
      if (v && SERVICES.some((s) => s.id === v)) return v as ServiceId;
    } catch {}
    return "spotify";
  }

  // Lyrics: re-plan on every playback report or new lyrics.
  $effect(() => {
    void np.playback;
    void lyr.lyrics;
    lyr.schedule();
  });

  // Progress ticks only while it can be seen.
  $effect(() => {
    if (!live) return;
    const id = setInterval(() => (now = Date.now()), 500);
    return () => clearInterval(id);
  });

  onMount(() => {
    np.start();
    const stopLyrics = lyr.start();
    const offs = [
      getCurrentWindow().listen<boolean>("stage-covered", (e) => (covered = e.payload)),
      getCurrentWindow().listen<boolean>("stage-fullscreen", (e) => (full = e.payload)),
      listen<HistoryItem[]>("np-history", (e) => (history = e.payload)),
      listen<Settings>("settings", (e) => (settings = e.payload)),
    ];
    Promise.all(offs.slice(0, 2))
      .then(() => invoke<[boolean, boolean]>("get_stage_state"))
      .then(([hidden, fullscreen]) => {
        covered = hidden;
        full = fullscreen;
      })
      .catch(() => {});
    invoke<Settings>("get_settings").then((s) => (settings = s));
    invoke<HistoryItem[]>("get_history").then((h) => (history = h));

    // The clock wakes once a minute, on the minute.
    let tick: ReturnType<typeof setTimeout>;
    const minute = () => {
      clock = new Date();
      tick = setTimeout(minute, 60_000 - (Date.now() % 60_000) + 50);
    };
    minute();

    return () => {
      stopLyrics();
      clearTimeout(tick);
      offs.forEach((p) => p.then((f) => f()));
    };
  });

  // ── Music video wallpaper ─────────────────────────────────────────────
  // The song's video plays muted behind everything, following the song:
  // paused with it, hidden-and-paused when the scene is covered, and nudged
  // back in sync only when it drifts (seeking a video is not free).
  const wantVideo = $derived(settings?.stageWallpaper === "video");
  const songKey = $derived(track ? `${track.title}\u001f${track.artist}` : "");
  let videoId = $state<string | null>(null);
  let videoStart = $state(0);
  let videoReady = $state(false);
  let videoFrame: HTMLIFrameElement | undefined = $state();
  let synced: { pos: number; at: number } | null = null;

  $effect(() => {
    const key = songKey;
    if (!wantVideo || !key) {
      videoId = null;
      return;
    }
    const t = untrack(() => track);
    let stale = false;
    invoke<string | null>("music_video", { title: t?.title ?? "", artist: t?.artist ?? "", durationMs: t?.durationMs ?? null })
      .then((id) => {
        if (stale) return;
        videoStart = Math.floor(np.positionAt() / 1000);
        videoReady = false;
        synced = null;
        videoId = id;
      })
      .catch(() => {
        if (!stale) videoId = null;
      });
    return () => {
      stale = true;
    };
  });

  // Covered: the frame is dropped (no decoding); it reloads and re-syncs after.
  $effect(() => {
    if (covered) {
      videoReady = false;
      synced = null;
    }
  });

  function videoCmd(func: string, args: unknown[] = []) {
    videoFrame?.contentWindow?.postMessage(JSON.stringify({ event: "command", func, args }), "*");
  }
  function videoLoaded() {
    // Give the player a moment to wire up its message handler, then subscribe
    // to its events (state + current time) so we can loop and stay in sync.
    setTimeout(() => {
      videoFrame?.contentWindow?.postMessage(JSON.stringify({ event: "listening", id: "glowtune", channel: "widget" }), "*");
      videoReady = true;
      videoCmd("mute");
      noCaptions();
      syncVideo(true);
    }, 600);
  }
  /** Our lyrics are the words on screen: switch YouTube's captions off even when
   * the viewer's YouTube settings force them on. */
  function noCaptions() {
    videoCmd("unloadModule", ["captions"]);
    videoCmd("unloadModule", ["cc"]);
    videoCmd("setOption", ["captions", "track", {}]);
  }
  /** Where the song is, mapped onto the video (wraps if the song is longer). */
  function songPosForVideo(): number {
    const pos = np.positionAt() / 1000;
    return videoDuration > 5 ? pos % videoDuration : pos;
  }
  function syncVideo(force = false) {
    if (!videoReady) return;
    const pos = songPosForVideo();
    const expected = synced ? synced.pos + (live ? (Date.now() - synced.at) / 1000 : 0) : -999;
    if (force || Math.abs(expected - pos) > 1.5) {
      videoCmd("seekTo", [pos, true]);
      synced = { pos, at: Date.now() };
    }
    videoCmd(live ? "playVideo" : "pauseVideo");
    if (live) noCaptions();
    if (!live && synced) synced = { pos, at: Date.now() };
  }
  // Events from the YouTube player: the video's real time (correct drift at
  // most every 3 s) and "ended" (loop it ourselves: a one-song playlist would
  // show YouTube's previous/next buttons over the scene).
  let videoDuration = 0;
  let lastFix = 0;
  function videoEvent(e: MessageEvent) {
    if (!videoFrame || e.source !== videoFrame.contentWindow || typeof e.data !== "string") return;
    let msg: { event?: string; info?: { currentTime?: number; duration?: number; playerState?: number } | number };
    try {
      msg = JSON.parse(e.data);
    } catch {
      return;
    }
    const info = msg.info;
    if (msg.event === "onStateChange" && info === 0) {
      videoCmd("seekTo", [0, true]);
      videoCmd("playVideo");
      return;
    }
    if (msg.event !== "infoDelivery" || typeof info !== "object" || !info) return;
    if (info.duration) videoDuration = info.duration;
    if (info.playerState === 0) {
      videoCmd("seekTo", [0, true]);
      if (live) videoCmd("playVideo");
    }
    if (typeof info.currentTime === "number" && live && Date.now() - lastFix > 3000) {
      const pos = songPosForVideo();
      if (Math.abs(info.currentTime - pos) > 1.5) {
        lastFix = Date.now();
        videoCmd("seekTo", [pos, true]);
        synced = { pos, at: Date.now() };
      }
    }
  }
  onMount(() => {
    window.addEventListener("message", videoEvent);
    return () => window.removeEventListener("message", videoEvent);
  });
  $effect(() => {
    void np.playback;
    void live;
    untrack(() => syncVideo());
  });

  async function save(patch: Partial<SceneSettings> & Record<string, unknown>) {
    if (!settings) return;
    const next = { ...settings, ...patch } as Settings;
    settings = next;
    try {
      settings = await invoke<Settings>("set_settings", { new: next });
    } catch {}
  }

  /** Plays the top match in the service (clicking a result). */
  function play(q: string, service: ServiceId = target) {
    if (!q.trim()) return;
    invoke("play_on", { service, query: q.trim() }).catch(() => {});
    searchOpen = false;
  }

  // Microphone with sparkles: the lyrics view.
  const MIC =
    "M9.5 13.5 4 19a1.4 1.4 0 0 0 2 2l5.5-5.5M14 4.5a4 4 0 1 1-4.5 4.5 4 4 0 0 1 4.5-4.5zM11 8l5 5M19 3v3M17.5 4.5h3M21 9v2M20 10h2";

  function toggleFull() {
    invoke("stage_fullscreen", { on: !full }).catch(() => {});
  }

  function serviceFor(source: string): ServiceId {
    const s = source.toLowerCase();
    if (s.includes("apple")) return "apple-music";
    if (s.includes("soundcloud")) return "soundcloud";
    if (s.includes("youtube")) return "youtube-music";
    return "spotify";
  }

  function keys(e: KeyboardEvent) {
    if (searchOpen) return;
    // Playback keys, matching the media keys printed on most keyboards:
    // F7 previous, Space / F8 play-pause, F9 next. Space while typing stays a
    // space; on a focused button it means play/pause too.
    const typing =
      document.activeElement instanceof HTMLInputElement || document.activeElement instanceof HTMLTextAreaElement;
    if (e.key === " " && !typing && !e.repeat) {
      e.preventDefault();
      np.control("playPause");
      return;
    }
    if (e.key === "F7" || e.key === "F8" || e.key === "F9") {
      e.preventDefault();
      np.control(e.key === "F7" ? "previous" : e.key === "F9" ? "next" : "playPause");
      return;
    }
    if (e.key === "Escape") {
      if (!searchOpen && !panelOpen) {
        if (editing) editing = false;
        else if (cinema) toggleCinema();
        else if (full) toggleFull();
      }
      searchOpen = false;
      panelOpen = false;
    }
    if (e.key.toLowerCase() === "l" && !e.ctrlKey && !(document.activeElement instanceof HTMLInputElement)) toggleCinema();
    if (primary && e.key.toLowerCase() === "e" && !e.ctrlKey && !(document.activeElement instanceof HTMLInputElement)) editing = !editing;
    if (e.key === "F11") {
      e.preventDefault();
      toggleFull();
    }
    if ((e.ctrlKey && e.key.toLowerCase() === "k") || (e.key === "/" && !(document.activeElement instanceof HTMLInputElement))) {
      e.preventDefault();
      searchOpen = true;
      panelOpen = false;
    }
  }

  function seekFrom(e: PointerEvent) {
    if (!duration) return;
    const r = (e.currentTarget as HTMLElement).getBoundingClientRect();
    np.seek(Math.min(1, Math.max(0, (e.clientX - r.left) / r.width)) * duration);
  }

  const rise = { y: 38, duration: 560, easing: cubicOut };
</script>

<svelte:window onkeydown={keys} />

{#snippet grip(id: Block)}
  {#if editing}
    <div
      class="grip"
      role="slider"
      tabindex="-1"
      aria-label={`Move ${BLOCK_NAMES[id]}`}
      aria-valuenow={Math.round(pos(id).s * 100)}
      onpointerdown={(e) => grab(id, e)}
      onpointermove={dragMove}
      onpointerup={drop}
      onpointercancel={drop}
      onwheel={(e) => resize(id, e)}
    >
      <span>{BLOCK_NAMES[id]}</span>
      {#each [["e", "Resize width"], ["s", "Resize height"], ["se", "Resize"]] as [edge, label]}
        <i
          class="rz rz-{edge}"
          role="presentation"
          title={label}
          onpointerdown={(e) => startResize(id, edge as "e" | "s" | "se", e)}
          onpointermove={resizeMove}
          onpointerup={resizeEnd}
          onpointercancel={resizeEnd}
        ></i>
      {/each}
    </div>
  {/if}
{/snippet}

<main
  class="scene"
  class:frozen={!motion}
  class:cinema
  class:side-left={cinema && cineSide === "left"}
  class:side-center={cinema && cineSide === "center"}
  class:side-right={cinema && cineSide === "right"}
  class:size-compact={cinema && cineSize === "compact"}
  class:size-large={cinema && cineSize === "large"}
  class:editing
  class:has-recent={primary && (settings?.stageRecent ?? true)}
  class:has-upcoming={primary && (settings?.stageUpcoming ?? true)}
  style:--c1={palette[0]}
  style:--c2={palette[1] ?? palette[0]}
  style:--c3={palette[2] ?? palette[1] ?? palette[0]}
>
  <!-- Wall: the album washed across the screen. -->
  <div class="wall" aria-hidden="true">
    <div class="drift">
      {#key track?.artUrl}
        {#if track?.artUrl}
          <img class="wash" src={track.artUrl} alt="" transition:fade={{ duration: 1200 }} />
        {/if}
      {/key}
    </div>
    <div class="field f1"></div>
    <div class="field f2"></div>
    {#if cinema && !videoId}
      {#key track?.artUrl}
        {#if track?.artUrl}
          <img class="poster" src={track.artUrl} alt="" transition:fade={{ duration: 1200 }} />
        {/if}
      {/key}
    {/if}
    {#if videoId && !covered}
      {#key videoId}
        <iframe
          class="mv"
          class:ready={videoReady}
          bind:this={videoFrame}
          onload={videoLoaded}
          title="Music video"
          tabindex="-1"
          allow="autoplay; encrypted-media"
          src={`https://www.youtube-nocookie.com/embed/${videoId}?autoplay=1&mute=1&controls=0&disablekb=1&fs=0&iv_load_policy=3&modestbranding=1&playsinline=1&rel=0&cc_load_policy=0&enablejsapi=1&start=${videoStart}&origin=${encodeURIComponent(location.origin)}`}
        ></iframe>
      {/key}
    {/if}
    <div class="veil" class:over-video={videoId && videoReady}></div>
  </div>

  {#if settings?.stageClock ?? true}
    <header class="clock {settings?.stageClockStyle ?? 'outline'}" style:translate={moveOf("clock")} style:scale={scaleOf("clock")} style:width={widthOf("clock")} style:height={heightOf("clock")} class:wset={!cinema && !!layout.clock?.w} class:hset={!cinema && !!layout.clock?.h} class:sel={editing && selected === "clock"}>
      {@render grip("clock")}
      <p class="date">{dateText}</p>
      <svg class="face" viewBox="0 0 1000 360" role="img" aria-label={`${timeParts.time} ${timeParts.suffix}`}>
        <defs>
          <linearGradient id="clock-ink" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" style="stop-color: color-mix(in oklab, var(--c1) 70%, white)" />
            <stop offset="1" style="stop-color: color-mix(in oklab, var(--c2) 70%, white)" />
          </linearGradient>
          <linearGradient id="clock-fill" x1="0" y1="0" x2="0" y2="1">
            <stop offset="0" style="stop-color: var(--c1); stop-opacity: 0.34" />
            <stop offset="1" style="stop-color: var(--c2); stop-opacity: 0.06" />
          </linearGradient>
        </defs>
        <text x="500" y="318" text-anchor="middle">{timeParts.time}</text>
      </svg>
      {#if timeParts.suffix}<p class="suffix">{timeParts.suffix}</p>{/if}
    </header>
  {/if}

  <section class="now" class:no-lyrics={!showLyrics}>
    {#if track}
      <figure class="card" style:translate={moveOf("card")} style:scale={scaleOf("card")} style:width={widthOf("card")} style:height={heightOf("card")} class:wset={!cinema && !!layout.card?.w} class:hset={!cinema && !!layout.card?.h} class:sel={editing && selected === "card"}>
        {@render grip("card")}
        <div class="art">
          {#key track.artUrl}
            {#if track.artUrl}
              <img src={track.artUrl} alt="" draggable="false" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
            {:else}
              <div class="art-empty"></div>
            {/if}
          {/key}
          <div class="sheen"></div>
        </div>
        <figcaption>
          <span class="t">{track.title}</span>
          <span class="a">{track.artist || track.source}</span>
        </figcaption>
      </figure>

      <div class="words" class:flowing={showLyrics && (cinema || lyricsStyle === "flow")} style:translate={moveOf("lyrics")} style:scale={scaleOf("lyrics")} style:width={widthOf("lyrics")} style:height={heightOf("lyrics")} class:wset={!cinema && !!layout.lyrics?.w} class:hset={!cinema && !!layout.lyrics?.h} class:sel={editing && selected === "lyrics"}>
        {@render grip("lyrics")}
        {#if showLyrics}
          <StageLyrics
            {lyr}
            style={cinema && lyricsStyle !== "karaoke" && lyricsStyle !== "sticker" && lyricsStyle !== "outline" ? "flow" : lyricsStyle}
            rate={np.playback.rate || 1}
            playing={live}
            light={Boolean(videoId && videoReady)}
          />
        {:else}
          {#key track.title}
            <div class="lines" in:fly={rise}>
              <p class="line title">{track.title}</p>
              <p class="after">{track.album || track.artist}</p>
            </div>
          {/key}
        {/if}
      </div>
    {:else}
      <div class="idle" in:fade>
        <p class="line title">Nothing playing</p>
        <p class="after">Play something anywhere, or search below to start.</p>
      </div>
    {/if}
  </section>

  {#if primary && (settings?.stageRecent ?? true) && history.length > 0}
    <aside class="recent glass" transition:fade={{ duration: 300 }} style:translate={moveOf("recent")} style:scale={scaleOf("recent")} style:width={widthOf("recent")} style:height={heightOf("recent")} class:wset={!cinema && !!layout.recent?.w} class:hset={!cinema && !!layout.recent?.h} class:sel={editing && selected === "recent"}>
      {@render grip("recent")}
      <h2>Recently played</h2>
      <div class="list"><ol>
        {#each history.slice(0, 9) as item, i (item.title + item.artist)}
          <li>
            <button
              class:current={i === 0 && track?.title === item.title}
              title={`Play “${item.title}” on ${SERVICES.find((s) => s.id === serviceFor(item.source))?.name}`}
              onclick={() => play(`${item.title} ${item.artist}`, serviceFor(item.source))}
            >
              {#if item.artUrl}<img src={item.artUrl} alt="" />{:else}<span class="ph"></span>{/if}
              <span class="txt">
                <span class="t">{item.title}</span>
                <span class="a">{item.artist}</span>
              </span>
              {#if i === 0 && track?.title === item.title && np.playing}
                <span class="eq" aria-label="Playing"><i></i><i></i><i></i></span>
              {/if}
            </button>
          </li>
        {/each}
      </ol></div>
    </aside>
  {/if}

  {#if primary && (settings?.stageUpcoming ?? true)}
    <aside class="recent upcoming glass" style:translate={moveOf("upcoming")} style:scale={scaleOf("upcoming")} style:width={widthOf("upcoming")} style:height={heightOf("upcoming")} class:wset={!cinema && !!layout.upcoming?.w} class:hset={!cinema && !!layout.upcoming?.h} class:sel={editing && selected === "upcoming"}>
      {@render grip("upcoming")}
      <h2>Up next</h2>
      <div class="list"><UpNext visible={!covered} /></div>
    </aside>
  {/if}

  {#if primary && searchOpen && !editing}
    <MusicBrowser
      bind:target
      anchor={searchBtn}
      onclose={(restore) => {
        searchOpen = false;
        if (restore) searchBtn?.focus();
      }}
    />
  {/if}

  {#if primary && panelOpen}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <div class="scrim" onclick={() => (panelOpen = false)}></div>
    <div class="panel glass" transition:fly={{ y: 16, duration: 260, easing: cubicOut }}>
      {#if settings}<SceneMenu
        {settings}
        {full}
        {save}
        onfullscreen={() => { panelOpen = false; toggleFull(); }}
        onmore={() => { panelOpen = false; invoke("open_settings"); }}
        onstore={() => { panelOpen = false; invoke("open_settings_tab", { tab: "store" }); }}
        onedit={() => { panelOpen = false; editing = true; }}
        onoff={() => save({ stageEnabled: false })}
      />{/if}
    </div>
  {/if}

  {#if primary && editing}
    <div class="editbar glass" role="toolbar" aria-label="Edit layout">
      <span>Drag to move. Drag an edge or corner to resize. Scroll to scale.</span>
      <button onclick={resetLayout}>Reset</button>
      <button class="done" onclick={() => (editing = false)}>Done</button>
    </div>
  {/if}

  {#if cinema}
    {#if primary}
    <nav class="toolbar" aria-label="Scene">
      <button class="icon" aria-label="Scene options" aria-expanded={panelOpen} onclick={() => { panelOpen = !panelOpen; searchOpen = false; }}>
        <svg viewBox="0 0 24 24"><path d="M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4" /></svg>
      </button>
      <div class="seg" role="radiogroup" aria-label="Lyrics position">
        {#each SIDES as s}
          <button
            class="icon"
            class:on={cineSide === s.id}
            role="radio"
            aria-checked={cineSide === s.id}
            aria-label={s.label}
            title={s.label}
            onclick={() => { cineSide = s.id; saveCine(); }}
          >
            <svg viewBox="0 0 24 24"><path d={s.icon} /></svg>
          </button>
        {/each}
      </div>
      <div class="seg" role="radiogroup" aria-label="Lyrics size">
        {#each SIZES as z}
          <button
            class="txt-btn"
            class:on={cineSize === z.id}
            role="radio"
            aria-checked={cineSize === z.id}
            onclick={() => { cineSize = z.id; saveCine(); }}
          >{z.label}</button>
        {/each}
      </div>
      <button class="icon on" aria-label="Close lyrics view" aria-pressed="true" onclick={toggleCinema}>
        <svg viewBox="0 0 24 24"><path d={MIC} /></svg>
      </button>
      <button class="icon" aria-label={full ? "Exit full screen" : "Full screen"} onclick={toggleFull}>
        <svg viewBox="0 0 24 24"><path d={full ? "M9 4v5H4M15 4v5h5M9 20v-5H4M15 20v-5h5" : "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5"} /></svg>
      </button>
    </nav>
    {/if}
    {#if primary && track}
      <div class="corner">
        <p class="ct">{track.title}</p>
        <p class="ca">{track.artist || track.source}</p>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="bar" onpointerdown={seekFrom}>
          <div class="fill" style:width={`${progress * 100}%`}></div>
        </div>
        <div class="times"><span>{formatTime(position)}</span><span>{formatTime(duration)}</span></div>
        <div class="transport"><PlayerControls /></div>
      </div>
    {/if}
  {/if}

  {#if primary}
  <footer class="dock glass" class:hidden={cinema} class:tucked={!(settings?.stageDock ?? true) && !editing} style:translate={moveOf("dock")} style:scale={scaleOf("dock")} style:width={widthOf("dock")} style:height={heightOf("dock")} class:wset={!cinema && !!layout.dock?.w} class:hset={!cinema && !!layout.dock?.h} class:sel={editing && selected === "dock"}>
    {@render grip("dock")}
    {#if track}
      <div class="np">
        {#if track.artUrl}<img src={track.artUrl} alt="" />{:else}<span class="ph"></span>{/if}
        <span class="txt"><span class="t">{track.title}</span><span class="a">{track.artist || track.source}</span></span>
      </div>
      <PlayerControls />
      <div class="progress">
        <span class="time">{formatTime(position)}</span>
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <div class="bar" onpointerdown={seekFrom}>
          <div class="fill" style:width={`${progress * 100}%`}></div>
        </div>
        <span class="time">{formatTime(duration)}</span>
      </div>
    {/if}
    <button
      bind:this={searchBtn}
      class="search browse"
      class:open={searchOpen}
      onclick={() => { searchOpen = !searchOpen; panelOpen = false; }}
      aria-label="Find something to play"
      aria-haspopup="dialog"
      aria-expanded={searchOpen}
      aria-controls="music-panel"
    >
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5 20 20" /></svg>
      <span>Find something to play</span>
    </button>
    <button class="icon" class:on={cinema} aria-label="Lyrics view" aria-pressed={cinema} title="Lyrics view (L)" onclick={toggleCinema}>
      <svg viewBox="0 0 24 24"><path d={MIC} /></svg>
    </button>
    <button class="icon" aria-label="Full screen" title="Full screen (F11)" onclick={toggleFull}>
      <svg viewBox="0 0 24 24"><path d="M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5" /></svg>
    </button>
    <button class="icon gear" aria-label="Desktop scene options" aria-expanded={panelOpen} onclick={() => { panelOpen = !panelOpen; searchOpen = false; }}>
      <svg viewBox="0 0 24 24"><path d="M4 7h10M18 7h2M4 17h4M12 17h8M16 5v4M10 15v4" /></svg>
    </button>
  </footer>
  {/if}
</main>

<style>
  .scene {
    --glass: rgba(14, 12, 22, 0.46);
    --glass-line: rgba(255, 255, 255, 0.09);
    --base: color-mix(in oklab, var(--c1) 16%, #06050b);
    position: relative;
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    background: var(--base);
    transition: background-color 1.2s ease;
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto minmax(0, 1fr) auto;
    padding: 4vh 4vw 3.2vh;
    gap: 2vh;
  }

  /* ── Wall ─────────────────────────────────────────── */
  .wall,
  .wall > * {
    position: absolute;
    inset: 0;
  }
  .wall {
    z-index: 0;
    overflow: hidden;
  }
  /* The art is blurred at a small size, then scaled up by the compositor:
     a huge soft wash for the price of a thumbnail. */
  .drift {
    inset: -12%;
    display: grid;
    place-items: center;
    animation: drift 70s steps(840) infinite alternate;
    will-change: transform;
  }
  .wash {
    grid-area: 1 / 1;
    width: 18vw;
    aspect-ratio: 16 / 10;
    object-fit: cover;
    filter: blur(9px) saturate(1.5) brightness(0.78);
    transform: scale(7.2);
    opacity: 0.85;
  }
  .field {
    inset: auto;
    width: 70vmax;
    height: 70vmax;
    border-radius: 50%;
    mix-blend-mode: screen;
    opacity: 0.5;
    will-change: transform;
  }
  .f1 {
    left: -18vmax;
    top: -24vmax;
    background: radial-gradient(closest-side, color-mix(in oklab, var(--c1) 80%, transparent), transparent);
    animation: float-a 48s steps(576) infinite alternate;
  }
  .f2 {
    right: -22vmax;
    bottom: -30vmax;
    background: radial-gradient(closest-side, color-mix(in oklab, var(--c2) 75%, transparent), transparent);
    animation: float-b 56s steps(672) infinite alternate;
  }
  .veil {
    background:
      radial-gradient(120% 90% at 50% 40%, transparent 40%, rgba(3, 2, 8, 0.62) 100%),
      linear-gradient(180deg, rgba(3, 2, 8, 0.18), rgba(3, 2, 8, 0.5));
  }
  /* Music video wallpaper: cover the screen like object-fit: cover. */
  .wall > .mv {
    inset: auto;
    top: 50%;
    left: 50%;
    width: max(100vw, 177.78vh);
    height: max(100vh, 56.25vw);
    transform: translate(-50%, -50%) scale(1.14);
    border: 0;
    pointer-events: none;
    opacity: 0;
    transition: opacity 1.4s ease;
  }
  .wall > .mv.ready {
    opacity: 1;
  }
  .veil.over-video {
    background:
      linear-gradient(90deg, rgba(3, 2, 8, 0.72) 0, rgba(3, 2, 8, 0.25) 55%, rgba(3, 2, 8, 0.1) 100%),
      linear-gradient(0deg, rgba(3, 2, 8, 0.65), transparent 45%);
  }
  .veil::after {
    /* Fine grain: no banding in the big gradients. */
    content: "";
    position: absolute;
    inset: 0;
    opacity: 0.07;
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='160' height='160'%3E%3Cfilter id='n'%3E%3CfeTurbulence type='fractalNoise' baseFrequency='0.9' numOctaves='2' stitchTiles='stitch'/%3E%3C/filter%3E%3Crect width='100%25' height='100%25' filter='url(%23n)'/%3E%3C/svg%3E");
  }
  @keyframes drift {
    from { transform: translate3d(-2.5%, -1.5%, 0) rotate(-2deg) scale(1.02); }
    to { transform: translate3d(2.5%, 2%, 0) rotate(2.5deg) scale(1.1); }
  }
  @keyframes float-a {
    from { transform: translate3d(0, 0, 0); }
    to { transform: translate3d(22vmax, 14vmax, 0); }
  }
  @keyframes float-b {
    from { transform: translate3d(0, 0, 0); }
    to { transform: translate3d(-26vmax, -12vmax, 0); }
  }
  .frozen .drift,
  .frozen .field,
  .frozen .eq i {
    animation-play-state: paused;
  }

  .scene > :not(.wall) {
    position: relative;
    z-index: 1;
  }

  /* Fixed rows: hiding the clock must not shift the dock into the stretchy row. */
  .scene > .clock {
    grid-row: 1;
  }
  .scene > .now {
    grid-row: 2;
  }
  .scene > .dock {
    grid-row: 3;
  }
  .cinema > .now {
    grid-row: 1 / -1;
  }

  /* ── Clock ────────────────────────────────────────── */
  .clock {
    justify-self: center;
    text-align: center;
    display: grid;
    justify-items: center;
    margin-top: 1vh;
  }
  .date {
    margin: 0 0 -1.2vh;
    font-size: clamp(14px, 1.15vw, 22px);
    font-weight: 560;
    letter-spacing: 0.01em;
    color: rgba(255, 255, 255, 0.72);
  }
  .face {
    height: clamp(120px, 25vh, 380px);
    width: auto;
    overflow: visible;
    filter: drop-shadow(0 0 18px color-mix(in oklab, var(--c1) 55%, transparent))
      drop-shadow(0 6px 30px rgba(0, 0, 0, 0.35));
  }
  .face text {
    font-family: var(--font-lyrics);
    font-size: 340px;
    font-weight: 260;
    font-stretch: 75%;
    letter-spacing: -6px;
    fill: url(#clock-fill);
    stroke: url(#clock-ink);
    stroke-width: 2.4px;
    vector-effect: non-scaling-stroke;
    paint-order: stroke;
  }
  /* Its own line under the time (it used to be pulled up into the digits). */
  .suffix {
    margin: 0.35em 0 0;
    line-height: 1;
    font-size: clamp(12px, 0.95vw, 18px);
    font-weight: 600;
    color: rgba(255, 255, 255, 0.55);
  }

  /* ── Now: art card + words ────────────────────────── */
  .now {
    align-self: center;
    justify-self: center;
    width: min(100%, 1500px);
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    align-items: center;
    gap: 4.5vw;
    padding-left: 2vw;
  }
  .has-upcoming .now { padding-right: clamp(260px, 17vw, 340px); }
  .has-recent .now {
    padding-left: clamp(260px, 17vw, 340px);
  }
  .card {
    margin: 0;
    width: clamp(180px, 15vw, 300px);
    perspective: 1400px;
  }
  .art {
    position: relative;
    aspect-ratio: 1;
    border-radius: 20px;
    overflow: hidden;
    transform: rotateY(14deg) rotateX(3deg);
    transform-origin: 60% 50%;
    box-shadow:
      0 30px 60px -18px rgba(0, 0, 0, 0.7),
      0 0 70px -10px color-mix(in oklab, var(--c1) 60%, transparent);
    -webkit-box-reflect: below 10px linear-gradient(transparent 68%, rgba(255, 255, 255, 0.16));
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .art img,
  .art-empty {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .art-empty {
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .sheen {
    position: absolute;
    inset: 0;
    background: linear-gradient(115deg, rgba(255, 255, 255, 0.22), transparent 38%, transparent 70%, rgba(255, 255, 255, 0.06));
    mix-blend-mode: soft-light;
  }
  figcaption {
    margin-top: calc(clamp(180px, 15vw, 300px) * 0.36);
    display: grid;
    gap: 2px;
    padding-left: 4px;
  }
  figcaption .t {
    font-weight: 650;
    font-size: clamp(14px, 1vw, 19px);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  figcaption .a {
    font-size: clamp(12px, 0.85vw, 16px);
    color: rgba(255, 255, 255, 0.6);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .words {
    display: grid;
    min-height: 40vh;
    align-content: center;
  }
  .lines {
    grid-area: 1 / 1;
    max-width: min(100%, 50vw);
  }
  .line,
  .after {
    margin: 0;
    font-family: var(--font-lyrics);
    text-wrap: balance;
  }
  .line {
    font-size: clamp(38px, 4.4vw, 104px);
    line-height: 1.02;
    font-weight: 780;
    font-stretch: 78%;
    letter-spacing: -0.015em;
    color: transparent;
    background: linear-gradient(180deg, #fff 35%, color-mix(in oklab, var(--c1) 45%, #fff) 100%);
    -webkit-background-clip: text;
    background-clip: text;
    filter: drop-shadow(0 0 26px color-mix(in oklab, var(--c1) 55%, transparent))
      drop-shadow(0 4px 18px rgba(0, 0, 0, 0.45));
  }
  .line.title {
    font-size: clamp(34px, 3.6vw, 86px);
  }
  .after {
    margin-top: 1.6vh;
    font-size: clamp(18px, 1.6vw, 36px);
    font-weight: 560;
    font-stretch: 88%;
    color: rgba(255, 255, 255, 0.46);
  }
  .idle {
    grid-column: 1 / -1;
    text-align: center;
  }

  /* ── Glass ────────────────────────────────────────── */
  .glass {
    background: var(--glass);
    border: 1px solid var(--glass-line);
    backdrop-filter: blur(28px) saturate(1.5);
    box-shadow: 0 24px 60px -20px rgba(0, 0, 0, 0.6), inset 0 1px 0 rgba(255, 255, 255, 0.06);
  }

  /* ── Recently played ──────────────────────────────── */
  .upcoming { left: auto !important; right: 2.2vw; }
  /* The box sizes to its content (capped) and never scrolls itself: the edit
     outline and its label sit just outside it and must stay visible. Only
     the inner list scrolls, and only vertically. */
  .recent {
    display: flex;
    flex-direction: column;
    max-height: min(480px, 44vh);
    min-height: 150px;
    overflow: visible;
    position: absolute !important;
    left: 2.2vw;
    top: 50%;
    transform: translateY(-55%);
    width: clamp(230px, 14.5vw, 300px);
    border-radius: 22px;
    padding: 16px 10px 10px;
  }
  .recent .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: thin;
    scrollbar-color: rgba(255, 255, 255, 0.22) transparent;
    margin-right: -4px;
    padding-right: 4px;
  }
  .recent h2 {
    flex: none;
  }
  h2 {
    margin: 0 8px 10px;
    font-size: 13px;
    font-weight: 650;
    color: rgba(255, 255, 255, 0.62);
  }
  ol {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  .recent button {
    width: 100%;
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) auto;
    align-items: center;
    gap: 11px;
    padding: 7px 8px;
    border: 0;
    border-radius: 12px;
    background: transparent;
    text-align: left;
    cursor: pointer;
    transition: background 160ms ease;
  }
  .recent button:hover {
    background: rgba(255, 255, 255, 0.07);
  }
  .recent button.current {
    background: color-mix(in oklab, var(--c1) 22%, rgba(255, 255, 255, 0.05));
    box-shadow: inset 2px 0 0 var(--c1);
  }
  .recent img,
  .ph {
    width: 40px;
    height: 40px;
    border-radius: 8px;
    object-fit: cover;
    background: rgba(255, 255, 255, 0.08);
  }
  .txt {
    display: grid;
    min-width: 0;
  }
  .txt .t,
  .txt .a {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .txt .t {
    font-size: 13.5px;
    font-weight: 600;
  }
  .txt .a {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.55);
  }
  .eq {
    display: flex;
    gap: 2px;
    align-items: end;
    height: 14px;
  }
  .eq i {
    width: 3px;
    border-radius: 2px;
    background: var(--c1);
    animation: eq 0.9s steps(9) infinite alternate;
  }
  .eq i:nth-child(2) { animation-delay: -0.3s; }
  .eq i:nth-child(3) { animation-delay: -0.6s; }
  @keyframes eq {
    from { height: 3px; }
    to { height: 14px; }
  }

  /* ── Dock ─────────────────────────────────────────── */
  .dock {
    justify-self: center;
    width: min(100%, 1180px);
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 10px 12px 10px 10px;
    border-radius: 24px;
  }
  .np {
    display: flex;
    align-items: center;
    gap: 11px;
    width: 230px;
    min-width: 0;
  }
  .np img,
  .np .ph {
    width: 46px;
    height: 46px;
    border-radius: 11px;
    object-fit: cover;
    flex: none;
  }
  .transport {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  button.icon {
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
    transition: background 160ms ease, transform 160ms ease;
  }
  button.icon:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  /* ── Edit layout ─────────────────────────────────── */
  .grip {
    position: absolute;
    inset: -10px;
    z-index: 6;
    border: 1.5px dashed color-mix(in oklab, var(--c1) 70%, white);
    border-radius: 18px;
    background: color-mix(in oklab, var(--c1) 10%, transparent);
    cursor: grab;
    touch-action: none;
  }
  .grip:active {
    cursor: grabbing;
    background: color-mix(in oklab, var(--c1) 18%, transparent);
  }
  .editing .sel,
  .editing .now:has(.sel) {
    z-index: 8 !important;
  }
  .sel > .grip {
    border-style: solid;
    background: color-mix(in oklab, var(--c1) 16%, transparent);
  }
  /* Resize handles: right edge, bottom edge, corner. */
  .rz {
    position: absolute;
    z-index: 1;
    touch-action: none;
  }
  .rz-e {
    top: 18%;
    bottom: 18%;
    right: -7px;
    width: 14px;
    cursor: ew-resize;
  }
  .rz-s {
    left: 18%;
    right: 18%;
    bottom: -7px;
    height: 14px;
    cursor: ns-resize;
  }
  .rz-se {
    right: -9px;
    bottom: -9px;
    width: 20px;
    height: 20px;
    cursor: nwse-resize;
  }
  .rz-e::after,
  .rz-s::after {
    content: "";
    position: absolute;
    border-radius: 3px;
    background: color-mix(in oklab, var(--c1) 70%, white);
    opacity: 0.8;
  }
  .rz-e::after {
    top: 50%;
    left: 5px;
    width: 4px;
    height: 34px;
    translate: 0 -50%;
  }
  .rz-s::after {
    left: 50%;
    top: 5px;
    height: 4px;
    width: 34px;
    translate: -50% 0;
  }
  .rz-se::after {
    content: "";
    position: absolute;
    inset: 3px;
    border-right: 3px solid color-mix(in oklab, var(--c1) 70%, white);
    border-bottom: 3px solid color-mix(in oklab, var(--c1) 70%, white);
    border-radius: 0 0 6px 0;
  }

  /* ── Your sizes: each block adapts to the box you give it ── */
  .wset {
    container-type: inline-size;
    min-width: 0;
    max-width: none;
  }
  .wset.hset {
    container-type: size;
  }
  /* Clip spill-over, but leave room for the edit outline, its label and the
     resize handles, which sit just outside the block (else a resized block
     can't be grabbed again). */
  .hset {
    max-height: none !important;
    min-height: 0;
    overflow: visible;
  }
  /* Clock: the face scales to fit the box (both ways); date follows. */
  .clock.wset {
    justify-self: center;
  }
  .clock.wset .face {
    width: 100%;
    height: auto;
    max-height: 32vh;
  }
  .clock.wset.hset {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }
  .clock.wset.hset .face {
    width: 100%;
    height: auto;
    max-height: calc(100cqh - 3.4em);
    flex: 0 1 auto;
    min-height: 0;
  }
  .clock.wset .date {
    font-size: clamp(10px, min(3.4cqi, 9cqh), 24px);
  }
  /* Album card: the art takes the largest square that fits, caption below. */
  .card.wset.hset {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
  }
  .card.wset.hset .art {
    width: max(40px, min(100cqi, calc(100cqh - 4.6em)));
    height: auto;
    aspect-ratio: 1;
    flex: none;
    -webkit-box-reflect: none;
  }
  .card.wset.hset figcaption {
    margin-top: 0.6em;
    width: max(40px, min(100cqi, calc(100cqh - 4.6em)));
  }
  .card.wset.hset figcaption .t {
    font-size: clamp(11px, min(7cqi, 5cqh), 22px);
  }
  .card.wset.hset figcaption .a {
    font-size: clamp(10px, min(5.5cqi, 4cqh), 17px);
  }
  @container (max-width: 170px) {
    figcaption .a {
      display: none;
    }
  }
  /* Short card: art only, as big as the box allows. */
  @container (max-height: 190px) {
    .card.wset.hset figcaption {
      display: none;
    }
    .card.wset.hset .art {
      width: max(40px, min(100cqi, 100cqh));
    }
  }
  /* Small clock: just the time, filling the box. */
  @container (max-height: 150px) {
    .clock .date,
    .clock .suffix {
      display: none;
    }
    .clock.wset.hset .face {
      max-height: 100cqh;
    }
  }
  /* Lyrics: type size follows the box, whichever side is tighter. */
  .words.wset {
    --flow-size: clamp(12px, 6.2cqi, 84px);
    --line-size: clamp(14px, 8.5cqi, 120px);
  }
  .words.wset.hset {
    --flow-size: clamp(13px, min(6.2cqi, 16cqh), 84px);
    --line-size: clamp(14px, min(8.5cqi, 24cqh), 120px);
  }
  .words.wset :global(.lines) {
    max-width: 100%;
  }
  .words.hset {
    align-content: stretch;
  }
  /* Recently played / Up next: rows shrink, art and second lines tuck away. */
  .recent.hset .list {
    flex: 1;
  }
  @container (max-width: 200px) {
    .recent img,
    .recent .ph {
      width: 30px;
      height: 30px;
    }
    .recent button {
      grid-template-columns: 30px minmax(0, 1fr) auto;
      gap: 8px;
    }
  }
  @container (max-width: 150px) {
    .recent .txt .a {
      display: none;
    }
  }
  /* Dock: as it narrows, the title, progress and search label tuck away. */
  .dock.wset {
    justify-self: center;
  }
  @container (max-width: 980px) {
    .np {
      width: auto;
    }
    .np .txt {
      display: none;
    }
  }
  @container (max-width: 760px) {
    .progress {
      display: none;
    }
  }
  @container (max-width: 560px) {
    .search span {
      display: none;
    }
    .search {
      width: auto;
    }
  }

  .grip span {
    position: absolute;
    top: -12px;
    left: 14px;
    padding: 3px 9px;
    border-radius: 999px;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    font-size: 11.5px;
    font-weight: 700;
    color: #fff;
  }
  .editing .upcoming,
  .editing .recent,
  .editing .card,
  .editing .words,
  .editing .clock,
  .editing .dock {
    position: relative;
  }
  .editing .recent {
    position: absolute !important;
  }
  .editbar {
    position: absolute !important;
    z-index: 7 !important;
    top: 2.4vh;
    left: 50%;
    translate: -50% 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 8px 8px 16px;
    border-radius: 999px;
    font-size: 13px;
  }
  .editbar button {
    height: 30px;
    padding: 0 14px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    background: transparent;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .editbar .done {
    border: 0;
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }

  button.icon.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    color: #fff;
  }
  button.icon:active {
    transform: scale(0.94);
  }
  button.icon svg,
  .search svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .progress {
    flex: 1;
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 120px;
  }
  .progress .time {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.55);
  }
  .bar {
    flex: 1;
    height: 18px;
    display: flex;
    align-items: center;
    cursor: pointer;
    position: relative;
  }
  .bar::before,
  .fill {
    height: 4px;
    border-radius: 2px;
  }
  .bar::before {
    content: "";
    position: absolute;
    inset: 7px 0;
    background: rgba(255, 255, 255, 0.14);
  }
  .fill {
    position: relative;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    box-shadow: 0 0 10px color-mix(in oklab, var(--c1) 70%, transparent);
  }
  .search {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 280px;
    height: 42px;
    padding: 0 14px;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.07);
    border: 1px solid rgba(255, 255, 255, 0.06);
    transition: background 160ms ease, border-color 160ms ease;
  }
  .search:focus-within {
    background: rgba(255, 255, 255, 0.11);
    border-color: color-mix(in oklab, var(--c1) 60%, transparent);
  }
  .search svg {
    width: 17px;
    height: 17px;
    opacity: 0.6;
    flex: none;
  }
  .browse { color: rgba(255,255,255,.7); cursor: pointer; text-align:left; }
  .browse.open {
    background: rgba(255, 255, 255, 0.12);
    border-color: color-mix(in oklab, var(--c1) 60%, transparent);
    color: #fff;
  }

  /* ── Search results ───────────────────────────────── */
  .scrim {
    position: absolute !important;
    inset: 0;
    z-index: 2 !important;
  }
  .panel {
    position: absolute !important;
    z-index: 3 !important;
    bottom: calc(3.2vh + 86px);
    border-radius: 24px;
  }
  .panel {
    right: max(4vw, calc(50% - 590px));
    width: 360px;
    padding: 14px 8px 8px;
  }
  /* Flowing lyrics need a tall window to scroll through. */
  .words.flowing {
    height: 56vh;
    min-height: 0;
    align-content: stretch;
  }

  .dock.hidden {
    display: none;
  }
  /* Dock off: tucked under the bottom edge, back when you point at it. */
  .dock {
    transition: opacity 260ms ease, transform 320ms cubic-bezier(0.2, 0.85, 0.25, 1);
  }
  .dock.tucked {
    opacity: 0;
    transform: translateY(74%);
  }
  .dock.tucked:hover,
  .dock.tucked:focus-within {
    opacity: 1;
    transform: none;
  }

  /* Clock looks */
  .clock.solid .face text {
    fill: url(#clock-ink);
    stroke: none;
    font-weight: 640;
  }
  .clock.minimal .face {
    height: clamp(90px, 15vh, 230px);
    filter: drop-shadow(0 4px 24px rgba(0, 0, 0, 0.45));
  }
  .clock.minimal .face text {
    fill: rgba(255, 255, 255, 0.94);
    stroke: none;
    font-weight: 200;
    font-stretch: 100%;
    letter-spacing: 0;
  }

  /* ── Full screen: lyrics on the left, art on the right, info in the corner ── */
  .poster {
    inset: 0 0 0 auto !important;
    width: 64vw;
    height: 100%;
    object-fit: cover;
    filter: brightness(0.62) saturate(1.15);
    -webkit-mask-image: linear-gradient(90deg, transparent 0, #000 42%);
    mask-image: linear-gradient(90deg, transparent 0, #000 42%);
  }
  .cinema .wash {
    opacity: 0.55;
  }
  .cinema .veil {
    background:
      linear-gradient(90deg, rgba(3, 2, 8, 0.78) 0, rgba(3, 2, 8, 0.35) 55%, rgba(3, 2, 8, 0.15) 100%),
      linear-gradient(0deg, rgba(3, 2, 8, 0.7), transparent 40%);
  }
  .cinema {
    padding: 6vh 4vw;
    grid-template-rows: minmax(0, 1fr);
  }
  .cinema .clock,
  .cinema .upcoming,
  .cinema .recent,
  .cinema .card {
    display: none !important;
  }
  .cinema .now,
  .cinema.has-recent .now {
    align-self: stretch;
    justify-self: start;
    width: 58vw;
    grid-template-columns: minmax(0, 1fr);
    padding-left: 0;
    padding-right: 0;
  }
  .cinema .words,
  .cinema .words.flowing {
    height: 88vh;
    align-content: center;
  }
  .cinema .words.flowing {
    align-content: stretch;
  }
  .cinema .panel {
    top: calc(3vh + 50px);
    bottom: auto;
    right: 2.4vw;
  }
  .cinema .words :global(.fl) {
    --flow-size: clamp(30px, 3.1vw, 72px);
  }
  .toolbar {
    position: absolute !important;
    top: 3vh;
    right: 2.4vw;
    display: flex;
    gap: 6px;
    z-index: 4 !important;
  }
  .toolbar .icon {
    background: rgba(255, 255, 255, 0.08);
  }
  .toolbar .seg {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.06);
  }
  .toolbar .seg .icon {
    background: transparent;
  }
  .toolbar .seg .icon.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .txt-btn {
    height: 34px;
    padding: 0 12px;
    border: 0;
    border-radius: 999px;
    background: transparent;
    font-size: 12.5px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.75);
    cursor: pointer;
  }
  .txt-btn.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    color: #fff;
  }

  /* Lyrics view: which side the lyrics sit on (art and info take the other). */
  .cinema.side-center .now {
    justify-self: center;
    width: min(70vw, 1100px);
  }
  .cinema.side-center .words :global(.lines),
  .cinema.side-center .words :global(.fl) {
    text-align: center;
    transform-origin: center center;
  }
  .cinema.side-right .now {
    justify-self: end;
  }
  .cinema.side-right .words :global(.fl) {
    text-align: right;
    transform-origin: right center;
  }
  .cinema.side-right .poster {
    inset: 0 auto 0 0 !important;
    -webkit-mask-image: linear-gradient(270deg, transparent 0, #000 42%);
    mask-image: linear-gradient(270deg, transparent 0, #000 42%);
  }
  .cinema.side-right .veil {
    background:
      linear-gradient(270deg, rgba(3, 2, 8, 0.78) 0, rgba(3, 2, 8, 0.35) 55%, rgba(3, 2, 8, 0.15) 100%),
      linear-gradient(0deg, rgba(3, 2, 8, 0.7), transparent 40%);
  }
  .cinema.side-right .corner {
    right: auto;
    left: 3vw;
    text-align: left;
  }
  /* Lyrics view size: compact (a narrow column of smaller lines) to large. */
  .cinema.size-compact .now {
    width: min(40vw, 640px);
  }
  .cinema.size-compact .words :global(.fl) {
    --flow-size: clamp(20px, 1.9vw, 40px);
  }
  .cinema.size-large .now {
    width: 72vw;
  }
  .cinema.size-large .words :global(.fl) {
    --flow-size: clamp(36px, 4.2vw, 96px);
  }
  .corner {
    position: absolute !important;
    right: 3vw;
    bottom: 5vh;
    width: clamp(320px, 24vw, 480px);
    display: grid;
    gap: 6px;
    text-align: right;
  }
  .corner .ct {
    margin: 0;
    font-family: var(--font-lyrics);
    font-size: clamp(22px, 1.8vw, 40px);
    font-weight: 760;
    font-stretch: 88%;
    line-height: 1.05;
  }
  .corner .ca {
    margin: 0 0 10px;
    font-size: clamp(13px, 0.95vw, 18px);
    color: rgba(255, 255, 255, 0.7);
  }
  .corner .times {
    display: flex;
    justify-content: space-between;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.6);
  }
  .corner .transport {
    justify-content: center;
    gap: 14px;
    margin-top: 4px;
  }

  /* ── Portrait screens ─────────────────────────────── */
  @media (max-aspect-ratio: 1/1) {
    .now,
    .has-recent .now,
    .has-upcoming .now {
      grid-template-columns: 1fr;
      justify-items: center;
      text-align: center;
      padding-left: 0;
      padding-right: 0;
      gap: 3vh;
    }
    .card {
      width: clamp(200px, 42vw, 420px);
    }
    figcaption {
      display: none;
    }
    .lines {
      max-width: none;
    }
    .line {
      font-size: clamp(34px, 7.5vw, 90px);
    }
    .after {
      font-size: clamp(18px, 3.2vw, 34px);
    }
    .recent, .upcoming {
      display: none;
    }
    .np {
      display: none;
    }
    .search {
      width: 200px;
    }
  }
</style>
