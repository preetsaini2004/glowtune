<script lang="ts">
  // Settings: a sidebar of sections, a search box to jump between them.
  // Every change applies live (the glow updates while you drag) and is saved
  // automatically.
  import { onMount, tick, untrack } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getVersion } from "@tauri-apps/api/app";
  import { np } from "$lib/nowPlaying.svelte";
  import ThemeEditor from "$lib/ThemeEditor.svelte";
  import Store from "$lib/Store.svelte";
  import SoundControls, { type SoundSettings } from "$lib/SoundControls.svelte";
  import ReleaseNotes from "$lib/ReleaseNotes.svelte";
  import FeedbackForm from "$lib/FeedbackForm.svelte";
  import { updates, ago, releaseDay } from "$lib/update.svelte";
  import { theme, FONT_STACKS } from "$lib/theme.svelte";
  import { searchSettings, didYouMean, SECTION_NAMES, type SettingHit } from "$lib/settingsIndex";
  import { parseOffset, formatOffset } from "$lib/lyricsTiming";

  type Settings = {
    glowEnabled: boolean;
    glowMonitors: string[];
    lyricsOnDesktop: boolean;
    lyricsOverFullscreen: boolean;
    desktopLyricsStyle: string;
    cleanLyrics: boolean;
    playerOnTop: boolean;
    playerStyle: string;
    playerSize: string;
    taskbarPlayer: string;
    glowHideFullscreen: boolean;
    blockAds: boolean;
    activeTheme: string;
    stageEnabled: boolean;
    stageMonitor: string;
    stageClock: boolean;
    stageLyrics: boolean;
    stageRecent: boolean;
    stageUpcoming: boolean;
    stageMotion: boolean;
    stageEmoji: boolean;
    stageLyricsStyle: string;
    hotkeys: boolean;
    listeningHistory: boolean;
    updateCheck: boolean;
    lyricsMotion: string;
    lyricsPlace: string;
    lyricsSize: number;
    lyricsFont: string;
  } & SoundSettings;
  type Top = { title: string; artist: string; plays: number; ms: number };
  type Stats = {
    totalMs: number;
    plays: number;
    songs: Top[];
    artists: Top[];
    bars: { start: number; ms: number }[];
    barDays: number;
    busiestHour: number | null;
    first: number | null;
  };
  type Range = "week" | "month" | "year" | "all";
  type Monitor = { name: string; primary: boolean; width: number; height: number };
  type LyricsInfo = { status: string; offsetMs: number; lines?: { timeMs: number; text: string }[] };
  type SectionId = "lyrics" | "scene" | "glow" | "sound" | "services" | "themes" | "store" | "player" | "listening" | "keys" | "feedback" | "about";
  type Hotkey = { keys: string; label: string; ok: boolean; note: string | null };

  const SECTIONS: { id: SectionId; name: string; icon: string; words: string }[] = [
    { id: "lyrics", name: "Lyrics", icon: "M4 6h16M4 12h10M4 18h13", words: "lyrics desktop timing sync offset style karaoke flow animation rise still position top middle bottom size font" },
    { id: "scene", name: "Desktop scene", icon: "M3 5h18v12H3zM8 21h8M12 17v4", words: "desktop scene wallpaper clock full screen recently played background motion" },
    { id: "glow", name: "Edge glow", icon: "M4 4h16v16H4zM8 8h8v8H8z", words: "glow edge light screens monitors fullscreen games" },
    { id: "sound", name: "Sound", icon: "M4 9v6M8 5v14M12 8v8M16 4v16M20 10v4", words: "sound equalizer eq bass treble presets crossfade automix blend audio effects" },
    { id: "services", name: "Services", icon: "M5 12a7 7 0 0 1 14 0M8 12a4 4 0 0 1 8 0M12 12v8", words: "services spotify apple music youtube soundcloud login sign in ads block" },
    { id: "themes", name: "Themes", icon: "M12 3a9 9 0 1 0 0 18c1 0 1.5-.8 1.5-1.6 0-1.2-1-1.4-1-2.4 0-.8.7-1.5 1.5-1.5H16a5 5 0 0 0 5-5c0-4.1-4-7.5-9-7.5z", words: "themes colors appearance fonts glass corners import export" },
    { id: "store", name: "Store", icon: "M3 8h18v13H3zM3 12h18M7 8V3h10v5M8 15v3M16 15v3", words: "store crate themes scene looks presets extensions install free" },
    { id: "player", name: "Player", icon: "M4 15h16v5H4zM8 4v8M16 4v8", words: "player mini on top" },
    { id: "listening", name: "Listening", icon: "M4 20V10M9 20V4M14 20v-7M19 20v-11", words: "listening stats history wrapped top songs artists hours played" },
    { id: "keys", name: "Hotkeys", icon: "M3 7h18v10H3zM7 11h.01M11 11h.01M15 11h.01M7 14h10", words: "hotkeys shortcuts keyboard keys play pause next volume presets sleep timer open settings options ctrl alt o" },
    { id: "feedback", name: "Feedback", icon: "M4 5h16v11H9l-5 4zM8 9h8M8 12h5", words: "feedback send message bug report complaint idea feature request review rate help contact support email" },
    { id: "about", name: "About", icon: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01", words: "about version update updates new version upgrade check for updates release notes" },
  ];
  const SERVICES = [
    { id: "spotify", name: "Spotify", note: "Free or Premium. Free plays with Spotify's ads." },
    { id: "apple-music", name: "Apple Music", note: "Needs an Apple Music subscription for full songs." },
    { id: "youtube-music", name: "YouTube Music", note: "Free with ads, or YouTube Premium." },
    { id: "soundcloud", name: "SoundCloud", note: "Free." },
  ];
  const STYLES = [
    { id: "flow", name: "Flow" },
    { id: "bold", name: "Bold" },
    { id: "karaoke", name: "Karaoke" },
    { id: "outline", name: "Outline" },
    { id: "sticker", name: "Sticker" },
    { id: "stars", name: "Stars" },
    { id: "depth", name: "Depth" },
    { id: "rain", name: "Rain" },
    { id: "orbit", name: "Orbit" },
    { id: "shards", name: "Shards" },
    { id: "code", name: "Code" },
    { id: "melt", name: "Melt" },
    { id: "tags", name: "Tags" },
  ];

  let s = $state<Settings | null>(null);
  let monitors = $state<Monitor[]>([]);
  let lyrics = $state<LyricsInfo>({ status: "none", offsetMs: 0 });
  /** Why the floating lyrics are hidden right now (lyrics_gate). */
  let lyricsHint = $state<{ reason: string; text: string } | null>(null);
  let error = $state("");
  let blocked = $state(0);
  let rules = $state<number | null>(null);
  let version = $state("");
  let section = $state<SectionId>("lyrics");
  let hotkeys = $state<Hotkey[]>([]);

  // ── Listening ── your own small "Wrapped".
  let range = $state<Range>("week");
  let stats = $state<Stats | null>(null);
  let arts = $state<Record<string, string | null>>({});
  let confirmClear = $state(false);
  const RANGES: { id: Range; name: string; words: string }[] = [
    { id: "week", name: "This week", words: "this week" },
    { id: "month", name: "This month", words: "in the last 30 days" },
    { id: "year", name: "This year", words: "in the last year" },
    { id: "all", name: "All time", words: "since you started" },
  ];
  function readStats() {
    invoke<Stats>("listening_stats", { range, tzOffsetMin: new Date().getTimezoneOffset() })
      .then((v) => (stats = v))
      .catch(() => {});
  }
  $effect(() => {
    if (section !== "listening") return;
    void range;
    readStats();
    // The song playing now joins in after 30 s: refresh while open.
    const id = setInterval(readStats, 30_000);
    return () => clearInterval(id);
  });
  const artKey = (t: Top) => `${t.title}\u001f${t.artist}`;
  // Covers for the top songs, found once each in the iTunes catalog.
  $effect(() => {
    for (const t of stats?.songs.slice(0, 5) ?? []) {
      const key = artKey(t);
      if (key in untrack(() => arts)) continue;
      arts[key] = null;
      invoke<{ songs: { artwork: string | null }[] }>("search_catalog", { query: `${t.title} ${t.artist}` })
        .then((r) => (arts[key] = r.songs[0]?.artwork ?? null))
        .catch(() => {});
    }
  });
  const artOf = (t: Top) => arts[artKey(t)] ?? null;
  /** An artist's picture: the cover of their top song here, if any. */
  function artistArt(a: Top): string | null {
    const s = stats?.songs.find((x) => x.artist.toLowerCase().includes(a.title.toLowerCase()) && artOf(x));
    return s ? artOf(s) : null;
  }
  function spent(ms: number): string {
    const m = Math.round(ms / 60_000);
    if (m < 60) return `${m} min`;
    const h = Math.floor(m / 60);
    return m % 60 ? `${h} h ${m % 60} min` : `${h} h`;
  }
  function hourName(h: number): string {
    return new Date(2000, 0, 1, h).toLocaleTimeString(undefined, { hour: "numeric" });
  }
  function barLabel(start: number, days: number): string {
    const d = new Date(start * 1000);
    return days === 1
      ? d.toLocaleDateString(undefined, { weekday: "short", day: "numeric" })
      : d.toLocaleDateString(undefined, { month: "short", day: "numeric" });
  }
  const barMax = $derived(Math.max(1, ...(stats?.bars.map((b) => b.ms) ?? [1])));
  async function clearHistory() {
    try {
      await invoke("clear_listening");
      confirmClear = false;
      arts = {};
      readStats();
    } catch (e) {
      error = String(e);
    }
  }
  // Re-read after the switch flips (keys taken by other apps show up then).
  $effect(() => {
    if (section !== "keys") return;
    void s?.hotkeys;
    const id = setTimeout(() => invoke<Hotkey[]>("hotkey_list").then((h) => (hotkeys = h)).catch(() => {}), 250);
    return () => clearTimeout(id);
  });
  let query = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  const palette = $derived(np.palette);
  const current = $derived(SECTIONS.find((x) => x.id === section)!);

  onMount(() => {
    const tab = new URLSearchParams(location.search).get("tab");
    if (tab === "theme") section = "themes";
    else if (tab && SECTIONS.some((x) => x.id === tab)) section = tab as SectionId;
    np.start();
    updates.start();
    invoke<Settings>("get_settings").then((v) => (s = v));
    invoke<Monitor[]>("list_monitors").then((m) => (monitors = m));
    invoke<LyricsInfo>("get_lyrics").then((l) => (lyrics = l));
    invoke<number>("adblock_stats").then((n) => (blocked = n)).catch(() => {});
    invoke<number>("adblock_rules").then((n) => (rules = n)).catch(() => {});
    getVersion().then((v) => (version = v)).catch(() => {});
    const un = listen<LyricsInfo>("lyrics", (e) => (lyrics = e.payload));
    invoke<{ reason: string; text: string } | null>("lyrics_hint").then((h) => (lyricsHint = h)).catch(() => {});
    const unHint = listen<{ reason: string; text: string } | null>("lyrics-hint", (e) => (lyricsHint = e.payload));
    // The tray and the desktop scene change settings too: stay in sync.
    const unSettings = listen<Settings>("settings", (e) => (s = e.payload));
    const unTab = listen<string>("settings-tab", (e) => {
      const tab = e.payload === "theme" ? "themes" : e.payload;
      if (SECTIONS.some((x) => x.id === tab)) {
        section = tab as SectionId;
        query = "";
      }
    });
    return () => {
      un.then((f) => f());
      unHint.then((f) => f());
      unSettings.then((f) => f());
      unTab.then((f) => f());
    };
  });

  /** Apply now, but coalesce rapid slider moves into one save every 120 ms. */
  function commit() {
    if (!s) return;
    clearTimeout(saveTimer);
    const snapshot = $state.snapshot(s);
    saveTimer = setTimeout(async () => {
      try {
        s = await invoke<Settings>("set_settings", { new: snapshot });
        error = "";
      } catch (e) {
        error = String(e);
      }
    }, 120);
  }

  function monitorOn(name: string): boolean {
    return !s || s.glowMonitors.length === 0 || s.glowMonitors.includes(name);
  }
  function toggleMonitor(name: string) {
    if (!s) return;
    const all = monitors.map((m) => m.name);
    const on = new Set(s.glowMonitors.length ? s.glowMonitors : all);
    if (on.has(name)) on.delete(name);
    else on.add(name);
    // All selected is stored as "all" (empty list) so new screens glow too.
    s.glowMonitors = on.size === all.length ? [] : all.filter((n) => on.has(n));
    commit();
  }

  function monitorLabel(m: Monitor, i: number) {
    const shape = m.height > m.width ? "portrait" : "landscape";
    return `${m.primary ? "Main screen" : `Screen ${i + 1}`}, ${m.width}×${m.height} ${shape}`;
  }

  async function nudge(delta: number) {
    try {
      await invoke("nudge_lyrics", { deltaMs: delta });
    } catch (e) {
      error = String(e);
    }
  }

  // ---- Lyrics timing: Later / the number box / Earlier, and Sync (back to 0 s) ----
  /** What the number box shows while typing (null = the real offset). */
  let offsetDraft = $state<string | null>(null);
  let offsetError = $state("");
  const synced = $derived(lyrics.status === "synced");

  async function setOffset(ms: number) {
    try {
      await invoke("set_lyrics_offset", { offsetMs: ms });
      offsetError = "";
      return true;
    } catch (e) {
      offsetError = String(e);
      return false;
    }
  }

  async function offsetKeys(e: KeyboardEvent) {
    const input = e.currentTarget as HTMLInputElement;
    if (e.key === "Enter") {
      const parsed = parseOffset(input.value);
      if ("error" in parsed) {
        offsetError = parsed.error;
        return;
      }
      if (await setOffset(parsed.ms)) {
        offsetDraft = null;
        input.blur();
      }
    } else if (e.key === "Escape") {
      e.preventDefault();
      offsetDraft = null;
      offsetError = "";
      input.blur();
    } else if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      const parsed = parseOffset(input.value);
      const base = "ms" in parsed ? parsed.ms : lyrics.offsetMs;
      const next = base + (e.key === "ArrowUp" ? 100 : -100);
      if (await setOffset(next)) offsetDraft = null;
    }
  }

  /** Sync: the lyrics file's own timing again, 0 s. */
  async function sync() {
    offsetDraft = null;
    await setOffset(0);
  }

  // Search: individual settings, typos allowed (lib/settingsIndex.ts).
  let findInput: HTMLInputElement | undefined = $state();
  const results = $derived(query.trim() ? searchSettings(query) : []);
  const guesses = $derived(query.trim() && !results.length ? didYouMean(query) : []);
  let flashTimer: ReturnType<typeof setTimeout> | undefined;

  function searchKeys(e: KeyboardEvent) {
    const first = results[0] ?? guesses[0];
    if (e.key === "Enter" && first) go(first);
    else if (e.key === "Escape" && query) {
      e.preventDefault();
      query = "";
    }
  }

  function windowKeys(e: KeyboardEvent) {
    if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === "f") {
      e.preventDefault();
      findInput?.focus();
      findInput?.select();
    }
  }

  const norm = (t: string | null) => (t ?? "").replace(/\s+/g, " ").trim().toLowerCase();

  /** Opens the setting's section, scrolls to it and highlights it. */
  async function go(hit: SettingHit) {
    query = "";
    section = hit.section as SectionId;
    const label = norm(hit.label ?? hit.name);
    // The section fades in, and the theme editor loads its theme first: look
    // for the label for up to 1.5 s.
    const until = performance.now() + 1500;
    while (performance.now() < until) {
      await tick();
      await new Promise((r) => requestAnimationFrame(r));
      const body = document.querySelector(".content .body");
      const el = [...(body?.querySelectorAll("h3, span, button, legend") ?? [])].find((n) => norm(n.textContent) === label);
      if (!el) continue;
      const target = (el.closest(".row, .nudge, .slider, .field, .check, label, .chips") ?? el.closest(".card") ?? el) as HTMLElement;
      target.scrollIntoView({ block: "center", behavior: "smooth" });
      document.querySelectorAll(".flash").forEach((n) => n.classList.remove("flash"));
      target.classList.add("flash");
      clearTimeout(flashTimer);
      flashTimer = setTimeout(() => target.classList.remove("flash"), 1800);
      return;
    }
  }
