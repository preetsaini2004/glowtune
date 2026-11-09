// Follows the synced lyrics of the current song: which line is active now.
//
// No per-frame loop: we compute when the next line starts and sleep until
// then (re-planned on every play/pause/seek report and on new lyrics).

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { np } from "./nowPlaying.svelte";

/** `words` is present only when the lyrics time each word (enhanced LRC). */
export type Line = { timeMs: number; text: string; words?: { timeMs: number; text: string }[] };
export type LyricsState = {
  status: "loading" | "synced" | "plain" | "instrumental" | "none";
  lines: Line[];
  offsetMs: number;
};

export class LyricsFollower {
  lyrics = $state<LyricsState>({ status: "none", lines: [], offsetMs: 0 });
  index = $state(-1);
  /** How far into the current line we were when it was (re)planned. */
  lineElapsedMs = $state(0);
  /** Bumped on every re-plan (karaoke fills restart from the right point). */
  plan = $state(0);
  #plans = 0;
  #timer: ReturnType<typeof setTimeout> | undefined;

  get synced() {
    return this.lyrics.status === "synced" && this.lyrics.lines.length > 0;
  }
  get current(): Line | null {
    return this.synced && this.index >= 0 ? this.lyrics.lines[this.index] : null;
  }
  get next(): Line | null {
    return this.synced ? (this.lyrics.lines[this.index + 1] ?? null) : null;
  }
  get previous(): Line | null {
    return this.synced && this.index > 0 ? this.lyrics.lines[this.index - 1] : null;
  }
  get lineMs(): number {
    const c = this.current;
    const n = this.next;
    return c && n ? n.timeMs - c.timeMs : 4000;
  }

  /** Index of the line active at `ms` (binary search), -1 before the first. */
  lineAt(ms: number): number {
    const lines = this.lyrics.lines;
    let lo = 0;
    let hi = lines.length - 1;
    let found = -1;
    while (lo <= hi) {
      const mid = (lo + hi) >> 1;
      if (lines[mid].timeMs <= ms) {
        found = mid;
        lo = mid + 1;
      } else hi = mid - 1;
    }
    return found;
  }

  schedule = () => {
    clearTimeout(this.#timer);
    if (!this.synced) {
      this.index = -1;
      return;
    }
    const lines = this.lyrics.lines;
    const pos = np.positionAt() + this.lyrics.offsetMs;
    const i = this.lineAt(pos);
    this.index = i;
    this.lineElapsedMs = i >= 0 ? Math.max(0, pos - lines[i].timeMs) : 0;
    this.plan = ++this.#plans;
    if (!np.playing) return;
    const upcoming = lines[i + 1];
    if (upcoming) {
      const wait = (upcoming.timeMs - pos) / (np.playback.rate || 1);
      this.#timer = setTimeout(this.schedule, Math.max(16, wait));
    }
  };

  /** Starts listening; call from onMount and run the returned cleanup on destroy. */
  start(): () => void {
    const unlisten = listen<LyricsState>("lyrics", (e) => (this.lyrics = e.payload));
    invoke<LyricsState>("get_lyrics")
      .then((l) => (this.lyrics = l))
      .catch(() => {});
    return () => {
      clearTimeout(this.#timer);
      unlisten.then((f) => f());
    };
  }
}
