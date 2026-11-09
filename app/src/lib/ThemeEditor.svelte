<script lang="ts">
  // Theme picker + editor. Every edit previews live (all windows and the
  // edge glow) without saving; "Save" writes a TOML file to the themes folder.
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";
  import { listen } from "@tauri-apps/api/event";
  import { open, save } from "@tauri-apps/plugin-dialog";
  import { theme as active, FONT_STACKS, type Theme } from "$lib/theme.svelte";

  type ThemeInfo = { id: string; name: string; author: string; bundled: boolean; swatch: string[] };

  type Preferences = { useTheme: boolean; disabledThemes: string[] };
  let preferences = $state<Preferences | null>(null);
  const enabled = $derived(preferences?.useTheme ?? true);
  const disabled = $derived(preferences?.disabledThemes ?? []);
  let themes = $state<ThemeInfo[]>([]);
  let activeId = $state("");
  let draft = $state<Theme | null>(null);
  let dirty = $state(false);
  let notice = $state("");
  let previewTimer: ReturnType<typeof setTimeout> | undefined;

  async function refresh() {
    themes = await invoke<ThemeInfo[]>("list_themes");
    preferences = await invoke<Preferences>("get_settings");
    activeId = await invoke<string>("active_theme_id");
  }

  onMount(() => {
    refresh();
    invoke<Theme>("get_theme").then((t) => (draft = structuredClone(t)));
    // Hot reload: a theme file changed on disk.
    const un = listen("themes-changed", () => refresh());
    const unSettings = listen<Preferences>("settings", (e) => {
      preferences = e.payload;
      refresh();
    });
    return () => {
      clearTimeout(previewTimer);
      un.then((f) => f());
      unSettings.then((f) => f());
    };
  });

  async function choose(id: string) {
    if (dirty && !confirm("Discard your unsaved theme changes?")) return;
    clearTimeout(previewTimer);
    try {
      const t = await invoke<Theme>("select_theme", { id });
      draft = structuredClone(t);
      activeId = id;
      dirty = false;
      notice = "";
    } catch (e) {
      notice = String(e);
    }
  }

  async function changePreferences(patch: Partial<Preferences>) {
    clearTimeout(previewTimer);
    try {
      if (dirty) await invoke<Theme>("select_theme", { id: activeId });
      const current = await invoke<Preferences>("get_settings");
      preferences = await invoke<Preferences>("set_settings", { new: { ...current, ...patch } });
      // Reload the saved theme after toggles, rather than editing the default fallback.
      draft = structuredClone(await invoke<Theme>("get_theme"));
      dirty = false;
      notice = "";
    } catch (e) {
      notice = String(e);
    }
  }

  async function toggleTheme(id: string, on: boolean) {
    if (dirty && !confirm("Discard your unsaved theme changes?")) return;
    try {
      const current = await invoke<Preferences>("get_settings");
      await changePreferences({
        disabledThemes: on ? current.disabledThemes.filter((key) => key !== id) : [...new Set([...current.disabledThemes, id])],
      });
    } catch (e) {
      notice = String(e);
    }
  }

  /** Live preview, coalesced while dragging sliders or color pickers. */
  function edit() {
    dirty = true;
    clearTimeout(previewTimer);
    const snapshot = draft ? $state.snapshot(draft) : null;
    previewTimer = setTimeout(async () => {
      if (!snapshot) return;
      const warnings = await invoke<string[]>("preview_theme", { theme: snapshot });
      notice = warnings.length ? warnings.join(" · ") : "";
    }, 60);
  }

  async function saveDraft() {
    if (!draft) return;
    try {
      activeId = await invoke<string>("save_theme", { theme: $state.snapshot(draft) });
      dirty = false;
      notice = `Saved “${draft.meta.name}”.`;
      await refresh();
    } catch (e) {
      notice = String(e);
    }
  }

  async function revert() {
    await choose(activeId);
  }

  async function remove(id: string, name: string) {
    if (!confirm(`Delete the theme “${name}”? This removes its file.`)) return;
    try {
      await invoke("delete_theme", { id });
      await refresh();
      if (id === activeId) await choose("midnight-glass");
    } catch (e) {
      notice = String(e);
    }
  }

  async function importFile() {
    const path = await open({ multiple: false, filters: [{ name: "Glowtune theme", extensions: ["toml"] }] });
    if (typeof path !== "string") return;
    try {
      const id = await invoke<string>("import_theme", { path });
      await refresh();
      await choose(id);
      notice = "Theme imported.";
    } catch (e) {
      notice = String(e);
    }
  }

  async function exportFile() {
    const path = await save({ defaultPath: `${activeId}.toml`, filters: [{ name: "Glowtune theme", extensions: ["toml"] }] });
    if (!path) return;
    try {
      await invoke("export_theme", { id: activeId, path });
      notice = "Theme exported.";
    } catch (e) {
      notice = String(e);
    }
  }

  function setGlowColor(i: number, value: string) {
    if (!draft) return;
    draft.colors.glow[i] = value;
    edit();
  }
  function addGlowColor() {
    if (!draft || draft.colors.glow.length >= 5) return;
    draft.colors.glow.push(draft.colors.accent);
    edit();
  }
  function removeGlowColor(i: number) {
    if (!draft || draft.colors.glow.length <= 1) return;
    draft.colors.glow.splice(i, 1);
    edit();
  }

  /** `<input type=color>` only understands #rrggbb. */
  const rgb = (hex: string) => (hex.length >= 7 ? hex.slice(0, 7) : "#000000");
  const isActiveBundled = $derived(themes.find((t) => t.id === activeId)?.bundled ?? true);
