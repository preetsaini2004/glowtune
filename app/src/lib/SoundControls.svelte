<script lang="ts">
  // Sound for the service windows, in two independent parts:
  //   part="eq"     a 10-band equalizer with presets (on/off: `eqEnabled`)
  //   part="blend"  how one song hands over to the next (Off / Crossfade / Automix)
  // Used in Settings → Sound and in the desktop scene's menu (`compact`).
  //
  //   sliders / presets ──save(patch)──► settings ──► the audio engine (live)
  import { onMount } from "svelte";
  import { invoke } from "@tauri-apps/api/core";

  export type SoundSettings = {
    eqEnabled: boolean;
    eqPreset: string;
    eqPreamp: number;
    eqGains: number[];
    soundBlend: string;
    crossfadeSecs: number;
  };
  type Preset = { id: string; name: string; preamp: number; gains: number[] };

  let {
    settings,
    save,
    part,
    compact = false,
  }: {
    settings: SoundSettings;
    save: (patch: Partial<SoundSettings>) => void;
    part: "eq" | "blend";
    compact?: boolean;
  } = $props();

  const FREQS = [31.25, 62.5, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
  const LABELS = ["31", "62", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];
  const MAX = 12;
  const BLENDS = [
    { id: "off", name: "Off" },
    { id: "crossfade", name: "Crossfade" },
    { id: "automix", name: "Automix" },
  ];

  let presets = $state<Preset[]>([]);
  onMount(() => {
    invoke<Preset[]>("eq_presets").then((p) => (presets = p)).catch(() => {});
  });

  const gains = $derived(FREQS.map((_, i) => settings.eqGains[i] ?? 0));
  const off = $derived(!settings.eqEnabled);

  function setBand(i: number, value: number) {
    const next = [...gains];
    next[i] = Math.round(value * 2) / 2;
    save({ eqGains: next, eqPreset: "custom" });
  }
  function usePreset(p: Preset) {
    save({ eqPreset: p.id, eqPreamp: p.preamp, eqGains: [...p.gains] });
  }

  // The curve drawn behind the sliders: the real combined response of the ten
  // filters (same formulas as the audio engine), so what you see is what plays.
  const W = 320;
  const H = 96;
  function responseDb(freq: number, bandGains: number[]): number {
    const sr = 48000;
    const w = (2 * Math.PI * freq) / sr;
    let db = 0;
    for (let i = 0; i < FREQS.length; i++) {
      const g = bandGains[i];
      if (Math.abs(g) < 0.01) continue;
      const a = Math.pow(10, g / 40);
      const w0 = (2 * Math.PI * FREQS[i]) / sr;
      const alpha = Math.sin(w0) / (2 * 1.41);
      const b = [1 + alpha * a, -2 * Math.cos(w0), 1 - alpha * a];
      const den = [1 + alpha / a, -2 * Math.cos(w0), 1 - alpha / a];
      const mag = (c: number[]) => {
        const re = c[0] + c[1] * Math.cos(w) + c[2] * Math.cos(2 * w);
        const im = -(c[1] * Math.sin(w) + c[2] * Math.sin(2 * w));
        return re * re + im * im;
      };
      db += 10 * Math.log10(mag(b) / mag(den));
    }
    return db;
  }
  const curve = $derived.by(() => {
    const points: string[] = [];
    for (let x = 0; x <= W; x += 4) {
      // 20 Hz … 20 kHz on a log scale; the bands sit at slider centers.
      const t = (x / W) * FREQS.length - 0.5;
      const freq = FREQS[0] * Math.pow(2, t);
      const y = H / 2 - (Math.max(-MAX, Math.min(MAX, responseDb(freq, gains))) / MAX) * (H / 2 - 4);
      points.push(`${x},${y.toFixed(1)}`);
    }
    return points.join(" ");
  });
</script>

{#if part === "eq"}
<div class="sound" class:compact class:off>
  <div class="eq">
    <svg class="curve" viewBox="0 0 {W} {H}" preserveAspectRatio="none" aria-hidden="true">
      <defs>
        <linearGradient id="eq-fill" x1="0" x2="1" y1="0" y2="0">
          <stop offset="0" stop-color="var(--c1)" />
          <stop offset="1" stop-color="var(--c2)" />
        </linearGradient>
      </defs>
      <line x1="0" x2={W} y1={H / 2} y2={H / 2} class="zero" />
      <polygon points="0,{H / 2} {curve} {W},{H / 2}" fill="url(#eq-fill)" class="area" />
      <polyline points={curve} class="line" stroke="url(#eq-fill)" />
    </svg>
    <div class="bands">
      {#each FREQS as _, i}
        <label class="band" title="Double-click to reset">
          <output>{gains[i] > 0 ? "+" : ""}{gains[i].toFixed(gains[i] % 1 ? 1 : 0)}</output>
          <input
            type="range"
            min={-MAX}
            max={MAX}
            step="0.5"
            value={gains[i]}
            disabled={off}
            aria-label="{LABELS[i]} hertz, decibels"
            oninput={(e) => setBand(i, Number((e.currentTarget as HTMLInputElement).value))}
            ondblclick={() => setBand(i, 0)}
          />
          <span>{LABELS[i]}</span>
        </label>
      {/each}
    </div>
  </div>

  <div class="chips" role="radiogroup" aria-label="Equalizer preset">
    {#each presets as p}
      <button role="radio" aria-checked={settings.eqPreset === p.id} class:on={settings.eqPreset === p.id} disabled={off} onclick={() => usePreset(p)}>
        {p.name}
      </button>
    {/each}
    {#if settings.eqPreset === "custom"}
      <button role="radio" aria-checked="true" class="on" disabled={off}>Custom</button>
    {/if}
  </div>

  <label class="slider">
    <span>Preamp</span>
    <input
      type="range"
      min={-MAX}
      max={MAX}
      step="0.5"
      value={settings.eqPreamp}
      disabled={off}
      oninput={(e) => save({ eqPreamp: Number((e.currentTarget as HTMLInputElement).value) })}
      ondblclick={() => save({ eqPreamp: 0 })}
    />
    <output>{settings.eqPreamp > 0 ? "+" : ""}{settings.eqPreamp.toFixed(1)} dB</output>
  </label>

</div>
{:else}
<div class="sound" class:compact>
  <div class="between">
    <div class="chips" role="radiogroup" aria-label="Between songs">
      {#each BLENDS as b}
        <button role="radio" aria-checked={settings.soundBlend === b.id} class:on={settings.soundBlend === b.id} onclick={() => save({ soundBlend: b.id })}>
          {b.name}
        </button>
      {/each}
    </div>
    {#if settings.soundBlend !== "off"}
      <label class="slider">
        <span>Length</span>
        <input
          type="range"
          min="1"
          max="12"
          step="1"
          value={settings.crossfadeSecs}
          oninput={(e) => save({ crossfadeSecs: Number((e.currentTarget as HTMLInputElement).value) })}
        />
        <output>{settings.crossfadeSecs} s</output>
      </label>
      <p class="hint">
        {settings.soundBlend === "automix"
          ? "Mixes like a DJ, on the beat. Songs without a clear beat simply fade, over the length above."
          : "Each song fades into the next."}
      </p>
      <p class="hint">
        The sound runs {settings.soundBlend === "automix" ? "about half a minute" : "a few seconds"} behind so the next song can be mixed in. Lyrics follow what you hear.
      </p>
    {/if}
  </div>
</div>
{/if}

<style>
  .sound {
    display: grid;
    gap: 14px;
    min-width: 0;
  }
  .sound.off > * {
    opacity: 0.45;
  }
  .eq {
    position: relative;
    padding: 6px 0 0;
  }
  /* The curve sits exactly behind the slider tracks. */
  .curve {
    position: absolute;
    left: 0;
    right: 0;
    top: 28px;
    width: 100%;
    height: 112px;
    pointer-events: none;
  }
  .compact .curve {
    height: 88px;
  }
  .zero {
    stroke: rgba(255, 255, 255, 0.12);
    stroke-width: 1;
    stroke-dasharray: 2 4;
    vector-effect: non-scaling-stroke;
  }
  .area {
    opacity: 0.16;
  }
  .line {
    fill: none;
    stroke-width: 2;
    stroke-linejoin: round;
    vector-effect: non-scaling-stroke;
  }
  .bands {
    position: relative;
    display: grid;
    grid-template-columns: repeat(10, minmax(0, 1fr));
  }
  .band {
    display: grid;
    justify-items: center;
    gap: 6px;
    font-size: 11px;
    color: rgba(255, 255, 255, 0.55);
  }
  .band output {
    min-width: 0;
    height: 16px;
    font-size: 10.5px;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.75);
  }
  /* Vertical sliders: a hairline track and a round thumb in the album colors. */
  .band input {
    appearance: none;
    writing-mode: vertical-lr;
    direction: rtl;
    width: 22px;
    height: 112px;
    margin: 0;
    background: transparent;
    cursor: ns-resize;
  }
  .compact .band input {
    height: 88px;
  }
  .band input::-webkit-slider-runnable-track {
    width: 2px;
    border-radius: 1px;
    background: rgba(255, 255, 255, 0.14);
  }
  .band input::-webkit-slider-thumb {
    appearance: none;
    width: 14px;
    height: 14px;
    margin-left: -6px;
    border-radius: 50%;
    border: 2px solid #fff;
    background: color-mix(in oklab, var(--c1) 70%, #000);
    box-shadow: 0 2px 8px rgba(0, 0, 0, 0.45);
    transition: transform 120ms ease;
  }
  .band input:hover::-webkit-slider-thumb,
  .band input:focus-visible::-webkit-slider-thumb {
    transform: scale(1.2);
  }
  .band input:focus-visible {
    outline: none;
  }
  .band input:disabled {
    cursor: default;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .chips button {
    border: 1px solid rgba(255, 255, 255, 0.1);
    background: transparent;
    border-radius: 999px;
    padding: 5px 11px;
    font: inherit;
    font-size: 12.5px;
    font-weight: 600;
    color: inherit;
    cursor: pointer;
  }
  .chips button.on {
    background: linear-gradient(135deg, var(--c1), var(--c2));
    border-color: transparent;
  }
  .chips button:disabled {
    cursor: default;
  }
  .slider {
    display: grid;
    grid-template-columns: 64px minmax(0, 1fr) 62px;
    align-items: center;
    gap: 10px;
    font-size: 13px;
  }
  .slider input {
    width: 100%;
    margin: 0;
    accent-color: var(--c1);
  }
  .slider output {
    text-align: right;
    font-variant-numeric: tabular-nums;
    color: rgba(255, 255, 255, 0.75);
  }
  .between {
    display: grid;
    gap: 10px;
  }
  .hint {
    margin: 0;
    font-size: 12.5px;
    line-height: 17px;
    color: rgba(255, 255, 255, 0.55);
  }
</style>
