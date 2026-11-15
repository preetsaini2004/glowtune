<script lang="ts">
  // The hotkey toast: a glass pill near the bottom of the screen saying what
  // a key just did. Rust shows the window, sends "osd" and hides it again.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { np } from "$lib/nowPlaying.svelte";

  type Toast = { icon: string; text: string; level: number | null };
  let toast = $state<Toast | null>(null);
  // Bumped on every toast so the pop-in plays again.
  let n = $state(0);
  let leaving = $state(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const palette = $derived(np.palette);
  const art = $derived(np.track?.artUrl ?? null);
  const showArt = $derived(toast && ["play", "pause", "next", "previous"].includes(toast.icon) && art);

  const ICONS: Record<string, string> = {
    play: "M8 5v14l11-7z",
    pause: "M7 5h3.5v14H7zM13.5 5H17v14h-3.5z",
    next: "M6 5v14l9-7zM16 5h2.5v14H16z",
    previous: "M18 5v14l-9-7zM8 5H5.5v14H8z",
    volume: "M4 9v6h4l5 4V5L8 9zM16 9.5a3.5 3.5 0 0 1 0 5M18.5 7a7 7 0 0 1 0 10",
    lyrics: "M4 6h16M4 12h10M4 18h13",
    preset: "M6 3h12v18l-6-4-6 4z",
    sleep: "M20 14.5A8.5 8.5 0 1 1 9.5 4a7 7 0 0 0 10.5 10.5z",
    player: "M3 6h18v12H3zM7 14h4",
    scene: "M3 5h18v14H3zM3 15l5-5 4 4 3-3 6 6",
    card: "M5 3h14v18H5zM8 14h8M8 17h5M9 6h6v5H9z",
  };

  function show(t: Toast) {
    // Holding volume up updates the pill in place instead of popping it again.
    if (!toast || leaving || toast.icon !== t.icon) n++;
    toast = t;
    leaving = false;
    clearTimeout(timer);
    timer = setTimeout(() => (leaving = true), 1450);
  }

  onMount(() => {
    np.start();
    const off = getCurrentWindow().listen<Toast>("osd", (e) => show(e.payload));
    // The first toast can arrive before the page listens.
    invoke<Toast | null>("last_toast").then((t) => t && !toast && show(t)).catch(() => {});
    return () => {
      clearTimeout(timer);
      off.then((f) => f());
    };
  });
</script>

<main style:--c1={palette[0]} style:--c2={palette[1] ?? palette[0]}>
  {#if toast}
    {#key n}
      <div class="pill" class:leaving>
        <span class="badge" class:art={showArt}>
          {#if showArt}
            <img src={art} alt="" />
            <span class="mini"><svg viewBox="0 0 24 24" aria-hidden="true"><path d={ICONS[toast.icon]} /></svg></span>
          {:else}
            <svg viewBox="0 0 24 24" aria-hidden="true" class:stroke={!["play", "pause", "next", "previous"].includes(toast.icon)}>
              <path d={ICONS[toast.icon] ?? ICONS.play} />
            </svg>
          {/if}
        </span>
        <span class="body">
          <span class="text">{toast.text}</span>
          {#if toast.level !== null}
            <span class="bar"><span style:width="{Math.round(toast.level * 100)}%"></span></span>
          {/if}
        </span>
      </div>
    {/key}
  {/if}
</main>

<style>
  :global(html),
  :global(body) {
    background: transparent !important;
    overflow: hidden;
  }
  main {
    height: 100vh;
    display: grid;
    place-items: center;
    user-select: none;
  }
  .pill {
    display: flex;
    align-items: center;
    gap: 12px;
    min-width: 200px;
    max-width: calc(100vw - 16px);
    box-sizing: border-box;
    padding: 9px 20px 9px 9px;
    border-radius: 999px;
    background:
      linear-gradient(100deg, color-mix(in oklab, var(--c1) 30%, transparent), transparent 70%),
      rgba(22, 20, 28, 0.94);
    border: 1px solid rgba(255, 255, 255, 0.1);
    box-shadow: 0 10px 28px rgba(0, 0, 0, 0.45);
    color: #fff;
    animation: pop 260ms cubic-bezier(0.2, 0.9, 0.3, 1.25) both;
  }
  .pill.leaving {
    animation: out 300ms ease forwards;
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateY(10px) scale(0.94);
    }
  }
  @keyframes out {
    to {
      opacity: 0;
      transform: translateY(6px) scale(0.97);
    }
  }
  .badge {
    position: relative;
    display: grid;
    place-items: center;
    width: 42px;
    height: 42px;
    flex: none;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .badge.art {
    background: none;
  }
  .badge img {
    width: 100%;
    height: 100%;
    border-radius: 50%;
    object-fit: cover;
  }
  .badge svg {
    width: 20px;
    height: 20px;
    fill: #fff;
  }
  .badge svg.stroke {
    fill: none;
    stroke: #fff;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .mini {
    position: absolute;
    right: -3px;
    bottom: -3px;
    display: grid;
    place-items: center;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    border: 2px solid rgba(22, 20, 28, 1);
  }
  .mini svg {
    width: 10px;
    height: 10px;
  }
  .body {
    display: grid;
    gap: 7px;
    min-width: 0;
    flex: 1;
  }
  .text {
    font-size: 15px;
    font-weight: 650;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .bar {
    display: block;
    height: 4px;
    width: 180px;
    max-width: 100%;
    border-radius: 2px;
    background: rgba(255, 255, 255, 0.16);
    overflow: hidden;
  }
  .bar span {
    display: block;
    height: 100%;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    transition: width 120ms ease;
  }
  @media (prefers-reduced-motion: reduce) {
    .pill,
    .pill.leaving {
      animation-duration: 1ms;
    }
  }
</style>
