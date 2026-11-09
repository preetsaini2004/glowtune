<script lang="ts">
  // Find something to play: a compact panel that grows out of the dock's search
  // control, on the same scene (non-modal; the scene stays usable around it).
  //
  //   empty query  → your services' own shelves (recommendations and mixes first,
  //                  then library), grouped by the shelf they came from
  //   typing       → matches in that music + the public catalog; Enter searches
  //                  the chosen service directly
  //
  // Data comes from the service readers (music_browser.rs): cached shelves show
  // at once, fresh ones replace them as each service answers.
  import { onMount, tick } from "svelte";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { SERVICE_NAMES, type MusicItem, type Shelf, type ServiceId } from "./musicBrowser";

  let {
    onclose,
    anchor,
    target = $bindable<ServiceId>("spotify"),
  }: { onclose: (restoreFocus?: boolean) => void; anchor: HTMLElement | undefined; target?: ServiceId } = $props();

  type CatalogItem = { title: string; subtitle: string; artwork: string | null; query: string; kind: string };
  type Kind = "all" | "song" | "album" | "playlist";

  let panel: HTMLDivElement | undefined = $state();
  let input: HTMLInputElement | undefined = $state();
  let query = $state("");
  let shelves = $state<Shelf[]>([]);
  let service = $state<ServiceId | "all">("all");
  let kind = $state<Kind>("all");
  let busy = $state(false);
  let error = $state("");
  let catalog = $state<CatalogItem[]>([]);
  let searching = $state(false);
  let catalogError = $state("");
  let expanded = $state<Record<string, boolean>>({});

  // ── Placement: anchored to the search control, inside the viewport ──────
  let place = $state({ left: 12, width: 560, bottom: 90, top: 0, max: 520, up: true, originX: 280 });
  function measure() {
    if (!anchor) return;
    const r = anchor.getBoundingClientRect();
    const width = Math.min(560, innerWidth - 24);
    const left = Math.min(Math.max(12, r.right - width), innerWidth - width - 12);
    const above = r.top - 22;
    const below = innerHeight - r.bottom - 22;
    const up = above >= Math.min(320, below) || above >= below;
    place = {
      left,
      width,
      bottom: innerHeight - r.top + 10,
      top: r.bottom + 10,
      max: Math.max(220, Math.min(560, up ? above : below)),
      up,
      originX: Math.min(width, Math.max(0, r.left + r.width / 2 - left)),
    };
  }

  // ── Opening motion: the search bar grows up into the panel ──────────────
  const reduced = typeof matchMedia === "function" && matchMedia("(prefers-reduced-motion: reduce)").matches;
  function grow(_node: Element, { up }: { up: boolean }) {
    if (reduced) return { duration: 120, css: (t: number) => `opacity:${t}` };
    return {
      duration: 300,
      easing: cubicOut,
      css: (t: number, u: number) =>
        `opacity:${Math.min(1, t * 1.8)};` +
        `transform:translateY(${(up ? 1 : -1) * u * 12}px) scale(${0.96 + 0.04 * t}, ${0.82 + 0.18 * t});` +
        `clip-path:inset(${up ? u * 92 : 0}% 0 ${up ? 0 : u * 92}% 0 round 22px);`,
    };
  }

  // ── Music ────────────────────────────────────────────────────────────────
  const RECOMMENDED = /made for|for you|mix|recommend|daily|discover|radio|quick picks|suggest|release radar|new for/i;
  type Group = { key: string; service: ServiceId; title: string; picked: boolean; items: MusicItem[] };
  const groups = $derived.by(() => {
    const out: Group[] = [];
    const byKey = new Map<string, Group>();
    for (const shelf of shelves) {
      if (service !== "all" && shelf.service !== service) continue;
      for (const item of shelf.items) {
        if (kind !== "all" && item.kind !== kind) continue;
        const title = item.section || "From your library";
        const key = `${shelf.service}|${title}`;
        let g = byKey.get(key);
        if (!g) {
          g = { key, service: shelf.service, title, picked: RECOMMENDED.test(title), items: [] };
          byKey.set(key, g);
          out.push(g);
        }
        g.items.push(item);
      }
    }
    // Recommendations and mixes lead; otherwise keep the services' own order.
    return out.map((g, i) => ({ g, i })).sort((a, b) => Number(b.g.picked) - Number(a.g.picked) || a.i - b.i).map((x) => x.g);
  });
  const matches = $derived.by(() => {
    const q = query.trim().toLowerCase();
    if (!q) return [];
    return shelves
      .filter((s) => service === "all" || s.service === service)
      .flatMap((s) => s.items)
      .filter((x) => (kind === "all" || x.kind === kind) && `${x.title} ${x.artist}`.toLowerCase().includes(q))
      .slice(0, 12);
  });
  const waiting = $derived(shelves.filter((s) => s.status !== "ready" && (service === "all" || s.service === service)));
  const hasMusic = $derived(groups.some((g) => g.items.length));

  async function refresh() {
    busy = true;
    error = "";
    try {
      shelves = await invoke<Shelf[]>("refresh_music_library");
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
    }
  }
  async function choose(item: MusicItem) {
    try {
      await invoke("open_music_item", { item });
      onclose();
    } catch (e) {
      error = String(e);
    }
  }
  async function playCatalog(item: CatalogItem) {
    try {
      await invoke("play_on", { service: target, query: item.query });
      onclose();
    } catch (e) {
      error = String(e);
    }
  }
  async function searchService() {
    if (!query.trim()) return;
    try {
      await invoke("open_search", { service: target, query: query.trim() });
      onclose();
    } catch (e) {
      error = String(e);
    }
  }
  function connect(s: Shelf) {
    const cmd = s.status === "connect" || s.status === "signin" ? "open_service" : "open_music_library";
    invoke(cmd, { service: s.service }).catch((e) => (error = String(e)));
  }
  function statusText(s: Shelf) {
    if (s.status === "connect") return "Not connected";
    if (s.status === "signin") return "Sign in for your picks";
    return busy ? "Loading…" : "Unavailable right now";
  }

  // Catalog search as you type.
  $effect(() => {
    const q = query.trim();
    catalogError = "";
    if (q.length < 2) {
      catalog = [];
      searching = false;
      return;
    }
    let current = true;
    searching = true;
    const timer = setTimeout(async () => {
      try {
        const r = await invoke<{ songs: CatalogItem[]; albums: CatalogItem[]; artists: CatalogItem[] }>("search_catalog", { query: q });
        if (current) catalog = [...r.songs, ...r.albums, ...r.artists];
      } catch {
        if (current) {
          catalog = [];
          catalogError = `Catalog search is unavailable. Press Enter to search ${SERVICE_NAMES[target]}.`;
        }
      } finally {
        if (current) searching = false;
      }
    }, 280);
    return () => {
      current = false;
      clearTimeout(timer);
    };
  });

  // ── Dismissal ────────────────────────────────────────────────────────────
  // Escape first (capture, so scene shortcuts don't also fire); an outside
  // pointer press closes too and still reaches whatever was pressed.
  function key(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      e.preventDefault();
      onclose(true);
    } else if ((e.ctrlKey && e.key.toLowerCase() === "k") || (e.key === "/" && document.activeElement !== input)) {
      e.stopPropagation();
      e.preventDefault();
      input?.focus();
    }
  }
  function outside(e: PointerEvent) {
    const t = e.target as Node;
    if (panel?.contains(t) || anchor?.contains(t)) return;
    onclose(false);
  }
  /** Arrow keys move between music choices. */
  function arrows(e: KeyboardEvent) {
    if (!["ArrowDown", "ArrowUp", "ArrowLeft", "ArrowRight"].includes(e.key) || !panel) return;
    const items = [...panel.querySelectorAll<HTMLElement>("[data-nav]")];
    if (!items.length) return;
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (at < 0) {
      if (e.key === "ArrowDown") {
        e.preventDefault();
        items[0].focus();
      }
      return;
    }
    e.preventDefault();
    const step = e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 1;
    const next = at + step;
    if (next < 0) input?.focus();
    else items[Math.min(items.length - 1, next)].focus();
  }

  onMount(() => {
    measure();
    tick().then(() => input?.focus());
    window.addEventListener("keydown", key, true);
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("resize", measure);
    const un = listen<Shelf[]>("music-library", (e) => (shelves = e.payload));
    invoke<Shelf[]>("get_music_library")
      .then((s) => {
        if (!shelves.length) shelves = s;
      })
      .catch(() => {});
    refresh();
    return () => {
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("resize", measure);
      un.then((f) => f());
    };
  });

  const KINDS: [Kind, string][] = [
    ["all", "Everything"],
    ["song", "Songs"],
    ["album", "Albums"],
    ["playlist", "Playlists"],
  ];
