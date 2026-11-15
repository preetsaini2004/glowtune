<script lang="ts">
  // A fan of one emoji thrown up out of a word as it's sung, falling back
  // with gravity. Pure CSS: each piece gets its own throw (--x, --up, --r)
  // and starts at the word's own delay (--d), so it pauses with the song.
  let { emoji, delay, seed = 1 }: { emoji: string; delay: number; seed?: number } = $props();

  function seeded(s: number) {
    let x = s >>> 0 || 1;
    return () => {
      x = (x + 0x6d2b79f5) >>> 0;
      let t = Math.imul(x ^ (x >>> 15), 1 | x);
      t ^= t + Math.imul(t ^ (t >>> 7), 61 | t);
      return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
    };
  }
  const PIECES = 6;
  const pieces = $derived.by(() => {
    const r = seeded(seed * 2654435761);
    return Array.from({ length: PIECES }, (_, k) => {
      // Spread across the upper half, left to right, with a little jitter.
      const spread = (k / (PIECES - 1) - 0.5) * 2;
      return {
        x: `${(spread * 1.9 + (r() - 0.5) * 0.5).toFixed(2)}em`,
        up: `${(1.1 + r() * 1.2).toFixed(2)}em`,
        r: `${Math.round(spread * 50 + (r() - 0.5) * 40)}deg`,
        s: (0.75 + r() * 0.5).toFixed(2),
        lag: `${Math.round(r() * 90)}ms`,
      };
    });
  });
</script>

<span class="burst" aria-hidden="true" style:--d={`${delay}ms`}>
  {#each pieces as p}
    <i style:--x={p.x} style:--up={p.up} style:--r={p.r} style:--s={p.s} style:--lag={p.lag}>{emoji}</i>
  {/each}
</span>

<style>
  .burst {
    position: absolute;
    left: 50%;
    top: 0;
    width: 0;
    height: 0;
    pointer-events: none;
    font-style: normal;
    font-weight: 400;
    letter-spacing: 0;
    text-transform: none;
  }
  .burst i {
    position: absolute;
    left: 0;
    top: 0;
    font-style: normal;
    font-size: 0.62em;
    line-height: 1;
    translate: -50% -50%;
    opacity: 0;
    animation: throw 1500ms linear calc(var(--d) + var(--lag)) both;
  }
  /* Up fast, slowing at the top, then falling faster and faster (gravity). */
  @keyframes throw {
    0% {
      opacity: 0;
      transform: translate(0, 0) rotate(0deg) scale(0.4);
      animation-timing-function: cubic-bezier(0.2, 0.7, 0.4, 1);
    }
    8% {
      opacity: 1;
    }
    32% {
      transform: translate(calc(var(--x) * 0.55), calc(var(--up) * -1)) rotate(calc(var(--r) * 0.4))
        scale(var(--s));
      animation-timing-function: cubic-bezier(0.5, 0, 0.9, 0.6);
    }
    75% {
      opacity: 1;
    }
    100% {
      opacity: 0;
      transform: translate(var(--x), 2.6em) rotate(var(--r)) scale(var(--s));
    }
  }
  :global(.paused) .burst i {
    animation-play-state: paused;
  }
  @media (prefers-reduced-motion: reduce) {
    .burst {
      display: none;
    }
  }
</style>
