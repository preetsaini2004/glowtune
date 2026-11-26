// Settings › Feedback: sends a message to the developer's inbox through Web3Forms.
//
//   the form ──► message() ──► fetch api.web3forms.com (from this window:
//   Web3Forms takes messages from a browser only) ──► the developer's email
//
// Offline: the message waits in this window's storage and goes out the next
// time Settings opens online. At most SEND_LIMIT messages an hour.
// The pure parts (subject, text, the limit) are tested by
// scripts/feedback.test.ts.

/** Web3Forms access key. Made to be public: it can only send email to the developer. */
export const ACCESS_KEY = "9abbe5f9-7455-4dbf-990e-e809e037006e";
const ENDPOINT = "https://api.web3forms.com/submit";

export const KINDS = [
  { id: "bug", name: "Something's broken", tag: "Bug" },
  { id: "complaint", name: "Didn't like it", tag: "Complaint" },
  { id: "idea", name: "Feature idea", tag: "Idea" },
  { id: "review", name: "I like it", tag: "Review" },
  { id: "other", name: "Other", tag: "Other" },
] as const;

export const AREAS = [
  "Lyrics",
  "Edge glow",
  "Desktop scene",
  "Player",
  "Music services",
  "Sound",
  "Themes and Store",
  "Hotkeys",
  "Install and updates",
  "Other",
] as const;

export type Kind = (typeof KINDS)[number]["id"];
export type InfoLine = { name: string; value: string };

export type Draft = {
  kind: Kind;
  area: string;
  /** 1–5, for a review; 0 = none. */
  rating: number;
  title: string;
  body: string;
  /** Any name or nickname the sender picks (required). */
  name: string;
  /** Optional, for a reply. */
  email: string;
  /** A review only: OK to show it, with the name, on Glowtune's website. */
  publish: boolean;
  /** App info lines, empty when the user turned it off. */
  info: InfoLine[];
};

export const TITLE_MAX = 120;
export const NAME_MAX = 60;
export const BODY_MIN = 10;
export const BODY_MAX = 5000;
/** Messages per hour. */
export const SEND_LIMIT = 3;
const HOUR_MS = 3_600_000;

/** What's wrong with the draft, or null when it can be sent. */
export function problem(d: Draft): string | null {
  if (!d.name.trim()) return "Put your name, or any nickname.";
  if (d.name.trim().length > NAME_MAX) return `Keep the name under ${NAME_MAX} characters.`;
  if (!d.title.trim()) return "Give it a short title.";
  if (d.title.trim().length > TITLE_MAX) return `Keep the title under ${TITLE_MAX} characters.`;
  if (d.body.trim().length < BODY_MIN) return "Tell a little more in the message.";
  if (d.body.trim().length > BODY_MAX) return `Keep the message under ${BODY_MAX} characters.`;
  const email = d.email.trim();
  if (email && !/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) return "That email doesn't look right. Leave it empty if you don't want a reply.";
  return null;
}

/** "[Glowtune][Bug][Lyrics] Lyrics not showing" */
export function subject(d: Draft): string {
  const tag = KINDS.find((k) => k.id === d.kind)?.tag ?? "Other";
  return `[Glowtune][${tag}][${d.area}] ${d.title.trim()}`;
}

/** The email's text. */
export function text(d: Draft): string {
  const head = [`From: ${d.name.trim()}`];
  if (d.kind === "review") {
    if (d.rating > 0) head.push(`Rating: ${"★".repeat(d.rating)}${"☆".repeat(5 - d.rating)} (${d.rating}/5)`);
    head.push(`OK to publish on the website: ${d.publish ? "yes" : "no"}`);
  }
  const parts = [head.join("\n"), d.body.trim()];
  if (d.info.length) parts.push("— App info —\n" + d.info.map((l) => `${l.name}: ${l.value}`).join("\n"));
  return parts.join("\n\n");
}

/** The JSON Web3Forms takes. `email` becomes the reply-to address. */
export function payload(d: Draft): Record<string, string> {
  const p: Record<string, string> = {
    access_key: ACCESS_KEY,
    subject: subject(d),
    from_name: `${d.name.trim()} via Glowtune`,
    name: d.name.trim(),
    message: text(d),
  };
  if (d.email.trim()) p.email = d.email.trim();
  return p;
}

/** Sends left this hour, given the times (ms) of earlier sends. */
export function sendsLeft(sentAt: number[], now: number): number {
  return Math.max(0, SEND_LIMIT - sentAt.filter((t) => now - t < HOUR_MS).length);
}

// ---- storage (this window only; failures are fine) ----------------------

const SENT = "glowtune.feedback.sent";
const QUEUE = "glowtune.feedback.queue";

function read<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key);
    return raw ? (JSON.parse(raw) as T) : fallback;
  } catch {
    return fallback;
  }
}

function write(key: string, value: unknown) {
  try {
    localStorage.setItem(key, JSON.stringify(value));
  } catch {
    /* private mode or full: nothing to keep */
  }
}

export function sentTimes(): number[] {
  return read<number[]>(SENT, []).filter((t) => Date.now() - t < HOUR_MS);
}

export function queued(): Record<string, string>[] {
  return read<Record<string, string>[]>(QUEUE, []);
}

/** Posts one message. Throws on a network failure (→ queue it). */
async function post(p: Record<string, string>): Promise<{ ok: boolean; message: string }> {
  const res = await fetch(ENDPOINT, {
    method: "POST",
    headers: { "Content-Type": "application/json", Accept: "application/json" },
    body: JSON.stringify(p),
  });
  let message = "";
  try {
    message = ((await res.json()) as { message?: string }).message ?? "";
  } catch {
    /* not JSON */
  }
  return { ok: res.ok, message };
}

export type Sent = "sent" | "queued";

/** Sends the draft, or keeps it for later when offline. */
export async function send(d: Draft): Promise<Sent> {
  const p = payload(d);
  write(SENT, [...sentTimes(), Date.now()]);
  if (!navigator.onLine) {
    write(QUEUE, [...queued(), p]);
    return "queued";
  }
  let r: { ok: boolean; message: string };
  try {
    r = await post(p);
  } catch {
    write(QUEUE, [...queued(), p]);
    return "queued";
  }
  if (!r.ok) throw new Error(r.message || "The message couldn't be sent. Try again later.");
  return "sent";
}

/** Sends what waited while offline. Returns how many went out. */
export async function flushQueue(): Promise<number> {
  const waiting = queued();
  if (!waiting.length || !navigator.onLine) return 0;
  const left: Record<string, string>[] = [];
  let sent = 0;
  for (const p of waiting) {
    try {
      const r = await post(p);
      if (r.ok) sent++;
      // Refused (not a network problem): sending it again won't help.
    } catch {
      left.push(p);
    }
  }
  write(QUEUE, left);
  return sent;
}
