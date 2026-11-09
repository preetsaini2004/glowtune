<script lang="ts">
  // Lyrics for the desktop scene, in five looks:
  //
  //   flow     the whole song as a column; the sung line lit word by word,
  //            others dimmed and softly blurred by distance; gaps show dots
  //   bold     one big line + the next one, rising
  //   karaoke  one line filling with light left to right
  //   outline  hollow letters, the sung line glowing
  //   sticker  chunky tilted lines with a stacked color shadow
  //
  // Word timing: LRCLIB gives one time per line, so a line's time is spread
  // over its words by length. Reads like word sync; it's an estimate.
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import type { LyricsFollower, Line } from "./lyrics.svelte";
  import { np } from "./nowPlaying.svelte";
  import { showLyric } from "./clean";

  type Style = "flow" | "bold" | "karaoke" | "outline" | "sticker";
  let {
    lyr,
    style,
    rate = 1,
    playing = true,
    light = false,
  }: { lyr: LyricsFollower; style: Style; rate?: number; playing?: boolean; light?: boolean } = $props();

  const lines = $derived(lyr.lyrics.lines);
  const index = $derived(lyr.index);
  const lineMs = $derived(lyr.lineMs / (rate || 1));
  const elapsed = $derived(lyr.lineElapsedMs / (rate || 1));

  type Word = { text: string; delay: number; dur: number; hold: boolean };
  /**
   * The words of a line with when each lights up (ms from now) and for how long.
   * Real per-word times when the lyrics have them; otherwise the line's time
   * is spread over its words by length (sung part ≈ 85 % of the gap).
   */
  function words(line: Line, durMs: number, elapsedMs: number): Word[] {
    const r = rate || 1;
    if (line.words?.length) {
      const ws = line.words;
      const lineEnd = line.timeMs + durMs * r;
      return ws.map((w, i) => {
        const end = i + 1 < ws.length ? ws[i + 1].timeMs : Math.min(lineEnd, w.timeMs + 1500);
        const dur = Math.max(120, (end - w.timeMs) / r);
        return { text: showLyric(w.text, np.cleanLyrics), delay: (w.timeMs - line.timeMs) / r - elapsedMs, dur, hold: dur > 1100 };
      });
    }
    const text = showLyric(line.text, np.cleanLyrics);
    const parts = text.split(/(\s+)/).filter((p) => p.trim().length);
    const weights = parts.map((p) => p.length + 2);
    const total = weights.reduce((a, b) => a + b, 0) || 1;
    const sung = Math.min(durMs * 0.85, Math.max(900, parts.length * 420));
    let t = 0;
    return parts.map((p, i) => {
      const dur = (weights[i] / total) * sung;
      // Long, held words swell and glow while they're sung.
      const w = { text: p, delay: t - elapsedMs, dur, hold: dur > 1100 };
      t += dur;
      return w;
    });
  }

  // Flow: every line slides so the sung one sits at a fixed height. Each line
  // starts a beat after the one above it, so the column ripples into place.
  let column: HTMLDivElement | undefined = $state();
  let shift = $state(0);
  function measure() {
    if (style !== "flow" || !column) return;
    const el = column.querySelector<HTMLElement>(`[data-i="${Math.max(0, index)}"]`);
    if (el) shift = el.offsetTop + el.offsetHeight / 2;
  }
  $effect(() => {
    void index;
    void style;
    void lines;
    measure();
  });
  // Resizing the block changes the type size and every line's position:
  // re-measure so the sung line stays in place (else it slides out of view).
  $effect(() => {
    if (!column) return;
    const ro = new ResizeObserver(() => measure());
    ro.observe(column);
    if (column.parentElement) ro.observe(column.parentElement);
    return () => ro.disconnect();
  });

  // Mouse wheel: browse the whole song (everything sharp); snaps back when idle.
  let browse = $state(0);
  let browsing = $state(false);
  let browseTimer: ReturnType<typeof setTimeout> | undefined;
  function wheel(e: WheelEvent) {
    e.preventDefault();
    browsing = true;
    browse += e.deltaY;
    clearTimeout(browseTimer);
    browseTimer = setTimeout(() => {
      browsing = false;
      browse = 0;
    }, 2600);
  }
  /** Click a line: jump the song there. */
  function seekTo(line: Line) {
    clearTimeout(browseTimer);
    browsing = false;
    browse = 0;
    np.seek(Math.max(0, line.timeMs - lyr.lyrics.offsetMs));
  }
  function delay(d: number): string {
    if (browsing) return "0ms";
    return `${Math.min(12, Math.max(0, d + 2)) * 38}ms`;
  }

  /** An instrumental gap: an empty line, or the intro before the first line. */
  const gapMs = $derived.by(() => {
    if (index < 0 && lines.length) return lines[0].timeMs; // intro
    const cur = lines[index];
    return cur && cur.text === "" ? lineMs : 0;
  });

  const rise = { y: 40, duration: 560, easing: cubicOut };
  const sink = { y: -40, duration: 420, easing: cubicOut };
