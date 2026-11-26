<script lang="ts">
  // The update pop-up: a glass card in the bottom-right corner, shown by
  // Rust (`update.rs`) when a new version is out. Never takes focus on its
  // own and never shows over a full-screen game.
  //
  // `/update?whatsnew` is the same card once, on the first start after an
  // update: what this version brought (window "whatsnew").
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { np } from "$lib/nowPlaying.svelte";
  import { updates, releaseDay } from "$lib/update.svelte";
  import ReleaseNotes from "$lib/ReleaseNotes.svelte";

  const whatsNew = new URLSearchParams(location.search).has("whatsnew");
  let news = $state<{ version: string; notes: string } | null>(null);

  const v = $derived(whatsNew ? null : updates.view);
  const busy = $derived(v?.status === "downloading" || v?.status === "installing");
  const palette = $derived(np.palette);
  const pct = $derived(Math.round((v?.progress ?? 0) * 100));

  onMount(() => {
    np.start();
    if (whatsNew) {
      invoke<{ version: string; notes: string }>("whats_new")
        .then((n) => (news = n))
        .catch(() => {});
    } else {
      updates.start();
    }
  });

  function gotIt() {
    invoke("close_whats_new").catch(() => {});
  }

  function onKey(e: KeyboardEvent) {
    if (e.key !== "Escape") return;
    if (whatsNew) gotIt();
    else if (!busy) updates.later();
  }
</script>

<svelte:window onkeydown={onKey} />

