<script lang="ts">
  // The taskbar player: a slim strip sitting on the Windows taskbar while a
  // song is on. Cover, title and artist, previous / play / next, and the
  // song's progress as a hairline along the bottom.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { np } from "$lib/nowPlaying.svelte";

  let now = $state(Date.now());
  const track = $derived(np.track);
  const duration = $derived(track?.durationMs ?? null);
  const progress = $derived(duration ? Math.min(1, np.positionAt(now) / duration) : 0);
  const palette = $derived(np.palette);

  // Shown only while there's a song (Rust keeps it on top while shown).
  $effect(() => {
    invoke("taskbar_visible", { on: Boolean(track) }).catch(() => {});
  });

  onMount(() => {
    np.start();
    const id = setInterval(() => {
      if (np.playing && !document.hidden) now = Date.now();
    }, 1000);
    return () => clearInterval(id);
  });
</script>

<main class="strip" class:playing={np.playing} style:--c1={palette[0]} style:--c2={palette[1] ?? palette[0]}>
  {#if track}
    <button class="song" title={`${track.title} — ${track.artist}`} onclick={() => invoke("show_player_window").catch(() => {})}>
      <span class="art">
        {#if track.artUrl}<img src={track.artUrl} alt="" draggable="false" />{/if}
      </span>
      <span class="txt">
        <span class="t">{track.title}</span>
        <span class="a">{track.artist || track.source}</span>
      </span>
    </button>
    <nav aria-label="Playback">
      <button aria-label="Previous" onclick={() => np.control("previous")}>
        <svg viewBox="0 0 24 24"><path d="M7 5.5v13M18.2 6.2 10 12l8.2 5.8z" /></svg>
      </button>
      <button class="play" aria-label={np.playing ? "Pause" : "Play"} onclick={() => np.control("playPause")}>
        {#if np.playing}
          <svg viewBox="0 0 24 24"><path d="M8.5 6v12M15.5 6v12" /></svg>
        {:else}
          <svg viewBox="0 0 24 24"><path d="M8 5.8v12.4L18.4 12z" /></svg>
        {/if}
      </button>
      <button aria-label="Next" onclick={() => np.control("next")}>
        <svg viewBox="0 0 24 24"><path d="M17 5.5v13M5.8 6.2 14 12l-8.2 5.8z" /></svg>
      </button>
    </nav>
    <i class="line" aria-hidden="true"><b style:transform={`scaleX(${progress})`}></b></i>
  {/if}
</main>

<style>
  :global(html),
  :global(body) {
    background: transparent !important;
    overflow: hidden;
  }
  .strip {
    position: relative;
    height: 100vh;
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 0 6px 0 4px;
    border-radius: 9px;
    background: linear-gradient(100deg, color-mix(in oklab, var(--c1) 26%, rgba(28, 26, 34, 0.96)), rgba(28, 26, 34, 0.96) 60%);
    border: 1px solid rgba(255, 255, 255, 0.08);
    overflow: hidden;
    color: #fff;
    font-family: "Segoe UI Variable Text", system-ui, sans-serif;
    user-select: none;
  }
  button {
    border: 0;
    background: transparent;
    color: inherit;
    cursor: pointer;
    padding: 0;
  }
  .song {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 8px;
    height: 100%;
    text-align: left;
    border-radius: 7px;
  }
  .song:hover {
    background: rgba(255, 255, 255, 0.06);
  }
  .art {
    flex: none;
    width: calc(100vh - 12px);
    aspect-ratio: 1;
    border-radius: 6px;
    overflow: hidden;
    background: linear-gradient(135deg, var(--c1), var(--c2));
  }
  .art img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .txt {
    min-width: 0;
    display: grid;
  }
  .t,
  .a {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .t {
    font-size: 12.5px;
    font-weight: 600;
    line-height: 16px;
  }
  .a {
    font-size: 11px;
    line-height: 14px;
    color: rgba(255, 255, 255, 0.6);
  }
  nav {
    display: flex;
    align-items: center;
  }
  nav button {
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    border-radius: 50%;
  }
  nav button:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  nav .play {
    background: color-mix(in oklab, var(--c1) 40%, rgba(255, 255, 255, 0.12));
  }
  nav svg {
    width: 15px;
    height: 15px;
    fill: currentColor;
    stroke: currentColor;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .line {
    position: absolute;
    left: 8px;
    right: 8px;
    bottom: 2px;
    height: 2px;
    border-radius: 1px;
    background: rgba(255, 255, 255, 0.1);
    overflow: hidden;
  }
  .line b {
    position: absolute;
    inset: 0;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    transform-origin: left center;
    transition: transform 1s linear;
  }
</style>
