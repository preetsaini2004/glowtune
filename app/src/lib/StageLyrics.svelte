<script lang="ts">
  // Lyrics for the desktop scene, in six looks:
  //
  //   flow     the whole song as a column; the sung line lit word by word,
  //            others dimmed and softly blurred by distance; gaps show dots
  //   bold     one big line + the next one, rising
  //   karaoke  one line filling with light left to right
  //   outline  hollow letters, the sung line glowing
  //   sticker  chunky tilted lines with a stacked color shadow
  //   stars    each line a constellation that draws itself as it's sung:
  //            words are stars, a thread runs to each word as it's reached,
  //            the sung word swells under a sparkle in the album's color
  //   depth    the sung line sharp in front; the last few lines fall back
  //            into the dark behind it, tilted, smaller and out of focus,
  //            each in its own cut of the type (a collage)
  //   rain     each word drops in as it's sung and lands in its place;
  //            earlier lines stack up above, dimmer
  //   orbit    the line is a ring of words around a glowing orb; the ring
  //            turns so the sung word always faces you
  //
  // Word timing: LRCLIB gives one time per line, so a line's time is spread
  // over its words by length. Reads like word sync; it's an estimate.
  import { fade, fly } from "svelte/transition";
  import { flip } from "svelte/animate";
  import { cubicOut } from "svelte/easing";
  import type { LyricsFollower, Line } from "./lyrics.svelte";
  import { np } from "./nowPlaying.svelte";
  import { showLyric } from "./clean";
  import { emojiFor } from "./emoji";
  import EmojiBurst from "./EmojiBurst.svelte";

  type Style = "flow" | "bold" | "karaoke" | "outline" | "sticker" | "stars" | "depth" | "rain" | "orbit";
  let {
    lyr,
    style,
    rate = 1,
    playing = true,
    light = false,
    emoji: emojiOn = false,
  }: { lyr: LyricsFollower; style: Style; rate?: number; playing?: boolean; light?: boolean; emoji?: boolean } = $props();

  const lines = $derived(lyr.lyrics.lines);
  const index = $derived(lyr.index);
  const lineMs = $derived(lyr.lineMs / (rate || 1));
  const elapsed = $derived(lyr.lineElapsedMs / (rate || 1));

  /** `emoji`: the word has one and Emoji is on (it bursts out as it's sung). */
  type Word = { text: string; delay: number; dur: number; hold: boolean; emoji?: string };
  const emojiOf = (text: string) => (emojiOn ? emojiFor(text) : undefined);
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
        const text = showLyric(w.text, np.cleanLyrics);
        return { text, delay: (w.timeMs - line.timeMs) / r - elapsedMs, dur, hold: dur > 1100, emoji: emojiOf(text) };
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
      const w = { text: p, delay: t - elapsedMs, dur, hold: dur > 1100, emoji: emojiOf(p) };
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

  // ── Stars ──────────────────────────────────────────────────────────────
  // A line's words become a small constellation. Positions come from the line
  // number, so a line always draws the same shape (and a replay matches).
  //
  //   Scattered across the whole box like a real constellation: each word
  //   takes the best of a dozen random spots (clear of the other stars, the
  //   wide labels getting extra room sideways, and not too far from the
  //   word before), so the thread zig-zags in every direction.
  type Star = Word & { x: number; y: number; strand: number };
  function seeded(seed: number) {
    let s = seed >>> 0 || 1;
    return () => {
      s = (s + 0x6d2b79f5) >>> 0;
      let t = Math.imul(s ^ (s >>> 15), 1 | s);
      t ^= t + Math.imul(t ^ (t >>> 7), 61 | t);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  function constellation(line: Line, i: number, durMs: number, elapsedMs: number): Star[] {
    const ws = words(line, durMs, elapsedMs);
    const n = ws.length;
    const rnd = seeded(i * 7919 + line.text.length * 31 + 17);
    const W = skyW || 800;
    const H = skyH || 500;
    // The step the thread should take: the box shared out between the words.
    const step = Math.max(80, Math.sqrt((W * H) / Math.max(1, n)) * 0.9);
    const pts: { x: number; y: number }[] = [];
    for (let k = 0; k < n; k++) {
      let best = { x: W / 2, y: H / 2 };
      let bestScore = -Infinity;
      for (let c = 0; c < 14; c++) {
        const x = W * (0.08 + rnd() * 0.84);
        const y = H * (0.14 + rnd() * 0.72);
        // Labels are wide: sideways distance counts for less.
        let clear = Infinity;
        for (const p of pts) clear = Math.min(clear, Math.hypot((x - p.x) * 0.55, y - p.y));
        const prev = pts[pts.length - 1];
        const far = prev ? Math.abs(Math.hypot(x - prev.x, y - prev.y) - step) : 0;
        const score = (pts.length ? Math.min(clear, step) : 0) - far * 0.45;
        if (score > bestScore) {
          bestScore = score;
          best = { x, y };
        }
      }
      pts.push(best);
    }
    return ws.map((w, k) => ({ ...w, x: (pts[k].x / W) * 100, y: (pts[k].y / H) * 100, strand: 0 }));
  }

  /** Twinkles for instrumental gaps (fixed places, so they don't jump). */
  const twinkles = Array.from({ length: 7 }, (_, k) => {
    const r = seeded(k * 104729 + 7);
    return { x: 10 + r() * 80, y: 14 + r() * 72, d: Math.round(r() * 2400), s: 0.6 + r() * 0.8 };
  });
  let skyW = $state(0);
  let skyH = $state(0);
  const drift = (_: Element) => ({
    duration: 700,
    easing: cubicOut,
    css: (t: number, u: number) => `opacity:${t * t * t}; transform: translate(${-u * 5}%, ${-u * 7}%) scale(${1 - u * 0.06})`,
  });
  const form = (_: Element) => ({
    duration: 520,
    easing: cubicOut,
    css: (t: number) => `opacity:${t}; transform: scale(${0.97 + t * 0.03})`,
  });

  // ── Depth ──────────────────────────────────────────────────────────────
  // The sung line plus up to three before it. A line keeps its element as it
  // ages, so moving back is one CSS transition (no per-frame work).
  //
  //   age 0  centre, sharp, words come forward as they're sung
  //   age n  pushed back n steps: off to one side, tilted, smaller, blurred
  const DEPTH_BACK = 3;
  const depthLines = $derived.by(() => {
    const out: { i: number; age: number }[] = [];
    for (let a = DEPTH_BACK; a >= 0; a--) {
      const i = index - a;
      if (i >= 0 && lines[i]?.text) out.push({ i, age: a });
    }
    return out;
  });
  /** Where an aged line rests (fixed per line, so it never jumps sides). */
  function depthPlace(i: number, age: number): string {
    if (age === 0) return "translate3d(0, 0, 0) rotate(0deg) scale(1)";
    const r = seeded(i * 4111 + 3);
    const side = r() < 0.5 ? -1 : 1;
    const up = i % 2 ? -1 : 1;
    const x = side * (5 + age * 6 + r() * 7);
    const y = up * (17 + age * 8 + r() * 5);
    const turn = side * (2 + r() * 5);
    return `translate3d(${x}cqw, ${y}cqh, ${-55 * age}px) rotate(${turn}deg) scale(${1 - age * 0.13})`;
  }
  const comeForward = (_: Element) => ({
    duration: 640,
    easing: cubicOut,
    css: (t: number, u: number) => `opacity:${t}; scale:${1 + u * 0.12}`,
  });
  const fadeBack = (_: Element) => ({ duration: 500, css: (t: number) => `opacity:${t * 0.1}` });

  // ── Rain ───────────────────────────────────────────────────────────────
  /** The last two lines above the sung one (they stack and dim). */
  const rainLines = $derived.by(() => {
    const out: number[] = [];
    for (let i = Math.max(0, index - 2); i <= index; i++) if (lines[i]?.text) out.push(i);
    return out;
  });
  /** Each word falls in at its own slight angle. */
  function tilt(i: number, k: number): string {
    const r = seeded(i * 131 + k * 7 + 1);
    return `${(r() - 0.5) * 26}deg`;
  }

  // ── Orbit ──────────────────────────────────────────────────────────────
  // Words sit on a ring (each gets arc in proportion to its length). The
  // ring turns on the compositor via one Web Animation whose keyframes land
  // each word at the front when it's sung; nothing runs per frame in JS.
  type Ring = { ws: Word[]; angles: number[]; radius: number; size: number };
  function ring(line: Line, durMs: number, elapsedMs: number): Ring {
    const ws = words(line, durMs, elapsedMs);
    const w = skyW || 600;
    const h = skyH || 400;
    const widths = ws.map((x) => x.text.length * 0.56 + 0.9); // in em
    const total = widths.reduce((a, b) => a + b, 0) || 1;
    // Big enough to read, small enough that the words fit round the ring.
    // Kept modest: the camera sits ~3.4 radii away, so the front word reads
    // about 1.4× larger and the ring stays inside the block.
    const radius = Math.max(110, Math.min(w * 0.3, h * 0.42));
    const size = Math.max(14, Math.min(h * 0.062, (2 * Math.PI * radius * 0.85) / total));
    let acc = 0;
    const angles = widths.map((x) => {
      const mid = acc + x / 2;
      acc += x;
      return (mid / total) * 360;
    });
    return { ws, angles, radius, size };
  }
  function spin(node: HTMLElement, p: { r: Ring; playing: boolean }) {
    const cam = Math.round(p.r.radius * 3.4);
    // Tilted like a planet's ring: leaning sideways, seen a little from above.
    const turn = (deg: number) => `perspective(${cam}px) rotateZ(-14deg) rotateX(-20deg) rotateY(${-deg}deg)`;
    let anim: Animation | undefined;
    const { ws, angles } = p.r;
    if (ws.length > 1) {
      const t0 = ws[0].delay;
      const last = ws[ws.length - 1];
      const total = Math.max(1, last.delay + last.dur - t0);
      const frames: Keyframe[] = [{ offset: 0, transform: turn(angles[0]) }];
      let prev = 0;
      for (let k = 1; k < ws.length; k++) {
        const at = Math.min(1, Math.max(prev, (ws[k].delay - t0) / total));
        const lead = Math.min(340, (ws[k].delay - ws[k - 1].delay) * 0.7) / total;
        const from = Math.max(prev, at - lead);
        frames.push({ offset: from, transform: turn(angles[k - 1]), easing: "cubic-bezier(0.3, 0.6, 0.25, 1)" });
        frames.push({ offset: at, transform: turn(angles[k]) });
        prev = at;
      }
      anim = node.animate(frames, { duration: total, delay: t0, fill: "both" });
      if (!p.playing) anim.pause();
    } else node.style.transform = turn(angles[0] ?? 0);
    return {
      update(n: { r: Ring; playing: boolean }) {
        if (!anim) return;
        if (!n.playing) anim.pause();
        else if (anim.playState === "paused") anim.play();
      },
      destroy() {
        // Keep the pose while the line drifts away (cancel would snap it flat).
        anim?.pause();
      },
    };
  }
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
                <span class="wb"><span class="w" class:hold={w.hold} style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`}>{w.text}</span>{#if w.emoji}<EmojiBurst emoji={w.emoji} delay={w.delay} seed={index * 31 + w.text.length} />{/if}</span>{" "}
              {/each}
            {/key}
          {:else}
            {showLyric(line.text, np.cleanLyrics) || "♪"}
          {/if}
        </p>
      {/each}
    </div>
  </div>
{:else if style === "stars"}
  <div class="stars" class:paused={!playing} class:light bind:clientWidth={skyW} bind:clientHeight={skyH}>
    {#key index}
      <div class="sky" in:form out:drift>
        {#if lyr.current && lyr.current.text !== ""}
          {#key lyr.plan}
            {@const stars = constellation(lyr.current, index, lineMs, elapsed)}
            <svg class="threads" viewBox={`0 0 ${skyW || 1} ${skyH || 1}`} aria-hidden="true">
              {#each stars as s, k}
                {#if k > 0 && stars[k - 1].strand === s.strand}
                  {@const p = stars[k - 1]}
                  <line
                    x1={(p.x * skyW) / 100}
                    y1={(p.y * skyH) / 100}
                    x2={(s.x * skyW) / 100}
                    y2={(s.y * skyH) / 100}
                    pathLength="1"
                    style:--d={`${s.delay - 320}ms`}
                  />
                {/if}
              {/each}
            </svg>
            {#each stars as s}
              <span class="star" class:hold={s.hold} style:left={`${s.x}%`} style:top={`${s.y}%`} style:--d={`${s.delay}ms`} style:--t={`${s.dur}ms`}>
                {#if s.emoji}
                  <!-- The word's emoji takes the sparkle's place, and bursts. -->
                  <span class="spark emo" aria-hidden="true">{s.emoji}</span>
                  <span class="star-burst"><EmojiBurst emoji={s.emoji} delay={s.delay} seed={index * 31 + s.text.length} /></span>
                {:else}
                  <svg class="spark" viewBox="-10 -10 20 20" aria-hidden="true">
                    <path d="M0 -10 C0.9 -2.4 2.4 -0.9 10 0 C2.4 0.9 0.9 2.4 0 10 C-0.9 2.4 -2.4 0.9 -10 0 C-2.4 -0.9 -0.9 -2.4 0 -10Z" />
                  </svg>
                {/if}
                <i class="dot"></i>
                <b class="word">{s.text}</b>
              </span>
            {/each}
          {/key}
        {:else}
          {#each twinkles as t}
            <i class="twinkle" style:left={`${t.x}%`} style:top={`${t.y}%`} style:--d={`${-t.d}ms`} style:--s={t.s}></i>
          {/each}
        {/if}
      </div>
    {/key}
  </div>
{:else if style === "depth"}
  <div class="depth" class:paused={!playing} class:light>
    {#each depthLines as d (d.i)}
      <p
        class="dl v{d.i % 4}"
        class:now={d.age === 0}
        style:transform={depthPlace(d.i, d.age)}
        style:--blur={`${d.age * 1.3}px`}
        style:--o={[1, 0.34, 0.2, 0.11][d.age]}
        in:comeForward
        out:fadeBack
      >
        {#if d.age === 0}
          {#key lyr.plan}
            {#each words(lines[d.i], lineMs, elapsed) as w}
              <span class="wb"><span class="dw" class:hold={w.hold} style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`}>{w.text}</span>{#if w.emoji}<EmojiBurst emoji={w.emoji} delay={w.delay} seed={index * 31 + w.text.length} />{/if}</span>{" "}
            {/each}
          {/key}
        {:else}
          {showLyric(lines[d.i].text, np.cleanLyrics)}
        {/if}
      </p>
    {/each}
    {#if !lyr.current || lyr.current.text === ""}
      <p class="dl now gap"><span class="dots"><span></span><span></span><span></span></span></p>
    {/if}
  </div>
{:else if style === "rain"}
  <div class="rain" class:paused={!playing} class:light>
    {#each rainLines as i (i)}
      <p class="rl" class:now={i === index} animate:flip={{ duration: 560, easing: cubicOut }} in:fade={{ duration: 200 }} out:fly={{ y: -30, duration: 420 }}>
        {#if i === index}
          {#key lyr.plan}
            {#each words(lines[i], lineMs, elapsed) as w, k}
              <span class="wb"><span class="drop" class:hold={w.hold} style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`} style:--r={tilt(i, k)}>{w.text}</span>{#if w.emoji}<EmojiBurst emoji={w.emoji} delay={w.delay} seed={index * 31 + w.text.length} />{/if}</span>{" "}
            {/each}
          {/key}
        {:else}
          {showLyric(lines[i].text, np.cleanLyrics)}
        {/if}
      </p>
    {/each}
    {#if !lyr.current || lyr.current.text === ""}
      <p class="rl now gap"><span class="dots"><span></span><span></span><span></span></span></p>
    {/if}
  </div>
{:else if style === "orbit"}
  <div class="orbit" class:paused={!playing} class:light bind:clientWidth={skyW} bind:clientHeight={skyH}>
    <div class="orb"></div>
    {#key index}
      <div class="sky" in:form out:drift>
        {#if lyr.current && lyr.current.text !== ""}
          {#key lyr.plan}
            {@const r = ring(lyr.current, lineMs, elapsed)}
            <div class="ring" use:spin={{ r, playing }} style:--R={`${r.radius}px`} style:--fs={`${r.size}px`}>
              <i class="path"></i>
              {#each r.ws as w, k}
                <span class="ow" class:hold={w.hold} style:--a={`${r.angles[k]}deg`} style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`}><b>{w.text}</b>{#if w.emoji}<EmojiBurst emoji={w.emoji} delay={w.delay} seed={index * 31 + w.text.length} />{/if}</span>
              {/each}
            </div>
          {/key}
        {/if}
      </div>
    {/key}
  </div>
{:else}
  <div class="single {style}" class:paused={!playing}>
    <!-- Only a new line flies in. A re-plan (any playback report) just restarts
         the karaoke fill in place; keying the fly on it stacked two copies. -->
    {#key index}
      <div class="pair" in:fly={rise} out:fly={sink}>
        {#if lyr.current && lyr.current.text !== ""}
          {#if style === "karaoke"}
            <p class="now">
              {#key lyr.plan}
                {#each words(lyr.current, lineMs, elapsed) as w}
                  <span class="wb"><span class="w" style:--d={`${w.delay}ms`} style:--t={`${w.dur}ms`}>{w.text}</span>{#if w.emoji}<EmojiBurst emoji={w.emoji} delay={w.delay} seed={index * 31 + w.text.length} />{/if}</span>{" "}
                {/each}
              {/key}
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
  /* Holds a word and its emoji burst (the burst starts at the word's top). */
  .wb {
    display: inline-block;
    position: relative;
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

  /* ── Stars ────────────────────────────────────────── */
  /* Three sizes per word: still to come (a pinprick with a faint name),
     being sung (big, under a sparkle), sung (a settled star). */
  .stars {
    --small: clamp(12px, 3.3cqh, 22px);
    --mid: clamp(13px, 3.9cqh, 26px);
    --big: clamp(28px, 9.5cqh, 80px);
    --glow: color-mix(in oklab, var(--c1) 70%, white);
    position: relative;
    width: 100%;
    height: 100%;
    min-height: clamp(260px, 46vh, 580px);
    container-type: size;
    font-family: var(--font-lyrics);
  }
  .sky {
    position: absolute;
    inset: 0;
  }
  .threads {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    overflow: visible;
  }
  /* A thread runs to each word just before it's sung. */
  .threads line {
    stroke: color-mix(in oklab, var(--c2) 35%, white);
    stroke-opacity: 0.5;
    stroke-width: 1.1;
    stroke-linecap: round;
    stroke-dasharray: 1;
    stroke-dashoffset: 1;
    animation: thread 320ms cubic-bezier(0.3, 0.7, 0.3, 1) var(--d) both;
  }
  @keyframes thread {
    to {
      stroke-dashoffset: 0;
    }
  }
  .star {
    position: absolute;
    width: 0;
    height: 0;
  }
  .dot {
    position: absolute;
    left: -2.5px;
    top: -2.5px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: #fff;
    opacity: 0.35;
    animation:
      dot-lit 260ms ease-out var(--d) both,
      dot-rest 900ms ease calc(var(--d) + var(--t)) forwards;
  }
  @keyframes dot-lit {
    to {
      opacity: 1;
      box-shadow: 0 0 10px 3px var(--glow);
    }
  }
  @keyframes dot-rest {
    from {
      opacity: 1;
      box-shadow: 0 0 10px 3px var(--glow);
    }
    to {
      opacity: 0.85;
      box-shadow: 0 0 4px 1px color-mix(in oklab, var(--glow) 50%, transparent);
    }
  }
  .word {
    position: absolute;
    left: 0;
    top: 0;
    transform: translate(-50%, 0.35em);
    white-space: nowrap;
    font-size: var(--small);
    font-weight: 560;
    letter-spacing: -0.01em;
    color: #fff;
    opacity: 0.3;
    animation:
      word-lit 300ms cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both,
      word-rest 700ms ease calc(var(--d) + var(--t)) forwards;
  }
  @keyframes word-lit {
    to {
      font-size: var(--big);
      font-weight: 720;
      opacity: 1;
      text-shadow: 0 0 28px color-mix(in oklab, var(--glow) 55%, transparent);
    }
  }
  @keyframes word-rest {
    from {
      font-size: var(--big);
      font-weight: 720;
      opacity: 1;
      text-shadow: 0 0 28px color-mix(in oklab, var(--glow) 55%, transparent);
    }
    to {
      font-size: var(--mid);
      opacity: 0.72;
    }
  }
  /* A held word rings out a little larger. */
  .star.hold {
    --big: clamp(34px, 11.5cqh, 96px);
  }
  /* The sparkle sits on the star while its word is sung. */
  .spark {
    position: absolute;
    left: 0;
    top: 0;
    width: clamp(34px, 10cqh, 84px);
    height: clamp(34px, 10cqh, 84px);
    translate: -50% -50%;
    fill: var(--glow);
    filter: drop-shadow(0 0 8px var(--glow));
    opacity: 0;
    animation: spark var(--t) ease-in-out var(--d) both;
  }
  @keyframes spark {
    0% {
      opacity: 0;
      transform: scale(0.3) rotate(-30deg);
    }
    18% {
      opacity: 1;
      transform: scale(1) rotate(0deg);
    }
    80% {
      opacity: 0.9;
      transform: scale(0.92) rotate(12deg);
    }
    100% {
      opacity: 0;
      transform: scale(0.5) rotate(20deg);
    }
  }
  /* A word with an emoji: the emoji is its sparkle. */
  .spark.emo {
    display: grid;
    place-items: center;
    font-size: clamp(26px, 8cqh, 64px);
    line-height: 1;
    font-style: normal;
    filter: drop-shadow(0 0 10px color-mix(in oklab, var(--glow) 60%, transparent));
  }
  .star-burst {
    position: absolute;
    left: 0;
    top: 0;
    font-size: clamp(24px, 7cqh, 56px);
  }
  /* Instrumental gaps: a few stars breathing. */
  .twinkle {
    position: absolute;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: #fff;
    scale: var(--s);
    animation: twinkle 3.2s ease-in-out var(--d) infinite;
  }
  @keyframes twinkle {
    0%,
    100% {
      opacity: 0.12;
    }
    50% {
      opacity: 0.8;
      box-shadow: 0 0 8px 2px var(--glow);
    }
  }
  .stars.paused * {
    animation-play-state: paused !important;
  }
  /* Over a music video: a soft shadow keeps the words readable. */
  .stars.light .word {
    filter: drop-shadow(0 2px 10px rgba(0, 0, 0, 0.75));
  }
  @media (prefers-reduced-motion: reduce) {
    .spark,
    .twinkle {
      animation: none;
      opacity: 0;
    }
  }

  /* ── Depth ────────────────────────────────────────── */
  .depth {
    --glow: color-mix(in oklab, var(--c1) 70%, white);
    position: relative;
    width: 100%;
    height: 100%;
    min-height: clamp(260px, 46vh, 580px);
    container-type: size;
    perspective: 900px;
  }
  .dl {
    position: absolute;
    left: 3%;
    right: 3%;
    top: 50%;
    translate: 0 -50%;
    text-align: center;
    font-size: clamp(26px, min(10cqh, 7.2cqw), 92px);
    line-height: 1.04;
    color: color-mix(in oklab, var(--c2) 35%, white);
    opacity: var(--o);
    filter: blur(var(--blur));
    transition:
      transform 950ms cubic-bezier(0.2, 0.8, 0.2, 1),
      opacity 700ms ease,
      filter 700ms ease,
      color 700ms ease;
    will-change: transform;
  }
  .dl.now {
    color: #fff;
    text-shadow: 0 0 36px color-mix(in oklab, var(--c1) 45%, transparent);
  }
  /* Four cuts of the same family, so a stack of lines reads as a collage. */
  .dl.v0 {
    font-weight: 820;
    font-stretch: 75%;
    text-transform: uppercase;
    letter-spacing: 0.005em;
  }
  .dl.v1 {
    font-weight: 300;
    font-style: oblique 10deg;
    letter-spacing: -0.02em;
  }
  .dl.v2 {
    font-weight: 680;
    font-stretch: 88%;
    letter-spacing: -0.015em;
  }
  .dl.v3 {
    font-size: clamp(22px, min(8cqh, 5.8cqw), 74px);
    font-weight: 440;
    letter-spacing: 0.1em;
    text-transform: lowercase;
  }
  .dw {
    display: inline-block;
    animation: emerge 480ms cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both;
  }
  .dw.hold {
    animation:
      emerge 480ms cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both,
      ring-out var(--t) ease-in-out var(--d) both;
  }
  @keyframes emerge {
    from {
      opacity: 0.14;
      filter: blur(6px);
      scale: 0.8;
    }
    to {
      opacity: 1;
      filter: blur(0);
      scale: 1;
    }
  }
  @keyframes ring-out {
    50% {
      text-shadow: 0 0 30px var(--glow);
    }
  }
  .light .dl,
  .light .dw {
    filter: none !important;
  }
  .light .dl.now {
    text-shadow: 0 2px 14px rgba(0, 0, 0, 0.7);
  }

  /* ── Rain ─────────────────────────────────────────── */
  .rain {
    --glow: color-mix(in oklab, var(--c1) 70%, white);
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 0.5em;
    height: 100%;
    min-height: clamp(260px, 46vh, 580px);
    padding-bottom: 3%;
    overflow: hidden;
    container-type: size;
    mask-image: linear-gradient(180deg, transparent 0, #000 28%);
  }
  .rl {
    font-size: clamp(16px, 4.6cqh, 34px);
    font-weight: 620;
    font-stretch: 90%;
    line-height: 1.1;
    color: rgba(255, 255, 255, 0.34);
    transition:
      font-size 560ms cubic-bezier(0.2, 0.8, 0.2, 1),
      color 560ms ease;
  }
  .rl.now {
    margin-top: 0.14em;
    font-size: clamp(28px, min(10cqh, 7cqw), 88px);
    font-weight: 800;
    font-stretch: 82%;
    letter-spacing: -0.012em;
    line-height: 1.04;
    color: #fff;
  }
  /* A word starts falling ~half a second early so it lands as it's sung. */
  .drop {
    display: inline-block;
    animation:
      fall 620ms linear calc(var(--d) - 510ms) both,
      splash 900ms ease-out var(--d) both;
  }
  .drop.hold {
    animation:
      fall 620ms linear calc(var(--d) - 510ms) both,
      splash var(--t) ease-out var(--d) both;
  }
  @keyframes fall {
    0% {
      opacity: 0;
      transform: translateY(-70cqh) rotate(var(--r));
      animation-timing-function: cubic-bezier(0.55, 0, 0.9, 0.55);
    }
    14% {
      opacity: 1;
    }
    82% {
      transform: translateY(0.06em) rotate(calc(var(--r) * -0.12));
      animation-timing-function: cubic-bezier(0.2, 0.7, 0.4, 1);
    }
    92% {
      transform: translateY(-0.08em) rotate(0deg);
      animation-timing-function: ease-in;
    }
    100% {
      opacity: 1;
      transform: none;
    }
  }
  @keyframes splash {
    0% {
      color: #fff;
      text-shadow: none;
    }
    14% {
      color: color-mix(in oklab, var(--c1) 30%, white);
      text-shadow:
        0 0 26px var(--glow),
        0 0.25em 0.7em color-mix(in oklab, var(--c2) 50%, transparent);
    }
    100% {
      color: #fff;
      text-shadow: 0 0 12px color-mix(in oklab, var(--glow) 30%, transparent);
    }
  }
  .light .rl {
    text-shadow: 0 2px 12px rgba(0, 0, 0, 0.7);
  }

  /* ── Orbit ────────────────────────────────────────── */
  .orbit {
    --glow: color-mix(in oklab, var(--c1) 70%, white);
    position: relative;
    width: 100%;
    height: 100%;
    min-height: clamp(260px, 46vh, 580px);
    container-type: size;
  }
  .orb {
    position: absolute;
    left: 50%;
    top: 44%;
    width: clamp(56px, 20cqh, 190px);
    aspect-ratio: 1;
    translate: -50% -50%;
    border-radius: 50%;
    background: radial-gradient(
      circle at 34% 30%,
      color-mix(in oklab, var(--c1) 35%, white) 0,
      var(--c1) 30%,
      color-mix(in oklab, var(--c2) 70%, black) 100%
    );
    box-shadow:
      0 0 50px 8px color-mix(in oklab, var(--c1) 45%, transparent),
      0 0 140px 40px color-mix(in oklab, var(--c2) 22%, transparent);
    animation: breathe 5.5s ease-in-out infinite;
  }
  @keyframes breathe {
    50% {
      scale: 1.07;
    }
  }
  .ring {
    position: absolute;
    left: 50%;
    top: 44%;
    width: 0;
    height: 0;
    transform-style: preserve-3d;
  }
  .path {
    position: absolute;
    left: calc(-1 * var(--R));
    top: calc(-1 * var(--R));
    width: calc(2 * var(--R));
    height: calc(2 * var(--R));
    border-radius: 50%;
    border: 1px solid color-mix(in oklab, var(--c2) 40%, rgba(255, 255, 255, 0.3));
    transform: rotateX(90deg);
  }
  /* Words face outward, so the far side of the ring is hidden. */
  .ow {
    position: absolute;
    left: 0;
    top: 0;
    transform: rotateY(var(--a)) translateZ(var(--R));
    backface-visibility: hidden;
  }
  .ow b {
    display: block;
    translate: -50% -50%;
    white-space: nowrap;
    font-size: var(--fs);
    font-weight: 650;
    font-stretch: 88%;
    color: #fff;
    opacity: 0.3;
    animation:
      o-lit 300ms cubic-bezier(0.2, 0.8, 0.2, 1) var(--d) both,
      o-rest 700ms ease calc(var(--d) + var(--t)) forwards;
  }
  @keyframes o-lit {
    to {
      opacity: 1;
      scale: 1.35;
      font-weight: 780;
      text-shadow: 0 0 22px color-mix(in oklab, var(--glow) 60%, transparent);
    }
  }
  @keyframes o-rest {
    from {
      opacity: 1;
      scale: 1.35;
      font-weight: 780;
      text-shadow: 0 0 22px color-mix(in oklab, var(--glow) 60%, transparent);
    }
    to {
      opacity: 0.66;
      scale: 1;
    }
  }
  .light .ow b {
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.75);
  }

  .depth.paused *,
  .rain.paused *,
  .orbit.paused * {
    animation-play-state: paused !important;
  }
  @media (prefers-reduced-motion: reduce) {
    .orb {
      animation: none;
    }
    .drop {
      animation: splash 900ms ease-out var(--d) both;
    }
  }
</style>
