// Lyrics timing helpers for Settings › Lyrics: the number box and tap to
// sync. Pure TypeScript (tested by scripts/lyrics-timing.test.ts).
//
// The sign matches Glowtune's offset everywhere: positive = lyrics show
// earlier (a line shows when position + offset reaches its time).

/** Furthest the timing can move either way, in ms (same as Rust). */
export const MAX_OFFSET_MS = 10_000;

/** How long people take to tap after hearing a line start. */
export const TAP_REACTION_MS = 150;

/**
 * "+0.5", "-1.25", ".5", "0,5", "2 s" → milliseconds, or an error message.
 * Whole numbers of 50 ms (what the nudge buttons use too).
 */
export function parseOffset(text: string): { ms: number } | { error: string } {
  const t = text.trim().replace(/\s*s(ec(onds?)?)?$/i, "").replace(",", ".").replace(/^−/, "-");
  if (!/^[+-]?(\d+(\.\d*)?|\.\d+)$/.test(t)) return { error: "Type seconds, like +0.5 or -1.25." };
  const seconds = Number(t);
  if (Math.abs(seconds) > MAX_OFFSET_MS / 1000) return { error: "Up to 10 seconds either way." };
  return { ms: Math.round((seconds * 1000) / 50) * 50 };
}

/** 500 → "+0.5", -1250 → "−1.25", 0 → "0". */
export function formatOffset(ms: number): string {
  if (ms === 0) return "0";
  const s = (Math.abs(ms) / 1000).toFixed(2).replace(/0$/, "").replace(/\.0$/, "");
  return `${ms > 0 ? "+" : "−"}${s}`;
}

export type TapLine = { timeMs: number; text: string };

/** The sung lines (no empty "♪" gaps). */
export function sungLines<T extends TapLine>(lines: T[]): T[] {
  return lines.filter((l) => l.text.trim());
}

/**
 * The line to tap along to: the first sung line starting more than 1.5 s
 * after where the lyrics are now (time to read it). Index into `lines`.
 */
export function tapStart(lines: TapLine[], lyricMs: number): number {
  const i = lines.findIndex((l) => l.timeMs > lyricMs + 1500);
  return i === -1 ? Math.max(0, lines.length - 1) : i;
}

/**
 * The tap happened at song position `songMs` when the singer started the
 * line timed `lineMs`: the offset that shows it right then.
 */
export function tapOffset(lineMs: number, songMs: number): number {
  const heardAt = songMs - TAP_REACTION_MS;
  const ms = Math.round((lineMs - heardAt) / 50) * 50;
  return Math.max(-MAX_OFFSET_MS, Math.min(MAX_OFFSET_MS, ms));
}
