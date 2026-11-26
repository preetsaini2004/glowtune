// node --test scripts/feedback.test.ts  (run by scripts/check.sh)
import { test } from "node:test";
import assert from "node:assert/strict";
import { problem, subject, text, payload, sendsLeft, SEND_LIMIT, type Draft } from "../app/src/lib/feedback.ts";

const draft = (d: Partial<Draft> = {}): Draft => ({
  kind: "bug",
  area: "Lyrics",
  rating: 0,
  name: "Sky",
  publish: false,
  title: "Lyrics not showing",
  body: "Spotify was maximised and nothing showed.",
  email: "",
  info: [],
  ...d,
});

test("the subject says the kind, the area and the title", () => {
  assert.equal(subject(draft()), "[Glowtune][Bug][Lyrics] Lyrics not showing");
  assert.equal(subject(draft({ kind: "review", area: "Player", title: "  Love it " })), "[Glowtune][Review][Player] Love it");
});

test("a draft needs a name, a title and a real message; the email is optional but checked", () => {
  assert.equal(problem(draft()), null);
  assert.match(problem(draft({ name: "  " }))!, /name/);
  assert.match(problem(draft({ title: " " }))!, /title/);
  assert.match(problem(draft({ body: "too short" }))!, /more/);
  assert.match(problem(draft({ email: "not an email" }))!, /email/);
  assert.equal(problem(draft({ email: "someone@example.com" })), null);
});

test("the text says who, the rating and publish OK for a review, and the app info when kept", () => {
  const t = text(draft({ kind: "review", rating: 4, publish: true, info: [{ name: "Glowtune", value: "0.5.0" }] }));
  assert.match(t, /^From: Sky\nRating: ★★★★☆ \(4\/5\)\nOK to publish on the website: yes\n\n/);
  assert.match(t, /— App info —\nGlowtune: 0\.5\.0$/);
  assert.match(text(draft({ kind: "review", rating: 5 })), /OK to publish on the website: no/);
  // No rating or publish line outside reviews, no info block when it's off.
  assert.equal(text(draft({ rating: 3, publish: true })), "From: Sky\n\nSpotify was maximised and nothing showed.");
});

test("the sender's name shows in the inbox", () => {
  assert.equal(payload(draft()).from_name, "Sky via Glowtune");
  assert.equal(payload(draft()).name, "Sky");
});

test("the reply-to email is sent only when given", () => {
  assert.equal("email" in payload(draft()), false);
  assert.equal(payload(draft({ email: " a@b.co " })).email, "a@b.co");
});

test("at most a few messages an hour", () => {
  const now = 10_000_000;
  assert.equal(sendsLeft([], now), SEND_LIMIT);
  assert.equal(sendsLeft([now - 1000, now - 2000], now), SEND_LIMIT - 2);
  assert.equal(sendsLeft([now - 1000, now - 2000, now - 3000], now), 0);
  // Older than an hour: forgotten.
  assert.equal(sendsLeft([now - 3_700_000], now), SEND_LIMIT);
});
