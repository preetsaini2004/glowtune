<script lang="ts">
  // Glass mini player. The whole surface is a drag handle; the album's own
  // colors light it from the art outward.
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { invoke } from "@tauri-apps/api/core";
  import { np, formatTime } from "$lib/nowPlaying.svelte";

  let now = $state(Date.now());
  let scrubbing = $state<number | null>(null); // ms while dragging the bar
  let bar: HTMLDivElement | undefined = $state();

  const track = $derived(np.track);
  const duration = $derived(track?.durationMs ?? null);
  const position = $derived(scrubbing ?? np.positionAt(now));
  const progress = $derived(duration ? Math.min(1, position / duration) : 0);
  const palette = $derived(np.palette);

  onMount(() => {
    np.start();
    // Tick only while playing and visible: no work when paused or hidden.
    const id = setInterval(() => {
      if (np.playing && !document.hidden) now = Date.now();
    }, 250);
    return () => clearInterval(id);
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

<main
  class="player"
  class:idle={!track}
  style:--c1={palette[0]}
  style:--c2={palette[1] ?? palette[0]}
  data-tauri-drag-region
>
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
    border-radius: var(--radius);
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
    border-radius: 10px;
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
</style>
