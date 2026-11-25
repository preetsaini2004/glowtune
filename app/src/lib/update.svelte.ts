// In-app updates, as the windows see them. Rust (`update.rs`) does the work
// and sends "update-status" on every change; this keeps a live copy.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

export type UpdateStatus = "idle" | "checking" | "latest" | "available" | "downloading" | "installing" | "error";

export type UpdateView = {
  /**
   * "github" (installed with Glowtune's installer) updates itself; "store" is
   * updated by the Microsoft Store; "manual" (a plain exe, a test build) never
   * installs anything.
   */
  channel: "github" | "store" | "manual";
  current: string;
  status: UpdateStatus;
  version: string | null;
  notes: string | null;
  date: string | null;
  progress: number | null;
  error: string | null;
  checkedAt: number | null;
};

class Updates {
  view = $state<UpdateView | null>(null);
  #started = false;

  start() {
    if (this.#started) return;
    this.#started = true;
    invoke<UpdateView>("update_status").then((v) => (this.view ??= v)).catch(() => {});
    listen<UpdateView>("update-status", (e) => (this.view = e.payload));
  }

  check() {
    return invoke<UpdateView>("check_update").catch(() => {});
  }

  install() {
    // Errors also arrive as an "error" status with a readable reason.
    return invoke("install_update").catch(() => {});
  }

  skip(version: string) {
    return invoke("skip_update", { version });
  }

  later() {
    return invoke("dismiss_update");
  }

  details() {
    return invoke("update_details");
  }
}

export const updates = new Updates();

/** "3 Dec" style, or "" if unknown. */
export function releaseDay(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return Number.isNaN(d.getTime()) ? "" : d.toLocaleDateString(undefined, { day: "numeric", month: "short", year: "numeric" });
}

/** "a few seconds ago", "5 minutes ago"... for "Last checked". */
export function ago(ms: number | null): string {
  if (!ms) return "";
  const s = Math.max(0, (Date.now() - ms) / 1000);
  if (s < 60) return "just now";
  const rtf = new Intl.RelativeTimeFormat(undefined, { numeric: "auto" });
  if (s < 3600) return rtf.format(-Math.round(s / 60), "minute");
  if (s < 86400) return rtf.format(-Math.round(s / 3600), "hour");
  return rtf.format(-Math.round(s / 86400), "day");
}

export type NoteBlock = { kind: "heading" | "item" | "text"; text: string };

/**
 * The release notes (Markdown) as simple blocks: headings, list items and
 * paragraphs. Links, images and emphasis marks are reduced to their text;
 * the notes are shown as plain text, never as HTML.
 */
export function noteBlocks(md: string | null): NoteBlock[] {
  if (!md) return [];
  const plain = (s: string) =>
    s
      .replace(/!\[[^\]]*\]\([^)]*\)/g, "")
      .replace(/\[([^\]]+)\]\([^)]*\)/g, "$1")
      .replace(/(\*\*|__|`)/g, "")
      .replace(/(^|\s)[*_]([^*_]+)[*_](?=\s|$|[.,;:!?])/g, "$1$2")
      .trim();
  const out: NoteBlock[] = [];
  for (const raw of md.split(/\r?\n/)) {
    const line = raw.trim().replace(/^>\s?/, "");
    // Skip rules, raw HTML and the "# Glowtune 0.4.0" title (the pop-up shows the version).
    if (!line || /^(-{3,}|\*{3,})$/.test(line) || line.startsWith("<") || /^#\s/.test(line)) continue;
    const h = line.match(/^#{1,6}\s+(.*)$/);
    const li = line.match(/^(?:[-*+]|\d+\.)\s+(.*)$/);
    const text = plain(h?.[1] ?? li?.[1] ?? line);
    if (!text) continue;
    out.push({ kind: h ? "heading" : li ? "item" : "text", text });
  }
  return out;
}
