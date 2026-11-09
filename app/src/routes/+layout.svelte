<script lang="ts">
  import "@fontsource-variable/instrument-sans/standard.css";
  import "@fontsource-variable/bricolage-grotesque/standard.css";
  import { onMount } from "svelte";
  import { theme } from "$lib/theme.svelte";
  let { children } = $props();
  onMount(() => {
    theme.start();
  });
</script>

{@render children()}

<style>
  :global(:root) {
    /* Glowtune tokens. --c1/--c2 are replaced live with the album palette. */
    --c1: #8b7cff;
    --c2: #5ca0ff;
    --ink: #f2eff7;
    --ink-soft: rgba(242, 239, 247, 0.62);
    --ink-faint: rgba(242, 239, 247, 0.38);
    --hairline: rgba(255, 255, 255, 0.08);
    --tint: rgba(20, 17, 28, 0.5);
    --surface: rgba(255, 255, 255, 0.08);
    --accent: #8b7cff;
    --radius: 14px;
    --lyrics-size: 50px;
    --font-ui: "Instrument Sans Variable", "Segoe UI Variable Text", system-ui, sans-serif;
    --font-lyrics: "Bricolage Grotesque Variable", "Segoe UI Variable Display", system-ui, sans-serif;
    color-scheme: dark;
    font-family: var(--font-ui);
    color: var(--ink);
    -webkit-font-smoothing: antialiased;
    text-rendering: optimizeLegibility;
  }
  /* Windows are transparent: the glass comes from the OS (acrylic). */
  :global(html),
  :global(body) {
    margin: 0;
    height: 100%;
    background: transparent;
    overflow: hidden;
    user-select: none;
  }
  :global(*) {
    box-sizing: border-box;
  }
  :global(button) {
    font: inherit;
    color: inherit;
  }
  :global(:focus-visible) {
    outline: 2px solid var(--c1);
    outline-offset: 2px;
  }
  /* Slim scrollbars that sit on the glass instead of a grey gutter. */
  :global(::-webkit-scrollbar) {
    width: 10px;
  }
  :global(::-webkit-scrollbar-track) {
    background: transparent;
  }
  :global(::-webkit-scrollbar-thumb) {
    border: 3px solid transparent;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.16) padding-box;
  }
  :global(::-webkit-scrollbar-thumb:hover) {
    background: rgba(255, 255, 255, 0.28) padding-box;
  }
  :global(input[type="range"]) {
    appearance: none;
    height: 20px;
    background: transparent;
    accent-color: var(--c1);
  }
  :global(input[type="range"]::-webkit-slider-runnable-track) {
    height: 4px;
    border-radius: 2px;
    background: linear-gradient(90deg, var(--c1), var(--c2));
  }
  :global(input[type="range"]::-webkit-slider-thumb) {
    appearance: none;
    width: 16px;
    height: 16px;
    margin-top: -6px;
    border-radius: 50%;
    background: var(--ink);
    box-shadow:
      0 0 0 3px rgba(0, 0, 0, 0.25),
      0 0 12px var(--c1);
  }
  @media (prefers-reduced-motion: reduce) {
    :global(*) {
      animation-duration: 1ms !important;
      transition-duration: 1ms !important;
    }
  }
</style>
