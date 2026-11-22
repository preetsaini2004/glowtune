<script lang="ts">
  import { onMount, tick } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { FONT_STACKS, type Theme } from "./theme.svelte";

  type Look = {
    stageLyricsStyle?: string;
    stageClockStyle?: string;
    stageWallpaper?: string;
    stageArtStyle?: string;
    stageDockStyle?: string;
    stageMotion?: boolean;
    stageClock?: boolean;
    stageRecent?: boolean;
    stageBeat?: boolean;
    stageEmoji?: boolean;
    artworkColors?: boolean;
    customColors?: string[];
    glowEffect?: string;
    glowParticles?: string;
  };
  type Pack = {
    id: string;
    kind: "theme" | "preset";
    name: string;
    author: string;
    description: string;
    colors: string[];
    tags: string[];
    installed: boolean;
    active: boolean;
    theme: Theme | null;
    settings?: Look;
    sample?: string;
    fresh?: boolean;
  };

  // Names as the rest of Glowtune shows them.
  const WALLS: Record<string, string> = { art: "Album art", cover: "Cover", video: "Music video", silk: "Silk", water: "Water", frost: "Frost", mosaic: "Mosaic", halftone: "Halftone", dots: "Dots" };
  const LIGHTS: Record<string, string> = { ambient: "Ambient", still: "Still", pulse: "Corner pulse", orbit: "Travelling swell", distortion: "Distortion", "lava-lamp": "Lava lamp", comet: "Comets", aurora: "Aurora", bars: "Equalizer", neon: "Neon", tide: "Tide" };
  const PARTICLES: Record<string, string> = { dots: "dots", stars: "stars", dust: "dust", embers: "embers", snow: "snow", fireflies: "fireflies", bubbles: "bubbles" };
  const LYRICS: Record<string, string> = { bold: "Bold", flow: "Flow", karaoke: "Karaoke", outline: "Outline", sticker: "Sticker", stars: "Stars", depth: "Depth", rain: "Rain", orbit: "Orbit", shards: "Shards", code: "Code", melt: "Melt", tags: "Tags" };
  const ART: Record<string, string> = { tilt: "Tilt", print: "Print", vinyl: "Vinyl", glow: "Glow" };
  const DOCKS: Record<string, string> = { bar: "Bar", pill: "Pill", islands: "Islands", line: "Line" };

  /** What a pack changes, in a few words each (the sheet's details list). */
  function details(p: Pack): [string, string][] {
    const out: [string, string][] = [];
    const t = p.theme;
    const look = p.settings;
    const effect = t?.glow.effect ?? look?.glowEffect;
    const particles = t?.glow.particles ?? look?.glowParticles;
    if (look?.stageWallpaper) out.push(["Wallpaper", WALLS[look.stageWallpaper] + (look.stageBeat ? ", on the beat" : "")]);
    if (look?.stageLyricsStyle) out.push(["Lyrics", LYRICS[look.stageLyricsStyle] + (look.stageEmoji ? ", with emoji" : "")]);
    if (look?.stageArtStyle) out.push(["Album", ART[look.stageArtStyle]]);
    if (look?.stageDockStyle) out.push(["Dock", DOCKS[look.stageDockStyle]]);
    if (effect) out.push(["Edge glow", LIGHTS[effect] + (particles && particles !== "off" ? ` with ${PARTICLES[particles]}` : "")]);
    else if (particles && particles !== "off") out.push(["Edge glow", `Your light, with ${PARTICLES[particles]}`]);
    if (t) out.push(["Lyrics font", t.font.lyrics_family]);
    return out;
  }

  let colorsOn = $state(true);
  async function readColors() {
    try {
      colorsOn = (await invoke<{ useTheme: boolean }>("get_settings")).useTheme;
    } catch {}
  }

  /** Applies a scene look you haven't got on, picked at random. */
  let surprising = $state(false);
  async function surprise() {
    const looks = packs.filter((p) => p.kind === "preset" && !p.active);
    if (!looks.length || surprising) return;
    const pick = looks[Math.floor(Math.random() * looks.length)];
    surprising = true;
    error = "";
    try {
      await invoke("store_install", { id: pick.id });
      await refresh();
      const glow = [pick.settings?.glowEffect, pick.settings?.glowParticles].filter(Boolean).length;
      surprised = glow ? `${pick.name} (it also changed your edge glow)` : pick.name;
    } catch (e) {
      error = String(e);
    } finally {
      surprising = false;
    }
  }
  let surprised = $state("");

  const shelves = [
    { kind: "theme", title: "Themes", note: "Glass, type and light" },
    { kind: "preset", title: "Scene looks", note: "A different way to listen" },
  ] as const;
  let packs = $state<Pack[]>([]);
  let query = $state("");
  let loading = $state(true);
  let error = $state("");
  let busy = $state(false);
  let selectedId = $state<string | null>(null);
  let sheet: HTMLDialogElement;
  const selected = $derived(packs.find((p) => p.id === selectedId));
  const filtered = $derived(packs.filter((p) =>
    [p.name, p.author, p.description, ...p.tags].join(" ").toLowerCase().includes(query.trim().toLowerCase()),
  ));

  async function refresh() {
    try {
      packs = await invoke<Pack[]>("store_catalog");
      error = "";
    } catch {
      error = "Couldn't open the Crate. Try again.";
    } finally {
      loading = false;
    }
  }

  onMount(() => {
    let disposed = false;
    const listeners = Promise.all([
      listen("themes-changed", () => { if (!disposed) void refresh(); }),
      listen("settings", () => { if (!disposed) { void refresh(); void readColors(); } }),
    ]);
    void refresh();
    void readColors();
    return () => {
      disposed = true;
      listeners.then((unlisten) => unlisten.forEach((off) => off()));
    };
  });

  async function open(pack: Pack) {
    selectedId = pack.id;
    error = "";
    await tick();
    sheet.showModal();
  }

  async function act(command: "store_install" | "store_use" | "store_remove" | "store_reinstall") {
    if (!selected || busy) return;
    if (command === "store_reinstall" && !confirm(`Reinstall “${selected.name}”? You get the Crate's latest version, and your own changes to this theme are replaced.`)) return;
    busy = true;
    error = "";
    try {
      await invoke(command, { id: selected.id });
      await refresh();
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }

  function shelfKeys(event: KeyboardEvent) {
    if (!["ArrowLeft", "ArrowRight", "Home", "End", "Enter"].includes(event.key)) return;
    const shelf = event.currentTarget as HTMLElement;
    const buttons = [...shelf.querySelectorAll<HTMLButtonElement>("button")];
    if (!buttons.length) return;
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement);
    if (event.key === "Enter") {
      if (index < 0) { event.preventDefault(); buttons[0].click(); }
      return;
    }
    event.preventDefault();
    const next = event.key === "Home" ? 0 : event.key === "End" ? buttons.length - 1
      : Math.max(0, Math.min(buttons.length - 1, index + (event.key === "ArrowRight" ? 1 : -1)));
    buttons[next].focus();
  }
