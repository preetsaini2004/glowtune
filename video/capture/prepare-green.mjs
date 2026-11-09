import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { createReadStream } from "node:fs";
import { writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { songStart } from "./green.mjs";

const cwd = fileURLToPath(new URL("../", import.meta.url));
const source = "raw/cavetown-green.webm";
const hash = createHash("sha1");
for await (const chunk of createReadStream(
  new URL("../" + source, import.meta.url),
))
  hash.update(chunk);
assert.equal(
  hash.digest("hex"),
  "2a76109d30cc118a031758ff41bac8bc7ed1e76f",
  "Use the exact license-reviewed Wikimedia file",
);

const ffmpeg = (args) =>
  execFileSync("ffmpeg", ["-y", "-v", "error", ...args], { cwd });
ffmpeg([
  "-i",
  source,
  "-t",
  "100",
  "-an",
  "-vf",
  "scale=1920:-2",
  "-c:v",
  "libx264",
  "-preset",
  "veryfast",
  "-crf",
  "19",
  "-pix_fmt",
  "yuv420p",
  "-movflags",
  "+faststart",
  "raw/green-wallpaper.mp4",
]);
ffmpeg([
  "-ss",
  "38",
  "-i",
  "raw/green-wallpaper.mp4",
  "-frames:v",
  "1",
  "-vf",
  "crop=ih:ih,scale=512:512",
  "-q:v",
  "3",
  "raw/green-cover.jpg",
]);
ffmpeg([
  "-ss",
  String(songStart),
  "-i",
  source,
  "-vn",
  "-t",
  "70",
  "-af",
  "loudnorm=I=-16:TP=-1.5:LRA=10,afade=t=in:d=0.65,afade=t=out:st=66:d=4",
  "-ar",
  "48000",
  "-c:a",
  "pcm_s16le",
  "raw/green.wav",
]);

const pcm = ffmpeg([
  "-i",
  "raw/green.wav",
  "-af",
  "lowpass=f=150",
  "-ar",
  "3000",
  "-ac",
  "1",
  "-f",
  "s16le",
  "pipe:1",
]);
assert.equal(pcm.length, 420000, "Expected exactly 70 seconds of audio");
const bands = Array.from({ length: 2100 }, (_, frame) => {
  let sum = 0;
  for (let i = 0; i < 100; i++)
    sum += pcm.readInt16LE((frame * 100 + i) * 2) ** 2;
  return Math.sqrt(sum / 100);
});
const sorted = [...bands].sort((a, b) => a - b);
const floor = sorted[600];
const ceiling = sorted[2000];
assert(ceiling > floor, "Music must have a usable energy range");
let follow = 0;
const envelope = bands.map((energy) => {
  const strength = Math.min(
    1,
    Math.max(0, (energy - floor) / (ceiling - floor)),
  );
  follow = Math.max(strength, follow * 0.82);
  return Number(follow.toFixed(2));
});
await writeFile(
  new URL("../src/green-pulse.json", import.meta.url),
  JSON.stringify(envelope) + "\n",
);
