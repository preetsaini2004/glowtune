<script lang="ts">
  // Settings › Feedback: a message straight to the developer's inbox (lib/feedback.ts).
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import {
    KINDS,
    AREAS,
    BODY_MIN,
    TITLE_MAX,
    NAME_MAX,
    problem,
    send,
    flushQueue,
    sendsLeft,
    sentTimes,
    type Kind,
    type InfoLine,
  } from "./feedback";

  let kind = $state<Kind>("bug");
  let area = $state<string>("Lyrics");
  let rating = $state(0);
  let title = $state("");
  let body = $state("");
  let name = $state(remembered());
  let email = $state("");
  let publish = $state(false);

  // The name is kept for next time (this window's storage; failures fine).
  function remembered(): string {
    try {
      return localStorage.getItem("glowtune.feedback.name") ?? "";
    } catch {
      return "";
    }
  }
  function remember(n: string) {
    try {
      localStorage.setItem("glowtune.feedback.name", n.trim());
    } catch {
      /* nothing to keep */
    }
  }
  let withInfo = $state(true);
  let info = $state<InfoLine[]>([]);
  let showInfo = $state(false);

  let busy = $state(false);
  let note = $state("");
  let warn = $state(false);
  /** Set after a send: the form gives way to a thank-you. */
  let done = $state<"" | "sent" | "queued">("");
  let left = $state(sendsLeft(sentTimes(), Date.now()));

  const placeholder = $derived(
    kind === "bug"
      ? "What happened, and what were you doing? Which song or app was playing?"
      : kind === "idea"
        ? "What would you like Glowtune to do?"
        : kind === "review"
          ? "What do you like? What would make it better?"
          : "Tell us what's on your mind.",
  );

  onMount(() => {
    invoke<InfoLine[]>("feedback_info").then((l) => (info = l)).catch(() => {});
    // Messages written while offline go out now.
    const flush = () =>
      flushQueue()
        .then((n) => {
          if (n > 0) note = n === 1 ? "Your earlier message was sent." : `Your ${n} earlier messages were sent.`;
        })
        .catch(() => {});
    flush();
    window.addEventListener("online", flush);
    return () => window.removeEventListener("online", flush);
  });

  async function submit(e: SubmitEvent) {
    e.preventDefault();
    const draft = { kind, area, rating, title, body, name, email, publish: kind === "review" && publish, info: withInfo ? info : [] };
    const why = problem(draft);
    if (why) {
      note = why;
      warn = true;
      return;
    }
    left = sendsLeft(sentTimes(), Date.now());
    if (left === 0) {
      note = "That's a few messages this hour. Try again a little later.";
      warn = true;
      return;
    }
    busy = true;
    note = "";
    warn = false;
    try {
      done = await send(draft);
      remember(name);
      left = sendsLeft(sentTimes(), Date.now());
    } catch (err) {
      note = err instanceof Error ? err.message : String(err);
      warn = true;
    } finally {
      busy = false;
    }
  }

  function another() {
    done = "";
    title = "";
    body = "";
    rating = 0;
    publish = false;
    note = "";
    warn = false;
  }
</script>

