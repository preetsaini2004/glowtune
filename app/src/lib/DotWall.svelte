<script lang="ts">
  // "Dots" wallpaper: a field of soft dots in the song's colours over a soft
  // copy of the cover, like water seen from above.
  //
  //   kick drum ──► a wave starts at a random spot and rolls outward:
  //                 crests lift the dots (up, bigger, brighter), troughs
  //                 drop them, and it fades as it spreads.
  //   energy    ──► each kick adds to it, it ebbs away in a few seconds:
  //                 the whole surface swells with a busy song and lies
  //                 almost flat in a quiet part or a gap.
  //
  // Canvas 2D at ~30 frames a second; none when the scene is covered, the
  // song is paused or motion is off (one still frame then, once the last
  // waves have died out).
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  let {
    colors,
    src,
    running,
    beat = true,
  }: {
    /** The song's colours (hex), first two used. */
    colors: string[];
    /** The cover, shown soft underneath. */
    src: string | null;
    running: boolean;
    beat?: boolean;
  } = $props();

  const FRAME_MS = 1000 / 30;
  /** A wave rolls this long (s) before it's gone. */
  const WAVE_LIFE = 3.6;
  const MAX_WAVES = 7;
  /** Speed of a wave front, in screen heights per second. */
  const WAVE_SPEED = 0.32;
  /** Distance between crests (screen heights). */
  const WAVELENGTH = 0.085;

  type Wave = { x: number; y: number; born: number; amp: number };

  let canvas: HTMLCanvasElement | undefined = $state();

  onMount(() => {
    const c = canvas!;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    const reduced = matchMedia("(prefers-reduced-motion: reduce)");
    let raf = 0;
    let last = 0;
    let clock = 0; // seconds of wallpaper time (stands still while paused)
    let energy = 0; // 0..1, how busy the song is right now
    let waves: Wave[] = [];

    /** #rrggbb a quarter of the way to white. */
    function bright(hex: string): string {
      const m = /^#?([0-9a-f]{6})/i.exec(hex);
      if (!m) return hex;
      const n = parseInt(m[1], 16);
      const ch = (v: number) => Math.round(v + (255 - v) * 0.25);
      return `rgb(${ch((n >> 16) & 255)}, ${ch((n >> 8) & 255)}, ${ch(n & 255)})`;
    }

    function size() {
      const k = Math.min(devicePixelRatio || 1, 1.5);
      c.width = Math.round(c.clientWidth * k);
      c.height = Math.round(c.clientHeight * k);
    }

    /** Height of the water at (nx, ny), screen fractions; roughly −1..1. */
    function height(nx: number, ny: number, aspect: number): number {
      // The swell: long, slow, crossing waves, big only when the song is busy.
      const swell =
        (Math.sin(nx * 5.2 + ny * 2.1 + clock * 0.9) * 0.6 + Math.sin(nx * -2.7 + ny * 6.3 + clock * 0.63) * 0.4) *
        (0.15 + energy * 0.7);
      let h = swell;
      for (const w of waves) {
        const age = clock - w.born;
        const d = Math.hypot((nx - w.x) * aspect, ny - w.y);
        const x = d - age * WAVE_SPEED; // < 0: behind the front
        if (x > 0.05) continue; // not reached yet
        // A packet of a few crests trailing the front, fading with age and
        // as it spreads.
        const envelope = Math.exp(-(x * x) / (x < 0 ? 0.03 : 0.0012));
        const fade = Math.exp(-age * 1.05) / (1 + d * 2.2);
        h += Math.sin((x / WAVELENGTH) * 2 * Math.PI) * envelope * fade * w.amp * 2.1;
      }
      return h;
    }

    function draw() {
      const { width, height: H } = c;
      const scale = width / 1920;
      const spacing = 27 * scale;
      const aspect = width / H;
      const lift = 12 * scale; // px a crest lifts a dot
      // A little lighter than the song's colours, and added onto what's
      // below ("lighter"), so the dots shine instead of sitting flat.
      const [a, b] = [bright(colors[0] ?? "#8b7cff"), bright(colors[1] ?? colors[0] ?? "#5ca0ff")];
      ctx!.clearRect(0, 0, width, H);
      ctx!.globalCompositeOperation = "lighter";
      for (let row = -1; row < H / spacing + 1; row++) {
        const y = row * spacing;
        for (let col = -1; col <= width / spacing + 1; col++) {
          const x = (col + (row % 2) * 0.5) * spacing;
          const nx = x / width;
          const ny = y / H;
          const h = height(nx, ny, aspect);
          const tint = (Math.sin(nx * 6.1 - ny * 4.3 + clock * 0.2) + 1) / 2;
          const vignette = 1 - Math.min(0.55, Math.hypot(nx - 0.5, ny - 0.5) * 0.5);
          ctx!.fillStyle = tint + h * 0.25 > 0.5 ? a : b;
          ctx!.globalAlpha = Math.max(0.1, Math.min(1, (0.5 + h * 0.45) * vignette));
          ctx!.beginPath();
          ctx!.arc(x, y - h * lift, Math.max(0.5, (1.8 + h * 1.6) * scale), 0, 2 * Math.PI);
          ctx!.fill();
        }
      }
      ctx!.globalAlpha = 1;
      ctx!.globalCompositeOperation = "source-over";
    }

    const alive = () => (running && !reduced.matches) || waves.length > 0;
    function frame(now: number) {
      raf = 0;
      if (now - last >= FRAME_MS) {
        const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
        last = now;
        // Waves keep rolling out after a pause, then the surface stills.
        clock += dt;
        energy *= Math.exp(-dt / 2.4);
        waves = waves.filter((w) => clock - w.born < WAVE_LIFE);
        draw();
      }
      if (alive()) raf = requestAnimationFrame(frame);
      else last = 0;
    }
    function wake() {
      if (!raf) raf = requestAnimationFrame(frame);
    }

    const ro = new ResizeObserver(() => {
      size();
      draw();
    });
    ro.observe(c);
    size();
    draw();

    const kicks = getCurrentWindow().listen<number>("kick", (e) => {
      if (!beat || !running || reduced.matches) return;
      const strength = Math.max(0.35, Math.min(1, e.payload));
      energy = Math.min(1, energy + strength * 0.3);
      // A new wave somewhere random, kept away from the very edges.
      waves.push({ x: 0.1 + Math.random() * 0.8, y: 0.12 + Math.random() * 0.76, born: clock, amp: 0.5 + strength * 0.5 });
      if (waves.length > MAX_WAVES) waves.shift();
      wake();
    });
    const stop = $effect.root(() => {
      $effect(() => {
        void colors;
        draw();
      });
      $effect(() => {
        void running;
        wake();
      });
      $effect(() => {
        invoke("want_kicks", { on: beat && running && !reduced.matches }).catch(() => {});
      });
    });

    return () => {
      stop();
      invoke("want_kicks", { on: false }).catch(() => {});
      kicks.then((f) => f());
      cancelAnimationFrame(raf);
      ro.disconnect();
    };
  });
