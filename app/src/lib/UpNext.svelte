<script lang="ts">
  import { invoke } from "@tauri-apps/api/core";
  import { np } from "./nowPlaying.svelte";
  import { type Queue, type MusicItem, SERVICE_NAMES } from "./musicBrowser";
  let { visible }: { visible: boolean } = $props();
  let queue = $state<Queue>({ items: [], message: "Loading the player’s queue…" });
  let busy = $state(false);
  let sequence = 0;
  // Play a song from the queue (opens its exact link in the service window).
  async function play(item: MusicItem) {
    try {
      await invoke("open_music_item", { item });
      setTimeout(refresh, 1500);
    } catch {}
  }
  async function refresh() {
    const request = ++sequence;
    busy = true;
    try {
      const result = await invoke<Queue>("get_music_queue");
      if (request === sequence) queue = result;
    } catch { if (request === sequence) queue = { items: [], message: "Queue unavailable. Try refreshing." }; }
    finally { if (request === sequence) busy = false; }
  }
  $effect(() => {
    void np.track?.title;
    void np.track?.source;
    queue = { items: [], message: "Loading the player’s queue…" };
    refresh();
  });
  $effect(() => {
    if (!visible || !np.playing) return;
    const timer = setInterval(refresh, 10_000);
    return () => clearInterval(timer);
  });
</script>

<div class="toolbar"><span>{queue.label ?? "Player queue"}</span><button disabled={busy} onclick={refresh}>{busy ? "Refreshing…" : "Refresh"}</button></div>
{#if queue.items.length}
  <ol>{#each queue.items.slice(0, 9) as item, i (item.service + item.url + i)}
    <li><button class="row" title={`Play “${item.title}”`} onclick={() => play(item)}><span class="number">{i + 1}</span>{#if item.artwork}<img src={item.artwork} alt="" />{:else}<span class="placeholder">♫</span>{/if}<span class="info"><strong>{item.title}</strong><span>{item.artist}</span><small>{SERVICE_NAMES[item.service]}</small></span></button></li>
  {/each}</ol>
{:else}<p class="empty">{queue.message}</p>{/if}

<style>
  .toolbar { display:flex;align-items:center;justify-content:space-between;gap:8px;margin:0 8px 12px;color:#ffffff88;font-size:11px; }
  button { font:inherit;color:inherit;cursor:pointer;border:1px solid #ffffff24;border-radius:8px;background:#ffffff08;padding:5px 8px; } button:disabled { opacity:.6;cursor:default; } button:focus-visible { outline:2px solid var(--c1); }
  ol { list-style:none;margin:0;padding:0; } .row { width:100%;display:grid;grid-template-columns:14px 40px minmax(0,1fr);gap:8px;align-items:center;padding:7px 8px;border:0;border-radius:12px;background:transparent;text-align:left;transition:background 160ms ease; } .row:hover { background:rgba(255,255,255,.07); } li:first-child .row { background:color-mix(in oklab,var(--c1) 14%,transparent); }
  img,.placeholder { width:40px;height:40px;object-fit:cover;border-radius:8px;background:#ffffff10; } .placeholder { display:grid;place-items:center; } .number { font-size:11px;color:#ffffff70; } .info { display:grid;gap:2px;min-width:0; } strong,.info span,small { overflow:hidden;white-space:nowrap;text-overflow:ellipsis; } strong { font-size:13px;font-weight:600; } .info span { font-size:12px;color:#ffffff99; } small { font-size:10px;color:#ffffff70; } .empty { font-size:12px;line-height:1.7;color:#ffffff99;padding:12px 10px; }
</style>
