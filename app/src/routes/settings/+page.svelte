<script lang="ts">
  // Settings: a sidebar of sections, a search box to jump between them.
  // Every change applies live (the glow updates while you drag) and is saved
  // automatically.
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { getVersion } from "@tauri-apps/api/app";
  import { np } from "$lib/nowPlaying.svelte";
  import ThemeEditor from "$lib/ThemeEditor.svelte";
  import Store from "$lib/Store.svelte";

  type Settings = {
    glowEnabled: boolean;
    glowMonitors: string[];
    lyricsOnDesktop: boolean;
    cleanLyrics: boolean;
    playerOnTop: boolean;
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
    stageLyricsStyle: string;
  };
  type Monitor = { name: string; primary: boolean; width: number; height: number };
  type LyricsInfo = { status: string; offsetMs: number };
  type SectionId = "lyrics" | "scene" | "glow" | "services" | "themes" | "store" | "player" | "about";

  const SECTIONS: { id: SectionId; name: string; icon: string; words: string }[] = [
    { id: "lyrics", name: "Lyrics", icon: "M4 6h16M4 12h10M4 18h13", words: "lyrics desktop timing sync offset style karaoke flow" },
    { id: "scene", name: "Desktop scene", icon: "M3 5h18v12H3zM8 21h8M12 17v4", words: "desktop scene wallpaper clock full screen recently played background motion" },
    { id: "glow", name: "Edge glow", icon: "M4 4h16v16H4zM8 8h8v8H8z", words: "glow edge light screens monitors fullscreen games" },
    { id: "services", name: "Services", icon: "M5 12a7 7 0 0 1 14 0M8 12a4 4 0 0 1 8 0M12 12v8", words: "services spotify apple music youtube soundcloud login sign in ads block" },
    { id: "themes", name: "Themes", icon: "M12 3a9 9 0 1 0 0 18c1 0 1.5-.8 1.5-1.6 0-1.2-1-1.4-1-2.4 0-.8.7-1.5 1.5-1.5H16a5 5 0 0 0 5-5c0-4.1-4-7.5-9-7.5z", words: "themes colors appearance fonts glass corners import export" },
    { id: "store", name: "Store", icon: "M3 8h18v13H3zM3 12h18M7 8V3h10v5M8 15v3M16 15v3", words: "store crate themes scene looks presets extensions install free" },
    { id: "player", name: "Player", icon: "M4 15h16v5H4zM8 4v8M16 4v8", words: "player mini on top" },
    { id: "about", name: "About", icon: "M12 21a9 9 0 1 0 0-18 9 9 0 0 0 0 18zM12 11v5M12 8h.01", words: "about version" },
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
  ];

  let s = $state<Settings | null>(null);
  let monitors = $state<Monitor[]>([]);
  let lyrics = $state<LyricsInfo>({ status: "none", offsetMs: 0 });
  let error = $state("");
  let blocked = $state(0);
  let version = $state("");
  let section = $state<SectionId>("lyrics");
  let query = $state("");
  let saveTimer: ReturnType<typeof setTimeout> | undefined;

  const palette = $derived(np.palette);
  const shown = $derived(
    query.trim()
      ? SECTIONS.filter((x) => (x.name + " " + x.words).toLowerCase().includes(query.trim().toLowerCase()))
      : SECTIONS,
  );
  const current = $derived(SECTIONS.find((x) => x.id === section)!);

  onMount(() => {
    const tab = new URLSearchParams(location.search).get("tab");
    if (tab === "theme") section = "themes";
    else if (tab && SECTIONS.some((x) => x.id === tab)) section = tab as SectionId;
    np.start();
    invoke<Settings>("get_settings").then((v) => (s = v));
    invoke<Monitor[]>("list_monitors").then((m) => (monitors = m));
    invoke<LyricsInfo>("get_lyrics").then((l) => (lyrics = l));
    invoke<number>("adblock_stats").then((n) => (blocked = n)).catch(() => {});
    getVersion().then((v) => (version = v)).catch(() => {});
    const un = listen<LyricsInfo>("lyrics", (e) => (lyrics = e.payload));
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

  function searchKeys(e: KeyboardEvent) {
    if (e.key === "Enter" && shown.length) {
      section = shown[0].id;
      query = "";
    }
  }
</script>

<main style:--c1={palette[0]} style:--c2={palette[1] ?? palette[0]}>
  <nav class="side" data-tauri-drag-region>
    <p class="brand" data-tauri-drag-region>Glowtune</p>
    <h1 data-tauri-drag-region>Settings</h1>
    <label class="find">
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5 20 20" /></svg>
      <input bind:value={query} onkeydown={searchKeys} placeholder="Search settings" aria-label="Search settings" spellcheck="false" />
    </label>
    <ul>
      {#each shown as x (x.id)}
        {#if x.id === "about"}<li class="rule" aria-hidden="true"></li>{/if}
        <li>
          <button class:on={section === x.id} aria-current={section === x.id ? "page" : undefined} onclick={() => (section = x.id)}>
            <svg viewBox="0 0 24 24" aria-hidden="true"><path d={x.icon} /></svg>
            {x.name}
          </button>
        </li>
      {/each}
      {#if shown.length === 0}<li class="none">Nothing matches.</li>{/if}
    </ul>
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
                  {#if lyrics.status === "synced"}Synced lyrics found for this song.
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
            <div class="nudge" class:muted={lyrics.status !== "synced"}>
              <span>Timing for this song</span>
              <div class="stepper">
                <button onclick={() => nudge(-500)} disabled={lyrics.status !== "synced"} aria-label="Show lyrics later">Later</button>
                <output>{lyrics.offsetMs === 0 ? "In sync" : `${lyrics.offsetMs > 0 ? "+" : "−"}${Math.abs(lyrics.offsetMs / 1000).toFixed(1)} s`}</output>
                <button onclick={() => nudge(500)} disabled={lyrics.status !== "synced"} aria-label="Show lyrics earlier">Earlier</button>
              </div>
            </div>
          </div>
          <div class="card">
            <h3>Style in the desktop scene</h3>
            <p class="hint">Flow lights each word as it's sung; the others show one big line at a time.</p>
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
            <p class="hint small">The floating desktop lyrics follow the theme (Themes → Lyrics).</p>
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
                  In the service windows. Spotify's own audio ads still play on a free account.
                  {#if blocked > 0}{blocked.toLocaleString()} blocked so far.{/if}
                </p>
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
        {:else if section === "about"}
          <div class="card about">
            <p class="big">Glowtune</p>
            <p class="hint">Version {version || "…"}</p>
            <p class="hint">Follows whatever you play, lights your screen in its colors, and shows the lyrics. Lyrics from LRCLIB; search artwork from the iTunes catalog.</p>
          </div>
        {/if}
        {#if error}<p class="error" role="alert">{error}</p>{/if}
      </div>
    {/key}
  </section>
</main>

<style>
  main {
    height: 100vh;
    display: grid;
    grid-template-columns: 210px minmax(0, 1fr);
    background:
      radial-gradient(120% 80% at 100% 0%, color-mix(in oklab, var(--c1) 16%, transparent), transparent 60%),
      var(--tint);
    border-radius: var(--radius);
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
    font-family: var(--font-lyrics);
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
    border-radius: 10px;
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
    border-radius: 9px;
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
    font-family: var(--font-lyrics);
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
    border-radius: 14px;
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
    border-radius: 9px;
    background: rgba(255, 255, 255, 0.06);
    font-size: 13px;
    cursor: pointer;
  }
  .open:hover {
    background: color-mix(in oklab, var(--c1) 25%, rgba(255, 255, 255, 0.06));
  }
  .about .big {
    margin: 0;
    font-family: var(--font-lyrics);
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
  .nudge.muted {
    opacity: 0.45;
  }
  .stepper {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .stepper button {
    padding: 5px 12px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
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