<main style:--c1={palette[0]} style:--c2={palette[1] ?? palette[0]}>
  {#if news}
    <header data-tauri-drag-region>
      <span class="mark" aria-hidden="true">
        <svg viewBox="0 0 24 24"><path d="M12 3.5l2.2 5.3 5.3 2.2-5.3 2.2L12 18.5l-2.2-5.3L4.5 11l5.3-2.2z" /></svg>
      </span>
      <div class="titles" data-tauri-drag-region>
        <p class="eyebrow">What's new</p>
        <h1>Glowtune {news.version}</h1>
        <p class="sub">Updated. Your settings, themes and sign-ins stayed.</p>
      </div>
      <button class="x" title="Close (Esc)" aria-label="Close" onclick={gotIt}>
        <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
      </button>
    </header>

    <section class="scroll" aria-label="What's new">
      <ReleaseNotes notes={news.notes} />
    </section>

    <footer>
      <div class="buttons">
        <span class="grow"></span>
        <button class="primary" onclick={gotIt}>Got it</button>
      </div>
    </footer>
  {:else if v}
    <header data-tauri-drag-region>
      <span class="mark" aria-hidden="true">
        <svg viewBox="0 0 24 24"><path d="M12 4v11M7 10l5 5 5-5M5 20h14" /></svg>
      </span>
      <div class="titles" data-tauri-drag-region>
        <p class="eyebrow">Update ready</p>
        <h1>Glowtune {v.version ?? ""}</h1>
        <p class="sub">
          {[`You have ${v.current}`, releaseDay(v.date) && `released ${releaseDay(v.date)}`].filter(Boolean).join(" · ")}
        </p>
      </div>
      {#if !busy}
        <button class="x" title="Later (Esc)" aria-label="Later" onclick={() => updates.later()}>
          <svg viewBox="0 0 24 24" aria-hidden="true"><path d="M6 6l12 12M18 6L6 18" /></svg>
        </button>
      {/if}
    </header>

    <section class="scroll" aria-label="What's new">
      <ReleaseNotes notes={v.notes} />
    </section>

    <footer>
      {#if v.status === "downloading" || v.status === "installing"}
        <div class="progress" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={pct}>
          <span style:width="{v.status === 'installing' ? 100 : pct}%"></span>
        </div>
        <p class="state">
          {v.status === "installing" ? "Restarting Glowtune…" : `Downloading… ${pct}%`}
        </p>
      {:else}
        {#if v.status === "error" && v.error}
          <p class="error" role="alert">{v.error}</p>
        {:else if np.playing}
          <p class="state">Your music pauses for a few seconds while Glowtune restarts.</p>
        {/if}
        <div class="buttons">
          {#if v.version}
            <button class="primary" onclick={() => updates.install()}>
              {v.status === "error" ? "Try again" : "Update now"}
            </button>
            <button onclick={() => updates.later()}>Later</button>
          {:else}
            <button onclick={() => updates.later()}>Close</button>
          {/if}
          <span class="grow"></span>
          {#if v.version}
            <button class="quiet" onclick={() => updates.skip(v.version!)}>Skip this version</button>
          {/if}
        </div>
      {/if}
    </footer>
  {/if}
</main>

<style>
  main {
    --font-ui: var(--font-app);
    font-family: var(--font-app);
    height: 100vh;
    display: grid;
    grid-template-rows: auto minmax(0, 1fr) auto;
    background:
      radial-gradient(120% 70% at 0% 0%, color-mix(in oklab, var(--c1) 26%, transparent), transparent 62%),
      radial-gradient(90% 60% at 100% 100%, color-mix(in oklab, var(--c2) 14%, transparent), transparent 70%),
      var(--tint);
    animation: rise 380ms cubic-bezier(0.2, 0.9, 0.25, 1) both;
  }
  @keyframes rise {
    from {
      opacity: 0;
      transform: translateY(14px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    main {
      animation: none;
    }
  }
  header {
    display: flex;
    gap: 14px;
    align-items: flex-start;
    padding: 20px 18px 14px 20px;
  }
  .mark {
    flex: none;
    display: grid;
    place-items: center;
    width: 44px;
    height: 44px;
    border-radius: 14px;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    box-shadow: 0 6px 22px color-mix(in oklab, var(--c1) 45%, transparent);
  }
  .mark svg {
    width: 22px;
    height: 22px;
    fill: none;
    stroke: #fff;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
  .titles {
    flex: 1;
    min-width: 0;
  }
  .eyebrow {
    margin: 0;
    font-size: 11px;
    font-weight: 650;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    color: color-mix(in oklab, var(--c1) 55%, #fff);
  }
  h1 {
    margin: 2px 0 0;
    font-family: var(--font-display);
    font-size: 23px;
    font-weight: 740;
    font-stretch: 90%;
    line-height: 1.15;
  }
  .sub {
    margin: 3px 0 0;
    font-size: 12.5px;
    color: var(--ink-faint);
  }
  .x {
    flex: none;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    margin: -4px -4px 0 0;
    border: 0;
    border-radius: 50%;
    background: transparent;
    cursor: pointer;
  }
  .x:hover {
    background: rgba(255, 255, 255, 0.08);
  }
  .x svg {
    width: 15px;
    height: 15px;
    fill: none;
    stroke: var(--ink-soft);
    stroke-width: 2;
    stroke-linecap: round;
  }
  .scroll {
    overflow-y: auto;
    margin: 0 8px;
    padding: 12px 12px 12px;
    border-top: 1px solid var(--hairline);
    border-bottom: 1px solid var(--hairline);
    mask-image: linear-gradient(to bottom, transparent, #000 10px, #000 calc(100% - 14px), transparent);
  }
  .scroll::-webkit-scrollbar {
    width: 6px;
  }
  .scroll::-webkit-scrollbar-thumb {
    border-radius: 3px;
    background: rgba(255, 255, 255, 0.14);
  }
  footer {
    display: grid;
    gap: 10px;
    padding: 14px 18px 18px 20px;
  }
  .buttons {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .grow {
    flex: 1;
  }
  button {
    height: 32px;
    padding: 0 14px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.7);
    background: rgba(255, 255, 255, 0.07);
    font-size: 13px;
    cursor: pointer;
  }
  button:hover {
    background: rgba(255, 255, 255, 0.12);
  }
  button:focus-visible {
    outline: 2px solid color-mix(in oklab, var(--c1) 70%, #fff);
    outline-offset: 2px;
  }
  .primary {
    border-color: transparent;
    background: linear-gradient(135deg, var(--c1), color-mix(in oklab, var(--c2) 80%, var(--c1)));
    font-weight: 620;
    color: #fff;
    box-shadow: 0 4px 16px color-mix(in oklab, var(--c1) 40%, transparent);
  }
  .primary:hover {
    background: linear-gradient(135deg, color-mix(in oklab, var(--c1) 85%, #fff), var(--c2));
  }
  .quiet {
    padding: 0 6px;
    border: 0;
    background: none;
    color: var(--ink-faint);
    font-size: 12.5px;
  }
  .quiet:hover {
    background: none;
    color: var(--ink-soft);
    text-decoration: underline;
  }
  .state,
  .error {
    margin: 0;
    font-size: 12.5px;
    line-height: 17px;
    color: var(--ink-soft);
  }
  .error {
    color: #ffb4b4;
  }
  .progress {
    height: 6px;
    border-radius: 3px;
    overflow: hidden;
    background: rgba(255, 255, 255, 0.08);
  }
  .progress span {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: linear-gradient(90deg, var(--c1), var(--c2));
    transition: width 240ms ease;
  }
</style>
