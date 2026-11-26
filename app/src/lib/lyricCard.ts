// Lyric card: the line being sung, with the song's cover and colours, as a
// 1080×1350 picture (the 4:5 size social feeds show whole).
//
//   ┌──────────────────────────┐
//   │ ▣ Title                  │   cover + song, top left
//   │   Artist                 │
//   │                          │
//   │  The line being sung,    │   the hero: big, wrapped to fit
//   │  big and bold            │
//   │                          │
//   │ ▬▬▬▬            Glowtune │   colour bar + wordmark
//   └──────────────────────────┘
//   background: the cover, blurred and darkened, lit by the album colours
//
// Drawn on a canvas here, saved by Rust into Pictures\Glowtune.

import { invoke } from "@tauri-apps/api/core";
import type { LyricsFollower } from "./lyrics.svelte";
import { np } from "./nowPlaying.svelte";

const W = 1080;
const H = 1350;
const PAD = 96;
const DEFAULT_LYRIC_FONT = '"Bricolage Grotesque Variable", "Segoe UI Variable Display", "Nirmala UI", system-ui, sans-serif';

/** The theme's lyrics font (Settings › Themes › Lyrics font), with Nirmala UI
 * kept for Hindi and Punjabi script. */
function lyricFont(): string {
  const theme = getComputedStyle(document.documentElement).getPropertyValue("--font-lyrics").trim();
  if (!theme) return DEFAULT_LYRIC_FONT;
  if (theme.includes("Nirmala UI")) return theme;
  // Before the generic fallbacks, which would otherwise win.
  return theme.includes("system-ui")
    ? theme.replace("system-ui", `"Nirmala UI", system-ui`)
    : `${theme.replace(/[\s,]+$/, "")}, "Nirmala UI", sans-serif`;
}
const UI_FONT = '"Instrument Sans Variable", "Segoe UI Variable Text", "Nirmala UI", system-ui, sans-serif';

export type CardInput = { line: string; title: string; artist: string; art: string | null; palette: string[] };

function loadImage(url: string): Promise<HTMLImageElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    // glowart serves with CORS, so the canvas stays exportable.
    img.crossOrigin = "anonymous";
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = url;
  });
}

/** Words → lines no wider than `max` (long words get their own line). */
function wrap(ctx: CanvasRenderingContext2D, text: string, max: number): string[] {
  const words = text.split(/\s+/).filter(Boolean);
  const lines: string[] = [];
  let cur = "";
  for (const w of words) {
    const next = cur ? `${cur} ${w}` : w;
    if (cur && ctx.measureText(next).width > max) {
      lines.push(cur);
      cur = w;
    } else {
      cur = next;
    }
  }
  if (cur) lines.push(cur);
  return lines;
}

function roundRect(ctx: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  ctx.beginPath();
  ctx.roundRect(x, y, w, h, r);
}

/** One line of text shortened with "…" to fit `max`. */
function fitLine(ctx: CanvasRenderingContext2D, text: string, max: number): string {
  if (ctx.measureText(text).width <= max) return text;
  let t = text;
  while (t.length > 1 && ctx.measureText(t + "…").width > max) t = t.slice(0, -1);
  return t.trimEnd() + "…";
}

