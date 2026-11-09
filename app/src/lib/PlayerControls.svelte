<script lang="ts">
  // The full control row: like · shuffle · previous · play/pause · next · repeat · volume.
  //
  //   desktop apps (Spotify app…) → Windows media controls (shuffle, repeat)
  //                                 + the app's own volume
  //   Glowtune's web players      → the page's own buttons (shuffle, repeat,
  //                                 like, volume), since browsers don't tell
  //                                 Windows about them
  // Each button shows only when the playing app supports it.
  import { invoke } from "@tauri-apps/api/core";
  import { np } from "./nowPlaying.svelte";

  let { volume: showVolume = true }: { volume?: boolean } = $props();

  type Web = { shuffle: boolean | null; repeat: "off" | "list" | "track" | null; liked: boolean | null; volume: number | null };
  const isWeb = $derived(np.track?.source === "Web player");
  let web = $state<Web | null>(null);
  let appVolume = $state<number | null>(null);
  let sendTimer: ReturnType<typeof setTimeout> | undefined;

  async function webCall(action: string, value?: number) {
    try {
      web = await invoke<Web | null>("web_player", { action, value });
    } catch {
      web = null;
    }
  }

  // Re-read when the song or app changes; web players also every 3 s while
  // playing (their buttons can be pressed inside the page too).
  $effect(() => {
    void np.track?.source;
    void np.track?.title;
    if (isWeb) {
      webCall("state");
    } else {
      web = null;
      np.getVolume()
        .then((v) => (appVolume = v))
        .catch(() => (appVolume = null));
    }
  });
  $effect(() => {
    if (!isWeb || !np.playing) return;
    const id = setInterval(() => {
      if (!document.hidden) webCall("state");
    }, 3000);
    return () => clearInterval(id);
  });

  const shuffle = $derived(isWeb ? (web?.shuffle ?? null) : np.playback.shuffle);
  const repeat = $derived(isWeb ? (web?.repeat ?? null) : np.playback.repeat);
  const liked = $derived(isWeb ? (web?.liked ?? null) : null);
  const volume = $derived(isWeb ? (web?.volume ?? null) : appVolume);

  function toggleShuffle() {
    if (isWeb) webCall("shuffle");
    else np.setShuffle(!shuffle);
  }
  function cycleRepeat() {
    if (isWeb) webCall("repeat");
    else np.cycleRepeat();
  }
  function setVolume(v: number) {
    if (isWeb && web) web = { ...web, volume: v };
    else appVolume = v;
    clearTimeout(sendTimer);
    sendTimer = setTimeout(() => (isWeb ? webCall("volume", v) : np.setVolume(v)), 60);
  }
  function wheel(e: WheelEvent) {
    if (volume === null) return;
    e.preventDefault();
    setVolume(Math.min(1, Math.max(0, volume - Math.sign(e.deltaY) * 0.05)));
  }
</script>

