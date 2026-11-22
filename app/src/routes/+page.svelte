<script lang="ts">
  // Glass mini player. The whole surface is a drag handle; the album's own
  // colors light it from the art outward.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { np, formatTime } from "$lib/nowPlaying.svelte";
  import { LyricsFollower } from "$lib/lyrics.svelte";
  import { showLyric } from "$lib/clean";
  import { Volume } from "$lib/volume.svelte";
  import { saveLyricCard } from "$lib/lyricCard";

  // Five looks (Settings → Player, or the little button on hover). Rust sizes
  // the window for each and drops the glass behind the round / capsule ones.
  //
  //   glass  art · title · progress · controls in a glass bar (the original)
  //   disc   the cover as a turning record, a progress ring round it,
  //          controls when you point at it
  //   card   a tall card: big cover, title, progress, controls
  //   pill   a slim capsule: spinning cover, title, play and next
  //   lyric  the line being sung, big, beside a small cover
  //   clear  no window at all: the cover, the song and the controls float
  //          on the desktop
  type Style = "glass" | "disc" | "card" | "pill" | "lyric" | "clear";
  const STYLES: Style[] = ["glass", "disc", "card", "pill", "lyric", "clear"];
  let style = $state<Style>("glass");
  const lyr = new LyricsFollower();
  // Volume: the speaker button opens a slider; the wheel works anywhere.
  const vol = new Volume();
  let volOpen = $state(false);
  $effect(() => {
    void np.track?.source;
    void np.track?.title;
    vol.read();
  });
  // Size: small / medium / large (Rust sizes the window to match).
  let size = $state<"s" | "m" | "l">("m");
  const ZOOM = { s: 0.84, m: 1, l: 1.22 };

  // The big cover for the looks that show the art large.
  let bigArt = $state<string | null>(null);
  const songKey = $derived(np.track ? `${np.track.title}\u001f${np.track.artist}` : "");
  $effect(() => {
    const key = songKey;
    bigArt = null;
    if (!key || (style !== "card" && style !== "disc")) return;
    const t = np.track;
    let stale = false;
    invoke<string | null>("hires_art", { title: t?.title ?? "", artist: t?.artist ?? "" })
      .then((u) => {
        if (!stale) bigArt = u;
      })
      .catch(() => {});
    return () => {
      stale = true;
    };
  });
  const art = $derived(bigArt ?? np.track?.artUrl ?? null);

  $effect(() => {
    void np.playback;
    void lyr.lyrics;
    lyr.schedule();
  });

  async function cycle() {
    type S = { playerStyle: string };
    const s = await invoke<S>("get_settings");
    const i = STYLES.indexOf(s.playerStyle as Style);
    s.playerStyle = STYLES[(i + 1) % STYLES.length];
    await invoke("set_settings", { new: s }).catch(() => {});
  }

  let now = $state(Date.now());
  let scrubbing = $state<number | null>(null); // ms while dragging the bar
  let bar: HTMLDivElement | undefined = $state();

  const track = $derived(np.track);
  const duration = $derived(track?.durationMs ?? null);
  const position = $derived(scrubbing ?? np.positionAt(now));
  const progress = $derived(duration ? Math.min(1, position / duration) : 0);
  const palette = $derived(np.palette);


  // Ctrl+Alt+C: a lyric card of the line being sung.
  async function makeCard() {
    try {
      await saveLyricCard(lyr);
    } catch (e) {
      invoke("toast", { icon: "card", text: e instanceof Error ? e.message : String(e) }).catch(() => {});
    }
  }

  onMount(() => {
    np.start();
    const stopLyrics = lyr.start();
    const unCard = listen<string>("make-lyric-card", (e) => {
      if (e.payload === getCurrentWindow().label) makeCard();
    });
    type S = { playerStyle?: string; playerSize?: string };
    const take = (s: S) => {
      if (STYLES.includes(s.playerStyle as Style)) style = s.playerStyle as Style;
      if (s.playerSize === "s" || s.playerSize === "m" || s.playerSize === "l") size = s.playerSize;
    };
    invoke<S>("get_settings").then(take).catch(() => {});
    const unlisten = listen<S>("settings", (e) => take(e.payload));
    // Tick only while playing and visible: no work when paused or hidden.
    const id = setInterval(() => {
      if (np.playing && !document.hidden) now = Date.now();
    }, 250);
    return () => {
      clearInterval(id);
      stopLyrics();
      unlisten.then((f) => f());
      unCard.then((f) => f());
    };
  });

  function msAt(clientX: number): number | null {
    if (!bar || !duration) return null;
    const r = bar.getBoundingClientRect();
    return Math.min(1, Math.max(0, (clientX - r.left) / r.width)) * duration;
  }

  function startScrub(e: PointerEvent) {
    if (!duration) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    scrubbing = msAt(e.clientX);
  }
  function moveScrub(e: PointerEvent) {
    if (scrubbing !== null) scrubbing = msAt(e.clientX);
  }
  function endScrub() {
    if (scrubbing !== null) np.seek(scrubbing);
    scrubbing = null;
  }
  function scrubKeys(e: KeyboardEvent) {
    if (!duration) return;
    const step = e.shiftKey ? 30_000 : 5_000;
    if (e.key === "ArrowRight") np.seek(Math.min(duration, position + step));
    if (e.key === "ArrowLeft") np.seek(Math.max(0, position - step));
  }

  function keys(e: KeyboardEvent) {
    if (e.key === "Escape") getCurrentWindow().close();
    if (e.key === " " && e.target === document.body) {
      e.preventDefault();
      np.control("playPause");
    }
  }
