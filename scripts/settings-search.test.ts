// node --test scripts/settings-search.test.ts  (run by scripts/check.sh)
import { test } from "node:test";
import assert from "node:assert/strict";
import { searchSettings, didYouMean, distance, SETTINGS, SECTION_NAMES } from "../app/src/lib/settingsIndex.ts";

const top = (q: string) => searchSettings(q)[0]?.name;

test("typos still find the setting (the examples from the idea list)", () => {
  assert.equal(top("reverse motion"), "Reverse motion");
  assert.equal(top("revers moton"), "Reverse motion");
  assert.equal(top("lyric siz"), "Lyrics size");
  assert.equal(top("lyrics size"), "Lyrics size");
  assert.equal(top("equlizer"), "Equalizer");
  assert.ok(searchSettings("partcles").every((h) => /particle/i.test(h.name + " " + (h.words ?? ""))));
  assert.match(top("partcles") ?? "", /^Particle/);
});

test("synonyms and single words", () => {
  assert.equal(top("automix"), "Between songs");
  const fade = searchSettings("crossfade").slice(0, 2).map((h) => h.name);
  assert.ok(fade.includes("Between songs") && fade.includes("Crossfade length"), fade.join());
  assert.equal(top("sleep"), "Hotkeys");
  assert.equal(top("karaoke"), "Lyrics animation");
  assert.equal(top("rgb"), "Edge glow");
  assert.equal(top("wallpaper"), "Desktop scene");
  assert.equal(top("swear"), "Hide explicit words");
  assert.equal(top("update"), "Updates");
});

test("half-typed words match", () => {
  assert.match(top("partic") ?? "", /^Particle /);
  assert.equal(top("corner rad"), "Corner radius");
});

test("nonsense finds nothing, but did-you-mean still guesses", () => {
  assert.deepEqual(searchSettings("zzzzqqq"), []);
  assert.deepEqual(searchSettings(""), []);
  assert.equal(didYouMean("revrse mtion")[0]?.name, "Reverse motion");
});

test("every setting is findable by at least two different words", () => {
  for (const e of SETTINGS) {
    const own = new Set((e.name + " " + (e.words ?? "")).toLowerCase().split(/[^\p{L}\p{N}]+/u).filter((w) => w.length > 2));
    let found = 0;
    for (const w of own) if (searchSettings(w, 50).some((h) => h.name === e.name)) found++;
    assert.ok(found >= 2, `${e.name} is found by only ${found} word(s)`);
    assert.ok(SECTION_NAMES[e.section], `${e.name}: unknown section ${e.section}`);
  }
});

test("distance counts swapped letters once", () => {
  assert.equal(distance("moiton", "motion"), 1);
  assert.equal(distance("abc", "abc"), 0);
  assert.equal(distance("kitten", "sitting"), 3);
});
