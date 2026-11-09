// Active theme, shared by every window. Rust sends the full theme ("theme"
// event / get_theme) and we map it onto CSS variables on :root:
//
//   [colors] → --tint --ink --accent (+ --c1/--c2 when not adaptive)
//   [glass]  → --tint alpha, --radius
//   [font]   → --font-ui, --font-lyrics, --lyrics-size
//   [lyrics] → style / position read directly by the lyrics window
//   [glow]   → handled natively by the overlay (Rust)

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type ColorMode = "adaptive" | "static" | "gradient";
export type LyricsStyle = "rise" | "karaoke" | "static";
export type LyricsPosition = "bottom-center" | "top-center" | "center";
export type Monitors = "all" | "primary" | string[];

export type Theme = {
  meta: { name: string; author: string; version: number };
  colors: { mode: ColorMode; background: string; surface: string; text: string; accent: string; glow: string[] };
  glass: { blur: number; opacity: number; radius: number };
  font: { family: string; lyrics_family: string; lyrics_size: number };
  glow: {
    enabled: boolean;
    thickness: number;
    intensity: number;
    beat_reactivity: number;
    drift_speed: number;
    effect: "ambient" | "still" | "pulse" | "orbit" | "distortion" | "lava-lamp";
    effect_speed: number;
    effect_strength: number;
    reverse_motion: boolean;
    particles: "off" | "dots" | "stars" | "dust";
    particle_density: number;
    particle_size: number;
    particle_speed: number;
    particle_sparkle: number;
    corners: "rounded" | "square";
    monitors: Monitors;
  };
  lyrics: { style: LyricsStyle; position: LyricsPosition; show_on_desktop: boolean };
};

/** Fonts that ship with Glowtune (bundled, work offline), plus Windows system faces. */
export const FONT_STACKS: Record<string, string> = {
  "Instrument Sans": '"Instrument Sans Variable", system-ui, sans-serif',
  "Bricolage Grotesque": '"Bricolage Grotesque Variable", system-ui, sans-serif',
  "Segoe UI": '"Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif',
  Georgia: 'Georgia, "Times New Roman", serif',
};

function stack(family: string): string {
  return FONT_STACKS[family] ?? `"${family.replace(/"/g, "")}", system-ui, sans-serif`;
}

/** "#rrggbb[aa]" → rgba() with the given alpha multiplier applied. */
function withAlpha(hex: string, alpha: number): string {
  const h = hex.replace("#", "");
  const full = h.length === 3 ? [...h].map((c) => c + c).join("") : h;
  const r = parseInt(full.slice(0, 2), 16);
  const g = parseInt(full.slice(2, 4), 16);
  const b = parseInt(full.slice(4, 6), 16);
  const a = full.length === 8 ? parseInt(full.slice(6, 8), 16) / 255 : 1;
  if ([r, g, b].some(Number.isNaN)) return `rgba(20, 17, 28, ${alpha})`;
  return `rgba(${r}, ${g}, ${b}, ${(a * alpha).toFixed(3)})`;
}

class ThemeState {
  theme = $state<Theme | null>(null);

  /** Colors windows should use: the theme's own when not adaptive. */
  fixedPalette(): string[] | null {
    const t = this.theme;
    if (!t || t.colors.mode === "adaptive") return null;
    return t.colors.mode === "static" ? [t.colors.glow[0], t.colors.glow[0]] : t.colors.glow;
  }

  apply(t: Theme) {
    this.theme = t;
    const root = document.documentElement.style;
    root.setProperty("--tint", withAlpha(t.colors.background, t.glass.opacity));
    root.setProperty("--ink", t.colors.text);
    root.setProperty("--ink-soft", withAlpha(t.colors.text, 0.62));
    root.setProperty("--ink-faint", withAlpha(t.colors.text, 0.38));
    root.setProperty("--surface", t.colors.surface);
    root.setProperty("--accent", t.colors.accent);
    root.setProperty("--radius", `${t.glass.radius}px`);
    root.setProperty("--font-ui", stack(t.font.family));
    root.setProperty("--font-lyrics", stack(t.font.lyrics_family));
    root.setProperty("--lyrics-size", `${t.font.lyrics_size}px`);
  }

  async start() {
    await listen<Theme>("theme", (e) => this.apply(e.payload));
    try {
      this.apply(await invoke<Theme>("get_theme"));
    } catch {
      // Older backend / no theme yet: keep the built-in CSS defaults.
    }
  }
}

export const theme = new ThemeState();