<div class="controls">
  {#if liked != null}
    <button class="icon small" class:on={liked} aria-label={liked ? "Remove from liked songs" : "Like"} aria-pressed={liked} onclick={() => webCall("like")}>
      <svg viewBox="0 0 24 24"><path class:fill={liked} d="M12 20s-7-4.4-7-10a4 4 0 0 1 7-2.6A4 4 0 0 1 19 10c0 5.6-7 10-7 10z" /></svg>
    </button>
  {/if}
  {#if shuffle != null}
    <button class="icon small" class:on={shuffle} aria-label="Shuffle" aria-pressed={shuffle} onclick={toggleShuffle}>
      <svg viewBox="0 0 24 24"><path d="M3 7h3.5c2 0 3.3 1 4.3 2.7l2.4 4.6c1 1.7 2.3 2.7 4.3 2.7H21M17.5 4 21 7l-3.5 3M3 17h3.5c1.4 0 2.4-.5 3.2-1.4M13.3 8.4c.8-.9 1.8-1.4 3.2-1.4M17.5 14l3.5 3-3.5 3" /></svg>
    </button>
  {/if}
  <button class="icon" aria-label="Previous" onclick={() => np.control("previous")}>
    <svg viewBox="0 0 24 24"><path class="fill" d="M7 5.5v13M18.2 6.2 10 12l8.2 5.8z" /></svg>
  </button>
  <button class="icon play" aria-label={np.playing ? "Pause" : "Play"} onclick={() => np.control("playPause")}>
    {#if np.playing}
      <svg viewBox="0 0 24 24"><path d="M8.5 6v12M15.5 6v12" /></svg>
    {:else}
      <svg viewBox="0 0 24 24"><path class="fill" d="M8 5.8v12.4L18 12z" /></svg>
    {/if}
  </button>
  <button class="icon" aria-label="Next" onclick={() => np.control("next")}>
    <svg viewBox="0 0 24 24"><path class="fill" d="M17 5.5v13M5.8 6.2 14 12l-8.2 5.8z" /></svg>
  </button>
  {#if repeat != null}
    <button
      class="icon small"
      class:on={repeat !== "off"}
      aria-label={repeat === "track" ? "Repeat this song" : repeat === "list" ? "Repeat all" : "Repeat off"}
      onclick={cycleRepeat}
    >
      <svg viewBox="0 0 24 24"><path d="M17 3l3 3-3 3M4 11V9a3 3 0 0 1 3-3h13M7 21l-3-3 3-3M20 13v2a3 3 0 0 1-3 3H4" /></svg>
      {#if repeat === "track"}<span class="one">1</span>{/if}
    </button>
  {/if}
  {#if showVolume && volume !== null}
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div class="vol" onwheel={wheel}>
      <button class="icon small" aria-label={volume === 0 ? "Unmute" : "Mute"} onclick={() => setVolume(volume === 0 ? 0.6 : 0)}>
        <svg viewBox="0 0 24 24">
          <path class="fill" d="M4 9.5h3.5L12 6v12l-4.5-3.5H4z" />
          {#if volume > 0}<path d="M15.5 9.5a3.5 3.5 0 0 1 0 5" />{/if}
          {#if volume > 0.5}<path d="M18 7a7 7 0 0 1 0 10" />{/if}
        </svg>
      </button>
      <input
        type="range"
        min="0"
        max="1"
        step="0.01"
        value={volume}
        aria-label="Volume"
        style:--p={`${volume * 100}%`}
        oninput={(e) => setVolume(Number((e.currentTarget as HTMLInputElement).value))}
      />
    </div>
  {/if}
</div>

<style>
  .controls {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  button.icon {
    position: relative;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: inherit;
    cursor: pointer;
    transition: background 160ms ease, transform 160ms ease, color 160ms ease;
  }
  button.icon.small {
    width: 32px;
    height: 32px;
    color: rgba(255, 255, 255, 0.62);
  }
  button.icon.small.on {
    color: color-mix(in oklab, var(--c1) 70%, white);
  }
  button.icon.small.on::after {
    content: "";
    position: absolute;
    bottom: 2px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: currentColor;
  }
  button.icon:hover {
    background: rgba(255, 255, 255, 0.1);
  }
  button.icon:active {
    transform: scale(0.94);
  }
  svg {
    width: 20px;
    height: 20px;
    fill: none;
    stroke: currentColor;
    stroke-width: 2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .small svg {
    width: 18px;
    height: 18px;
  }
  path.fill {
    fill: currentColor;
  }
  .play {
    width: 44px !important;
    height: 44px !important;
    background: linear-gradient(135deg, var(--c1), var(--c2)) !important;
    color: #fff;
    box-shadow: 0 6px 20px -6px var(--c1);
  }
  .play:hover {
    filter: brightness(1.1);
  }
  .one {
    position: absolute;
    top: 3px;
    right: 3px;
    font-size: 9px;
    font-weight: 800;
  }
  .vol {
    display: flex;
    align-items: center;
    gap: 2px;
    margin-left: 4px;
  }
  .vol input {
    width: 86px;
    height: 18px;
    appearance: none;
    background: transparent;
    cursor: pointer;
  }
  .vol input::-webkit-slider-runnable-track {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, #fff var(--p), rgba(255, 255, 255, 0.2) var(--p));
  }
  .vol input::-webkit-slider-thumb {
    appearance: none;
    width: 12px;
    height: 12px;
    margin-top: -4px;
    border-radius: 50%;
    background: #fff;
    opacity: 0;
    transition: opacity 160ms ease;
  }
  .vol:hover input::-webkit-slider-thumb,
  .vol input:focus-visible::-webkit-slider-thumb {
    opacity: 1;
  }
</style>