</script>

{#if style === "flow"}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div class="flow" class:paused={!playing} class:light class:browsing onwheel={wheel}>
    <div class="column" bind:this={column}>
      {#if index < 0 && gapMs > 3000}
        <p class="fl active intro" style:transform={`translateY(calc(38cqh - ${shift + browse}px))`}>
          <span class="dots"><span></span><span></span><span></span></span>
        </p>
      {/if}
      {#each lines as line, i (i)}
        {@const d = i - index}
        {@const near = Math.abs(d) <= 9 || browsing}
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
        <p
          class="fl"
          class:active={d === 0}
          class:far={!near}
          data-i={i}
          title={line.text ? "Play from here" : undefined}
          onclick={() => seekTo(line)}
          style:transform={`translateY(calc(38cqh - ${shift + browse}px))`}
          style:transition-delay={delay(d)}
          style:--blur={browsing ? "0px" : `${Math.min(4.5, Math.abs(d) * 1.1)}px`}
          style:--dim={d === 0 ? 1 : browsing ? 0.62 : Math.max(0.16, 0.52 - Math.abs(d) * 0.07)}
        >
          {#if d === 0 && line.text === ""}
            <span class="dots"><span></span><span></span><span></span></span>
          {:else if d === 0}
            {#key lyr.plan}
              {#each words(line, lineMs, elapsed) as w}
                <span class="w" class:hold={w.hold} style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`}>{w.text}</span>{" "}
              {/each}
            {/key}
          {:else}
            {showLyric(line.text, np.cleanLyrics) || "♪"}
          {/if}
        </p>
      {/each}
    </div>
  </div>
{:else}
  <div class="single {style}" class:paused={!playing}>
    {#key style === "karaoke" ? `${index}:${lyr.plan}` : index}
      <div class="pair" in:fly={rise} out:fly={sink}>
        {#if lyr.current && lyr.current.text !== ""}
          {#if style === "karaoke"}
            <p class="now">
              {#each words(lyr.current, lineMs, elapsed) as w}
                <span class="w" style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`}>{w.text}</span>{" "}
              {/each}
            </p>
          {:else}
            <p class="now" class:tilt={style === "sticker" && index % 2 === 1}>{showLyric(lyr.current.text, np.cleanLyrics)}</p>
          {/if}
        {:else}
          <p class="now gap"><span class="dots"><span></span><span></span><span></span></span></p>
        {/if}
        {#if lyr.next}<p class="after">{showLyric(lyr.next.text, np.cleanLyrics) || "♪"}</p>{/if}
      </div>
    {/key}
  </div>
{/if}

<style>
  p {
    margin: 0;
    font-family: var(--font-lyrics);
    text-wrap: balance;
  }

  /* ── Word light-up (flow + karaoke) ───────────────── */
  .w {
    display: inline-block;
    color: transparent;
    background: linear-gradient(90deg, #fff 45%, rgba(255, 255, 255, 0.32) 55%) 100% 0 / 220% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    animation:
      wipe var(--t) linear var(--d) both,
      lift calc(var(--t) + 380ms) cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both;
  }
  @keyframes wipe {
    to {
      background-position: 0 0;
    }
  }
  @keyframes lift {
    from {
      transform: translateY(0.06em);
      text-shadow: none;
    }
    to {
      transform: translateY(0);
    }
  }
  /* A held word swells a little and blooms while it rings out. */
  .w.hold {
    animation:
      wipe var(--t) linear var(--d) both,
      lift calc(var(--t) + 380ms) cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both,
      bloom var(--t) ease-in-out var(--d) both;
  }
  @keyframes bloom {
    0% {
      scale: 1;
      filter: drop-shadow(0 0 0 transparent);
    }
    55% {
      scale: 1.06;
      filter: drop-shadow(0 0 18px color-mix(in oklab, var(--c1) 80%, white));
    }
    100% {
      scale: 1.02;
      filter: drop-shadow(0 0 8px color-mix(in oklab, var(--c1) 50%, transparent));
    }
  }
  .paused .w,
  .paused .dots span {
    animation-play-state: paused;
  }

  /* ── Interlude dots ───────────────────────────────── */
  .dots {
    display: inline-flex;
    gap: 0.28em;
    align-items: center;
    height: 1em;
  }
  .dots span {
    width: 0.28em;
    height: 0.28em;
    border-radius: 50%;
    background: #fff;
    opacity: 0.25;
    animation: dot 1.6s ease-in-out infinite;
  }
  .dots span:nth-child(2) {
    animation-delay: 0.2s;
  }
  .dots span:nth-child(3) {
    animation-delay: 0.4s;
  }
  @keyframes dot {
    0%,
    100% {
      opacity: 0.25;
      transform: scale(0.85);
    }
    50% {
      opacity: 1;
      transform: scale(1.15);
    }
  }

  /* ── Flow ─────────────────────────────────────────── */
  .flow {
    position: relative;
    height: 100%;
    overflow: hidden;
    mask-image: linear-gradient(180deg, transparent 0, #000 16%, #000 72%, transparent 100%);
  }
  .column {
    position: relative;
  }
  .fl {
    font-size: var(--flow-size, clamp(30px, 2.9vw, 64px));
    line-height: 1.12;
    font-weight: 760;
    font-stretch: 90%;
    letter-spacing: -0.012em;
    padding: 0.3em 0;
    color: rgba(255, 255, 255, var(--dim));
    filter: blur(var(--blur));
    transform-origin: left center;
    cursor: pointer;
    transition:
      color 500ms ease,
      filter 500ms ease,
      opacity 500ms ease,
      transform 760ms cubic-bezier(0.2, 0.85, 0.25, 1);
  }
  /* Only lines near the sung one get a GPU layer and a blur; the rest of the
     song is out of rendering entirely (a song can have hundreds of lines). */
  .fl:not(.far) {
    will-change: transform;
  }
  .fl.far {
    opacity: 0;
    visibility: hidden;
    filter: none;
    pointer-events: none;
  }
  /* Over a playing video every filter is redrawn each video frame: fade only. */
  .light .fl,
  .light .fl.active .w {
    filter: none;
  }
  .light .w.hold {
    animation:
      wipe var(--t) linear var(--d) both,
      lift calc(var(--t) + 380ms) cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both;
  }
  .fl:not(.active):hover {
    color: rgba(255, 255, 255, 0.8);
    filter: none;
  }
  .browsing .fl {
    transition-duration: 500ms, 300ms, 300ms, 260ms;
  }
  .fl.active {
    filter: none;
    text-shadow: 0 0 34px color-mix(in oklab, var(--c1) 45%, transparent);
  }
  .fl.active .w {
    filter: drop-shadow(0 0 14px color-mix(in oklab, var(--c1) 55%, transparent));
  }

  /* ── Single-line looks ────────────────────────────── */
  .single {
    display: grid;
  }
  .pair {
    grid-area: 1 / 1;
  }
  .now {
    font-size: var(--line-size, clamp(38px, 4.4vw, 104px));
    line-height: 1.02;
    font-weight: 780;
    font-stretch: 78%;
    letter-spacing: -0.015em;
  }
  .now.gap {
    font-size: clamp(30px, 3vw, 64px);
  }
  .after {
    margin-top: 1.6vh;
    font-size: clamp(18px, 1.6vw, 36px);
    font-weight: 560;
    font-stretch: 88%;
    color: rgba(255, 255, 255, 0.46);
  }

  .bold .now {
    color: transparent;
    background: linear-gradient(180deg, #fff 35%, color-mix(in oklab, var(--c1) 45%, #fff) 100%);
    -webkit-background-clip: text;
    background-clip: text;
    filter: drop-shadow(0 0 26px color-mix(in oklab, var(--c1) 55%, transparent))
      drop-shadow(0 4px 18px rgba(0, 0, 0, 0.45));
  }

  .karaoke .now {
    filter: drop-shadow(0 4px 18px rgba(0, 0, 0, 0.45));
  }

  .outline .now {
    color: transparent;
    -webkit-text-stroke: 2px rgba(255, 255, 255, 0.92);
    font-weight: 700;
    font-stretch: 75%;
    filter: drop-shadow(0 0 12px color-mix(in oklab, var(--c1) 80%, transparent))
      drop-shadow(0 0 40px color-mix(in oklab, var(--c2) 50%, transparent));
  }
  .outline .after {
    color: transparent;
    -webkit-text-stroke: 1px rgba(255, 255, 255, 0.4);
  }

  .sticker .now {
    font-weight: 800;
    font-stretch: 75%;
    color: #fff;
    transform: rotate(-2.2deg);
    text-shadow:
      0.05em 0.05em 0 var(--c1),
      0.1em 0.1em 0 var(--c2),
      0.16em 0.16em 0.18em rgba(0, 0, 0, 0.45);
  }
  .sticker .now.tilt {
    transform: rotate(1.6deg);
  }
  .sticker .after {
    font-weight: 700;
    color: color-mix(in oklab, var(--c1) 50%, rgba(255, 255, 255, 0.6));
    transform: rotate(-1deg);
  }

  @media (prefers-reduced-motion: reduce) {
    .column,
    .fl {
      transition: none;
    }
  }
</style>
