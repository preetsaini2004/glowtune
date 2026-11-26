<script lang="ts">
  // The desktop scene's menu: one dark panel of rows. Each row has an icon, a
  // label, often a switch, and a chevron that opens its options in place.
  import { slide } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { cubicOut } from "svelte/easing";
  import ColorWheel from "./ColorWheel.svelte";
  import SoundControls, { type SoundSettings } from "./SoundControls.svelte";

  export type SceneSettings = Record<string, unknown> & {
    glowEnabled: boolean;
    glowHideFullscreen: boolean;
    stageClock: boolean;
    stageLyrics: boolean;
    stageRecent: boolean;
    stageUpcoming: boolean;
    stageMotion: boolean;
    stageEmoji: boolean;
    stageBeat?: boolean;
    stageDock: boolean;
    stageLyricsStyle: string;
    cleanLyrics: boolean;
    stageClockStyle: string;
    stageWallpaper: string;
    stageArtStyle: string;
    stageDockStyle: string;
    artworkColors: boolean;
    customColors: string[];
    scenePresets?: Preset[];
  } & SoundSettings;
  export type Preset = { name: string; look: Record<string, unknown> };

  let {
    settings,
    full,
    save,
    layoutNow,
    oncard,
    onfullscreen,
    onmore,
    onstore,
    onedit,
    onoff,
  }: {
    settings: SceneSettings;
    full: boolean;
    save: (patch: Partial<SceneSettings>) => void;
    layoutNow: () => unknown;
    oncard: () => void;
    onfullscreen: () => void;
    onmore: () => void;
    onstore: () => void;
    onedit: () => void;
    onoff: () => void;
  } = $props();

  let open = $state<string | null>("colors");
  const toggle = (id: string) => (open = open === id ? null : id);

  // ── Presets ── your saved looks: click one to bring it back.
  const presets = $derived(settings.scenePresets ?? []);
  let presetName = $state("");
  let presetError = $state("");
  // The scene settings a preset holds (the glow and layout are compared by eye).
  const LOOK_KEYS = [
    "stageClock", "stageLyrics", "stageRecent", "stageUpcoming", "stageMotion", "stageEmoji", "stageBeat", "stageDock",
    "stageLyricsStyle", "stageClockStyle", "stageWallpaper", "stageArtStyle", "stageDockStyle",
    "artworkColors", "glowEnabled", "desktopLyricsStyle", "activeTheme",
  ];
  function isCurrent(p: Preset): boolean {
    return (
      LOOK_KEYS.every((k) => p.look[k] === settings[k]) &&
      (settings.artworkColors || String(p.look.customColors) === String(settings.customColors))
    );
  }
  const existing = $derived(presets.find((p) => p.name.toLowerCase() === presetName.trim().toLowerCase()));
  async function savePreset() {
    const name = presetName.trim();
    if (!name) return;
    try {
      await invoke("save_scene_preset", { name, layout: layoutNow() });
      presetName = "";
      presetError = "";
    } catch (e) {
      presetError = String(e);
    }
  }
  async function applyPreset(name: string) {
    try {
      await invoke("apply_scene_preset", { name });
      presetError = "";
      readGlow(); // the preset may have changed the theme's glow
    } catch (e) {
      presetError = String(e);
    }
  }
  // ── Sleep timer ── pauses the music after a while or after this song.
  type Sleep = { endsAtMs: number | null; endOfSong: boolean };
  // Lyrics timing: the same as Ctrl+Alt+, . and T (sync = back to 0 s).
  let timingNote = $state("");
  async function timing(action: "later" | "earlier" | "sync") {
    try {
      timingNote = await invoke<string>("lyrics_timing", { action });
    } catch (e) {
      timingNote = String(e);
    }
  }

  let sleep = $state<Sleep>({ endsAtMs: null, endOfSong: false });
  let sleepNow = $state(Date.now());
  const SLEEPS = [
    { id: null, name: "Off" },
    { id: 15, name: "15 min" },
    { id: 30, name: "30 min" },
    { id: 45, name: "45 min" },
    { id: 60, name: "1 hour" },
    { id: 0, name: "After this song" },
  ];
  const sleepLeft = $derived(sleep.endsAtMs ? Math.max(0, Math.ceil((sleep.endsAtMs - sleepNow) / 60_000)) : null);
  $effect(() => {
    invoke<Sleep>("get_sleep_timer").then((v) => (sleep = v)).catch(() => {});
    const off = listen<Sleep>("sleep-timer", (e) => (sleep = e.payload));
    return () => void off.then((f) => f());
  });
  $effect(() => {
    if (!sleep.endsAtMs) return;
    const id = setInterval(() => (sleepNow = Date.now()), 15_000);
    return () => clearInterval(id);
  });
  function setSleep(minutes: number | null) {
    sleepNow = Date.now();
    invoke<Sleep>("set_sleep_timer", { minutes }).then((v) => (sleep = v)).catch(() => {});
  }
  function sleepOn(id: number | null): boolean {
    if (id === null) return !sleep.endsAtMs && !sleep.endOfSong;
    if (id === 0) return sleep.endOfSong;
    // The choice the time left falls under (30 min shows "30 min" until 15 are left).
    if (sleepLeft === null) return false;
    const step = [15, 30, 45, 60].find((m) => m >= sleepLeft) ?? 60;
    return step === id;
  }

  async function deletePreset(name: string) {
    try {
      await invoke("delete_scene_preset", { name });
    } catch (e) {
      presetError = String(e);
    }
  }

  const LYRICS = [
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
  // The glow's shape and particles live in the active theme: the menu reads
  // it, changes one field and saves it (finer controls stay in Settings).
  const GLOWS = [
    { id: "ambient", name: "Ambient" },
    { id: "still", name: "Still" },
    { id: "pulse", name: "Pulse" },
    { id: "orbit", name: "Orbit" },
    { id: "distortion", name: "Ribbons" },
    { id: "lava-lamp", name: "Lava" },
    { id: "comet", name: "Comets" },
    { id: "aurora", name: "Aurora" },
    { id: "bars", name: "Equalizer" },
    { id: "neon", name: "Neon" },
    { id: "tide", name: "Tide" },
  ];
  const PARTICLES = [
    { id: "off", name: "Off" },
    { id: "dots", name: "Dots" },
    { id: "stars", name: "Stars" },
    { id: "dust", name: "Dust" },
    { id: "embers", name: "Embers" },
    { id: "snow", name: "Snow" },
    { id: "fireflies", name: "Fireflies" },
    { id: "bubbles", name: "Bubbles" },
  ];
  type Glow = { effect: string; particles: string };
  let glow = $state<Glow | null>(null);
  let glowError = $state("");
  let particlesOpen = $state(false);
  function readGlow() {
    invoke<{ glow: Glow }>("get_theme")
      .then((t) => (glow = { effect: t.glow.effect, particles: t.glow.particles }))
      .catch(() => (glow = null));
  }
  $effect(() => {
    if (open === "light") readGlow();
  });
  async function setGlow(key: keyof Glow, value: string) {
    if (glow) glow = { ...glow, [key]: value };
    try {
      const t = await invoke<{ glow: Record<string, unknown> }>("get_theme");
      t.glow[key] = value;
      await invoke("save_theme", { theme: t });
      glowError = "";
    } catch (e) {
      glowError = String(e);
    }
  }
  const ARTS = [
    { id: "tilt", name: "Tilt" },
    { id: "print", name: "Print" },
    { id: "vinyl", name: "Vinyl" },
    { id: "glow", name: "Glow" },
  ];
  const DOCKS = [
    { id: "bar", name: "Bar" },
    { id: "pill", name: "Pill" },
    { id: "islands", name: "Islands" },
    { id: "line", name: "Line" },
  ];
  const CLOCKS = [
    { id: "outline", name: "Outline" },
    { id: "solid", name: "Solid" },
    { id: "minimal", name: "Minimal" },
  ];
  const WALLS = [
    { id: "art", name: "Album art" },
    { id: "video", name: "Music video" },
    { id: "cover", name: "Cover" },
    { id: "water", name: "Water" },
    { id: "frost", name: "Frost" },
    { id: "halftone", name: "Halftone" },
    { id: "mosaic", name: "Mosaic" },
    { id: "silk", name: "Silk" },
    { id: "dots", name: "Dots" },
  ];
  const EXTRA = ["#ffb13d", "#3dffa8", "#a63dff"];

  function setCount(n: number) {
    const c = [...settings.customColors];
    while (c.length < n) c.push(EXTRA[c.length % EXTRA.length]);
    save({ customColors: c.slice(0, n) });
  }
  function setColor(i: number, hex: string) {
    const c = [...settings.customColors];
    c[i] = hex;
    save({ customColors: c });
  }
  const NAMES = ["Primary", "Secondary", "Third"];

  const ICON = {
    colors: "M12 3a9 9 0 1 0 0 18c1 0 1.5-.8 1.5-1.6 0-1.2-1-1.4-1-2.4 0-.8.7-1.5 1.5-1.5H16a5 5 0 0 0 5-5c0-4.1-4-7.5-9-7.5z",
    light: "M12 2v3M12 19v3M2 12h3M19 12h3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M4.9 19.1 7 17M17 7l2.1-2.1M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8z",
    wall: "M3 5h18v14H3zM3 15l5-5 4 4 3-3 6 6",
    lyrics: "M4 6h16M4 12h10M4 18h13",
    sound: "M4 9v6M8 5v14M12 8v8M16 4v16M20 10v4",
    blend: "M3 7c6 0 6 10 12 10h6M3 17c6 0 6-10 12-10h6M18 4l3 3-3 3M18 14l3 3-3 3",
    clock: "M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z",
    playlist: "M4 6h11M4 12h11M4 18h7M18 15v6M15 18h6",
    dock: "M3 17h18v4H3zM7 13h10",
    album: "M4 4h16v16H4zM12 9a3 3 0 1 0 0 6 3 3 0 0 0 0-6z",
    upnext: "M4 6h10M4 12h10M4 18h6M17 10l4 3-4 3z",
    full: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
    more: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1",
    off: "M12 3v8M6.3 6.3a8 8 0 1 0 11.4 0",
    preset: "M6 3h12v18l-6-4-6 4z",
    sleep: "M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z",
  };
</script>

{#snippet sw(key: keyof SceneSettings, label: string)}
  <input
    type="checkbox"
    role="switch"
    aria-label={label}
    checked={Boolean(settings[key])}
    onchange={(e) => save({ [key]: (e.currentTarget as HTMLInputElement).checked })}
  />
{/snippet}

{#snippet head(id: string, icon: string, label: string, swKey: keyof SceneSettings | null)}
  <div class="head">
    <button class="open" aria-expanded={open === id} onclick={() => toggle(id)}>
      <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={icon} /></svg></span>
      <span class="name">{label}</span>
      <svg class="chev" class:down={open === id} viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
    </button>
    {#if swKey}{@render sw(swKey, label)}{/if}
  </div>
{/snippet}

{#snippet chips(list: { id: string; name: string }[], key: keyof SceneSettings)}
  <div class="chips" role="radiogroup">
    {#each list as c}
      <button role="radio" aria-checked={settings[key] === c.id} class:on={settings[key] === c.id} onclick={() => save({ [key]: c.id })}>
        {c.name}
      </button>
    {/each}
  </div>
{/snippet}

<div class="menu">
  <p class="title">Glowtune</p>

  <section>
    {@render head("presets", ICON.preset, "Presets", null)}
    {#if open === "presets"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        {#if presets.length}
          <div class="presets">
            {#each presets as p, i (p.name)}
              <div class="preset" class:on={isCurrent(p)}>
                <button class="use" onclick={() => applyPreset(p.name)} title="Switch to {p.name} (Ctrl+Alt+{i + 1})">
                  <span class="key">{i + 1}</span>
                  <span class="pname">{p.name}</span>
                </button>
                <button class="del" aria-label="Delete {p.name}" title="Delete" onclick={() => deletePreset(p.name)}>
                  <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6 6 18" /></svg>
                </button>
              </div>
            {/each}
          </div>
        {:else}
          <p class="hint">Set up the scene how you like it, layout included, then save it here.</p>
        {/if}
        <form class="save-preset" onsubmit={(e) => { e.preventDefault(); savePreset(); }}>
          <input bind:value={presetName} maxlength="40" placeholder="Name this look" aria-label="Preset name" />
          <button type="submit" disabled={!presetName.trim()}>{existing ? "Update" : "Save"}</button>
        </form>
        {#if presetError}<p class="hint">{presetError}</p>{/if}
        {#if presets.length}<p class="hint">Ctrl+Alt+1 to {presets.length} switches presets from anywhere.</p>{/if}
      </div>
    {/if}
  </section>

  <section>
    {@render head("colors", ICON.colors, "Colors", null)}
    {#if open === "colors"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        <label class="line">
          <span>Use artwork colors</span>
          {@render sw("artworkColors", "Use artwork colors")}
        </label>
        <div class="line" class:muted={settings.artworkColors}>
          <span>Colors</span>
          <div class="seg" role="radiogroup" aria-label="Number of colors">
            {#each [1, 2, 3] as n}
              <button role="radio" aria-checked={settings.customColors.length === n} class:on={settings.customColors.length === n} disabled={settings.artworkColors} onclick={() => setCount(n)}>{n}</button>
            {/each}
          </div>
        </div>
        {#if !settings.artworkColors}
          <div class="wheels" style:--n={settings.customColors.length}>
            {#each settings.customColors as c, i (i)}
              <ColorWheel value={c} label={NAMES[i]} onchange={(hex) => setColor(i, hex)} />
            {/each}
          </div>
        {/if}
      </div>
    {/if}
  </section>

  <section>
    {@render head("light", ICON.light, "Light", "glowEnabled")}
    {#if open === "light"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        <label class="line">
          <span>Turn off during full-screen games and videos</span>
          {@render sw("glowHideFullscreen", "Turn off during full-screen games and videos")}
        </label>
        {#if glow}
          <span class="label">Shape</span>
          <div class="chips" role="radiogroup" aria-label="Glow shape">
            {#each GLOWS as g}
              <button role="radio" aria-checked={glow.effect === g.id} class:on={glow.effect === g.id} onclick={() => setGlow("effect", g.id)}>{g.name}</button>
            {/each}
          </div>
          <button class="subhead" aria-expanded={particlesOpen} onclick={() => (particlesOpen = !particlesOpen)}>
            <span>Particles</span>
            <span class="now">{PARTICLES.find((p) => p.id === glow?.particles)?.name ?? ""}</span>
            <svg class="chev" class:down={particlesOpen} viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
          </button>
          {#if particlesOpen}
            <div class="chips" role="radiogroup" aria-label="Particles" transition:slide={{ duration: 180, easing: cubicOut }}>
              {#each PARTICLES as p}
                <button role="radio" aria-checked={glow.particles === p.id} class:on={glow.particles === p.id} onclick={() => setGlow("particles", p.id)}>{p.name}</button>
              {/each}
            </div>
          {/if}
          {#if glowError}<p class="hint">{glowError}</p>{/if}
        {/if}
        <p class="hint">Saved in your theme. More options in All settings → Themes.</p>
      </div>
    {/if}
  </section>

  <section>
    {@render head("wall", ICON.wall, "Wallpaper", null)}
    {#if open === "wall"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        {@render chips(WALLS, "stageWallpaper")}
        <label class="line">
          <span>Moving background</span>
          {@render sw("stageMotion", "Moving background")}
        </label>
        {#if ["water", "frost", "halftone", "mosaic", "dots"].includes(settings.stageWallpaper)}
          <label class="line">
            <span>Pulse to the beat</span>
            {@render sw("stageBeat", "Pulse to the beat")}
          </label>
        {/if}
      </div>
    {/if}
  </section>

  <section>
    {@render head("lyrics", ICON.lyrics, "Lyrics", "stageLyrics")}
    {#if open === "lyrics"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        {@render chips(LYRICS, "stageLyricsStyle")}
        <label class="line">
          <span>Hide explicit words</span>
          {@render sw("cleanLyrics", "Hide explicit words")}
        </label>
        <label class="line">
          <span>Emoji</span>
          {@render sw("stageEmoji", "Emoji")}
        </label>
        <p class="hint">Click any line to jump there. Scroll to look ahead.</p>
        <div class="timing" role="group" aria-label="Lyrics timing for this song">
          <span>Timing</span>
          <div class="chips">
            <button onclick={() => timing("later")} title="Ctrl+Alt+,">Later</button>
            <button onclick={() => timing("sync")} title="Ctrl+Alt+T">Sync</button>
            <button onclick={() => timing("earlier")} title="Ctrl+Alt+.">Earlier</button>
          </div>
        </div>
        <p class="hint" aria-live="polite">
          {timingNote || "Sync puts the timing back to 0 s. Ctrl+Alt+, and . move it from anywhere."}
        </p>
      </div>
    {/if}
  </section>

  <section>
    {@render head("sound", ICON.sound, "Equalizer", "eqEnabled")}
    {#if open === "sound"}
      <div class="body wide" transition:slide={{ duration: 220, easing: cubicOut }}>
        <SoundControls part="eq" {settings} {save} compact />
        <p class="hint">For music played in Glowtune's service windows.</p>
      </div>
    {/if}
  </section>

  <section>
    {@render head("blend", ICON.blend, "Between songs", null)}
    {#if open === "blend"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        <SoundControls part="blend" {settings} {save} compact />
      </div>
    {/if}
  </section>

  <section>
    <div class="head">
      <button class="open" aria-expanded={open === "sleep"} onclick={() => toggle("sleep")}>
        <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.sleep} /></svg></span>
        <span class="name">Sleep timer</span>
        {#if sleepLeft !== null}<span class="badge">{sleepLeft} min</span>{:else if sleep.endOfSong}<span class="badge">After this song</span>{/if}
        <svg class="chev" class:down={open === "sleep"} viewBox="0 0 24 24" aria-hidden="true"><path d="M9 6l6 6-6 6" /></svg>
      </button>
    </div>
    {#if open === "sleep"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        <div class="chips" role="radiogroup" aria-label="Sleep timer">
          {#each SLEEPS as c}
            <button role="radio" aria-checked={sleepOn(c.id)} class:on={sleepOn(c.id)} onclick={() => setSleep(c.id)}>{c.name}</button>
          {/each}
        </div>
        <p class="hint">
          {#if sleepLeft !== null}The music pauses in {sleepLeft} min.{:else if sleep.endOfSong}The music pauses when this song ends.{:else}Pauses the music for you. Ctrl+Alt+S steps through these from anywhere.{/if}
        </p>
      </div>
    {/if}
  </section>

  <section>
    {@render head("album", ICON.album, "Album art", null)}
    {#if open === "album"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        {@render chips(ARTS, "stageArtStyle")}
      </div>
    {/if}
  </section>

  <section>
    {@render head("clock", ICON.clock, "Clock", "stageClock")}
    {#if open === "clock"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        {@render chips(CLOCKS, "stageClockStyle")}
      </div>
    {/if}
  </section>

  <section>
    <div class="head">
      <span class="open static">
        <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.playlist} /></svg></span>
        <span class="name">Recently played</span>
      </span>
      {@render sw("stageRecent", "Recently played")}
    </div>
  </section>

  <section>
    <div class="head">
      <span class="open static">
        <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.upnext} /></svg></span>
        <span class="name">Up next</span>
      </span>
      {@render sw("stageUpcoming", "Up next")}
    </div>
  </section>

  <section>
    {@render head("dock", ICON.dock, "Dock", "stageDock")}
    {#if open === "dock"}
      <div class="body" transition:slide={{ duration: 220, easing: cubicOut }}>
        {@render chips(DOCKS, "stageDockStyle")}
      </div>
    {/if}
  </section>

  <div class="sep"></div>
  <button class="act" onclick={onfullscreen}>
    <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.full} /></svg></span>
    <span class="name">{full ? "Back to wallpaper" : "Full screen"}</span>
    <kbd>{full ? "Esc" : "F11"}</kbd>
  </button>
  <button class="act" onclick={onedit}>
    <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M4 4h7v7H4zM13 13h7v7h-7zM16 4l4 4-4 4M8 20l-4-4 4-4" /></svg></span>
    <span class="name">Edit layout</span>
    <kbd>E</kbd>
  </button>
  <button class="act" onclick={oncard}>
    <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M5 3h14v18H5zM8 14h8M8 17h5M9 6h6v5H9z" /></svg></span>
    <span class="name">Lyric card</span>
    <kbd>Ctrl+Alt+C</kbd>
  </button>
  <button class="act" onclick={onstore}>
    <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d="M3 8h18v13H3zM3 12h18M7 8V3h10v5M8 15v3M16 15v3" /></svg></span>
    <span class="name">Store</span>
  </button>
  <button class="act" onclick={onmore}>
    <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.more} /></svg></span>
    <span class="name">All settings</span>
  </button>
  <button class="act" onclick={onoff}>
    <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.off} /></svg></span>
    <span class="name">Turn off desktop scene</span>
  </button>
</div>

<style>
  .menu {
    display: grid;
    gap: 2px;
    max-height: min(78vh, 760px);
    overflow-y: auto;
    padding: 2px;
  }
  .title {
    margin: 2px 10px 8px;
    font-family: var(--font-display);
    font-size: 15px;
    font-weight: 760;
    letter-spacing: -0.01em;
  }
  section {
    border-radius: 14px;
  }
  section:has(.body) {
    background: rgba(255, 255, 255, 0.04);
  }
  .head {
    display: flex;
    align-items: center;
    gap: 8px;
    padding-right: 10px;
  }
  .open,
  .act {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 11px;
    padding: 8px 10px;
    border: 0;
    border-radius: 12px;
    background: transparent;
    font: inherit;
    font-size: 14px;
    text-align: left;
    cursor: pointer;
  }
  .open.static {
    cursor: default;
  }
  .open:not(.static):hover,
  .act:hover {
    background: rgba(255, 255, 255, 0.07);
  }
  .ic {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    flex: none;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.08);
  }
  .ic svg,
  .chev {
    width: 16px;
    height: 16px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .name {
    flex: 1;
    font-weight: 600;
  }
  .chev {
    opacity: 0.5;
    transition: transform 200ms ease;
  }
  .chev.down {
    transform: rotate(90deg);
  }
  .body {
    display: grid;
    gap: 12px;
    padding: 4px 12px 14px 49px;
  }
  .body.wide {
    padding-left: 12px;
  }
  .line {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    font-size: 13.5px;
    transition: opacity 200ms ease;
  }
  .line.muted {
    opacity: 0.4;
  }
  .seg {
    display: flex;
    padding: 2px;
    border-radius: 9px;
    background: rgba(0, 0, 0, 0.3);
  }
  .seg button {
    width: 30px;
    height: 24px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    font-size: 12.5px;
    font-weight: 700;
    cursor: pointer;
  }
  .seg button.on {
    background: rgba(255, 255, 255, 0.16);
  }
  .wheels {
    display: grid;
    grid-template-columns: repeat(var(--n), minmax(0, 1fr));
    gap: 12px;
    margin-left: -37px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chips button {
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: transparent;
    border-radius: 999px;
    padding: 5px 11px;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .chips button.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    border-color: transparent;
  }
  .timing {
    display: grid;
    gap: 6px;
    margin-top: 4px;
  }
  .timing > span {
    font-size: 12.5px;
    opacity: 0.75;
  }
  .badge {
    font-size: 11.5px;
    font-weight: 650;
    padding: 2px 8px;
    border-radius: 999px;
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .presets {
    display: grid;
    gap: 4px;
  }
  .preset {
    display: flex;
    align-items: center;
    border-radius: 10px;
    border: 1px solid rgba(255, 255, 255, 0.08);
    transition: background 160ms ease, border-color 160ms ease;
  }
  .preset:hover {
    background: rgba(255, 255, 255, 0.05);
  }
  .preset.on {
    border-color: transparent;
    background:
      linear-gradient(rgba(20, 20, 26, 0.72), rgba(20, 20, 26, 0.72)) padding-box,
      linear-gradient(135deg, var(--c1), var(--c2)) border-box;
    border: 1px solid transparent;
  }
  .preset .use {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 7px 8px;
    border: 0;
    background: transparent;
    font-size: 13px;
    font-weight: 650;
    text-align: left;
    cursor: pointer;
  }
  .preset .key {
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    flex: none;
    border-radius: 6px;
    border: 1px solid rgba(255, 255, 255, 0.18);
    font-size: 11px;
    font-weight: 700;
    color: rgba(255, 255, 255, 0.7);
  }
  .preset.on .key {
    border-color: transparent;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    color: #fff;
  }
  .preset .pname {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .preset .del {
    display: grid;
    place-items: center;
    width: 26px;
    height: 26px;
    margin-right: 4px;
    border: 0;
    border-radius: 7px;
    background: transparent;
    opacity: 0;
    cursor: pointer;
    transition: opacity 140ms ease;
  }
  .preset:hover .del,
  .preset .del:focus-visible {
    opacity: 0.7;
  }
  .preset .del:hover {
    opacity: 1;
    background: rgba(255, 255, 255, 0.1);
  }
  .preset .del svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
  }
  .save-preset {
    display: flex;
    gap: 6px;
  }
  .save-preset input {
    flex: 1;
    min-width: 0;
    padding: 6px 10px;
    border-radius: 9px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: rgba(0, 0, 0, 0.3);
    font: inherit;
    font-size: 13px;
    color: inherit;
  }
  .save-preset input:focus {
    outline: none;
    border-color: rgba(255, 255, 255, 0.35);
  }
  .save-preset button {
    padding: 6px 14px;
    border: 0;
    border-radius: 9px;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    font-size: 12.5px;
    font-weight: 700;
    cursor: pointer;
  }
  .save-preset button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .label {
    font-size: 12px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.6);
  }
  .subhead {
    display: flex;
    align-items: center;
    gap: 8px;
    width: 100%;
    padding: 6px 0;
    border: 0;
    background: transparent;
    font-size: 13px;
    font-weight: 650;
    text-align: left;
    cursor: pointer;
  }
  .subhead .now {
    margin-left: auto;
    font-size: 12px;
    font-weight: 500;
    color: rgba(255, 255, 255, 0.55);
  }
  .subhead .chev {
    width: 14px;
    height: 14px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    transition: rotate 180ms ease;
  }
  .subhead .chev.down {
    rotate: 90deg;
  }
  .hint {
    margin: 0;
    font-size: 12.5px;
    line-height: 17px;
    color: rgba(255, 255, 255, 0.55);
  }
  .sep {
    height: 1px;
    margin: 6px 10px;
    background: rgba(255, 255, 255, 0.08);
  }
  kbd {
    font: inherit;
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 5px;
    border: 1px solid rgba(255, 255, 255, 0.16);
    color: rgba(255, 255, 255, 0.6);
  }
  input[role="switch"] {
    appearance: none;
    flex: none;
    width: 38px;
    height: 22px;
    margin: 0;
    border-radius: 11px;
    background: rgba(255, 255, 255, 0.16);
    position: relative;
    cursor: pointer;
    transition: background 180ms ease;
  }
  input[role="switch"]::after {
    content: "";
    position: absolute;
    top: 3px;
    left: 3px;
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    transition: transform 180ms ease;
  }
  input[role="switch"]:checked {
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  input[role="switch"]:checked::after {
    transform: translateX(16px);
  }
</style>