</script>

<div
  bind:this={panel}
  id="music-panel"
  class="panel"
  class:down={!place.up}
  role="dialog"
  aria-label="Find something to play"
  tabindex="-1"
  onkeydown={arrows}
  style:left={`${place.left}px`}
  style:width={`${place.width}px`}
  style:bottom={place.up ? `${place.bottom}px` : undefined}
  style:top={place.up ? undefined : `${place.top}px`}
  style:max-height={`${place.max}px`}
  style:transform-origin={`${place.originX}px ${place.up ? "100%" : "0"}`}
  transition:grow={{ up: place.up }}
>
  <form class="find" onsubmit={(e) => { e.preventDefault(); searchService(); }}>
    <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M10.5 17a6.5 6.5 0 1 0 0-13 6.5 6.5 0 0 0 0 13zM15.5 15.5 20 20" /></svg>
    <input
      bind:this={input}
      bind:value={query}
      placeholder="Songs, artists, albums"
      aria-label="Search your music and the catalog"
      spellcheck="false"
      autocomplete="off"
    />
    {#if busy}<span class="sync" title="Updating from your services" aria-label="Updating"></span>{/if}
    <button type="button" class="x" aria-label="Close" onclick={() => onclose(true)}>
      <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M7 7l10 10M17 7 7 17" /></svg>
    </button>
  </form>

  <div class="filters">
    <div class="chips" role="group" aria-label="Services">
      <button class:on={service === "all"} aria-pressed={service === "all"} onclick={() => (service = "all")}>All</button>
      {#each Object.entries(SERVICE_NAMES) as [id, name]}
        <button
          class:on={service === id}
          aria-pressed={service === id}
          onclick={() => {
            service = id as ServiceId;
            target = id as ServiceId;
          }}>{name}</button
        >
      {/each}
    </div>
    <div class="chips kinds" role="group" aria-label="Music type">
      {#each KINDS as [id, name]}
        <button class:on={kind === id} aria-pressed={kind === id} onclick={() => (kind = id)}>{name}</button>
      {/each}
    </div>
  </div>

  <div class="body">
    {#if error}<p class="note" role="alert">{error}</p>{/if}

    {#if !query.trim()}
      {#if hasMusic}
        {#each groups as g (g.key)}
          <section aria-label={g.title}>
            <header>
              <h3>{g.title}</h3>
              <span class="src">{g.picked ? "Picked for you · " : ""}{SERVICE_NAMES[g.service]}</span>
            </header>
            <div class="grid">
              {#each expanded[g.key] ? g.items : g.items.slice(0, 8) as item (item.url)}
                <button class="card" data-nav onclick={() => choose(item)} title={`${item.title}${item.artist ? ` · ${item.artist}` : ""}`}>
                  {#if item.artwork}<img src={item.artwork} alt="" loading="lazy" />{:else}<span class="ph" aria-hidden="true">♫</span>{/if}
                  <strong>{item.title}</strong>
                  <span>{item.artist || item.kind}</span>
                </button>
              {/each}
            </div>
            {#if g.items.length > 8}
              <button class="more" onclick={() => (expanded = { ...expanded, [g.key]: !expanded[g.key] })}>
                {expanded[g.key] ? "Show less" : `Show all ${g.items.length}`}
              </button>
            {/if}
          </section>
        {/each}
      {:else if busy}
        <div class="grid" aria-label="Loading your music">
          {#each Array(8) as _}<div class="skeleton"><i></i><b></b><b></b></div>{/each}
        </div>
      {:else}
        <p class="note">Nothing to show yet. Open a service below and sign in, and its picks appear here.</p>
      {/if}
    {:else}
      {#if matches.length}
        <section aria-label="In your music">
          <header><h3>In your music</h3><span class="src">From your services' shelves</span></header>
          <div class="rows">
            {#each matches as item (item.url)}
              <button class="row" data-nav onclick={() => choose(item)}>
                {#if item.artwork}<img src={item.artwork} alt="" loading="lazy" />{:else}<span class="ph" aria-hidden="true">♫</span>{/if}
                <span class="txt"><strong>{item.title}</strong><span>{item.artist || item.kind}</span></span>
                <small>{SERVICE_NAMES[item.service]}</small>
              </button>
            {/each}
          </div>
        </section>
      {/if}
      <section aria-label="Catalog">
        <header>
          <h3>Catalog</h3>
          <label class="src">Play on
            <select bind:value={target}>
              {#each Object.entries(SERVICE_NAMES) as [id, name]}<option value={id}>{name}</option>{/each}
            </select>
          </label>
        </header>
        {#if searching}<p class="note">Searching…</p>{/if}
        {#if catalogError}<p class="note" role="status">{catalogError}</p>{/if}
        <div class="rows">
          {#each catalog.filter((x) => kind === "all" || x.kind === kind).slice(0, 14) as item}
            <button class="row" data-nav onclick={() => playCatalog(item)}>
              {#if item.artwork}<img src={item.artwork} alt="" loading="lazy" />{:else}<span class="ph" aria-hidden="true">♫</span>{/if}
              <span class="txt"><strong>{item.title}</strong><span>{item.subtitle}</span></span>
              <small>{item.kind}</small>
            </button>
          {/each}
        </div>
        {#if !searching && query.trim().length >= 2 && !catalog.length && !catalogError}
          <p class="note">No catalog matches. Press Enter to search {SERVICE_NAMES[target]}.</p>
        {/if}
      </section>
    {/if}

    {#if waiting.length}
      <div class="services">
        {#each waiting as s (s.service)}
          <div class="svc">
            <span><strong>{SERVICE_NAMES[s.service]}</strong> · {statusText(s)}</span>
            <button onclick={() => connect(s)}>{s.status === "connect" || s.status === "signin" ? "Open" : "Library"}</button>
          </div>
        {/each}
      </div>
    {/if}
  </div>

  <footer>
    <span>Shelves come from the Home and Library pages of the services you're signed into.</span>
    <button disabled={busy} onclick={refresh}>{busy ? "Updating…" : "Refresh"}</button>
  </footer>
</div>

<style>
  .panel {
    position: fixed !important;
    z-index: 9 !important;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: 22px;
    border: 1px solid var(--glass-line, rgba(255, 255, 255, 0.1));
    background:
      linear-gradient(180deg, rgba(255, 255, 255, 0.07), transparent 120px),
      color-mix(in oklab, var(--c1) 9%, rgba(13, 11, 19, 0.9));
    backdrop-filter: blur(28px) saturate(1.5);
    box-shadow:
      0 28px 70px -24px rgba(0, 0, 0, 0.75),
      inset 0 1px 0 rgba(255, 255, 255, 0.09);
    color: var(--ink, #fff);
    font-family: var(--font-ui);
  }
  .panel:focus {
    outline: none;
  }
  button,
  select,
  input {
    font: inherit;
    color: inherit;
  }
  button {
    cursor: pointer;
  }
  button:focus-visible,
  input:focus-visible,
  select:focus-visible {
    outline: 2px solid color-mix(in oklab, var(--c1) 75%, white);
    outline-offset: 2px;
  }

  /* Search field: the dock's search bar, grown up. */
  .find {
    display: flex;
    align-items: center;
    gap: 9px;
    margin: 12px 12px 8px;
    height: 44px;
    padding: 0 6px 0 14px;
    border-radius: 14px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid color-mix(in oklab, var(--c1) 45%, transparent);
  }
  .find svg {
    width: 17px;
    height: 17px;
    flex: none;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    opacity: 0.65;
  }
  .find input {
    flex: 1;
    min-width: 0;
    border: 0;
    outline: 0;
    background: transparent;
    font-size: 14.5px;
  }
  .find input::placeholder {
    color: rgba(255, 255, 255, 0.45);
  }
  .x {
    display: grid;
    place-items: center;
    width: 32px;
    height: 32px;
    border: 0;
    border-radius: 10px;
    background: transparent;
  }
  .x:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  .sync {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 2px solid rgba(255, 255, 255, 0.2);
    border-top-color: var(--c1);
    animation: spin 0.9s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }

  .filters {
    display: grid;
    gap: 6px;
    padding: 0 12px 10px;
    border-bottom: 1px solid rgba(255, 255, 255, 0.07);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
  }
  .chips button {
    height: 26px;
    padding: 0 10px;
    border: 1px solid rgba(255, 255, 255, 0.1);
    border-radius: 999px;
    background: transparent;
    font-size: 12px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.78);
  }
  .chips button:hover {
    background: rgba(255, 255, 255, 0.07);
  }
  .chips button.on {
    border-color: transparent;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    color: #fff;
  }
  .kinds button {
    height: 24px;
    font-weight: 500;
  }

  .body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    padding: 4px 12px 10px;
    scrollbar-width: thin;
  }
  section {
    margin-top: 10px;
  }
  section header {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: 10px;
    margin: 0 2px 8px;
  }
  h3 {
    margin: 0;
    font-family: var(--font-lyrics);
    font-size: 15px;
    font-weight: 720;
    letter-spacing: -0.01em;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .src {
    flex: none;
    font-size: 11.5px;
    color: rgba(255, 255, 255, 0.55);
  }
  .src select {
    margin-left: 4px;
    padding: 2px 4px;
    border: 1px solid rgba(255, 255, 255, 0.15);
    border-radius: 7px;
    background: #1d1a26;
    font-size: 11.5px;
  }

  .grid {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 8px;
  }
  .card {
    display: grid;
    align-content: start;
    gap: 3px;
    min-width: 0;
    padding: 6px;
    border: 0;
    border-radius: 13px;
    background: transparent;
    text-align: left;
    transition: background 140ms ease;
  }
  .card:hover,
  .card:focus-visible {
    background: rgba(255, 255, 255, 0.08);
  }
  .card img,
  .card .ph {
    width: 100%;
    aspect-ratio: 1;
    object-fit: cover;
    border-radius: 9px;
    margin-bottom: 4px;
    background: rgba(255, 255, 255, 0.06);
    box-shadow: 0 8px 18px -10px rgba(0, 0, 0, 0.8);
  }
  .ph {
    display: grid;
    place-items: center;
    color: var(--c1);
    font-size: 22px;
  }
  .card strong,
  .card span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .card strong {
    font-size: 12.5px;
    font-weight: 650;
  }
  .card span {
    font-size: 11.5px;
    color: rgba(255, 255, 255, 0.6);
  }
  .more {
    margin: 4px 2px 0;
    padding: 4px 0;
    border: 0;
    background: transparent;
    font-size: 12px;
    font-weight: 600;
    color: color-mix(in oklab, var(--c1) 70%, white);
  }

  .rows {
    display: grid;
    gap: 2px;
  }
  .row {
    display: grid;
    grid-template-columns: 40px minmax(0, 1fr) auto;
    align-items: center;
    gap: 10px;
    padding: 6px;
    border: 0;
    border-radius: 11px;
    background: transparent;
    text-align: left;
  }
  .row:hover,
  .row:focus-visible {
    background: rgba(255, 255, 255, 0.08);
  }
  .row img,
  .row .ph {
    width: 40px;
    height: 40px;
    border-radius: 8px;
    object-fit: cover;
    background: rgba(255, 255, 255, 0.06);
  }
  .txt {
    display: grid;
    min-width: 0;
  }
  .txt strong,
  .txt span {
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
  .txt strong {
    font-size: 13px;
    font-weight: 650;
  }
  .txt span {
    font-size: 12px;
    color: rgba(255, 255, 255, 0.6);
  }
  .row small {
    font-size: 11px;
    color: rgba(255, 255, 255, 0.45);
  }

  .skeleton {
    display: grid;
    gap: 5px;
    padding: 6px;
  }
  .skeleton i,
  .skeleton b {
    display: block;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.07);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .skeleton i {
    aspect-ratio: 1;
  }
  .skeleton b {
    height: 9px;
    width: 80%;
  }
  .skeleton b + b {
    width: 55%;
  }
  @keyframes pulse {
    50% {
      opacity: 0.45;
    }
  }

  .note {
    margin: 10px 2px;
    font-size: 12.5px;
    line-height: 1.55;
    color: rgba(255, 255, 255, 0.65);
  }
  .services {
    display: grid;
    gap: 4px;
    margin-top: 14px;
    padding-top: 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
  }
  .svc {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 10px;
    font-size: 12px;
    color: rgba(255, 255, 255, 0.6);
  }
  .svc strong {
    color: rgba(255, 255, 255, 0.85);
    font-weight: 600;
  }
  .svc button,
  footer button {
    height: 26px;
    padding: 0 11px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    border-radius: 999px;
    background: transparent;
    font-size: 11.5px;
    font-weight: 600;
  }
  .svc button:hover,
  footer button:hover:not(:disabled) {
    background: rgba(255, 255, 255, 0.08);
  }
  footer button:disabled {
    opacity: 0.55;
    cursor: default;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    padding: 8px 14px 10px;
    border-top: 1px solid rgba(255, 255, 255, 0.07);
    font-size: 11px;
    color: rgba(255, 255, 255, 0.45);
  }

  @media (max-width: 520px) {
    .grid {
      grid-template-columns: repeat(3, minmax(0, 1fr));
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .sync,
    .skeleton i,
    .skeleton b {
      animation: none;
    }
  }
</style>
