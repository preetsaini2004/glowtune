<script lang="ts">
  // Desktop lyrics: the current line big and lit in the album's colors, the
  // next line faded below. Lines rise as the song moves on.
  //
  // No per-frame loop: we compute when the next line starts and sleep until
  // then (re-planned on every play/pause/seek report).
  import { onMount } from "svelte";
  import { fly } from "svelte/transition";
  import { cubicOut } from "svelte/easing";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { np } from "$lib/nowPlaying.svelte";
  import { showLyric } from "$lib/clean";
  import { theme } from "$lib/theme.svelte";

  type Line = { timeMs: number; text: string };
  type LyricsState = {
    status: "loading" | "synced" | "plain" | "instrumental" | "none";
    lines: Line[];
    offsetMs: number;
  };

  let lyrics = $state<LyricsState>({ status: "none", lines: [], offsetMs: 0 });
  let index = $state(-1);
  let timer: ReturnType<typeof setTimeout> | undefined;
  // How far into the current line we were when it was (re)scheduled, for karaoke.
  let lineElapsedMs = $state(0);
  // Bumped on every re-plan so a karaoke fill restarts at the right point after a seek.
  let plans = 0;
  let plan = $state(0);

  const style = $derived(theme.theme?.lyrics.style ?? "rise");
  const position = $derived(theme.theme?.lyrics.position ?? "bottom-center");
  // Rise: lines travel up. Static: plain swap. Karaoke: quick fade, the fill does the work.
  const enter = $derived(style === "rise" ? { y: 46, duration: 520, easing: cubicOut } : { y: 0, duration: style === "static" ? 0 : 160 });
  const leave = $derived(style === "rise" ? { y: -46, duration: 420, easing: cubicOut } : { y: 0, duration: style === "static" ? 0 : 160 });

  const palette = $derived(np.palette);
  const synced = $derived(lyrics.status === "synced" && lyrics.lines.length > 0);
  const current = $derived(synced && index >= 0 ? lyrics.lines[index] : null);
  const next = $derived(synced ? lyrics.lines[index + 1] ?? null : null);
  const visible = $derived(synced && np.playing && current !== null);
  const lineMs = $derived(current && next ? next.timeMs - current.timeMs : 4000);

  /** Index of the line active at `ms` (binary search), -1 before the first. */
  function lineAt(ms: number): number {
    const lines = lyrics.lines;
    let lo = 0;
    let hi = lines.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (lines[mid].timeMs <= ms) {
        found = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return found;
  }

  function schedule() {
    clearTimeout(timer);
    if (!synced) {
      index = -1;
      return;
    }
    const pos = np.positionAt() + lyrics.offsetMs;
    const i = lineAt(pos);
    index = i; // write only: reading it here would re-trigger the effect
    lineElapsedMs = i >= 0 ? Math.max(0, pos - lyrics.lines[i].timeMs) : 0;
    plan = ++plans; // plain counter: no reactive read inside the effect
    if (!np.playing) return;
    const upcoming = lyrics.lines[i + 1];
    if (upcoming) {
      const wait = (upcoming.timeMs - pos) / (np.playback.rate || 1);
      timer = setTimeout(schedule, Math.max(16, wait));
    }
  }

  // Re-plan whenever playback or lyrics change.
  $effect(() => {
    void np.playback;
    void lyrics;
    schedule();
  });

  // "Move lyrics": dashed frame, drag anywhere, Done saves the spot.
  let moving = $state(false);
  function finishMove() {
    invoke("lyrics_move", { on: false }).catch(() => {});
  }
  function resetMove() {
    invoke("lyrics_reset_position").catch(() => {});
  }

  onMount(() => {
    const unMove = listen<boolean>("lyrics-move", (e) => (moving = e.payload));
    np.start();
    const unlisten = listen<LyricsState>("lyrics", (e) => (lyrics = e.payload));
    invoke<LyricsState>("get_lyrics").then((l) => (lyrics = l)).catch(() => {});
    return () => {
      clearTimeout(timer);
      unlisten.then((f) => f());
      unMove.then((f) => f());
    };
  });
</script>

<main
  class="stage {position}"
  style:--c1={palette[0]}
  style:--c2={palette[1] ?? palette[0]}
  class:shown={visible || moving}
  class:moving
>
  {#key style === "karaoke" ? `${index}:${plan}` : index}
    {#if current}
      <div class="pair" in:fly={enter} out:fly={leave}>
        <p
          class="now"
          class:gap={current.text === ""}
          class:karaoke={style === "karaoke" && current.text !== ""}
          style:--line-ms={`${lineMs / (np.playback.rate || 1)}ms`}
          style:--line-delay={`-${lineElapsedMs / (np.playback.rate || 1)}ms`}
        >
          {showLyric(current.text, np.cleanLyrics) || "♪"}
        </p>
        {#if next}
          <p class="next">{showLyric(next.text, np.cleanLyrics) || "♪"}</p>
        {/if}
      </div>
    {/if}
  {/key}
  {#if moving}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="mover" onpointerdown={(e) => { if ((e.target as HTMLElement).closest("button")) return; getCurrentWindow().startDragging(); }}>
      {#if !current}<p class="sample">Your lyrics show here</p>{/if}
      <div class="mover-bar">
        <span>Drag to place your lyrics</span>
        <button onclick={resetMove}>Reset</button>
        <button class="done" onclick={finishMove}>Done</button>
      </div>
    </div>
  {/if}
</main>

<style>
  .mover {
    position: fixed;
    inset: 6px;
    border: 2px dashed color-mix(in oklab, var(--c1) 70%, white);
    border-radius: 18px;
    background: rgba(10, 8, 16, 0.35);
    cursor: grab;
    display: grid;
    place-items: center;
  }
  .mover:active {
    cursor: grabbing;
  }
  .sample {
    margin: 0;
    font-family: var(--font-lyrics);
    font-size: 40px;
    font-weight: 720;
    color: #fff;
  }
  .mover-bar {
    position: absolute;
    top: 10px;
    right: 12px;
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    color: #fff;
  }
  .mover-bar button {
    height: 28px;
    padding: 0 12px;
    border: 1px solid rgba(255, 255, 255, 0.25);
    border-radius: 999px;
    background: rgba(0, 0, 0, 0.4);
    color: #fff;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
  }
  .mover-bar .done {
    border: 0;
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }

  .stage {
    position: relative;
    height: 100vh;
    display: grid;
    place-items: end center;
    padding: 28px 40px;
    opacity: 0;
    transition: opacity 700ms ease;
  }
  .stage.shown {
    opacity: 1;
  }
  .stage.top-center {
    place-items: start center;
  }
  .stage.center {
    place-items: center;
  }
  /* Both lines share one grid cell so the outgoing pair can overlap the incoming one. */
  .pair {
    grid-area: 1 / 1;
    width: 100%;
    text-align: center;
  }
  p {
    margin: 0;
    font-family: var(--font-lyrics);
    text-wrap: balance;
  }
  .now {
    font-size: var(--lyrics-size);
    line-height: 1.12;
    font-weight: 720;
    font-stretch: 92%;
    letter-spacing: -0.012em;
    color: #fff;
    /* Lit by the song: a halo in the album color, plus a dark under-shadow
       so it stays readable on any wallpaper or window. */
    text-shadow:
      0 0 22px color-mix(in oklab, var(--c1) 75%, transparent),
      0 0 54px color-mix(in oklab, var(--c2) 45%, transparent),
      0 2px 14px rgba(0, 0, 0, 0.55);
  }
  .now.gap {
    opacity: 0.6;
  }
  /* Karaoke: the line fills with light from left to right over its duration. */
  .now.karaoke {
    color: transparent;
    background: linear-gradient(90deg, #fff 50%, rgba(255, 255, 255, 0.34) 50%) 100% 0 / 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    animation: fill var(--line-ms) linear var(--line-delay) both;
  }
  @keyframes fill {
    to {
      background-position: 0 0;
    }
  }
  .next {
    margin-top: 10px;
    font-size: calc(var(--lyrics-size) * 0.56);
    line-height: 1.2;
    font-weight: 520;
    font-stretch: 96%;
    color: rgba(255, 255, 255, 0.5);
    text-shadow: 0 2px 10px rgba(0, 0, 0, 0.6);
  }
</style>
