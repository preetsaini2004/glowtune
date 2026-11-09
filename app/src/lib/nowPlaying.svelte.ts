// Shared "now playing" state for every Glowtune window.
//
//   Rust hub ──"np-track"────► track (title, artist, art, palette)
//            ──"np-playback"─► playback (status, position @ reportedAt, rate)
//   get_state ─────────────────► initial snapshot when a window opens
//
// Position is interpolated locally (PLAN.md 8.3): the player only reports it
// occasionally, so between reports we advance it by elapsed time × rate.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { theme } from "./theme.svelte";

export type Track = {
  title: string;
  artist: string;
  album: string;
  durationMs: number | null;
  source: string;
  artUrl: string | null;
  palette: string[];
};

export type Playback = {
  status: "playing" | "paused" | "stopped";
  positionMs: number;
  rate: number;
  reportedAtMs: number;
  /** null = the player can't shuffle / repeat (hide the button). */
  shuffle?: boolean | null;
  repeat?: "off" | "track" | "list" | null;
};

type Snapshot = { track: Track | null; playback: Playback };

export const DEFAULT_PALETTE = ["#8b7cff", "#5ca0ff"];

class NowPlaying {
  #started = false;
  track = $state<Track | null>(null);
  playback = $state<Playback>({ status: "stopped", positionMs: 0, rate: 1, reportedAtMs: Date.now() });
  /** The user's own colors (Settings: "Use artwork colors" off), else null. */
  userPalette = $state<string[] | null>(null);
  /** Settings: "Hide explicit words". */
  cleanLyrics = $state(false);

  get playing() {
    return this.playback.status === "playing";
  }

  /** Colors for the UI: the user's own, else the theme's (static/gradient), else the album's. */
  get palette() {
    if (this.userPalette?.length) return this.userPalette;
    const fixed = theme.fixedPalette();
    if (fixed) return fixed;
    const p = this.track?.palette ?? [];
    return p.length ? p : DEFAULT_PALETTE;
  }

  /** Interpolated position right now, in ms. */
  positionAt(now = Date.now()): number {
    const p = this.playback;
    let pos = p.positionMs;
    if (p.status === "playing") pos += Math.max(0, now - p.reportedAtMs) * (p.rate || 1);
    const d = this.track?.durationMs;
    return d ? Math.min(pos, d) : pos;
  }

  async start() {
    if (this.#started) return;
    this.#started = true;
    type Colors = { artworkColors: boolean; customColors: string[]; cleanLyrics?: boolean };
    const colors = (s: Colors) => {
      this.userPalette = s.artworkColors ? null : s.customColors;
      this.cleanLyrics = Boolean(s.cleanLyrics);
    };
    await listen<Colors>("settings", (e) => colors(e.payload));
    invoke<Colors>("get_settings").then(colors).catch(() => {});
    await listen<Track | null>("np-track", (e) => (this.track = e.payload));
    await listen<Playback>("np-playback", (e) => (this.playback = e.payload));
    const snap = await invoke<Snapshot>("get_state");
    this.track = snap.track;
    this.playback = snap.playback;
  }

  control(action: "playPause" | "next" | "previous") {
    return invoke("control", { action });
  }

  setShuffle(on: boolean) {
    this.playback = { ...this.playback, shuffle: on };
    return invoke("control", { action: "shuffle", on });
  }

  /** off → list → track → off, like most players. */
  cycleRepeat() {
    const next = { off: "list", list: "track", track: "off" } as const;
    const mode = next[this.playback.repeat ?? "off"];
    this.playback = { ...this.playback, repeat: mode };
    return invoke("control", { action: "repeat", mode });
  }

  /** The playing app's own volume (0..1), or null if Windows doesn't expose it. */
  getVolume() {
    return invoke<number | null>("get_volume");
  }

  setVolume(volume: number) {
    return invoke("control", { action: "volume", volume });
  }

  seek(positionMs: number) {
    // Optimistic: move now, the player confirms with a new report.
    this.playback = { ...this.playback, positionMs, reportedAtMs: Date.now() };
    return invoke("control", { action: "seek", positionMs });
  }
}

export const np = new NowPlaying();

/** "3:07" / "1:02:03" */
export function formatTime(ms: number | null | undefined): string {
  if (ms == null || !isFinite(ms)) return "–:––";
  const s = Math.max(0, Math.floor(ms / 1000));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  const ss = String(s % 60).padStart(2, "0");
  return h ? `${h}:${String(m).padStart(2, "0")}:${ss}` : `${m}:${ss}`;
}