</script>

<svelte:window onkeydown={windowKeys} />

<main style:--c1={palette[0]} style:--c2={palette[1] ?? palette[0]}>
  <nav class="side" data-tauri-drag-region>
    <p class="brand" data-tauri-drag-region>Glowtune</p>
    <h1 data-tauri-drag-region>Settings</h1>
    <label class="find">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5 20 20" /></svg>
      <input bind:this={findInput} bind:value={query} onkeydown={searchKeys} placeholder="Search settings" title="Search settings (Ctrl+F)" aria-label="Search settings" spellcheck="false" />
    </label>
    {#if query.trim()}
      <ul class="results" aria-label="Matching settings">
        {#if !results.length}
          <li class="none">{guesses.length ? "Nothing matches. Did you mean:" : "Nothing matches."}</li>
        {/if}
        {#each results.length ? results : guesses as hit, i (hit.section + hit.name)}
          <li>
            <button class:first={i === 0} onclick={() => go(hit)}>
              <span class="what">{hit.name}</span>
              <span class="where">{SECTION_NAMES[hit.section]}</span>
            </button>
          </li>
        {/each}
      </ul>
    {:else}
      <ul>
        {#each SECTIONS as x (x.id)}
          {#if x.id === "about"}<li class="rule" aria-hidden="true"></li>{/if}
          <li>
            <button class:on={section === x.id} aria-current={section === x.id ? "page" : undefined} onclick={() => (section = x.id)}>
              <svg viewBox="0 0 24 24" aria-hidden="true"><path d={x.icon} /></svg>
              {x.name}
            </button>
          </li>
        {/each}
      </ul>
    {/if}
  </nav>

  <section class="content">
    <header data-tauri-drag-region>
      <h2 data-tauri-drag-region>{current.name}</h2>
      <button class="close" aria-label="Close settings" onclick={() => getCurrentWindow().close()}>
        <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7 7 17" /></svg>
      </button>
    </header>

    {#key section}
      <div class="body" in:fade={{ duration: 160 }}>
        {#if section === "themes"}
          <ThemeEditor />
        {:else if section === "store"}
          <Store />
        {:else if !s}
          <p class="hint">Loading…</p>
        {:else if section === "lyrics"}
          <div class="card">
            <div class="row">
              <div>
                <h3>Lyrics on desktop</h3>
                <p class="hint">
                  {#if lyricsHint && lyricsHint.reason !== "plain" && lyricsHint.reason !== "none" && lyricsHint.reason !== "instrumental"}<span class="why">{lyricsHint.text}</span>
                  {:else if lyrics.status === "synced"}Synced lyrics found for this song.
                  {:else if lyrics.status === "loading"}Looking up lyrics…
                  {:else if lyrics.status === "instrumental"}This song is instrumental.
                  {:else if lyrics.status === "plain"}Only unsynced lyrics exist for this song.
                  {:else}No lyrics for what's playing right now.{/if}
                </p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.lyricsOnDesktop} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Lyrics on desktop</span>
              </label>
            </div>
            <div class="row">
              <div>
                <h3>Desktop look</h3>
                <p class="hint">How the floating lyrics look. Theme follows the active theme; the others are the scene's looks.</p>
              </div>
              <div class="chips" role="radiogroup" aria-label="Desktop lyrics look">
                {#each [{ id: "theme", name: "Theme" }, { id: "stars", name: "Stars" }, { id: "rain", name: "Rain" }, { id: "shards", name: "Shards" }, { id: "code", name: "Code" }, { id: "melt", name: "Melt" }] as st}
                  <button
                    role="radio"
                    aria-checked={s.desktopLyricsStyle === st.id}
                    class:on={s.desktopLyricsStyle === st.id}
                    onclick={() => {
                      if (s) s.desktopLyricsStyle = st.id;
                      commit();
                    }}>{st.name}</button
                  >
                {/each}
              </div>
            </div>
            <div class="row">
              <div>
                <h3>Show over full-screen games</h3>
                <p class="hint">Keeps the lyrics up while a game or video fills the screen. Works with borderless or windowed full screen.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.lyricsOverFullscreen} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Show lyrics over full-screen games</span>
              </label>
            </div>
            <div class="row">
              <div>
                <h3>Hide explicit words</h3>
                <p class="hint">Swear words in the lyrics show as stars, like f***. The song itself is unchanged.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.cleanLyrics} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Hide explicit words</span>
              </label>
            </div>
            <div class="nudge">
              <span>Position on screen</span>
              <div class="stepper">
                <button onclick={() => invoke("lyrics_move", { on: true })}>Move lyrics</button>
                <button onclick={() => invoke("lyrics_reset_position")}>Reset</button>
              </div>
            </div>
          </div>
          <div class="card timing" class:muted={!synced}>
            <div class="row">
              <div>
                <h3>Timing for this song</h3>
                <p class="hint">
                  {synced ? "+ shows the lyrics earlier, − later. Saved for this song." : "Only songs with synced lyrics can be timed."}
                </p>
              </div>
              <div class="stepper">
                <button onclick={() => nudge(-500)} disabled={!synced} aria-label="Show lyrics later">Later</button>
                <label class="offset" class:bad={offsetError}>
                  <input
                    value={offsetDraft ?? formatOffset(lyrics.offsetMs)}
                    oninput={(e) => {
                      offsetDraft = (e.currentTarget as HTMLInputElement).value;
                      offsetError = "";
                    }}
                    onkeydown={offsetKeys}
                    onblur={() => {
                      offsetDraft = null;
                      offsetError = "";
                    }}
                    disabled={!synced}
                    inputmode="decimal"
                    spellcheck="false"
                    aria-label="Lyrics timing in seconds, + earlier, − later"
                    title="Type seconds and press Enter. ↑ / ↓ move it 0.1 s."
                  />
                  <span>s</span>
                </label>
                <button onclick={() => nudge(500)} disabled={!synced} aria-label="Show lyrics earlier">Earlier</button>
              </div>
            </div>
            {#if offsetError}<p class="hint warn">{offsetError}</p>{/if}
            <div class="sync-row">
              <button class="open primary" onclick={sync} disabled={!synced || lyrics.offsetMs === 0} title="Ctrl+Alt+T">Sync</button>
              <p class="hint small">{lyrics.offsetMs === 0 ? "In sync: 0 s." : "Back to 0 s, where the lyrics line up with the song. Ctrl+Alt+T does it from anywhere."}</p>
            </div>
          </div>
          <div class="card lyric-look">
            <h3>How the floating lyrics look</h3>
            <p class="hint">"Theme's" follows the active theme, so Store themes still look as made.</p>
            <div class="row">
              <div>
                <h3 class="sub">Animation</h3>
                <p class="hint">
                  {s.desktopLyricsStyle === "theme" ? "For the Theme desktop look." : "Only for the Theme desktop look (Desktop look above)."}
                </p>
              </div>
              <div class="chips" role="radiogroup" aria-label="Lyrics animation">
                {#each [["", "Theme's"], ["rise", "Rise"], ["karaoke", "Karaoke"], ["static", "Still"]] as [id, name]}
                  <button
                    role="radio"
                    aria-checked={s.lyricsMotion === id}
                    class:on={s.lyricsMotion === id}
                    disabled={s.desktopLyricsStyle !== "theme"}
                    onclick={() => {
                      if (s) s.lyricsMotion = id;
                      commit();
                    }}>{name}</button
                  >
                {/each}
              </div>
            </div>
            <div class="row">
              <h3 class="sub">Position</h3>
              <div class="chips" role="radiogroup" aria-label="Lyrics position">
                {#each [["", "Theme's"], ["bottom-center", "Bottom"], ["center", "Middle"], ["top-center", "Top"]] as [id, name]}
                  <button
                    role="radio"
                    aria-checked={s.lyricsPlace === id}
                    class:on={s.lyricsPlace === id}
                    onclick={() => {
                      if (s) s.lyricsPlace = id;
                      commit();
                    }}>{name}</button
                  >
                {/each}
              </div>
            </div>
            <div class="row">
              <h3 class="sub">Size</h3>
              <div class="size">
                <input
                  type="range"
                  min="16"
                  max="96"
                  step="1"
                  aria-label="Lyrics size"
                  value={s.lyricsSize || theme.theme?.font.lyrics_size || 50}
                  oninput={(e) => {
                    if (s) s.lyricsSize = Number((e.currentTarget as HTMLInputElement).value);
                  }}
                  onchange={commit}
                />
                <output>{s.lyricsSize || theme.theme?.font.lyrics_size || 50}px</output>
                <button
                  class="chip-btn"
                  class:on={s.lyricsSize === 0}
                  onclick={() => {
                    if (s) s.lyricsSize = 0;
                    commit();
                  }}>Theme's</button
                >
              </div>
            </div>
            <div class="row">
              <div>
                <h3 class="sub">Font</h3>
                <p class="hint">Used everywhere lyrics show: the scene, the floating lyrics, the player and lyric cards.</p>
              </div>
              <select class="pick" bind:value={s.lyricsFont} onchange={commit} aria-label="Lyrics font">
                <option value="">Theme's</option>
                {#each Object.keys(FONT_STACKS) as f}<option value={f}>{f}</option>{/each}
              </select>
            </div>
          </div>
          <div class="card">
            <h3>Style in the desktop scene</h3>
            <p class="hint">How the lyrics look on the desktop scene.</p>
            <div class="chips" role="radiogroup" aria-label="Lyrics style">
              {#each STYLES as st}
                <button
                  role="radio"
                  aria-checked={s.stageLyricsStyle === st.id}
                  class:on={s.stageLyricsStyle === st.id}
                  onclick={() => {
                    if (s) s.stageLyricsStyle = st.id;
                    commit();
                  }}>{st.name}</button
                >
              {/each}
            </div>
          </div>
        {:else if section === "scene"}
          <div class="card">
            <div class="row">
              <div>
                <h3>Desktop scene</h3>
                <p class="hint">A live wallpaper behind your windows: album art, clock, lyrics and search. Press F11 on it for full screen.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.stageEnabled} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Desktop scene</span>
              </label>
            </div>
          </div>
          <fieldset class="card" disabled={!s.stageEnabled}>
            <h3>Show</h3>
            {#each [
              { key: "stageClock", label: "Clock" },
              { key: "stageLyrics", label: "Lyrics" },
              { key: "stageRecent", label: "Recently played" },
              { key: "stageUpcoming", label: "Up next" },
              { key: "stageMotion", label: "Moving background" },
              { key: "stageEmoji", label: "Emoji in lyrics" },
            ] as const as row}
              <label class="check">
                <input type="checkbox" bind:checked={s[row.key]} onchange={commit} />
                <span>{row.label}</span>
              </label>
            {/each}
          </fieldset>
            <fieldset class="card" disabled={!s.stageEnabled}>
              <h3>Screen</h3>
              <label class="check">
                <input
                  type="radio"
                  name="stage-screen"
                  checked={s.stageMonitor === "*"}
                  onchange={() => {
                    if (s) s.stageMonitor = "*";
                    commit();
                  }}
                />
                <span>All screens</span>
              </label>
              {#each monitors as m, i (m.name)}
                <label class="check">
                  <input
                    type="radio"
                    name="stage-screen"
                    checked={s.stageMonitor === m.name || (!s.stageMonitor && m.primary)}
                    onchange={() => {
                      if (s) s.stageMonitor = m.primary ? "" : m.name;
                      commit();
                    }}
                  />
                  <span>{monitorLabel(m, i)}</span>
                </label>
              {/each}
            </fieldset>
        {:else if section === "glow"}
          <div class="card">
            <div class="row">
              <div>
                <h3>Edge glow</h3>
                <p class="hint">Light around your screens in the album's colors. Its look comes from the theme.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.glowEnabled} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Edge glow</span>
              </label>
            </div>
          </div>
          <fieldset class="card" disabled={!s.glowEnabled}>
            <label class="check">
              <input type="checkbox" bind:checked={s.glowHideFullscreen} onchange={commit} />
              <span>Hide over fullscreen apps and games</span>
            </label>
            {#if monitors.length > 1}
              <h3 class="sub">Screens that glow</h3>
              {#each monitors as m, i (m.name)}
                <label class="check">
                  <input type="checkbox" checked={monitorOn(m.name)} onchange={() => toggleMonitor(m.name)} />
                  <span>{monitorLabel(m, i)}</span>
                </label>
              {/each}
            {/if}
          </fieldset>
        {:else if section === "sound"}
          <div class="card">
            <div class="row">
              <div>
                <h3>Equalizer</h3>
                <p class="hint">Shape the sound of music played in Glowtune's service windows.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.eqEnabled} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Equalizer</span>
              </label>
            </div>
            <SoundControls
              part="eq"
              settings={s}
              save={(patch) => {
                if (!s) return;
                Object.assign(s, patch);
                commit();
              }}
            />
          </div>
          <div class="card">
            <div>
              <h3>Between songs</h3>
              <p class="hint">How one song hands over to the next. Works with the equalizer on or off.</p>
            </div>
            <SoundControls
              part="blend"
              settings={s}
              save={(patch) => {
                if (!s) return;
                Object.assign(s, patch);
                commit();
              }}
            />
          </div>
          <p class="hint">
            For music played inside Glowtune. Processed on this PC; nothing is saved or sent.
          </p>
        {:else if section === "services"}
          <div class="card">
            <h3>Your music services</h3>
            <p class="hint">Sign in once inside Glowtune; each service remembers you. Search results from the desktop scene play here.</p>
            <ul class="services">
              {#each SERVICES as sv}
                <li>
                  <div>
                    <p class="name">{sv.name}</p>
                    <p class="hint small">{sv.note}</p>
                  </div>
                  <button class="open" onclick={() => invoke("open_service", { service: sv.id })}>Open</button>
                </li>
              {/each}
            </ul>
          </div>
          <div class="card">
            <div class="row">
              <div>
                <h3>Block ads and trackers</h3>
                <p class="hint">
                  In the service windows. Audio ads that can't be blocked (Spotify free) are silenced and skipped through.
                  {#if blocked > 0}{blocked.toLocaleString()} blocked so far.{/if}
                </p>
                {#if rules === 0}
                  <p class="hint warn">The filter lists haven't downloaded yet, so nothing is blocked. Check your internet connection, then restart Glowtune.</p>
                {:else if rules}
                  <p class="hint small">{rules.toLocaleString()} rules loaded, refreshed weekly.</p>
                {/if}
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.blockAds} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Block ads and trackers</span>
              </label>
            </div>
          </div>
        {:else if section === "player"}
          <div class="card">
            <h3>Size</h3>
            <div class="chips" role="radiogroup" aria-label="Player size">
              {#each [{ id: "s", name: "Small" }, { id: "m", name: "Medium" }, { id: "l", name: "Large" }] as st}
                <button
                  role="radio"
                  aria-checked={s.playerSize === st.id}
                  class:on={s.playerSize === st.id}
                  onclick={() => {
                    if (s) s.playerSize = st.id;
                    commit();
                  }}>{st.name}</button
                >
              {/each}
            </div>
          </div>
          <div class="card">
            <h3>Taskbar player</h3>
            <p class="hint">A slim player on the taskbar itself while music is on, in the empty space at its left or right end. Click the song to open the big player.</p>
            <div class="chips" role="radiogroup" aria-label="Taskbar player">
              {#each [{ id: "off", name: "Off" }, { id: "left", name: "Left end" }, { id: "right", name: "Right end" }] as st}
                <button
                  role="radio"
                  aria-checked={s.taskbarPlayer === st.id}
                  class:on={s.taskbarPlayer === st.id}
                  onclick={() => {
                    if (s) s.taskbarPlayer = st.id;
                    commit();
                  }}>{st.name}</button
                >
              {/each}
            </div>
          </div>
          <div class="card">
            <h3>Look</h3>
            <p class="hint">The button in the player's top-left corner switches looks too.</p>
            <div class="chips" role="radiogroup" aria-label="Player look">
              {#each [{ id: "glass", name: "Glass" }, { id: "disc", name: "Disc" }, { id: "card", name: "Card" }, { id: "pill", name: "Pill" }, { id: "lyric", name: "Lyric" }, { id: "clear", name: "Clear" }] as st}
                <button
                  role="radio"
                  aria-checked={s.playerStyle === st.id}
                  class:on={s.playerStyle === st.id}
                  onclick={() => {
                    if (s) s.playerStyle = st.id;
                    commit();
                  }}>{st.name}</button
                >
              {/each}
            </div>
          </div>
          <div class="card">
            <div class="row">
              <div>
                <h3>Keep player on top</h3>
                <p class="hint">The mini player floats above other windows.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.playerOnTop} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Keep player on top</span>
              </label>
            </div>
          </div>
        {:else if section === "listening"}
          <div class="chips" role="radiogroup" aria-label="Time range">
            {#each RANGES as r}
              <button role="radio" aria-checked={range === r.id} class:on={range === r.id} onclick={() => (range = r.id)}>{r.name}</button>
            {/each}
          </div>
          {#if stats && stats.plays > 0}
            <div class="card listen-hero">
              <p class="spent">{spent(stats.totalMs)}</p>
              <p class="hint">
                of music {RANGES.find((r) => r.id === range)?.words}, across {stats.plays} {stats.plays === 1 ? "play" : "plays"}.
                {#if stats.busiestHour !== null}You listen most around {hourName(stats.busiestHour)}.{/if}
              </p>
              <div class="bars" role="img" aria-label={stats.barDays === 1 ? "Listening time per day" : "Listening time per week"}>
                {#each stats.bars as b, i}
                  <span class="bar" class:now={i === stats.bars.length - 1} style:height="{Math.max(2, (b.ms / barMax) * 100)}%" title="{barLabel(b.start, stats.barDays)}: {spent(b.ms)}"></span>
                {/each}
              </div>
              <div class="bar-ends">
                <span>{barLabel(stats.bars[0].start, stats.barDays)}</span>
                <span>{stats.barDays === 1 ? "Today" : "This week"}</span>
              </div>
            </div>
            <div class="tops">
              <div class="card">
                <h3>Top songs</h3>
                <ol class="toplist">
                  {#each stats.songs as t, i}
                    <li>
                      <span class="rank">{i + 1}</span>
                      <span class="cover">{#if artOf(t)}<img src={artOf(t)} alt="" />{/if}</span>
                      <span class="who"><span class="t">{t.title}</span><span class="a">{t.artist}</span></span>
                      <span class="n">{t.plays}×</span>
                    </li>
                  {/each}
                </ol>
              </div>
              <div class="card">
                <h3>Top artists</h3>
                <ol class="toplist">
                  {#each stats.artists as a, i}
                    <li>
                      <span class="rank">{i + 1}</span>
                      <span class="cover round">{#if artistArt(a)}<img src={artistArt(a)} alt="" />{:else}{a.title.slice(0, 1)}{/if}</span>
                      <span class="who"><span class="t">{a.title}</span><span class="a">{spent(a.ms)}</span></span>
                      <span class="n">{a.plays}×</span>
                    </li>
                  {/each}
                </ol>
              </div>
            </div>
          {:else if stats}
            <div class="card">
              <h3>Nothing here yet</h3>
              <p class="hint">
                {s.listeningHistory
                  ? "Play some music: a song shows up here once it has played for 30 seconds."
                  : "Turn on listening history below to see your top songs and artists."}
              </p>
            </div>
          {/if}
          <div class="card">
            <div class="row">
              <div>
                <h3>Keep listening history</h3>
                <p class="hint">Song, artist and time played, kept on this PC only. Ads aren't counted.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.listeningHistory} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Keep listening history</span>
              </label>
            </div>
            {#if confirmClear}
              <div class="row">
                <p class="hint warn">Delete your whole listening history? This can't be undone.</p>
                <div class="clear-btns">
                  <button class="open" onclick={() => (confirmClear = false)}>Keep it</button>
                  <button class="open danger" onclick={clearHistory}>Delete history</button>
                </div>
              </div>
            {:else}
              <div><button class="open" onclick={() => (confirmClear = true)}>Delete history…</button></div>
            {/if}
          </div>
        {:else if section === "keys"}
          <div class="card">
            <div class="row">
              <div>
                <h3>Hotkeys</h3>
                <p class="hint">Control the music from any app, even in a game.</p>
              </div>
              <label class="switch">
                <input type="checkbox" bind:checked={s.hotkeys} onchange={commit} />
                <span aria-hidden="true"></span>
                <span class="sr">Hotkeys</span>
              </label>
            </div>
          </div>
          <div class="card keys" class:muted={!s.hotkeys}>
            <ul>
              {#each hotkeys as k}
                <li>
                  <span class="what">{k.label}</span>
                  <span class="combo">
                    {#each k.keys.split("+") as part, i}{#if i}<span class="plus">+</span>{/if}<kbd>{part}</kbd>{/each}
                  </span>
                  {#if s.hotkeys && k.note}<span class="taken">{k.note}</span>{/if}
                </li>
              {/each}
            </ul>
            <p class="hint small">Sleep timer: 15 → 30 → 45 → 60 min → after this song → off. Presets are saved in the scene menu.</p>
          </div>
        {:else if section === "feedback"}
          <div class="card">
            <h3>Give us your feedback</h3>
            <p class="hint">Something broken, an idea, or just what you think. Tell us here; no account needed.</p>
          </div>
          <FeedbackForm />
          <div class="card">
            <h3>Other ways</h3>
            <div class="report">
              <button class="open" onclick={() => invoke("open_support", { to: "issues" })}>Report on GitHub</button>
              <button class="open" onclick={() => invoke("open_support", { to: "email" })}>Email us</button>
              <button class="open" onclick={() => invoke("open_support", { to: "rate" })}>Rate Glowtune in the Microsoft Store</button>
            </div>
          </div>
        {:else if section === "about"}
          <div class="card about">
            <p class="big">Glowtune</p>
            <p class="hint">Version {version || "…"}</p>
            <p class="hint">Follows whatever you play, lights your screen in its colors, and shows the lyrics. Lyrics from LRCLIB; artwork from the iTunes catalog.</p>
          </div>
          {@const u = updates.view}
          {#if u?.channel === "store"}
            <div class="card">
              <h3>Updates</h3>
              <p class="hint">This copy comes from the Microsoft Store, which keeps it up to date.</p>
            </div>
          {:else if u?.channel === "manual"}
            <div class="card">
              <div class="row">
                <div>
                  <h3>Updates</h3>
                  <p class="hint">This copy wasn't installed with the Glowtune installer, so it can't update itself. New versions are on GitHub.</p>
                </div>
                <button class="open" onclick={() => invoke("open_support", { to: "releases" })}>Releases</button>
              </div>
            </div>
          {:else if u}
            <div class="card updates" class:ready={u.status === "available" || u.status === "downloading" || u.status === "installing"}>
              <div class="row">
                <div>
                  {#if u.status === "available" || u.status === "downloading" || u.status === "installing"}
                    <h3>Glowtune {u.version} is ready</h3>
                    <p class="hint">{[`You have ${u.current}`, releaseDay(u.date) && `released ${releaseDay(u.date)}`].filter(Boolean).join(" · ")}</p>
                  {:else if u.status === "checking"}
                    <h3>Checking for updates…</h3>
                  {:else if u.status === "latest"}
                    <h3>You're up to date</h3>
                    <p class="hint">Last checked {ago(u.checkedAt)}.</p>
                  {:else if u.status === "error"}
                    <h3>Updates</h3>
                    <p class="hint warn">{u.error}</p>
                  {:else}
                    <h3>Updates</h3>
                    <p class="hint">New versions install with one click, and your settings and sign-ins stay.</p>
                  {/if}
                </div>
                {#if u.status === "available" || (u.status === "error" && u.version)}
                  <button class="open primary" onclick={() => updates.install()}>
                    {u.status === "error" ? "Try again" : "Update now"}
                  </button>
                {:else if u.status !== "downloading" && u.status !== "installing"}
                  <button class="open" disabled={u.status === "checking"} onclick={() => updates.check()}>Check for updates</button>
                {/if}
              </div>
              {#if u.status === "downloading" || u.status === "installing"}
                <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round((u.progress ?? 0) * 100)}>
                  <span style:width="{u.status === 'installing' ? 100 : Math.round((u.progress ?? 0) * 100)}%"></span>
                </div>
                <p class="hint small">
                  {u.status === "installing" ? "Restarting Glowtune…" : `Downloading… ${Math.round((u.progress ?? 0) * 100)}%`}
                </p>
              {/if}
              {#if u.version && u.status !== "latest"}
                <div class="whatsnew">
                  <ReleaseNotes notes={u.notes} />
                </div>
                {#if u.status === "available"}
                  <button class="link" onclick={() => updates.skip(u.version!)}>Skip this version</button>
                {/if}
              {/if}
              {#if s}
                <div class="row">
                  <div>
                    <h3 class="sub">Check automatically</h3>
                    <p class="hint">When Glowtune starts and every 6 hours. It only asks GitHub for the newest version number.</p>
                  </div>
                  <label class="switch">
                    <input type="checkbox" bind:checked={s.updateCheck} onchange={commit} />
                    <span aria-hidden="true"></span>
                    <span class="sr">Check for updates automatically</span>
                  </label>
                </div>
              {/if}
            </div>
          {/if}
          <div class="card early">
            <h3>Early build</h3>
            <p class="hint">Glowtune is new and still has bugs. Some features may glitch, and Apple Music and SoundCloud are barely tested.</p>
          </div>
          <div class="card">
            <h3>Found a bug?</h3>
            <p class="hint">Tell us what happened and what you were playing, right from here.</p>
            <div class="report">
              <button class="open primary" onclick={() => (section = "feedback")}>Send feedback</button>
              <button class="open" onclick={() => invoke("open_support", { to: "issues" })}>Report on GitHub</button>
            </div>
          </div>
        {/if}
        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </div>
    {/key}
  </section>
</main>

<style>
  main {
    /* Settings keeps its own fonts: a theme's fonts are for the player and lyrics. */
    --font-ui: var(--font-app);
    font-family: var(--font-app);
    height: 100vh;
    display: grid;
    grid-template-columns: 210px minmax(0, 1fr);
    background:
      radial-gradient(120% 80% at 100% 0%, color-mix(in oklab, var(--c1) 16%, transparent), transparent 60%),
      var(--tint);
    border-radius: 0; /* the window has its own corners */
    overflow: hidden;
  }

  /* ── Sidebar ─────────────────────────────────────── */
  .side {
    padding: 18px 12px 14px;
    background: rgba(0, 0, 0, 0.22);
    border-right: 1px solid var(--hairline);
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .brand {
    margin: 0 8px;
    font-size: 12px;
    font-weight: 600;
    color: var(--ink-soft);
  }
  h1 {
    margin: 2px 8px 14px;
    font-family: var(--font-display);
    font-size: 24px;
    font-weight: 720;
    font-stretch: 90%;
    letter-spacing: -0.01em;
  }
  .find {
    display: flex;
    align-items: center;
    gap: 7px;
    height: 34px;
    padding: 0 10px;
    margin-bottom: 12px;
    border-radius: calc(var(--radius) * 0.7);
    background: rgba(255, 255, 255, 0.06);
    border: 1px solid var(--hairline);
  }
  .find:focus-within {
    border-color: color-mix(in oklab, var(--c1) 60%, transparent);
  }
  .find svg,
  .side li svg {
    width: 16px;
    height: 16px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
    opacity: 0.75;
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font: inherit;
    font-size: 13px;
    color: inherit;
  }
  .find input::placeholder {
    color: var(--ink-faint);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 2px;
  }
  .side li button {
    width: 100%;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 34px;
    padding: 0 10px;
    border: 0;
    border-radius: calc(var(--radius) * 0.65);
    background: transparent;
    font-size: 13.5px;
    color: var(--ink-soft);
    text-align: left;
    cursor: pointer;
    transition: background 140ms ease, color 140ms ease;
  }
  .side li button:hover {
    background: rgba(255, 255, 255, 0.06);
    color: var(--ink);
  }
  .side li button.on {
    background: color-mix(in oklab, var(--c1) 22%, rgba(255, 255, 255, 0.06));
    color: var(--ink);
  }
  /* Search results: the setting, with its section under it. */
  .results li button {
    height: auto;
    padding: 7px 10px;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    text-align: left;
  }
  .results li button.first {
    background: rgba(255, 255, 255, 0.05);
  }
  .results .what {
    color: var(--ink);
    font-size: 13px;
    line-height: 17px;
  }
  .results .where {
    color: var(--ink-faint);
    font-size: 11.5px;
    line-height: 15px;
  }
  /* The setting a search result jumped to. */
  :global(.flash) {
    border-radius: calc(var(--radius) * 0.6);
    animation: flash 1.8s ease-out;
  }
  @keyframes flash {
    0%,
    30% {
      box-shadow: 0 0 0 2px color-mix(in oklab, var(--c1) 80%, #fff);
      background-color: color-mix(in oklab, var(--c1) 16%, transparent);
    }
    100% {
      box-shadow: 0 0 0 2px transparent;
      background-color: transparent;
    }
  }
  @media (prefers-reduced-motion: reduce) {
    :global(.flash) {
      animation-duration: 0.01s;
      outline: 2px solid color-mix(in oklab, var(--c1) 80%, #fff);
    }
  }
  .rule {
    height: 1px;
    margin: 8px 8px;
    background: var(--hairline);
  }
  .none {
    padding: 8px 10px;
    font-size: 13px;
    color: var(--ink-faint);
  }

  /* ── Content ─────────────────────────────────────── */
  .content {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  header {
    display: flex;
    align-items: center;
    height: 62px;
    padding: 6px 16px 0 28px;
    flex: none;
  }
  h2 {
    margin: 0;
    font-family: var(--font-display);
    font-size: 21px;
    font-weight: 700;
    font-stretch: 92%;
  }
  .close {
    margin-left: auto;
    width: 30px;
    height: 30px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(255, 255, 255, 0.06);
    color: var(--ink-soft);
    cursor: pointer;
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.12);
    color: var(--ink);
  }
  .close svg {
    width: 14px;
    height: 14px;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    fill: none;
  }
  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    /* 28px sides: the theme editor's sticky footer bleeds to these edges. */
    padding: 4px 28px 24px;
    display: grid;
    align-content: start;
    gap: 12px;
  }
  .card {
    margin: 0;
    padding: 16px 18px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: rgba(255, 255, 255, 0.04);
    display: grid;
    gap: 10px;
    min-width: 0;
  }
  fieldset.card {
    transition: opacity 200ms ease;
  }
  fieldset:disabled {
    opacity: 0.4;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 24px;
  }
  h3 {
    margin: 0;
    font-size: 14.5px;
    font-weight: 620;
  }
  h3.sub {
    margin-top: 6px;
  }
  .hint {
    margin: 3px 0 0;
    font-size: 13px;
    line-height: 18px;
    color: var(--ink-soft);
  }
  .hint.small {
    font-size: 12px;
    margin: 0;
  }
  .hint.warn {
    color: #ffcf9a;
  }
  /* Why the floating lyrics are hidden right now. */
  .hint .why {
    color: #ffcf9a;
  }
  .listen-hero {
    gap: 6px;
  }
  .spent {
    margin: 0;
    font-family: var(--font-display);
    font-size: 46px;
    font-weight: 800;
    letter-spacing: -0.02em;
    line-height: 1.05;
    background: linear-gradient(100deg, #fff 30%, color-mix(in oklab, var(--c1) 70%, #fff));
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }
  .bars {
    display: flex;
    align-items: flex-end;
    gap: 3px;
    height: 84px;
    margin-top: 12px;
  }
  .bar {
    flex: 1;
    min-width: 2px;
    border-radius: 3px 3px 1px 1px;
    background: color-mix(in oklab, var(--c1) 45%, rgba(255, 255, 255, 0.12));
  }
  .bar.now {
    background: linear-gradient(180deg, var(--c1), var(--c2));
  }
  .bar-ends {
    display: flex;
    justify-content: space-between;
    font-size: 11.5px;
    color: var(--ink-faint);
  }
  .tops {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: 12px;
  }
  .toplist {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 6px;
  }
  .toplist li {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }
  .toplist .rank {
    width: 16px;
    flex: none;
    font-size: 12px;
    font-weight: 700;
    color: var(--ink-faint);
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .toplist li:first-child .rank {
    color: var(--ink);
  }
  .toplist .cover {
    display: grid;
    place-items: center;
    width: 36px;
    height: 36px;
    flex: none;
    border-radius: 7px;
    overflow: hidden;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    font-weight: 750;
  }
  .toplist .cover.round {
    border-radius: 50%;
  }
  .toplist .cover img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .toplist .who {
    flex: 1;
    min-width: 0;
    display: grid;
  }
  .toplist .t,
  .toplist .a {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .toplist .t {
    font-size: 13.5px;
    font-weight: 620;
  }
  .toplist .a {
    font-size: 12px;
    color: var(--ink-soft);
  }
  .toplist .n {
    font-size: 12px;
    color: var(--ink-soft);
    font-variant-numeric: tabular-nums;
  }
  .clear-btns {
    display: flex;
    gap: 8px;
    flex: none;
  }
  .report {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
  }
  .updates.ready {
    border-color: color-mix(in oklab, var(--c1) 45%, transparent);
    background:
      radial-gradient(120% 90% at 0% 0%, color-mix(in oklab, var(--c1) 14%, transparent), transparent 70%),
      rgba(255, 255, 255, 0.04);
  }
  .updates .progress {
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.08);
  }
  .updates .progress span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    transition: width 240ms ease;
  }
  .whatsnew {
    max-height: 220px;
    overflow-y: auto;
    padding: 10px 12px;
    border-radius: calc(var(--radius) * 0.7);
    background: rgba(0, 0, 0, 0.16);
  }
  .open.primary {
    border-color: transparent;
    background: linear-gradient(135deg, var(--c1), color-mix(in oklab, var(--c2) 80%, var(--c1)));
    color: #fff;
    font-weight: 620;
  }
  .open:disabled {
    opacity: 0.5;
    cursor: default;
  }
  .link {
    justify-self: start;
    padding: 0;
    border: 0;
    background: none;
    font-size: 12.5px;
    color: var(--ink-faint);
    cursor: pointer;
  }
  .link:hover {
    color: var(--ink-soft);
    text-decoration: underline;
  }
  .early {
    border-color: color-mix(in oklab, #ffb13d 40%, transparent);
    background: color-mix(in oklab, #ffb13d 8%, rgba(255, 255, 255, 0.03));
  }
  .open.danger {
    border-color: rgba(255, 120, 120, 0.4);
    color: #ffb4b4;
  }
  .keys {
    transition: opacity 200ms ease;
  }
  .keys.muted {
    opacity: 0.45;
  }
  .keys ul {
    list-style: none;
    margin: 0 0 10px;
    padding: 0;
    display: grid;
  }
  .keys li {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 0;
    border-bottom: 1px solid var(--hairline);
    font-size: 13.5px;
  }
  .keys li:last-child {
    border-bottom: 0;
  }
  .keys .what {
    flex: 1;
  }
  .keys .combo {
    display: flex;
    align-items: center;
    gap: 3px;
  }
  .keys .plus {
    font-size: 11px;
    color: var(--ink-faint);
  }
  .keys kbd {
    min-width: 14px;
    padding: 2px 7px;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    border-bottom-width: 2px;
    background: rgba(255, 255, 255, 0.05);
    font: inherit;
    font-size: 12px;
    font-weight: 650;
    text-align: center;
  }
  .keys .taken {
    font-size: 11.5px;
    color: #ffcf9a;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13.5px;
  }
  .check input {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--c1);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chips button {
    border: 1px solid var(--hairline);
    background: transparent;
    border-radius: 999px;
    padding: 6px 13px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .chips button.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    border-color: transparent;
  }
  .chips button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .lyric-look .row {
    padding-top: 10px;
    border-top: 1px solid var(--hairline);
  }
  .lyric-look .row h3.sub {
    margin: 0;
  }
  .size {
    display: flex;
    align-items: center;
    gap: 10px;
  }
  .size input {
    width: 170px;
  }
  .size output {
    min-width: 40px;
    font-size: 12.5px;
    font-variant-numeric: tabular-nums;
    color: var(--ink-soft);
  }
  .chip-btn {
    border: 1px solid var(--hairline);
    background: transparent;
    border-radius: 999px;
    padding: 6px 13px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .chip-btn.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    border-color: transparent;
  }
  .pick {
    height: 32px;
    min-width: 180px;
    padding: 0 10px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.6);
    background: rgba(0, 0, 0, 0.25);
    color: var(--ink);
    font: inherit;
    font-size: 13px;
  }
  .services li {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    padding: 9px 0;
    border-top: 1px solid var(--hairline);
  }
  .services li:first-child {
    border-top: 0;
  }
  .name {
    margin: 0;
    font-size: 14px;
    font-weight: 600;
  }
  .open {
    flex: none;
    height: 30px;
    padding: 0 14px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.65);
    background: rgba(255, 255, 255, 0.06);
    font-size: 13px;
    cursor: pointer;
  }
  .open:hover {
    background: color-mix(in oklab, var(--c1) 25%, rgba(255, 255, 255, 0.06));
  }
  .about .big {
    margin: 0;
    font-family: var(--font-display);
    font-size: 30px;
    font-weight: 760;
    font-stretch: 88%;
    background: linear-gradient(90deg, #fff, color-mix(in oklab, var(--c1) 60%, #fff));
    -webkit-background-clip: text;
    background-clip: text;
    color: transparent;
  }

  /* Switch */
  .switch {
    position: relative;
    flex: none;
    width: 42px;
    height: 24px;
    cursor: pointer;
  }
  .switch input {
    position: absolute;
    opacity: 0;
    inset: 0;
    margin: 0;
    cursor: pointer;
  }
  .switch span[aria-hidden] {
    position: absolute;
    inset: 0;
    border-radius: 12px;
    background: rgba(255, 255, 255, 0.16);
    transition: background 180ms ease;
  }
  .switch span[aria-hidden]::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 18px;
    height: 18px;
    border-radius: 50%;
    background: var(--ink);
    transition: transform 180ms ease;
  }
  .switch input:checked + span {
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .switch input:checked + span::after {
    transform: translateX(18px);
  }
  .switch input:focus-visible + span {
    outline: 2px solid var(--c1);
    outline-offset: 2px;
  }
  .sr {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
  }

  .nudge {
    display: flex;
    align-items: center;
    justify-content: space-between;
    font-size: 13.5px;
    color: var(--ink-soft);
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .stepper button {
    padding: 5px 12px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.6);
    background: rgba(255, 255, 255, 0.06);
    cursor: pointer;
    font-size: 13px;
  }
  .stepper button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.12);
  }
  .stepper button:disabled {
    cursor: default;
  }
  /* Lyrics timing card */
  .timing.muted .row,
  .timing.muted .sync-row {
    opacity: 0.55;
  }
  .offset {
    display: flex;
    align-items: center;
    gap: 3px;
    height: 30px;
    padding: 0 8px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.6);
    background: rgba(0, 0, 0, 0.25);
    font-size: 13px;
    color: var(--ink-faint);
  }
  .offset:focus-within {
    border-color: color-mix(in oklab, var(--c1) 60%, transparent);
  }
  .offset.bad {
    border-color: rgba(255, 120, 120, 0.6);
  }
  .offset input {
    width: 52px;
    border: 0;
    outline: 0;
    background: transparent;
    color: var(--ink);
    font: inherit;
    font-size: 13px;
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .sync-row {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 12px;
  }
  .sync-row .hint {
    margin: 0;
  }
  output {
    min-width: 64px;
    text-align: center;
    color: var(--ink);
    font-variant-numeric: tabular-nums;
  }
  .error {
    margin: 0;
    font-size: 13px;
    color: #ffb4b4;
  }
</style>