</script>

<div class="dots" style:--d1={colors[0]} style:--d2={colors[1] ?? colors[0]}>
  {#key src}
    {#if src}<img src={src} alt="" />{/if}
  {/key}
  <canvas bind:this={canvas} aria-hidden="true"></canvas>
  <div class="vignette"></div>
</div>

<style>
  .dots {
    position: absolute;
    inset: 0;
    background:
      radial-gradient(ellipse at 18% 82%, color-mix(in oklab, var(--d1) 34%, transparent), transparent 62%),
      radial-gradient(ellipse at 85% 30%, color-mix(in oklab, var(--d2) 32%, transparent), transparent 60%),
      color-mix(in oklab, var(--d1) 14%, #1b1c24);
  }
  .dots > * {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
  }
  /* The cover, soft and present: the dots are water over it. */
  img {
    object-fit: cover;
    opacity: 0.45;
    filter: blur(14px) saturate(1.15);
    transform: scale(1.06);
  }
  /* One soft glow round every dot, in the song's colour (one GPU pass). */
  canvas {
    filter: drop-shadow(0 0 3px color-mix(in oklab, var(--d1) 80%, white)) drop-shadow(0 0 9px color-mix(in oklab, var(--d1) 55%, transparent));
  }
  .vignette {
    background: radial-gradient(ellipse at center, transparent 55%, rgba(5, 6, 8, 0.4) 100%);
  }
</style>
