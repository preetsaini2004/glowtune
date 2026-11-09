<script lang="ts">
  // The desktop scene's menu: one dark panel of rows. Each row has an icon, a
  // label, often a switch, and a chevron that opens its options in place.
  import { slide } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import ColorWheel from "./ColorWheel.svelte";

  export type SceneSettings = Record<string, unknown> & {
    glowEnabled: boolean;
    glowHideFullscreen: boolean;
    stageClock: boolean;
    stageLyrics: boolean;
    stageRecent: boolean;
    stageUpcoming: boolean;
    stageMotion: boolean;
    stageDock: boolean;
    stageLyricsStyle: string;
    cleanLyrics: boolean;
    stageClockStyle: string;
    stageWallpaper: string;
    artworkColors: boolean;
    customColors: string[];
  };

  let {
    settings,
    full,
    save,
    onfullscreen,
    onmore,
    onstore,
    onedit,
    onoff,
  }: {
    settings: SceneSettings;
    full: boolean;
    save: (patch: Partial<SceneSettings>) => void;
    onfullscreen: () => void;
    onmore: () => void;
    onstore: () => void;
    onedit: () => void;
    onoff: () => void;
  } = $props();

  let open = $state<string | null>("colors");
  const toggle = (id: string) => (open = open === id ? null : id);

  const LYRICS = [
    { id: "flow", name: "Flow" },
    { id: "bold", name: "Bold" },
    { id: "karaoke", name: "Karaoke" },
    { id: "outline", name: "Outline" },
    { id: "sticker", name: "Sticker" },
  ];
  const CLOCKS = [
    { id: "outline", name: "Outline" },
    { id: "solid", name: "Solid" },
    { id: "minimal", name: "Minimal" },
  ];
  const WALLS = [
    { id: "art", name: "Album art" },
    { id: "video", name: "Music video" },
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
    clock: "M12 7v5l3 2M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0z",
    playlist: "M4 6h11M4 12h11M4 18h7M18 15v6M15 18h6",
    dock: "M3 17h18v4H3zM7 13h10",
    full: "M4 9V4h5M20 9V4h-5M4 15v5h5M20 15v5h-5",
    more: "M12 8a4 4 0 1 0 0 8 4 4 0 0 0 0-8zM12 2v3M12 19v3M4.9 4.9l2.1 2.1M17 17l2.1 2.1M2 12h3M19 12h3M4.9 19.1 7 17M17 7l2.1-2.1",
    off: "M12 3v8M6.3 6.3a8 8 0 1 0 11.4 0",
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
        <p class="hint">The glow around your screens, in the colors above. Shape and pulse come from the theme.</p>
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
        <p class="hint">Click any line to jump there. Scroll to look ahead.</p>
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
      <span class="open static"><span class="name">Up next</span></span>
      {@render sw("stageUpcoming", "Up next")}
    </div>
  </section>

  <section>
    <div class="head">
      <span class="open static">
        <span class="ic"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICON.dock} /></svg></span>
        <span class="name">Dock</span>
      </span>
      {@render sw("stageDock", "Dock")}
    </div>
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
    font-family: var(--font-lyrics);
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
