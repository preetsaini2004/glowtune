// node --test scripts/emoji.test.ts  (run by scripts/check.sh)
import { test } from "node:test";
import assert from "node:assert/strict";
import { emojiFor } from "../app/src/lib/emoji.ts";

test("sung words find their emoji through punctuation, case and plurals", () => {
  assert.equal(emojiFor("Love,"), "❤️");
  assert.equal(emojiFor("lovin'"), "❤️");
  assert.equal(emojiFor("(fire)"), "🔥");
  assert.equal(emojiFor("Diamonds"), "💎");
  assert.equal(emojiFor("cars"), "🏎️");
  assert.equal(emojiFor("Benz"), "🏎️");
});

test("Hindi and Punjabi words in roman letters", () => {
  assert.equal(emojiFor("dil"), "❤️");
  assert.equal(emojiFor("dard"), "💔");
  assert.equal(emojiFor("aankhein"), "👀");
  assert.equal(emojiFor("chann"), "🌙");
  assert.equal(emojiFor("gaddi"), "🏎️");
});

test("words with another meaning in Hinglish, and plain words, get none", () => {
  assert.equal(emojiFor("sun"), undefined); // "listen"
  assert.equal(emojiFor("sona"), undefined); // gold / sleep
  assert.equal(emojiFor("the"), undefined);
  assert.equal(emojiFor("I"), undefined);
  assert.equal(emojiFor("..."), undefined);
});
