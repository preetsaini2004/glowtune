<script lang="ts">
  // A release's notes as plain text blocks (never HTML): headings, bullets,
  // paragraphs. Used by the update pop-up and Settings → About.
  import { noteBlocks } from "$lib/update.svelte";

  let { notes }: { notes: string | null } = $props();
  const blocks = $derived(noteBlocks(notes));
</script>

{#if blocks.length}
  <div class="notes">
    {#each blocks as b, i (i)}
      {#if b.kind === "heading"}
        <p class="h">{b.text}</p>
      {:else if b.kind === "item"}
        <p class="li">{b.text}</p>
      {:else}
        <p>{b.text}</p>
      {/if}
    {/each}
  </div>
{:else}
  <p class="none">No notes for this version.</p>
{/if}

<style>
  .notes {
    display: grid;
    gap: 6px;
    font-size: 13px;
    line-height: 18px;
    color: var(--ink-soft);
    user-select: text;
  }
  p {
    margin: 0;
  }
  .h {
    margin-top: 6px;
    color: var(--ink);
    font-size: 11.5px;
    font-weight: 650;
    letter-spacing: 0.06em;
    text-transform: uppercase;
  }
  .h:first-child {
    margin-top: 0;
  }
  .li {
    position: relative;
    padding-left: 14px;
  }
  .li::before {
    content: "";
    position: absolute;
    left: 2px;
    top: 7px;
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: color-mix(in oklab, var(--c1) 70%, #fff);
  }
  .none {
    margin: 0;
    font-size: 13px;
    color: var(--ink-faint);
  }
</style>