</script>

{#snippet preview(pack: Pack)}
  {@const t = pack.theme}
  {@const look = pack.settings}
  {@const colors = look?.artworkColors === false ? look.customColors ?? pack.colors : pack.colors}
  {@const wall = look?.stageWallpaper ?? "art"}
  {@const lyricStyle = look?.stageLyricsStyle ?? (t?.lyrics.style === "karaoke" ? "karaoke" : "bold")}
  {@const particles = t?.glow.particles ?? look?.glowParticles ?? "off"}
  {@const words = (pack.sample ?? "Stay a little longer").split(" ")}
  <span
    class="scene w-{wall} fx-{t?.glow.effect ?? look?.glowEffect ?? 'ambient'}"
    class:video={wall === "video"}
    style:--a={colors[0]}
    style:--b={colors[1] ?? colors[0]}
    style:--paper={t?.colors.background ?? "#11151e"}
    style:--words={t?.colors.text ?? "#fff8ef"}
    style:--lyric-font={FONT_STACKS[t?.font.lyrics_family ?? "Bricolage Grotesque"]}
    style:--glass-radius={`${(t?.glass.radius ?? 14) / 3}px`}
    style:--glass-opacity={t?.glass.opacity ?? 0.5}
    style:--light={t?.glow.enabled === false ? 0 : (t?.glow.intensity ?? 0.8)}
    style:--edge={`${(t?.glow.thickness ?? 40) / 12}px`}
    style:--edge-radius={t?.glow.corners === "square" ? "0px" : "8px"}
    aria-hidden="true"
  >
    <span class="wash"></span>
    {#if wall === "cover" || wall === "frost" || wall === "water" || wall === "halftone" || wall === "mosaic"}<span class="cover"></span>{/if}
    <span class="edge"></span>
    {#if particles !== "off"}<span class="particles p-{particles}">{#each Array(12) as _}<i></i>{/each}</span>{/if}
    {#if !t && look?.stageClock !== false}
      <span class="clock" class:clock-outline={look?.stageClockStyle === "outline"} class:minimal={look?.stageClockStyle === "minimal"}>10:24</span>
    {/if}
    <span class="glass"><span class="art a-{look?.stageArtStyle ?? 'tilt'}"></span><span class="track-lines"><i></i><i></i></span><span class="play">▷</span></span>
    <span
      class="lyric l-{lyricStyle}"
      class:top={t?.lyrics.position === "top-center"}
      class:middle={t?.lyrics.position === "center"}
      style:font-size={t ? `${Math.max(14, t.font.lyrics_size / 3)}px` : undefined}
    >{#if lyricStyle === "stars"}{#each words as w, i}<span class="star" style:--i={i}>{w}</span>{/each}{:else}{words.slice(0, -1).join(" ")}<br /><em>{words.at(-1)}</em>{/if}</span>
    {#if look?.stageDockStyle}<span class="dock d-{look.stageDockStyle}"><i></i><i></i><i></i></span>{/if}
    {#if look?.stageRecent}<span class="recent"><i></i><i></i><i></i></span>{/if}
    <span class="scene-note">{t ? LIGHTS[t.glow.effect] ?? "Ambient" : WALLS[wall]}</span>
  </span>
{/snippet}

<div class="crate">
  <div class="intro">
    <div><p class="eyebrow">Good finds. All free.</p><h3>The Crate<span aria-hidden="true">.</span></h3></div>
    <svg class="crate-mark" viewBox="0 0 64 64" aria-hidden="true"><path d="m11 24 42-5v32l-42 5zM11 32l42-5M20 36v15M43 33v15M18 22l-2-12 29-4 3 14M24 21l-1-6 17-2 1 6" /><circle cx="32" cy="15" r="3" /></svg>
  </div>
  <p class="lede">Fresh light for your listening space.<br />Dig through. Find your mood.</p>
  <div class="lucky">
    <button onclick={surprise} disabled={surprising || loading}>{surprising ? "Picking…" : "Surprise me"}</button>
    <span role="status">{surprised ? `Now on: ${surprised}. Not for you? Press it again.` : "Puts on a scene look at random."}</span>
  </div>
  <label class="search">
    <svg viewBox="0 0 24 24" aria-hidden="true"><circle cx="10" cy="10" r="6" /><path d="m15 15 5 5" /></svg>
    <input bind:value={query} placeholder="Find a color, a mood, a look…" aria-label="Search the Crate" spellcheck="false" />
    {#if query}<button onclick={() => (query = "")} aria-label="Clear search">×</button>{/if}
  </label>

  {#if loading}<p class="status" role="status">Opening the Crate…</p>{/if}
  {#if error && !selectedId}<p class="error" role="alert">{error} <button onclick={refresh}>Try again</button></p>{/if}
  {#each shelves as shelf, index}
    {@const items = filtered.filter((p) => p.kind === shelf.kind)}
    <section aria-labelledby={`shelf-${shelf.kind}`}>
      <div class="shelf-head"><span class="number">0{index + 1}</span><h4 id={`shelf-${shelf.kind}`}>{shelf.title}</h4><span class="count">{items.length}</span></div>
      <p class="shelf-note">{shelf.note}<span aria-hidden="true">Scroll or use ← →</span></p>
      <div class="shelf" role="toolbar" aria-label={`${shelf.title} shelf`} tabindex="0" onkeydown={shelfKeys}>
        {#each items as pack (pack.id)}
          <button class="pack" onclick={() => open(pack)} aria-label={`${pack.name}${pack.active ? ", in use" : pack.installed ? ", installed" : ""}. View ${shelf.title.toLowerCase()}.`}>
            <span class="record-space" style:--disc-colors={pack.colors.join(", ")}>
              <span class="disc" aria-hidden="true"><i></i></span>
              <span class="sleeve">{@render preview(pack)}<span class="spine" aria-hidden="true"></span></span>
              {#if pack.active || pack.installed}<span class="badge">{pack.active ? "In use" : "Installed ✓"}</span>{/if}
              {#if pack.fresh}<span class="fresh">New</span>{/if}
            </span>
            <span class="pack-name">{pack.name}</span>
            <span class="pack-tags">{pack.tags.slice(0, 2).join(" / ")}</span>
          </button>
        {/each}
        {#if !loading && items.length === 0}<p class="empty">No {shelf.title.toLowerCase()} match this search.</p>{/if}
      </div>
    </section>
  {/each}
  {#if !query.trim() || "extensions coming soon".includes(query.trim().toLowerCase())}
    <section aria-labelledby="extensions-title">
      <div class="shelf-head"><span class="number">03</span><h4 id="extensions-title">Extensions</h4></div>
      <div class="shelf extension-shelf">
        <div class="coming"><span class="blank-record" aria-hidden="true">◎</span><p>Room for<br />something new</p><span>Coming soon</span></div>
      </div>
    </section>
  {/if}
  <p class="footnote">Made for Glowtune. Yours to make your own.</p>
</div>

<dialog bind:this={sheet} onclose={() => { selectedId = null; error = ""; }} aria-labelledby="pack-title" aria-describedby="pack-description">
  {#if selected}
    <div class="sheet-top"><span>{selected.kind === "theme" ? "Theme" : "Scene look"} / Free</span><button class="close-sheet" onclick={() => sheet.close()} aria-label="Close pack details">×</button></div>
    <div class="large-preview">{@render preview(selected)}</div>
    <div class="sheet-body">
      <p class="byline">By {selected.author}</p>
      <h3 id="pack-title">{selected.name}</h3>
      <p id="pack-description">{selected.description}</p>
      <div class="tags">{#each selected.tags as tag}<span>{tag}</span>{/each}</div>
      <dl class="details">{#each details(selected) as [label, text]}<dt>{label}</dt><dd>{text}</dd>{/each}</dl>
      {#if selected.kind === "preset"}<p class="apply-note">Applies to your desktop scene{selected.settings?.glowEffect || selected.settings?.glowParticles ? ", and sets your theme’s edge glow" : ""}. Your screens and controls stay as they are.</p>{/if}
      {#if selected.kind === "theme" && !colorsOn}<p class="apply-note">“Use the theme’s colors” is off in Settings › Themes, so this theme’s look shows with the album’s colors.</p>{/if}
      {#if error}<p class="error" role="alert">{error}</p>{/if}
      <div class="actions" aria-busy={busy}>
        <button class="primary" disabled={busy || selected.active} onclick={() => act(selected?.kind === "theme" && selected.installed ? "store_use" : "store_install")}>
          {busy ? "Working…" : selected.kind === "preset" ? selected.active ? "Applied ✓" : "Apply look" : selected.active ? "In use ✓" : selected.installed ? "Installed ✓ – Use" : "Install"}
        </button>
        {#if selected.kind === "theme" && selected.installed}
          <button class="remove" disabled={busy} onclick={() => act("store_reinstall")} title="Get the latest version of this theme">Reinstall</button>
          <button class="remove" disabled={busy} onclick={() => act("store_remove")}>Remove</button>
        {/if}
      </div>
    </div>
  {/if}
</dialog>

<style>
  .crate { min-width: 0; font-family: "Instrument Sans Variable", system-ui, sans-serif; }
  .intro { display: flex; align-items: center; justify-content: space-between; }
  .eyebrow { margin: 5px 0 4px; font-size: 11px; color: var(--ink-soft); }
  h3 { font-family: "Bricolage Grotesque Variable", sans-serif; margin: 0; font-size: 46px; font-weight: 760; letter-spacing: -.045em; line-height: 1.08; }
  .intro h3 span { color: var(--c1); }
  .crate-mark { width: 58px; height: 58px; stroke: var(--c1); stroke-width: 1.4; fill: none; transform: rotate(8deg); opacity: .75; }
  .lede { margin: 12px 0 20px; font-size: 13px; line-height: 1.5; color: var(--ink-soft); }
  .search { display: flex; align-items: center; gap: 9px; padding: 10px 12px; background: #ffffff07; border: 1px solid var(--hairline); border-radius: 10px; }
  .search svg { width: 16px; height: 16px; stroke: currentColor; stroke-width: 1.5; fill: none; opacity: .6; }
  .search input { flex: 1; min-width: 0; border: 0; background: transparent; color: inherit; outline: 0; font: inherit; font-size: 12px; }
  .search:focus-within { outline: 2px solid var(--c1); outline-offset: 2px; }
  .search button { background: transparent; border: 0; cursor: pointer; }
  section { margin-top: 25px; }
  .shelf-head { display: flex; align-items: baseline; gap: 9px; }
  .number { font-size: 10px; font-variant-numeric: tabular-nums; color: var(--ink-faint); }
  h4 { margin: 0; font-family: "Bricolage Grotesque Variable", sans-serif; font-size: 21px; font-weight: 650; letter-spacing: -.025em; }
  .count { color: var(--ink-soft); font-size: 10px; border: 1px solid var(--hairline); border-radius: 20px; padding: 1px 6px; }
  .shelf-note { display: flex; justify-content: space-between; margin: 4px 0 0 23px; font-size: 11px; color: var(--ink-soft); }
  .shelf-note span { color: var(--ink-faint); font-size: 10px; }
  .shelf { display: flex; gap: 28px; overflow-x: auto; padding: 22px 64px 17px 5px; scroll-snap-type: x proximity; scrollbar-width: thin; scrollbar-color: color-mix(in oklab, var(--c1) 25%, #ffffff20) transparent; border-bottom: 3px solid #ffffff12; box-shadow: 0 7px 10px -9px #000; }
  .pack { flex: 0 0 148px; width: 148px; border: 0; padding: 0; background: transparent; text-align: left; color: inherit; cursor: pointer; scroll-snap-align: start; scroll-margin-inline: 12px 64px; border-radius: 4px; }
  .record-space { display: block; width: 148px; height: 148px; position: relative; isolation: isolate; }
  .disc { position: absolute; width: 132px; height: 132px; left: 8px; top: 8px; border-radius: 50%; background: repeating-radial-gradient(circle, transparent 0 2px, #0005 3px 4px), conic-gradient(var(--disc-colors), #141419, var(--disc-colors)); box-shadow: 1px 3px 14px #0009; transition: translate 350ms cubic-bezier(.2,.7,.2,1); animation: spin 12s linear infinite paused; }
  .disc i { position: absolute; inset: 39%; border: 10px solid #17171e; border-radius: 50%; background: #e4decc; }
  .pack:hover .disc, .pack:focus-visible .disc { translate: 62px 0; animation-play-state: running; }
  .sleeve { position: absolute; inset: 0; overflow: hidden; border: 1px solid #ffffff20; border-radius: 3px 5px 5px 3px; box-shadow: 2px 5px 14px #0006; transition: transform 250ms ease; }
  .pack:hover .sleeve, .pack:focus-visible .sleeve { transform: rotate(-3deg) translateY(-3px); }
  .spine { position: absolute; inset: 0 auto 0 0; width: 4px; background: linear-gradient(90deg,#fff2,#0004); }
  .badge { position: absolute; bottom: 7px; right: 6px; border-radius: 4px; padding: 3px 5px; background: #10131ee8; color: #f5f5f7; font-size: 9px; box-shadow: 0 1px 5px #0005; }
  .pack-name { display: block; margin-top: 11px; font-size: 13px; font-weight: 600; }
  .pack-tags { display: block; margin-top: 3px; font-size: 10px; color: var(--ink-soft); }
  .scene { position: relative; display: block; width: 100%; height: 100%; overflow: hidden; background: var(--paper); color: var(--words); container-type: inline-size; }
  .lucky { display: flex; align-items: center; gap: 12px; margin: -6px 0 18px; }
  .lucky button { padding: 8px 14px; border-radius: 999px; border: 1px solid #ffffff30; background: linear-gradient(110deg, color-mix(in oklab, var(--c1) 30%, transparent), color-mix(in oklab, var(--c2) 30%, transparent)); color: inherit; font: inherit; font-size: 12px; font-weight: 650; cursor: pointer; }
  .lucky button:disabled { opacity: .6; cursor: default; }
  .lucky span { font-size: 11px; color: var(--ink-soft); }
  .fresh { position: absolute; top: 7px; left: 9px; border-radius: 4px; padding: 2px 5px; background: var(--c1); color: #0d0d12; font-size: 9px; font-weight: 700; box-shadow: 0 1px 5px #0005; }
  .details { display: grid; grid-template-columns: auto 1fr; gap: 5px 14px; margin: 16px 0 0; font-size: 12px; }
  .details dt { color: #aaa6bc; }
  .details dd { margin: 0; color: #e8e5f0; }
  .cover { position: absolute; inset: 12% 22% 26%; border-radius: 3px; background: linear-gradient(135deg, var(--a), var(--b)); opacity: .8; }
  .w-cover .wash { opacity: .25; }
  .w-water .cover { filter: blur(1px); }
  .w-water .cover::after { content: ""; position: absolute; inset: 0; background: repeating-linear-gradient(95deg, transparent 0 7px, #ffffff40 8px, transparent 9px 15px); mask: linear-gradient(transparent, #000 30%); }
  .w-frost .cover { filter: blur(3px) saturate(.7); }
  .w-frost .cover::after { content: ""; position: absolute; inset: -6%; background: radial-gradient(circle at 30% 40%, #ffffff70, transparent 40%), radial-gradient(circle at 70% 70%, #ffffff50, transparent 45%); }
  .w-halftone .cover { background: radial-gradient(circle, var(--a) 42%, transparent 46%) 0 0 / 7px 7px, radial-gradient(circle, var(--b) 30%, transparent 34%) 3.5px 3.5px / 7px 7px; opacity: .9; }
  .w-mosaic .cover { mask: repeating-linear-gradient(90deg, #000 0 8px, #0006 8px 9px), repeating-linear-gradient(0deg, #000 0 8px, #0006 8px 9px); mask-composite: intersect; }
  .w-silk .wash { opacity: .55; background: repeating-linear-gradient(115deg, color-mix(in srgb, var(--a) 60%, transparent) 0 14%, color-mix(in srgb, var(--b) 50%, transparent) 22% 34%, transparent 44% 52%); filter: blur(6px); }
  .fx-neon .edge { inset: 6%; border-width: 1.5px; border-radius: 6px; box-shadow: 0 0 8px var(--a), inset 0 0 8px var(--b); }
  .fx-bars .edge { border: 0; box-shadow: none; background: repeating-linear-gradient(90deg, var(--a) 0 3px, transparent 3px 6px) bottom / 100% 12% no-repeat, repeating-linear-gradient(90deg, var(--b) 0 3px, transparent 3px 6px) top / 100% 7% no-repeat; }
  .fx-comet .edge { border-color: transparent; box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--a) 30%, transparent); background: conic-gradient(from 40deg, transparent 0 62%, var(--a) 80%, transparent 82% 100%); mask: linear-gradient(#000 0 0) border-box, linear-gradient(#000 0 0) padding-box; mask-composite: exclude; }
  .fx-aurora .edge { border-top-width: calc(var(--edge) * 2.4); border-color: color-mix(in srgb, var(--a) 55%, transparent) transparent transparent; box-shadow: inset 0 14px 22px -6px var(--a), inset 0 26px 30px -14px var(--b); }
  .fx-tide .edge { border-color: transparent; box-shadow: inset 10px 0 18px -6px var(--a), inset -10px 0 18px -6px var(--b), inset 0 -16px 18px -10px var(--a); }
  .particles { position: absolute; inset: 0; pointer-events: none; }
  .particles i { position: absolute; width: 2.5%; aspect-ratio: 1; border-radius: 50%; background: var(--a); opacity: .85; }
  .particles i:nth-child(1) { left: 6%; top: 18%; } .particles i:nth-child(2) { left: 90%; top: 30%; } .particles i:nth-child(3) { left: 12%; top: 70%; }
  .particles i:nth-child(4) { left: 84%; top: 78%; } .particles i:nth-child(5) { left: 30%; top: 8%; } .particles i:nth-child(6) { left: 66%; top: 6%; }
  .particles i:nth-child(7) { left: 94%; top: 55%; } .particles i:nth-child(8) { left: 4%; top: 46%; } .particles i:nth-child(9) { left: 48%; top: 92%; }
  .particles i:nth-child(10) { left: 74%; top: 90%; } .particles i:nth-child(11) { left: 22%; top: 90%; } .particles i:nth-child(12) { left: 56%; top: 4%; }
  .p-snow i { background: #fff; width: 3%; } .p-stars i { background: #fff; width: 1.8%; box-shadow: 0 0 4px #fff; }
  .p-embers i { background: #ffb065; box-shadow: 0 0 5px #ff7a2e; } .p-fireflies i { background: #f6ff9c; box-shadow: 0 0 7px 1px #e4ff6a; }
  .p-bubbles i { background: transparent; border: 1px solid #ffffffa0; width: 4.5%; } .p-dust i { background: #fff3d6; opacity: .45; width: 1.5%; }
  .art.a-vinyl { border-radius: 50%; background: radial-gradient(circle, #111 0 9%, transparent 10%), repeating-radial-gradient(circle, #0003 0 1px, transparent 1px 3px), linear-gradient(135deg, var(--a), var(--b)); }
  .art.a-print { outline: 2px solid #f4efe4; outline-offset: -1px; }
  .art.a-glow { box-shadow: 0 0 8px var(--a); }
  .art.a-tilt { rotate: -6deg; }
  .dock { position: absolute; left: 50%; bottom: 3%; translate: -50% 0; display: flex; justify-content: center; gap: 3px; }
  .dock i { display: block; height: 5px; border-radius: 3px; background: #ffffff2e; }
  .d-bar { width: 86%; } .d-bar i { flex: 1; border-radius: 1px; } .d-bar i + i { display: none; }
  .d-pill i { width: 40cqw; border-radius: 99px; } .d-pill i + i { display: none; }
  .d-islands i { width: 12cqw; border-radius: 99px; }
  .d-line { width: 86%; } .d-line i { flex: 1; height: 1px; background: var(--a); } .d-line i + i { display: none; }
  .lyric .star { display: inline-block; margin-right: .35em; font-size: .55em; translate: 0 calc(sin(var(--i) * 1.7) * .8em); }
  .lyric .star::before { content: ""; display: block; width: 3px; height: 3px; margin: 0 auto 2px; border-radius: 50%; background: var(--words); box-shadow: 0 0 6px var(--a); }
  .l-rain em { display: inline-block; translate: 0 18%; opacity: .55; filter: blur(.4px); }
  .l-depth { text-shadow: 6px 6px 0 color-mix(in srgb, var(--words) 22%, transparent); }
  .l-orbit { rotate: -9deg; }
  .l-shards { text-shadow: -1.5px 0 0 var(--a), 1.5px 0 0 var(--b), 0 2px 12px #0006; }
  .l-shards em { display: inline-block; rotate: -8deg; clip-path: polygon(0 0, 100% 0, 100% 46%, 0 60%); }
  .l-code em { color: transparent; background: radial-gradient(circle, #ffd23d 38%, transparent 42%) 0 55% / .62em .62em repeat-x; }
  .l-melt em { display: inline-block; transform-origin: top; scale: 1 1.35; color: color-mix(in srgb, var(--a) 60%, var(--words)); text-shadow: 0 .25em 0 color-mix(in srgb, var(--a) 60%, transparent), 0 .5em 0 color-mix(in srgb, var(--b) 40%, transparent); }
  .l-tags { font-size: 9cqw; line-height: 1.5; }
  .l-tags, .l-tags em { padding: .05em .3em; background: #fbf7f2; color: color-mix(in srgb, var(--a) 70%, #1a1020); border-radius: 2px; text-shadow: none; }
  .l-tags em { display: inline-block; margin-top: .25em; rotate: 4deg; }
  .w-dots .wash { opacity: .5; background: radial-gradient(circle, var(--a) 22%, transparent 26%) 0 0 / 7px 7px, radial-gradient(ellipse at 40% 60%, color-mix(in srgb, var(--b) 40%, transparent), transparent 70%); }
  .l-orbit em { display: inline-block; rotate: 14deg; }
  .wash { position: absolute; inset: 0; opacity: .43; background: radial-gradient(ellipse at 15% 85%,var(--a),transparent 68%), radial-gradient(ellipse at 90% 12%,var(--b),transparent 65%); }
  .video .wash { inset: 14% 0; opacity: .7; background: linear-gradient(120deg,#0009 20%,transparent 20% 30%,#0008 30% 50%,transparent 50%),linear-gradient(30deg,var(--a),var(--b)); }
  .edge { position: absolute; inset: 0; opacity: var(--light); border-radius: var(--edge-radius); border: var(--edge) solid color-mix(in srgb,var(--a) 65%,transparent); box-shadow: inset 0 0 18px var(--a),inset 0 -5px 15px var(--b); }
  .glass { position: absolute; top: 38%; right: 8%; width: 40%; height: 20%; display: flex; align-items: center; gap: 5%; padding: 5%; border-radius: var(--glass-radius); background: rgb(255 255 255 / calc(var(--glass-opacity) * .2)); border: 1px solid #ffffff24; box-shadow: 0 5px 15px #0004; }
  .art { width: 30%; aspect-ratio: 1; border-radius: 2px; background: linear-gradient(135deg,var(--a),var(--b)); }
  .track-lines { flex: 1; display: grid; gap: 4px; }
  .track-lines i { display: block; height: 2px; background: var(--words); opacity: .5; }
  .track-lines i + i { width: 60%; opacity: .25; }
  .play { font-size: 10px; }
  .clock { position: absolute; top: 9%; left: 10%; font-size: 28cqw; font-family: "Bricolage Grotesque Variable",sans-serif; letter-spacing: -.06em; line-height: 1; font-weight: 700; opacity: .75; }
  .clock.clock-outline { color: transparent; -webkit-text-stroke: .7px var(--words); }
  .clock.minimal { font-family: "Instrument Sans Variable",sans-serif; font-size: 10cqw; font-weight: 400; letter-spacing: -.02em; }
  .lyric { position: absolute; bottom: 16%; left: 10%; font-family: var(--lyric-font); font-size: 14cqw; line-height: .98; font-weight: 750; letter-spacing: -.045em; text-shadow: 0 2px 12px #0006; }
  .lyric em { font: inherit; }
  .lyric.top { bottom: auto; top: 13%; }
  .lyric.middle { bottom: auto; top: 37%; }
  .lyric.l-outline { color: transparent; -webkit-text-stroke: .7px var(--words); }
  .lyric.l-sticker { background: var(--words); color: var(--paper); padding: 3% 4%; rotate: -7deg; text-shadow: none; }
  .lyric.l-flow em { opacity: .35; }
  .lyric.l-karaoke em { color: var(--a); }
  .recent { display: flex; position: absolute; right: 8%; bottom: 15%; gap: 3px; }
  .recent i { width: 8cqw; aspect-ratio: 1; background: var(--b); opacity: .4; border-radius: 2px; }
  .scene-note { position: absolute; bottom: 6%; left: 10%; font-size: 5cqw; opacity: .6; letter-spacing: .03em; }
  .coming { flex: 0 0 148px; height: 148px; padding: 14px; border: 1px dashed var(--hairline); border-radius: 4px; background: linear-gradient(120deg,#ffffff04,transparent); }
  .blank-record { display: block; color: var(--ink-faint); font-size: 30px; line-height: 1; }
  .coming p { margin: 12px 0 7px; font-family: "Bricolage Grotesque Variable",sans-serif; font-size: 15px; line-height: 1.1; }
  .coming > span:last-child { font-size: 10px; color: var(--ink-soft); }
  .footnote { margin: 22px 0 0; font-size: 10px; color: var(--ink-faint); }
  .empty, .status { font-size: 12px; color: var(--ink-soft); }
  .empty { min-width: 240px; padding-block: 25px; }
  button:focus-visible, .shelf:focus-visible { outline: 2px solid var(--c1); outline-offset: 4px; }
  dialog { width: min(400px,calc(100vw - 40px)); max-height: calc(100vh - 40px); padding: 0; border: 1px solid #ffffff25; border-radius: 18px; background: #15151e; color: #f5f5f7; box-shadow: 0 24px 80px #000b; font-family: "Instrument Sans Variable",sans-serif; }
  dialog::backdrop { background: #05050bbc; backdrop-filter: blur(7px); }
  .sheet-top { display: flex; justify-content: space-between; align-items: center; padding: 12px 16px; color: #bbb9c7; font-size: 11px; }
  .close-sheet { width: 28px; height: 28px; background: #ffffff0c; border: 0; border-radius: 50%; color: #fff; font-size: 20px; cursor: pointer; }
  .large-preview { width: 100%; height: 215px; }
  .large-preview :global(.scene) { container-type: size; }
  .large-preview :global(.lyric) { font-size: 35px !important; }
  .large-preview :global(.clock) { font-size: 66px; }
  .large-preview :global(.clock.minimal) { font-size: 26px; }
  .large-preview :global(.scene-note) { font-size: 9px; }
  .sheet-body { padding: 20px; }
  .byline { margin: 0 0 5px; font-size: 11px; color: #b6b3c3; }
  .sheet-body h3 { font-size: 30px; }
  #pack-description { font-size: 13px; line-height: 1.55; color: #d2cfdc; margin: 12px 0; }
  .tags { display: flex; flex-wrap: wrap; gap: 5px; }
  .tags span { border: 1px solid #ffffff1c; border-radius: 20px; padding: 3px 9px; color: #c7c3d4; font-size: 10px; }
  .apply-note { font-size: 11px; line-height: 1.5; color: #aaa6bc; margin: 14px 0 0; }
  .actions { display: flex; gap: 10px; margin-top: 20px; }
  .actions button { padding: 10px 17px; border-radius: 8px; font: inherit; font-size: 12px; cursor: pointer; }
  .primary { flex: 1; background: linear-gradient(110deg,color-mix(in oklab,var(--c1) 35%,#282436),color-mix(in oklab,var(--c2) 35%,#282436)); color: #fff; border: 1px solid #ffffff30; font-weight: 650 !important; }
  .remove { border: 1px solid #ffffff22; background: transparent; color: #d2cfdc; }
  .actions button:disabled { opacity: .6; cursor: default; }
  .error { font-size: 12px; color: #ffb4b4; line-height: 1.5; }
  .error button { background: none; border: 0; color: inherit; text-decoration: underline; cursor: pointer; }
  @keyframes spin { to { rotate: 360deg; } }
  @media (prefers-reduced-motion: reduce) { .disc { animation: none; transition: none; } .sleeve { transition: none; } }
</style>