</script>

<svelte:window onkeydown={keys} />

{#snippet transport(big: boolean)}
  <button class="icon" aria-label="Previous" onclick={() => np.control("previous")}>
    <svg viewBox="0 0 24 24"><path d="M7 5.5v13M18.2 6.2 10 12l8.2 5.8z" /></svg>
  </button>
  <button class="icon play" class:big aria-label={np.playing ? "Pause" : "Play"} onclick={() => np.control("playPause")}>
    {#if np.playing}
      <svg viewBox="0 0 24 24"><path d="M8.5 6v12M15.5 6v12" /></svg>
    {:else}
      <svg viewBox="0 0 24 24"><path d="M8 5.8v12.4L18.4 12z" /></svg>
    {/if}
  </button>
  <button class="icon" aria-label="Next" onclick={() => np.control("next")}>
    <svg viewBox="0 0 24 24"><path d="M17 5.5v13M5.8 6.2 14 12l-8.2 5.8z" /></svg>
  </button>
{/snippet}

<main
  class="player s-{style}"
  class:alt={style !== "glass"}
  class:idle={!track}
  class:playing={np.playing}
  style:--c1={palette[0]}
  style:--c2={palette[1] ?? palette[0]}
  style:zoom={ZOOM[size]}
  style:height={`calc(100vh / ${ZOOM[size]})`}
  onwheel={(e) => vol.wheel(e)}
  data-tauri-drag-region
>
  {#if style !== "glass"}
    {#if !track}
      <div class="alt-empty" data-tauri-drag-region>
        <p class="title" data-tauri-drag-region>Nothing playing</p>
      </div>
    {:else if style === "disc"}
      <!-- The cover is the record; the ring round it is the song so far. -->
      <div class="disc-wrap" data-tauri-drag-region>
        <svg class="ring" viewBox="0 0 100 100" aria-hidden="true">
          <circle class="track-ring" cx="50" cy="50" r="47" />
          <circle class="done-ring" cx="50" cy="50" r="47" pathLength="1" style:stroke-dashoffset={1 - progress} />
        </svg>
        <div class="disc" data-tauri-drag-region>
          {#if art}<img src={art} alt="" draggable="false" data-tauri-drag-region />{:else}<div class="art-empty"></div>{/if}
          <i class="grooves" aria-hidden="true"></i>
        </div>
        <div class="disc-hover">
          <p class="title" title={track.title}>{track.title}</p>
          <div class="row">{@render transport(false)}</div>
          <p class="time">{formatTime(position)} / {formatTime(duration)}</p>
        </div>
      </div>
    {:else if style === "card"}
      <div class="card-light" aria-hidden="true"></div>
      <div class="card-art" data-tauri-drag-region>
        {#key art}
          {#if art}<img src={art} alt="" draggable="false" data-tauri-drag-region />{:else}<div class="art-empty"></div>{/if}
        {/key}
      </div>
      <div class="card-meta" data-tauri-drag-region>
        <p class="title" title={track.title} data-tauri-drag-region>{track.title}</p>
        <p class="artist" data-tauri-drag-region>{track.artist || track.source}</p>
      </div>
      <div class="card-bar">
        <span class="time">{formatTime(position)}</span>
        <div class="line"><i style:transform={`scaleX(${progress})`}></i></div>
        <span class="time">{formatTime(duration)}</span>
      </div>
      <div class="row card-controls">{@render transport(true)}</div>
    {:else if style === "pill"}
      <div class="pill-art" data-tauri-drag-region>
        {#if art}<img src={art} alt="" draggable="false" data-tauri-drag-region />{:else}<div class="art-empty"></div>{/if}
      </div>
      <div class="pill-meta" data-tauri-drag-region>
        <p class="title" title={track.title} data-tauri-drag-region>{track.title}</p>
        <p class="artist" data-tauri-drag-region>{track.artist || track.source}</p>
      </div>
      <div class="row">{@render transport(false)}</div>
      <div class="pill-line" aria-hidden="true"><i style:transform={`scaleX(${progress})`}></i></div>
    {:else if style === "clear"}
      <div class="clear-art" data-tauri-drag-region>
        {#key art}
          {#if art}<img src={art} alt="" draggable="false" data-tauri-drag-region />{:else}<div class="art-empty"></div>{/if}
        {/key}
      </div>
      <div class="clear-meta" data-tauri-drag-region>
        <p class="title" title={track.title} data-tauri-drag-region>{track.title}</p>
        <p class="artist" data-tauri-drag-region>{track.artist || track.source}</p>
        <div class="row clear-controls">{@render transport(false)}</div>
        <div class="clear-line" aria-hidden="true"><i style:transform={`scaleX(${progress})`}></i></div>
      </div>
    {:else}
      <div class="lyric-art" data-tauri-drag-region>
        {#if art}<img src={art} alt="" draggable="false" data-tauri-drag-region />{:else}<div class="art-empty"></div>{/if}
      </div>
      <div class="lyric-words" data-tauri-drag-region>
        {#if lyr.current && lyr.current.text}
          {#key lyr.index}
            <p class="sung" data-tauri-drag-region>{showLyric(lyr.current.text, np.cleanLyrics)}</p>
          {/key}
          <p class="coming" data-tauri-drag-region>{lyr.next ? showLyric(lyr.next.text, np.cleanLyrics) || "♪" : ""}</p>
        {:else}
          <p class="sung" data-tauri-drag-region>{track.title}</p>
          <p class="coming" data-tauri-drag-region>{track.artist || track.source}</p>
        {/if}
      </div>
      <button class="icon play" aria-label={np.playing ? "Pause" : "Play"} onclick={() => np.control("playPause")}>
        {#if np.playing}
          <svg viewBox="0 0 24 24"><path d="M8.5 6v12M15.5 6v12" /></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path d="M8 5.8v12.4L18.4 12z" /></svg>
        {/if}
      </button>
      <div class="lyric-line" aria-hidden="true"><i style:transform={`scaleX(${progress})`}></i></div>
    {/if}
  {:else}
  <div class="light" aria-hidden="true"></div>

  {#if track}
    <div class="art" data-tauri-drag-region>
      {#key track.artUrl}
        {#if track.artUrl}
          <img src={track.artUrl} alt="" draggable="false" onerror={(e) => ((e.currentTarget as HTMLImageElement).style.visibility = "hidden")} />
        {:else}
          <div class="art-empty"></div>
        {/if}
      {/key}
    </div>

    <div class="meta" data-tauri-drag-region>
      <p class="title" title={track.title} data-tauri-drag-region>{track.title}</p>
      <p class="artist" title={`${track.artist} on ${track.source}`} data-tauri-drag-region>
        {track.artist || track.source}
      </p>

      <div class="timeline">
        <div
          class="bar"
          bind:this={bar}
          role="slider"
          tabindex="0"
          aria-label="Position"
          aria-valuemin={0}
          aria-valuemax={duration ?? 0}
          aria-valuenow={Math.round(position)}
          aria-valuetext={formatTime(position)}
          onpointerdown={startScrub}
          onpointermove={moveScrub}
          onpointerup={endScrub}
          onpointercancel={endScrub}
          onkeydown={scrubKeys}
        >
          <div class="fill" class:live={np.playing && scrubbing === null} style:transform={`scaleX(${progress})`}></div>
          <div class="thumb" style:left={`${progress * 100}%`}></div>
        </div>
        <span class="time">{formatTime(position)}</span>
      </div>
    </div>

    <nav class="controls" aria-label="Playback">
      <button class="icon" aria-label="Previous" onclick={() => np.control("previous")}>
        <svg viewBox="0 0 24 24"><path d="M7 5.5v13M18.2 6.2 10 12l8.2 5.8z" /></svg>
      </button>
      <button class="icon play" aria-label={np.playing ? "Pause" : "Play"} onclick={() => np.control("playPause")}>
        {#if np.playing}
          <svg viewBox="0 0 24 24"><path d="M8.5 6v12M15.5 6v12" /></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path d="M8 5.8v12.4L18.4 12z" /></svg>
        {/if}
      </button>
      <button class="icon" aria-label="Next" onclick={() => np.control("next")}>
        <svg viewBox="0 0 24 24"><path d="M17 5.5v13M5.8 6.2 14 12l-8.2 5.8z" /></svg>
      </button>
    </nav>
  {:else}
    <div class="empty" data-tauri-drag-region>
      <p class="title" data-tauri-drag-region>Nothing playing</p>
      <p class="artist" data-tauri-drag-region>Play music anywhere and Glowtune follows it.</p>
      <div class="services">
        <button onclick={() => invoke("open_service", { service: "spotify" })}>Spotify</button>
        <button onclick={() => invoke("open_service", { service: "apple-music" })}>Apple Music</button>
        <button onclick={() => invoke("open_service", { service: "youtube-music" })}>YouTube Music</button>
        <button onclick={() => invoke("open_service", { service: "soundcloud" })}>SoundCloud</button>
      </div>
    </div>
  {/if}
  {/if}

  {#if track && vol.level !== null}
    <div class="vol" class:open={volOpen}>
      <button class="spk" aria-label={volOpen ? "Close volume" : "Volume"} aria-expanded={volOpen} onclick={() => (volOpen = !volOpen)}>
        <svg viewBox="0 0 24 24">
          <path class="fill" d="M4 9.5h3.5L12 6v12l-4.5-3.5H4z" />
          {#if vol.level > 0}<path d="M15.5 9.5a3.5 3.5 0 0 1 0 5" />{/if}
          {#if vol.level > 0.5}<path d="M18 7a7 7 0 0 1 0 10" />{/if}
        </svg>
      </button>
      {#if volOpen}
        <input
          type="range"
          min="0"
          max="1"
          step="0.01"
          value={vol.level}
          aria-label="Volume"
          style:--p={`${vol.level * 100}%`}
          oninput={(e) => vol.set(Number((e.currentTarget as HTMLInputElement).value))}
        />
        <span class="pct">{Math.round(vol.level * 100)}</span>
      {/if}
    </div>
  {/if}
  <button class="cycle" aria-label="Change the player's look" title="Change look" onclick={cycle}>
    <svg viewBox="0 0 24 24"><path d="M4 12a8 8 0 0 1 14-5.3M20 12a8 8 0 0 1-14 5.3M18 3v4h-4M6 21v-4h4" /></svg>
  </button>
  <button class="close" aria-label="Hide player" onclick={() => getCurrentWindow().close()}>
    <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7 7 17" /></svg>
  </button>
</main>

<style>
  .player {
    position: relative;
    height: 100vh;
    display: grid;
    grid-template-columns: 96px 1fr auto;
    align-items: center;
    gap: 18px;
    padding: 18px 20px 18px 18px;
    background: var(--tint);
    border-radius: 0; /* glass looks: the window has its own corners */
    overflow: hidden;
    isolation: isolate;
  }

  /* The song's light: a soft spill of the album colors from behind the art. */
  .light {
    position: absolute;
    inset: -40% auto -40% -18%;
    width: 75%;
    z-index: -1;
    background:
      radial-gradient(closest-side, color-mix(in oklab, var(--c1) 55%, transparent), transparent),
      radial-gradient(closest-side at 70% 70%, color-mix(in oklab, var(--c2) 35%, transparent), transparent);
    filter: blur(28px);
    opacity: 0.55;
    transition: opacity 1.2s ease, background 1.6s ease;
    pointer-events: none;
  }
  .idle .light {
    opacity: 0.25;
  }

  .art {
    position: relative;
    width: 96px;
    height: 96px;
    border-radius: calc(var(--radius) * 0.7);
    overflow: hidden;
    box-shadow:
      0 0 0 1px var(--hairline),
      0 10px 30px -8px color-mix(in oklab, var(--c1) 70%, transparent);
    transition: box-shadow 1.6s ease;
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
    opacity: 0.6;
  }

  .meta {
    min-width: 0;
    display: flex;
    flex-direction: column;
  }
  .title,
  .artist {
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .title {
    font-size: 16px;
    line-height: 22px;
    font-weight: 620;
    letter-spacing: -0.005em;
  }
  .artist {
    font-size: 13.5px;
    line-height: 19px;
    color: var(--ink-soft);
  }

  .timeline {
    display: flex;
    align-items: center;
    gap: 10px;
    margin-top: 14px;
  }
  .bar {
    position: relative;
    flex: 1;
    height: 16px;
    cursor: pointer;
    touch-action: none;
  }
  .bar::before {
    content: "";
    position: absolute;
    inset: 6px 0;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.13);
  }
  .fill {
    position: absolute;
    inset: 6px 0;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    transform-origin: left center;
  }
  .fill.live {
    transition: transform 250ms linear;
  }
  .thumb {
    position: absolute;
    top: 50%;
    width: 12px;
    height: 12px;
    margin: -6px 0 0 -6px;
    border-radius: 50%;
    background: var(--ink);
    box-shadow: 0 0 10px var(--c1);
    opacity: 0;
    transition: opacity 120ms ease;
  }
  .bar:hover .thumb,
  .bar:focus-visible .thumb,
  .bar:active .thumb {
    opacity: 1;
  }
  .time {
    min-width: 34px;
    font-size: 12px;
    color: var(--ink-faint);
    font-variant-numeric: tabular-nums;
    text-align: right;
  }

  .controls {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-right: 10px;
  }
  .icon {
    display: grid;
    place-items: center;
    width: 34px;
    height: 34px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
    transition: background 120ms ease, transform 120ms ease;
  }
  .icon:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .icon:active {
    transform: scale(0.92);
  }
  .icon svg {
    width: 20px;
    height: 20px;
    fill: currentColor;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .play {
    width: 42px;
    height: 42px;
    background: color-mix(in oklab, var(--c1) 30%, rgba(255, 255, 255, 0.1));
  }
  .play:hover {
    background: color-mix(in oklab, var(--c1) 45%, rgba(255, 255, 255, 0.12));
  }

  .empty {
    grid-column: 1 / -1;
    padding-left: 4px;
    max-width: 440px;
  }
  .empty .artist {
    white-space: normal;
    margin-top: 4px;
  }
  .services {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    margin-top: 10px;
  }
  .services button {
    height: 28px;
    padding: 0 9px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
    background: var(--surface);
    font-size: 11.5px;
    cursor: pointer;
  }
  .services button:hover {
    background: color-mix(in oklab, var(--c1) 25%, var(--surface));
  }

  .close {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: transparent;
    color: var(--ink-faint);
    cursor: pointer;
    opacity: 0;
    transition: opacity 150ms ease, background 150ms ease;
  }
  .player:hover .close,
  .close:focus-visible {
    opacity: 1;
  }
  .close:hover {
    background: rgba(255, 255, 255, 0.1);
    color: var(--ink);
  }
  .close svg {
    width: 12px;
    height: 12px;
    stroke: currentColor;
    stroke-width: 2.4;
    stroke-linecap: round;
    fill: none;
  }

  /* ── Volume (bottom-right; the wheel works anywhere) ─ */
  .vol {
    position: absolute;
    right: 6px;
    bottom: 6px;
    z-index: 6;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 2px;
    border-radius: 999px;
    opacity: 0.8;
    transition: opacity 150ms ease, background 150ms ease;
  }
  .player:hover .vol,
  .vol.open,
  .vol:focus-within {
    opacity: 1;
  }
  .vol.open {
    padding: 2px 10px 2px 2px;
    background: rgba(10, 9, 14, 0.82);
    box-shadow: 0 6px 20px -6px rgba(0, 0, 0, 0.7);
  }
  .spk {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: 1px solid rgba(255, 255, 255, 0.18);
    border-radius: 50%;
    background: color-mix(in oklab, var(--c1) 30%, rgba(10, 9, 14, 0.75));
    box-shadow: 0 2px 10px -2px rgba(0, 0, 0, 0.6);
    color: var(--ink);
    cursor: pointer;
  }
  .spk:hover {
    background: color-mix(in oklab, var(--c1) 50%, rgba(10, 9, 14, 0.75));
  }
  .spk svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .spk .fill {
    fill: currentColor;
  }
  .vol input {
    width: 96px;
    height: 4px;
    appearance: none;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--c1) var(--p), rgba(255, 255, 255, 0.18) var(--p));
    cursor: pointer;
  }
  .vol input::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 0 8px var(--c1);
  }
  .pct {
    min-width: 22px;
    font-size: 11px;
    color: var(--ink-soft);
    font-variant-numeric: tabular-nums;
  }

  /* Disc: the window's corners are see-through, so the volume sits inside
     the record, at the bottom of the circle. */
  .s-disc .vol {
    right: auto;
    left: 50%;
    bottom: 26px;
    translate: -50% 0;
  }
  /* The Pill's controls fill its right end: volume sits over the cover. */
  .s-pill .vol {
    left: 4px;
    right: auto;
    bottom: 2px;
  }

  /* ── Look switcher (top-left, on hover) ───────────── */
  .cycle {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 5;
    width: 22px;
    height: 22px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(0, 0, 0, 0.25);
    color: var(--ink-soft);
    cursor: pointer;
    opacity: 0;
    transition: opacity 150ms ease;
  }
  .close {
    z-index: 5;
  }
  .player:hover .cycle,
  .cycle:focus-visible {
    opacity: 1;
  }
  .cycle svg {
    width: 13px;
    height: 13px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  /* ── Shared bits of the new looks ─────────────────── */
  .alt {
    display: block;
    padding: 0;
    gap: 0;
  }
  .row {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
  }
  .play.big {
    width: 54px;
    height: 54px;
  }
  .play.big svg {
    width: 24px;
    height: 24px;
  }
  .alt-empty {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 0 16px;
    text-align: center;
  }
  @keyframes turn {
    to {
      rotate: 360deg;
    }
  }

  /* ── Disc: a turning record with a progress ring ──── */
  .s-disc {
    background: transparent;
    border-radius: 50%;
  }
  .s-disc .alt-empty {
    border-radius: 50%;
    background: radial-gradient(circle, rgba(20, 17, 28, 0.92) 60%, transparent 62%);
  }
  .disc-wrap {
    position: absolute;
    inset: 8px;
    border-radius: 50%;
  }
  .ring {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    rotate: -90deg;
    pointer-events: none;
  }
  .ring circle {
    fill: none;
    stroke-width: 2.4;
  }
  .track-ring {
    stroke: rgba(255, 255, 255, 0.16);
  }
  .done-ring {
    stroke: color-mix(in oklab, var(--c1) 70%, white);
    stroke-linecap: round;
    stroke-dasharray: 1;
    filter: drop-shadow(0 0 4px var(--c1));
    transition: stroke-dashoffset 250ms linear;
  }
  .disc {
    position: absolute;
    inset: 9px;
    border-radius: 50%;
    overflow: hidden;
    box-shadow: 0 10px 30px -8px rgba(0, 0, 0, 0.8), 0 0 40px -10px var(--c1);
  }
  .disc img,
  .disc .art-empty {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
    animation: turn 8s linear infinite;
    animation-play-state: paused;
  }
  .playing .disc img {
    animation-play-state: running;
  }
  .grooves {
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background:
      radial-gradient(circle, #0a090e 0 4%, rgba(255, 255, 255, 0.25) 4.4% 4.8%, transparent 5.2%),
      repeating-radial-gradient(circle, rgba(0, 0, 0, 0.14) 0 1px, transparent 1.5px 3px),
      conic-gradient(from 210deg, transparent 0 6%, rgba(255, 255, 255, 0.16) 12%, transparent 19% 51%, rgba(255, 255, 255, 0.1) 57%, transparent 64%);
    pointer-events: none;
  }
  .disc-hover {
    position: absolute;
    inset: 9px;
    border-radius: 50%;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 6px;
    padding: 0 26px;
    background: rgba(8, 7, 12, 0.62);
    opacity: 0;
    transition: opacity 180ms ease;
  }
  .disc-wrap:hover .disc-hover,
  .disc-hover:focus-within {
    opacity: 1;
  }
  .disc-hover .title {
    font-size: 13px;
    max-width: 100%;
    text-align: center;
  }
  .disc-hover .time {
    font-size: 11px;
    color: var(--ink-faint);
    font-variant-numeric: tabular-nums;
  }

  /* ── Card: tall, cover first ──────────────────────── */
  .s-card {
    display: flex;
    flex-direction: column;
    align-items: stretch;
    padding: 16px 16px 14px;
  }
  .card-light {
    position: absolute;
    inset: -20% -20% 30%;
    z-index: -1;
    background: radial-gradient(closest-side, color-mix(in oklab, var(--c1) 55%, transparent), transparent);
    filter: blur(30px);
    opacity: 0.6;
    pointer-events: none;
  }
  .card-art {
    position: relative;
    aspect-ratio: 1;
    border-radius: 12px;
    overflow: hidden;
    box-shadow: 0 0 0 1px var(--hairline), 0 18px 40px -14px color-mix(in oklab, var(--c1) 70%, black);
  }
  .card-art img,
  .card-art .art-empty {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .card-meta {
    margin-top: 14px;
    text-align: center;
  }
  .card-meta .title {
    font-size: 17px;
  }
  .card-bar {
    display: flex;
    align-items: center;
    gap: 9px;
    margin-top: 12px;
  }
  .card-bar .time {
    font-size: 11px;
    color: var(--ink-faint);
    font-variant-numeric: tabular-nums;
  }
  .line,
  .pill-line,
  .lyric-line {
    position: relative;
    flex: 1;
    height: 4px;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.13);
    overflow: hidden;
  }
  .line i,
  .pill-line i,
  .lyric-line i {
    position: absolute;
    inset: 0;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    transform-origin: left center;
    transition: transform 250ms linear;
  }
  .card-controls {
    margin-top: 8px;
    gap: 14px;
  }

  /* ── Pill: a slim capsule ─────────────────────────── */
  .s-pill {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0 12px 0 8px;
    border-radius: 999px;
    background: linear-gradient(100deg, color-mix(in oklab, var(--c1) 22%, rgba(16, 14, 22, 0.94)), rgba(16, 14, 22, 0.94) 55%);
    border: 1px solid rgba(255, 255, 255, 0.1);
  }
  .s-pill .alt-empty {
    width: 100%;
  }
  .pill-art {
    position: relative;
    flex: none;
    width: 50px;
    height: 50px;
    border-radius: 50%;
    overflow: hidden;
    box-shadow: 0 0 0 2px rgba(255, 255, 255, 0.12), 0 0 18px -4px var(--c1);
  }
  .pill-art img,
  .pill-art .art-empty {
    width: 100%;
    height: 100%;
    object-fit: cover;
    animation: turn 10s linear infinite;
    animation-play-state: paused;
  }
  .playing .pill-art img {
    animation-play-state: running;
  }
  .pill-meta {
    flex: 1;
    min-width: 0;
  }
  .pill-meta .title {
    font-size: 14px;
    line-height: 19px;
  }
  .pill-meta .artist {
    font-size: 12px;
    line-height: 16px;
  }
  .s-pill .icon {
    width: 32px;
    height: 32px;
  }
  .s-pill .play {
    width: 38px;
    height: 38px;
  }
  .pill-line {
    position: absolute;
    left: 66px;
    right: 26px;
    bottom: 5px;
    height: 2px;
  }

  /* ── Clear: no window, the song floats on the desktop ── */
  /* The colour fringe (red left, blue right) keeps it readable on any
     wallpaper, like a print slightly out of register. */
  .s-clear {
    display: flex;
    align-items: center;
    gap: 18px;
    padding: 12px 44px 12px 14px;
    background: transparent;
    --fringe: -1.5px 0 rgba(255, 46, 90, 0.75), 1.5px 0 rgba(46, 140, 255, 0.75), 0 2px 14px rgba(0, 0, 0, 0.65);
  }
  .clear-art {
    position: relative;
    flex: none;
    width: 92px;
    height: 92px;
  }
  .clear-art img,
  .clear-art .art-empty {
    display: block;
    width: 100%;
    height: 100%;
    object-fit: cover;
    box-shadow: -3px 0 0 rgba(255, 46, 90, 0.6), 3px 0 0 rgba(46, 140, 255, 0.6), 0 12px 30px -8px rgba(0, 0, 0, 0.7);
  }
  .clear-meta {
    flex: 1;
    min-width: 0;
  }
  .s-clear .title {
    font-size: 22px;
    line-height: 28px;
    font-weight: 700;
    color: #fff;
    text-shadow: var(--fringe);
  }
  .s-clear .artist {
    font-size: 14px;
    color: rgba(255, 255, 255, 0.85);
    text-shadow: var(--fringe);
  }
  .clear-controls {
    justify-content: flex-start;
    margin: 4px 0 0 -7px;
    color: #fff;
    opacity: 0.6;
    filter: drop-shadow(0 1px 4px rgba(0, 0, 0, 0.8));
    transition: opacity 150ms ease;
  }
  .player:hover .clear-controls,
  .clear-controls:focus-within {
    opacity: 1;
  }
  .clear-line {
    height: 2px;
    margin-top: 6px;
    width: 70%;
    background: rgba(255, 255, 255, 0.18);
    border-radius: 1px;
    overflow: hidden;
  }
  .clear-line i {
    display: block;
    height: 100%;
    background: #fff;
    transform-origin: left;
  }
  .s-clear .alt-empty .title {
    color: #fff;
    text-shadow: var(--fringe);
  }
  .s-clear .vol:not(.open) {
    opacity: 0.6;
  }

  /* ── Lyric: the sung line, big ────────────────────── */
  .s-lyric {
    display: flex;
    align-items: center;
    gap: 16px;
    padding: 14px 18px 14px 14px;
  }
  .lyric-art {
    position: relative;
    flex: none;
    width: 72px;
    height: 72px;
    border-radius: 10px;
    overflow: hidden;
    box-shadow: 0 0 0 1px var(--hairline), 0 8px 24px -8px var(--c1);
  }
  .lyric-art img,
  .lyric-art .art-empty {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .lyric-words {
    flex: 1;
    min-width: 0;
  }
  .sung,
  .coming {
    margin: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-family: var(--font-lyrics);
  }
  .sung {
    font-size: 21px;
    line-height: 28px;
    font-weight: 750;
    letter-spacing: -0.01em;
    text-shadow: 0 0 22px color-mix(in oklab, var(--c1) 55%, transparent);
    animation: rise 420ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
  }
  .coming {
    margin-top: 3px;
    font-size: 14px;
    line-height: 19px;
    color: var(--ink-faint);
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(8px);
    }
  }
  .lyric-line {
    position: absolute;
    left: 14px;
    right: 14px;
    bottom: 6px;
    height: 2px;
  }

  @media (prefers-reduced-motion: reduce) {
    .disc img,
    .pill-art img,
    .sung {
      animation: none;
    }
  }
</style>