</script>

<label class="check">
  <input type="checkbox" checked={enabled} onchange={async (e) => {
    const input = e.currentTarget;
    if (!dirty || confirm("Discard your unsaved theme changes?")) await changePreferences({ useTheme: input.checked });
    input.checked = enabled;
  }} />
  <span>Use a theme</span>
</label>
<p class="hint">Turn off to use Glowtune’s default look. Your selected theme and files are kept.</p>

{#if enabled && disabled.includes(activeId)}
  <p class="hint">Your selected theme is disabled. Enable it below or pick another theme.</p>
{/if}

<div class="themes" role="radiogroup" aria-label="Themes">
  {#each themes.filter((t) => !disabled.includes(t.id)) as t (t.id)}
    <div class="theme" class:on={enabled && t.id === activeId}>
      <button role="radio" disabled={!enabled} aria-checked={enabled && t.id === activeId} onclick={() => choose(t.id)}>
        <span class="swatch" aria-hidden="true">
          {#each t.swatch as c}<i style:background={c}></i>{/each}
        </span>
        <span class="name">{t.name}</span>
      </button>
      {#if !t.bundled}
        <button class="delete" aria-label={`Delete ${t.name}`} onclick={() => remove(t.id, t.name)}>
          <svg viewBox="0 0 24 24"><path d="M7 7l10 10M17 7 7 17" /></svg>
        </button>
      {/if}
    </div>
  {/each}
</div>

<details>
  <summary>Manage installed themes</summary>
  <p class="hint">Disabled themes stay on disk and disappear from the picker. Re-enable them here.</p>
  {#each themes.filter((t) => !t.bundled) as t (t.id)}
    <label class="check">
      <input type="checkbox" checked={!disabled.includes(t.id)} onchange={async (e) => {
        const input = e.currentTarget;
        await toggleTheme(t.id, input.checked);
        input.checked = !disabled.includes(t.id);
      }} />
      <span>{t.name}</span>
    </label>
  {:else}
    <p class="hint">Import a theme or install one from The Crate.</p>
  {/each}
</details>

{#if !draft || !enabled || disabled.includes(activeId)}
  <p class="notice" role="status">{notice}</p>
{/if}

<div class="files">
  <button class="link" onclick={importFile}>Import theme</button>
  <button class="link" onclick={exportFile}>Export this theme</button>
</div>

{#if draft && enabled && !disabled.includes(activeId)}
  <div class="editor">
    <label class="field wide">
      <span>Name</span>
      <input type="text" maxlength="40" bind:value={draft.meta.name} oninput={edit} />
    </label>

    <h3>Colors</h3>
    <div class="segmented" role="radiogroup" aria-label="Glow colors">
      {#each [["adaptive", "From the album"], ["gradient", "My gradient"], ["static", "One color"]] as [mode, label]}
        <button
          role="radio"
          aria-checked={draft.colors.mode === mode}
          class:on={draft.colors.mode === mode}
          onclick={() => {
            if (draft) draft.colors.mode = mode as Theme["colors"]["mode"];
            edit();
          }}>{label}</button
        >
      {/each}
    </div>

    {#if draft.colors.mode !== "adaptive"}
      <div class="colors">
        {#each draft.colors.mode === "static" ? draft.colors.glow.slice(0, 1) : draft.colors.glow as c, i}
          <span class="chip">
            <input type="color" value={rgb(c)} aria-label={`Glow color ${i + 1}`} oninput={(e) => setGlowColor(i, e.currentTarget.value)} />
            {#if draft.colors.mode === "gradient" && draft.colors.glow.length > 1}
              <button class="chip-x" aria-label={`Remove color ${i + 1}`} onclick={() => removeGlowColor(i)}>×</button>
            {/if}
          </span>
        {/each}
        {#if draft.colors.mode === "gradient" && draft.colors.glow.length < 5}
          <button class="add" onclick={addGlowColor}>Add color</button>
        {/if}
      </div>
    {/if}

    <div class="grid">
      <label class="field"><span>Glass tint</span><input type="color" value={rgb(draft.colors.background)} oninput={(e) => { if (draft) draft.colors.background = e.currentTarget.value; edit(); }} /></label>
      <label class="field"><span>Text</span><input type="color" value={rgb(draft.colors.text)} oninput={(e) => { if (draft) draft.colors.text = e.currentTarget.value; edit(); }} /></label>
    </div>
    <label class="slider"><span>Glass tint strength</span><input type="range" min="0" max="1" step="0.05" bind:value={draft.glass.opacity} oninput={edit} /></label>
    <label class="slider"><span>Corner radius</span><input type="range" min="0" max="40" step="1" bind:value={draft.glass.radius} oninput={edit} /></label>

    <h3>Glow</h3>
    <label class="field wide">
      <span>Light formation</span>
      <select bind:value={draft.glow.effect} onchange={edit}>
        <option value="ambient">Ambient · original glow</option>
        <option value="still">Still · steady light</option>
        <option value="pulse">Corner pulse · expand with the beat</option>
        <option value="orbit">Travelling swell · move around the edges</option>
        <option value="distortion">Distortion · shifting ribbons</option>
        <option value="lava-lamp">Lava lamp · merging light blobs</option>
      </select>
    </label>
    <p class="hint">Changes the shape and position of the light. Each theme remembers its own effect.</p>
    <label class="slider"><span>Formation speed</span><input type="range" min="0" max="3" step="0.1" bind:value={draft.glow.effect_speed} disabled={draft.glow.effect === "still" || draft.glow.effect === "pulse"} oninput={edit} /></label>
    <label class="slider"><span>Formation strength</span><input type="range" min="0" max="1" step="0.05" bind:value={draft.glow.effect_strength} disabled={draft.glow.effect === "still"} oninput={edit} /></label>
    <label class="check">
      <input type="checkbox" bind:checked={draft.glow.reverse_motion} disabled={draft.glow.effect === "still"} onchange={edit} />
      <span>Reverse motion</span>
    </label>
    <label class="slider"><span>Brightness</span><input type="range" min="0.2" max="1.5" step="0.05" bind:value={draft.glow.intensity} oninput={edit} /></label>
    <label class="slider"><span>Thickness</span><input type="range" min="20" max="120" step="2" bind:value={draft.glow.thickness} oninput={edit} /></label>
    <label class="slider"><span>Beat reaction</span><input type="range" min="0" max="1" step="0.05" bind:value={draft.glow.beat_reactivity} disabled={draft.glow.effect === "still"} oninput={edit} /></label>
    <label class="slider"><span>Color drift</span><input type="range" min="0" max="6" step="0.1" bind:value={draft.glow.drift_speed} disabled={draft.glow.effect === "still"} oninput={edit} /></label>
    <label class="check">
      <input type="checkbox" checked={draft.glow.corners === "rounded"} onchange={(e) => { if (draft) draft.glow.corners = e.currentTarget.checked ? "rounded" : "square"; edit(); }} />
      <span>Rounded glow corners</span>
    </label>

    <h3>Particles</h3>
    <label class="field wide">
      <span>Particle style</span>
      <select bind:value={draft.glow.particles} onchange={edit}>
        <option value="off">Off</option>
        <option value="dots">Dots</option>
        <option value="stars">Stars</option>
        <option value="dust">Dust</option>
      </select>
    </label>
    <p class="hint">Small lights follow the glow’s colors, formation and beats. Still freezes their motion too.</p>
    {#if draft.glow.particles !== "off"}
      <label class="slider"><span>Particle amount</span><input type="range" min="0" max="1" step="0.05" bind:value={draft.glow.particle_density} oninput={edit} /></label>
      <label class="slider"><span>Particle size</span><input type="range" min="0.5" max="3" step="0.1" bind:value={draft.glow.particle_size} oninput={edit} /></label>
      <label class="slider"><span>Particle speed</span><input type="range" min="0" max="3" step="0.1" bind:value={draft.glow.particle_speed} disabled={draft.glow.effect === "still"} oninput={edit} /></label>
      <label class="slider"><span>Sparkle</span><input type="range" min="0" max="1" step="0.05" bind:value={draft.glow.particle_sparkle} disabled={draft.glow.effect === "still"} oninput={edit} /></label>
    {/if}

    <h3>Lyrics</h3>
    <div class="segmented" role="radiogroup" aria-label="Lyrics animation">
      {#each [["rise", "Rise"], ["karaoke", "Karaoke"], ["static", "Still"]] as [style, label]}
        <button
          role="radio"
          aria-checked={draft.lyrics.style === style}
          class:on={draft.lyrics.style === style}
          onclick={() => {
            if (draft) draft.lyrics.style = style as Theme["lyrics"]["style"];
            edit();
          }}>{label}</button
        >
      {/each}
    </div>
    <div class="segmented" role="radiogroup" aria-label="Lyrics position">
      {#each [["bottom-center", "Bottom"], ["center", "Middle"], ["top-center", "Top"]] as [pos, label]}
        <button
          role="radio"
          aria-checked={draft.lyrics.position === pos}
          class:on={draft.lyrics.position === pos}
          onclick={() => {
            if (draft) draft.lyrics.position = pos as Theme["lyrics"]["position"];
            edit();
          }}>{label}</button
        >
      {/each}
    </div>
    <label class="slider"><span>Lyrics size</span><input type="range" min="16" max="96" step="1" bind:value={draft.font.lyrics_size} oninput={edit} /></label>
    <div class="grid">
      <label class="field">
        <span>Lyrics font</span>
        <select bind:value={draft.font.lyrics_family} onchange={edit}>
          {#each Object.keys(FONT_STACKS) as f}<option value={f}>{f}</option>{/each}
        </select>
      </label>
      <label class="field">
        <span>Interface font</span>
        <select bind:value={draft.font.family} onchange={edit}>
          {#each Object.keys(FONT_STACKS) as f}<option value={f}>{f}</option>{/each}
        </select>
      </label>
    </div>
  </div>

  <footer>
    <p class="notice" aria-live="polite">
      {notice || (dirty ? (isActiveBundled ? "Saving creates your own copy of this theme." : "Unsaved changes.") : "")}
    </p>
    <div class="actions">
      <button class="ghost" disabled={!dirty} onclick={revert}>Revert</button>
      <button class="primary" disabled={!dirty} onclick={saveDraft}>Save theme</button>
    </div>
  </footer>
{/if}

<style>
  details { margin: 20px 0; }
  summary { cursor: pointer; }
  .hint { color: var(--ink-soft); font-size: 12px; line-height: 1.5; }
  button:disabled { opacity: 0.5; cursor: default; }
  .themes {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 8px;
  }
  .theme {
    position: relative;
  }
  .theme > button[role="radio"] {
    width: 100%;
    padding: 10px;
    border: 1px solid var(--hairline);
    border-radius: 10px;
    background: var(--surface);
    display: grid;
    gap: 8px;
    text-align: left;
    cursor: pointer;
  }
  .theme.on > button[role="radio"] {
    border-color: var(--c1);
    box-shadow: 0 0 0 1px var(--c1), 0 0 18px -6px var(--c1);
  }
  .swatch {
    display: flex;
    height: 22px;
    border-radius: 6px;
    overflow: hidden;
  }
  .swatch i {
    flex: 1;
  }
  .name {
    font-size: 13px;
    font-weight: 560;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .delete {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 20px;
    height: 20px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    display: grid;
    place-items: center;
    background: rgba(30, 26, 40, 0.95);
    color: var(--ink-soft);
    cursor: pointer;
    opacity: 0;
  }
  .theme:hover .delete,
  .delete:focus-visible {
    opacity: 1;
  }
  .delete svg {
    width: 10px;
    height: 10px;
    stroke: currentColor;
    stroke-width: 2.6;
    fill: none;
  }

  .files {
    display: flex;
    gap: 16px;
    margin: 10px 0 4px;
  }
  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--ink-soft);
    font-size: 13px;
    text-decoration: underline;
    text-underline-offset: 3px;
    cursor: pointer;
  }
  .link:hover {
    color: var(--ink);
  }

  .editor {
    display: grid;
    gap: 10px;
    margin-top: 6px;
  }
  h3 {
    margin: 12px 0 0;
    font-size: 14px;
    font-weight: 600;
  }
  .field {
    display: grid;
    gap: 5px;
    font-size: 12.5px;
    color: var(--ink-soft);
  }
  .field.wide {
    margin-top: 8px;
  }
  .grid {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
  }
  input[type="text"],
  select {
    height: 34px;
    padding: 0 10px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.25);
    color: var(--ink);
    font: inherit;
    font-size: 13.5px;
  }
  input[type="color"] {
    width: 100%;
    height: 34px;
    padding: 2px;
    border: 1px solid var(--hairline);
    border-radius: 8px;
    background: rgba(0, 0, 0, 0.25);
    cursor: pointer;
  }

  .segmented {
    display: flex;
    padding: 3px;
    border-radius: 10px;
    background: rgba(0, 0, 0, 0.25);
  }
  .segmented button {
    flex: 1;
    height: 30px;
    border: 0;
    border-radius: 8px;
    background: transparent;
    color: var(--ink-soft);
    font-size: 13px;
    cursor: pointer;
  }
  .segmented button.on {
    background: var(--surface);
    color: var(--ink);
    box-shadow: inset 0 0 0 1px var(--hairline);
  }

  .colors {
    display: flex;
    flex-wrap: wrap;
    gap: 8px;
    align-items: center;
  }
  .chip {
    position: relative;
    width: 56px;
  }
  .chip-x {
    position: absolute;
    top: -6px;
    right: -6px;
    width: 18px;
    height: 18px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: rgba(30, 26, 40, 0.95);
    color: var(--ink-soft);
    font-size: 12px;
    line-height: 18px;
    cursor: pointer;
  }
  .add {
    height: 34px;
    padding: 0 12px;
    border: 1px dashed var(--hairline);
    border-radius: 8px;
    background: transparent;
    color: var(--ink-soft);
    font-size: 13px;
    cursor: pointer;
  }

  .slider {
    display: grid;
    grid-template-columns: 140px 1fr;
    align-items: center;
    font-size: 13.5px;
    color: var(--ink-soft);
  }
  .check {
    display: flex;
    align-items: center;
    gap: 10px;
    font-size: 13.5px;
  }
  .check input {
    width: 16px;
    height: 16px;
    accent-color: var(--c1);
  }

  footer {
    position: sticky;
    bottom: -24px;
    margin: 16px -28px -24px;
    padding: 12px 28px 16px;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    background: rgba(16, 14, 22, 0.94);
    backdrop-filter: blur(12px);
    border-top: 1px solid var(--hairline);
  }
  .notice {
    margin: 0;
    font-size: 12.5px;
    color: var(--ink-soft);
  }
  .actions {
    display: flex;
    gap: 8px;
    flex: none;
  }
  .actions button {
    height: 32px;
    padding: 0 14px;
    border-radius: 8px;
    font-size: 13px;
    cursor: pointer;
  }
  .ghost {
    border: 1px solid var(--hairline);
    background: transparent;
  }
  .primary {
    border: 0;
    background: var(--c1);
    color: #0d0b12;
    font-weight: 600;
  }
  .actions button:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