{#if done}
  <div class="card thanks" role="status">
    <h3>{done === "sent" ? "Thanks! We got your message." : "Saved. It'll be sent when you're back online."}</h3>
    <p class="hint">
      {#if email.trim()}If there's a reply, it goes to {email.trim()}.{:else}Leave an email next time if you'd like a reply.{/if}
    </p>
    <button class="btn" onclick={another}>Send another</button>
  </div>
{:else}
  <form class="card" onsubmit={submit} novalidate>
    <div class="group">
      <span class="label" id="fb-kind">What's it about?</span>
      <div class="chips" role="radiogroup" aria-labelledby="fb-kind">
        {#each KINDS as k}
          <button type="button" role="radio" aria-checked={kind === k.id} class:on={kind === k.id} onclick={() => (kind = k.id)}>{k.name}</button>
        {/each}
      </div>
    </div>

    {#if kind === "review"}
      <div class="group">
        <span class="label" id="fb-stars">Your rating</span>
        <div class="stars" role="radiogroup" aria-labelledby="fb-stars">
          {#each [1, 2, 3, 4, 5] as n}
            <button type="button" role="radio" aria-checked={rating === n} aria-label="{n} of 5" class:on={rating >= n} onclick={() => (rating = n)}>★</button>
          {/each}
        </div>
      </div>
    {/if}

    <label class="group">
      <span class="label">Your name <em>(a nickname is fine)</em></span>
      <input bind:value={name} maxlength={NAME_MAX} placeholder="What should we call you?" autocomplete="nickname" spellcheck="false" />
    </label>

    <label class="group">
      <span class="label">Part of Glowtune</span>
      <select bind:value={area}>
        {#each AREAS as a}<option value={a}>{a}</option>{/each}
      </select>
    </label>

    <label class="group">
      <span class="label">Title</span>
      <input bind:value={title} maxlength={TITLE_MAX} placeholder="One line, like “Lyrics don't show with Spotify”" spellcheck="true" />
    </label>

    <label class="group">
      <span class="label">Message</span>
      <textarea bind:value={body} rows="6" {placeholder} spellcheck="true"></textarea>
      {#if body.trim().length > 0 && body.trim().length < BODY_MIN}<span class="hint small">A little more, please.</span>{/if}
    </label>

    <label class="group">
      <span class="label">Your email <em>(optional, only if you'd like a reply)</em></span>
      <input type="email" bind:value={email} placeholder="you@example.com" autocomplete="email" spellcheck="false" />
    </label>

    {#if kind === "review"}
      <label class="check publish">
        <input type="checkbox" bind:checked={publish} />
        <span>You can show this review on Glowtune's website, with my name</span>
      </label>
    {/if}

    <div class="info">
      <label class="check">
        <input type="checkbox" bind:checked={withInfo} />
        <span>Include app info</span>
      </label>
      <button type="button" class="link" aria-expanded={showInfo} onclick={() => (showInfo = !showInfo)}>{showInfo ? "Hide" : "See what's sent"}</button>
    </div>
    {#if showInfo}
      <dl class="lines" class:off={!withInfo}>
        {#each info as l}<dt>{l.name}</dt><dd>{l.value}</dd>{/each}
      </dl>
      <p class="hint small">Only this and what you typed. Never your songs, history or anything personal.</p>
    {/if}

    <div class="send">
      <button class="btn primary" type="submit" disabled={busy}>{busy ? "Sending…" : "Send feedback"}</button>
      {#if note}<p class="hint" class:warn role={warn ? "alert" : "status"}>{note}</p>{/if}
    </div>
  </form>
{/if}

<style>
  .card {
    margin: 0;
    padding: 16px 18px;
    border: 1px solid var(--hairline);
    border-radius: var(--radius);
    background: rgba(255, 255, 255, 0.04);
    display: grid;
    gap: 14px;
    min-width: 0;
  }
  h3 {
    margin: 0;
    font-size: 14.5px;
    font-weight: 620;
  }
  .group {
    display: grid;
    gap: 6px;
    min-width: 0;
  }
  .label {
    font-size: 13px;
    font-weight: 600;
  }
  .label em {
    font-style: normal;
    font-weight: 400;
    color: var(--ink-soft);
  }
  .hint {
    margin: 0;
    font-size: 13px;
    line-height: 18px;
    color: var(--ink-soft);
  }
  .hint.small {
    font-size: 12px;
  }
  .hint.warn {
    color: #ffcf9a;
  }
  input:not([type="checkbox"]),
  textarea,
  select {
    width: 100%;
    box-sizing: border-box;
    padding: 8px 10px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.65);
    background: rgba(0, 0, 0, 0.22);
    color: inherit;
    font: inherit;
    font-size: 13.5px;
  }
  textarea {
    resize: vertical;
    min-height: 110px;
    line-height: 1.45;
  }
  select option {
    background: #1d1d22;
  }
  input:focus-visible,
  textarea:focus-visible,
  select:focus-visible,
  button:focus-visible {
    outline: 2px solid color-mix(in oklab, var(--c1) 70%, #fff);
    outline-offset: 1px;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chips button {
    border: 1px solid var(--hairline);
    background: transparent;
    border-radius: 999px;
    padding: 6px 13px;
    font-size: 12.5px;
    font-weight: 600;
    color: inherit;
    cursor: pointer;
  }
  .chips button.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    border-color: transparent;
  }
  .stars {
    display: flex;
    gap: 2px;
  }
  .stars button {
    border: 0;
    background: none;
    font-size: 24px;
    line-height: 1;
    padding: 2px;
    color: rgba(255, 255, 255, 0.22);
    cursor: pointer;
  }
  .stars button.on {
    color: #ffd36b;
  }
  .info {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
  }
  .check {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 13px;
    cursor: pointer;
  }
  .check input {
    width: 16px;
    height: 16px;
    margin: 0;
    accent-color: var(--c1);
  }
  .link {
    border: 0;
    background: none;
    padding: 0;
    color: color-mix(in oklab, var(--c1) 55%, #fff);
    font-size: 12.5px;
    cursor: pointer;
  }
  .lines {
    margin: 0;
    padding: 10px 12px;
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    gap: 4px 14px;
    border-radius: calc(var(--radius) * 0.65);
    background: rgba(0, 0, 0, 0.2);
    font-size: 12.5px;
  }
  .lines.off {
    opacity: 0.4;
    text-decoration: line-through;
  }
  .lines dt {
    color: var(--ink-soft);
  }
  .lines dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .send {
    display: flex;
    align-items: center;
    gap: 14px;
    flex-wrap: wrap;
  }
  .btn {
    height: 32px;
    padding: 0 16px;
    border: 1px solid var(--hairline);
    border-radius: calc(var(--radius) * 0.65);
    background: rgba(255, 255, 255, 0.06);
    color: inherit;
    font-size: 13px;
    cursor: pointer;
    justify-self: start;
  }
  .btn.primary {
    border-color: transparent;
    background: linear-gradient(135deg, var(--c1), var(--c2));
    font-weight: 650;
  }
  .btn:disabled {
    opacity: 0.6;
    cursor: default;
  }
  .thanks h3 {
    font-size: 16px;
  }
</style>