export async function drawLyricCard(c: CardInput): Promise<Blob> {
  const LYRIC_FONT = lyricFont();
  await Promise.all([
    document.fonts.load(`800 90px ${LYRIC_FONT}`),
    document.fonts.load(`600 34px ${UI_FONT}`),
  ]).catch(() => {});
  await document.fonts.ready;
  const canvas = document.createElement("canvas");
  canvas.width = W;
  canvas.height = H;
  const ctx = canvas.getContext("2d")!;
  const c1 = c.palette[0] ?? "#8b7cff";
  const c2 = c.palette[1] ?? c1;
  const img = c.art ? await loadImage(c.art) : null;

  // Background: the cover, huge, blurred, darkened, lit by the album colours.
  ctx.fillStyle = "#0d0b12";
  ctx.fillRect(0, 0, W, H);
  if (img) {
    const k = Math.max(W / img.naturalWidth, H / img.naturalHeight) * 1.25;
    const iw = img.naturalWidth * k;
    const ih = img.naturalHeight * k;
    ctx.save();
    ctx.filter = "blur(70px) saturate(1.5)";
    ctx.drawImage(img, (W - iw) / 2, (H - ih) / 2, iw, ih);
    ctx.restore();
  }
  ctx.fillStyle = "rgba(10, 8, 16, 0.55)";
  ctx.fillRect(0, 0, W, H);
  const glow = (x: number, y: number, r: number, col: string, a: number) => {
    const g = ctx.createRadialGradient(x, y, 0, x, y, r);
    g.addColorStop(0, col);
    g.addColorStop(1, "transparent");
    ctx.save();
    ctx.globalAlpha = a;
    ctx.globalCompositeOperation = "screen";
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, W, H);
    ctx.restore();
  };
  glow(0, 0, 900, c1, 0.45);
  glow(W, H, 1000, c2, 0.4);
  // A soft fade at the bottom so the footer reads.
  const fade = ctx.createLinearGradient(0, H - 380, 0, H);
  fade.addColorStop(0, "rgba(0,0,0,0)");
  fade.addColorStop(1, "rgba(0,0,0,0.45)");
  ctx.fillStyle = fade;
  ctx.fillRect(0, H - 380, W, 380);

  // Song: cover + title + artist.
  const cs = 168;
  const top = PAD;
  if (img) {
    ctx.save();
    ctx.shadowColor = "rgba(0,0,0,0.5)";
    ctx.shadowBlur = 40;
    ctx.shadowOffsetY = 14;
    roundRect(ctx, PAD, top, cs, cs, 22);
    ctx.fillStyle = "#000";
    ctx.fill();
    ctx.restore();
    ctx.save();
    roundRect(ctx, PAD, top, cs, cs, 22);
    ctx.clip();
    const s = Math.max(cs / img.naturalWidth, cs / img.naturalHeight);
    const iw = img.naturalWidth * s;
    const ih = img.naturalHeight * s;
    ctx.drawImage(img, PAD + (cs - iw) / 2, top + (cs - ih) / 2, iw, ih);
    ctx.restore();
  }
  const tx = img ? PAD + cs + 34 : PAD;
  const tmax = W - PAD - tx;
  ctx.textBaseline = "alphabetic";
  ctx.fillStyle = "#fff";
  ctx.font = `700 44px ${UI_FONT}`;
  ctx.fillText(fitLine(ctx, c.title, tmax), tx, top + 74);
  ctx.fillStyle = "rgba(255,255,255,0.66)";
  ctx.font = `500 34px ${UI_FONT}`;
  ctx.fillText(fitLine(ctx, c.artist, tmax), tx, top + 124);

  // The line: as big as fits in the middle block (at most 6 lines).
  const areaTop = top + cs + 90;
  const areaBottom = H - 240;
  const maxW = W - PAD * 2;
  let size = 104;
  let lines: string[] = [];
  for (; size >= 44; size -= 4) {
    ctx.font = `800 ${size}px ${LYRIC_FONT}`;
    lines = wrap(ctx, c.line, maxW);
    const tooWide = lines.some((l) => ctx.measureText(l).width > maxW);
    if (lines.length <= 6 && !tooWide && lines.length * size * 1.1 <= areaBottom - areaTop) break;
  }
  if (lines.length > 6) lines = [...lines.slice(0, 5), fitLine(ctx, lines.slice(5).join(" "), maxW)];
  const lh = size * 1.1;
  const blockH = lines.length * lh;
  let y = areaTop + (areaBottom - areaTop - blockH) / 2 + size * 0.86;
  ctx.save();
  ctx.shadowColor = "rgba(0,0,0,0.35)";
  ctx.shadowBlur = 24;
  ctx.fillStyle = "#fff";
  for (const l of lines) {
    ctx.fillText(l, PAD, y);
    y += lh;
  }
  ctx.restore();

  // Footer: a colour bar and the wordmark.
  const fy = H - PAD;
  const bar = ctx.createLinearGradient(PAD, 0, PAD + 220, 0);
  bar.addColorStop(0, c1);
  bar.addColorStop(1, c2);
  roundRect(ctx, PAD, fy - 14, 220, 10, 5);
  ctx.fillStyle = bar;
  ctx.fill();
  ctx.font = `650 32px ${UI_FONT}`;
  ctx.fillStyle = "rgba(255,255,255,0.72)";
  ctx.textAlign = "right";
  ctx.fillText("Glowtune", W - PAD, fy);
  ctx.textAlign = "left";

  return new Promise((resolve, reject) =>
    canvas.toBlob((b) => (b ? resolve(b) : reject(new Error("could not draw the card"))), "image/png"),
  );
}

/** The line to put on a card now: the one being sung, else the last one. */
export function cardLine(lyr: LyricsFollower): string | null {
  const pick = [lyr.current, lyr.previous, lyr.next].find((l) => l && l.text.trim());
  return pick ? pick.text.trim() : null;
}

/** Draws the card for the playing song and saves it. Returns where it went. */
export async function saveLyricCard(lyr: LyricsFollower, art?: string | null): Promise<string> {
  const t = np.track;
  if (!t) throw new Error("Nothing is playing.");
  const line = cardLine(lyr);
  if (!line) throw new Error("No lyric line to put on a card yet.");
  const blob = await drawLyricCard({
    line,
    title: t.title,
    artist: t.artist,
    art: art ?? t.artUrl,
    palette: np.palette,
  });
  const bytes = new Uint8Array(await blob.arrayBuffer());
  // Best effort: also on the clipboard (works when this window has focus).
  try {
    await navigator.clipboard.write([new ClipboardItem({ "image/png": blob })]);
  } catch {}
  const name = encodeURIComponent(`${t.artist} - ${t.title}`);
  return invoke<string>("save_lyric_card", bytes, { headers: { "x-card-name": name } });
}
