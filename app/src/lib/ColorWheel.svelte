<script lang="ts">
  // A color wheel: angle = hue, distance from the center = saturation, plus a
  // brightness slider. Value in and out as "#rrggbb".
  let {
    value,
    label,
    onchange,
  }: { value: string; label: string; onchange: (hex: string) => void } = $props();

  type Hsv = { h: number; s: number; v: number };

  function hexToHsv(hex: string): Hsv {
    const n = parseInt(hex.slice(1), 16);
    const r = ((n >> 16) & 255) / 255;
    const g = ((n >> 8) & 255) / 255;
    const b = (n & 255) / 255;
    const max = Math.max(r, g, b);
    const d = max - Math.min(r, g, b);
    let h = 0;
    if (d) {
      if (max === r) h = ((g - b) / d) % 6;
      else if (max === g) h = (b - r) / d + 2;
      else h = (r - g) / d + 4;
    }
    return { h: ((h * 60) + 360) % 360, s: max ? d / max : 0, v: max };
  }

  function hsvToHex({ h, s, v }: Hsv): string {
    const f = (n: number) => {
      const k = (n + h / 60) % 6;
      return v - v * s * Math.max(0, Math.min(k, 4 - k, 1));
    };
    return "#" + [f(5), f(3), f(1)].map((x) => Math.round(x * 255).toString(16).padStart(2, "0")).join("");
  }

  const hsv = $derived(/^#[0-9a-f]{6}$/i.test(value) ? hexToHsv(value) : { h: 0, s: 0, v: 1 });
  // Dot position: hue around, saturation outward (radius 50%).
  const dotX = $derived(50 + Math.cos((hsv.h - 90) * (Math.PI / 180)) * hsv.s * 50);
  const dotY = $derived(50 + Math.sin((hsv.h - 90) * (Math.PI / 180)) * hsv.s * 50);

  let wheel: HTMLDivElement | undefined = $state();

  function pick(e: PointerEvent) {
    if (!wheel) return;
    const r = wheel.getBoundingClientRect();
    const x = e.clientX - (r.left + r.width / 2);
    const y = e.clientY - (r.top + r.height / 2);
    const s = Math.min(1, Math.hypot(x, y) / (r.width / 2));
    const h = (Math.atan2(y, x) * (180 / Math.PI) + 90 + 360) % 360;
    onchange(hsvToHex({ h, s, v: Math.max(0.35, hsv.v) }));
  }
  function down(e: PointerEvent) {
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    pick(e);
  }
  function move(e: PointerEvent) {
    if ((e.currentTarget as HTMLElement).hasPointerCapture(e.pointerId)) pick(e);
  }
  function keys(e: KeyboardEvent) {
    const step = e.shiftKey ? 15 : 5;
    if (e.key === "ArrowLeft") onchange(hsvToHex({ ...hsv, h: (hsv.h - step + 360) % 360 }));
    else if (e.key === "ArrowRight") onchange(hsvToHex({ ...hsv, h: (hsv.h + step) % 360 }));
    else if (e.key === "ArrowUp") onchange(hsvToHex({ ...hsv, s: Math.min(1, hsv.s + 0.05) }));
    else if (e.key === "ArrowDown") onchange(hsvToHex({ ...hsv, s: Math.max(0, hsv.s - 0.05) }));
    else return;
    e.preventDefault();
  }
</script>

<div class="cw">
  <span class="lbl">{label}</span>
  <div
    class="wheel"
    bind:this={wheel}
    role="slider"
    tabindex="0"
    aria-label={`${label} color`}
    aria-valuemin={0}
    aria-valuemax={360}
    aria-valuenow={Math.round(hsv.h)}
    aria-valuetext={value}
    onpointerdown={down}
    onpointermove={move}
    onkeydown={keys}
    style:--v={hsv.v}
  >
    <span class="dot" style:left={`${dotX}%`} style:top={`${dotY}%`} style:background={value}></span>
  </div>
  <input
    type="range"
    min="0.35"
    max="1"
    step="0.01"
    value={hsv.v}
    aria-label={`${label} brightness`}
    oninput={(e) => onchange(hsvToHex({ ...hsv, v: Number((e.currentTarget as HTMLInputElement).value) }))}
  />
</div>

<style>
  .cw {
    display: grid;
    justify-items: center;
    gap: 8px;
    min-width: 0;
  }
  .lbl {
    font-size: 12px;
    font-weight: 600;
    color: rgba(255, 255, 255, 0.6);
  }
  .wheel {
    position: relative;
    width: 100%;
    max-width: 118px;
    aspect-ratio: 1;
    border-radius: 50%;
    cursor: crosshair;
    touch-action: none;
    box-shadow: 0 6px 22px -8px rgba(0, 0, 0, 0.7);
  }
  .wheel::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: 50%;
    background:
      radial-gradient(closest-side, #fff, transparent),
      conic-gradient(red, yellow, lime, cyan, blue, magenta, red);
    filter: brightness(calc(0.35 + var(--v) * 0.65));
  }
  .wheel:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 3px;
  }
  .dot {
    position: absolute;
    width: 18px;
    height: 18px;
    margin: -9px 0 0 -9px;
    border-radius: 50%;
    border: 3px solid #fff;
    box-shadow: 0 1px 6px rgba(0, 0, 0, 0.6);
    pointer-events: none;
  }
  input {
    width: 100%;
    max-width: 118px;
  }
</style>
