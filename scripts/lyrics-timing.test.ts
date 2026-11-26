// node --test scripts/lyrics-timing.test.ts  (run by scripts/check.sh)
import { test } from "node:test";
import assert from "node:assert/strict";
import { parseOffset, formatOffset, tapOffset, tapStart, sungLines } from "../app/src/lib/lyricsTiming.ts";

test("the number box reads seconds with a sign", () => {
  assert.deepEqual(parseOffset("+0.5"), { ms: 500 });
  assert.deepEqual(parseOffset("0.5"), { ms: 500 });
  assert.deepEqual(parseOffset("-1.25"), { ms: -1250 });
  assert.deepEqual(parseOffset(".5"), { ms: 500 });
  assert.deepEqual(parseOffset("0,5"), { ms: 500 });
  assert.deepEqual(parseOffset("+2"), { ms: 2000 });
  assert.deepEqual(parseOffset(" -0.3 s "), { ms: -300 });
  assert.deepEqual(parseOffset("−0.5"), { ms: -500 });
  assert.deepEqual(parseOffset("0.123"), { ms: 100 });
  assert.ok("error" in parseOffset("abc"));
  assert.ok("error" in parseOffset("1.2.3"));
  assert.ok("error" in parseOffset(""));
  assert.ok("error" in parseOffset("+11"));
  assert.deepEqual(parseOffset("10"), { ms: 10000 });
});

test("offsets read back the same way", () => {
  assert.equal(formatOffset(0), "0");
  assert.equal(formatOffset(500), "+0.5");
  assert.equal(formatOffset(-1250), "−1.25");
  assert.equal(formatOffset(2000), "+2");
  for (const ms of [500, -1250, 2000, -300, 50]) {
    const back = parseOffset(formatOffset(ms));
    assert.deepEqual(back, { ms });
  }
});

test("a tap sets the timing so the line shows when it was heard", () => {
  // The line says 30 s; the singer started it when the song was at 32 s
  // (tap 150 ms later): lyrics are 2 s early, so show them 2 s later.
  assert.equal(tapOffset(30_000, 32_150), -2000);
  // Lyrics late by 1.5 s: show them earlier.
  assert.equal(tapOffset(30_000, 28_650), 1500);
  // Clamped to ±10 s.
  assert.equal(tapOffset(0, 60_000), -10_000);
});

test("tap along to a line coming up, not one already gone", () => {
  const lines = sungLines([
    { timeMs: 10_000, text: "one" },
    { timeMs: 14_000, text: "" },
    { timeMs: 15_000, text: "two" },
    { timeMs: 20_000, text: "three" },
  ]);
  assert.equal(lines.length, 3);
  assert.equal(lines[tapStart(lines, 12_000)].text, "two");
  assert.equal(lines[tapStart(lines, 14_000)].text, "three");
  assert.equal(lines[tapStart(lines, 99_000)].text, "three");
});
