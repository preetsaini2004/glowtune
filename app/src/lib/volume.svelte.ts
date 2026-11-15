// The playing app's volume, for compact controls (the mini player).
//
// Web players (Glowtune's service windows) are read and set through the
// page's own slider, which Glowtune turns into an instant gain; other apps
// through Windows' per-app volume. Same paths as the dock's control.

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { np } from "./nowPlaying.svelte";

type Web = { volume: number | null };

export class Volume {
  /** 0..1, or null when the app's volume can't be read. */
  level = $state<number | null>(null);
  #timer: ReturnType<typeof setTimeout> | undefined;

  constructor() {
    // A hotkey moved the volume: follow it (lives as long as the page).
    listen<number>("volume-changed", (e) => (this.level = e.payload)).catch(() => {});
  }

  get web() {
    return np.track?.source === "Web player";
  }

  /** Reads the current level (call when the song or app changes). */
  async read() {
    try {
      if (this.web) {
        const s = await invoke<Web | null>("web_player", { action: "state" });
        this.level = s?.volume ?? null;
      } else {
        this.level = await np.getVolume();
      }
    } catch {
      this.level = null;
    }
  }

  set(v: number) {
    const x = Math.min(1, Math.max(0, v));
    this.level = x;
    clearTimeout(this.#timer);
    this.#timer = setTimeout(() => {
      if (this.web) invoke("web_player", { action: "volume", value: x }).catch(() => {});
      else np.setVolume(x);
    }, 60);
  }

  /** Mouse wheel: 5 % a notch. */
  wheel(e: WheelEvent) {
    if (this.level === null) return;
    e.preventDefault();
    this.set(this.level - Math.sign(e.deltaY) * 0.05);
  }
}
