import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { createHash } from "node:crypto";
import { readFile, writeFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { newFeatures } from "./production.mjs";

const cwd = fileURLToPath(new URL("../", import.meta.url));
const files = [];
const probe = (path) =>
  JSON.parse(
    execFileSync(
      "ffprobe",
      ["-v", "error", "-show_streams", "-show_format", "-of", "json", path],
      { cwd, encoding: "utf8" },
    ),
  );
async function asset(path) {
  const bytes = await readFile(new URL("../" + path, import.meta.url));
  files.push({
    path,
    bytes: bytes.length,
    sha256: createHash("sha256").update(bytes).digest("hex"),
  });
  return bytes;
}
function video(path, width, height, seconds, audio) {
  const info = probe(path);
  const picture = info.streams.find((s) => s.codec_type === "video");
  assert.equal(picture.codec_name, "h264", path);
  assert(["yuv420p", "yuvj420p"].includes(picture.pix_fmt), path);
  assert.equal(picture.width, width, path);
  assert.equal(picture.height, height, path);
  assert.equal(picture.avg_frame_rate, "30/1", path);
  assert.equal(Number(picture.nb_frames), seconds * 30, path);
  assert(Math.abs(Number(info.format.duration) - seconds) < 0.06, path);
  assert.equal(
    info.streams.some((s) => s.codec_type === "audio"),
    audio,
    path,
  );
  if (audio)
    assert.equal(
      info.streams.find((s) => s.codec_type === "audio").codec_name,
      "aac",
    );
  execFileSync(
    "ffmpeg",
    ["-v", "error", "-xerror", "-i", path, "-f", "null", "-"],
    { cwd, stdio: "inherit" },
  );
}
function png(bytes, width, height) {
  assert.equal(bytes.toString("hex", 0, 8), "89504e470d0a1a0a");
  assert.equal(bytes.readUInt32BE(16), width);
  assert.equal(bytes.readUInt32BE(20), height);
}
function webp(bytes) {
  assert.equal(bytes.toString("ascii", 8, 12), "WEBP");
  let frames = 0;
  let duration = 0;
  let loops = false;
  for (let i = 12; i + 8 <= bytes.length; ) {
    const type = bytes.toString("ascii", i, i + 4);
    const length = bytes.readUInt32LE(i + 4);
    assert(i + 8 + length <= bytes.length, "Incomplete WebP chunk");
    if (type === "VP8X") {
      assert.equal(bytes.readUIntLE(i + 12, 3) + 1, 1280);
      assert.equal(bytes.readUIntLE(i + 15, 3) + 1, 720);
    }
    if (type === "ANIM") loops = bytes.readUInt16LE(i + 12) === 0;
    if (type === "ANMF") {
      frames++;
      const ms = bytes.readUIntLE(i + 20, 3);
      assert(ms === 66 || ms === 67, "Expected 15 fps frame timing");
      duration += ms;
    }
    i += 8 + length + (length % 2);
  }
  assert(
    loops && frames === 105 && Math.abs(duration - 7000) < 2,
    "Expected 7 seconds, 105 frames, infinite loop",
  );
}

video("out/glowtune-0.2-hero.mp4", 1920, 1080, 75, true);
await asset("out/glowtune-0.2-hero.mp4");
png(await asset("out/glowtune-0.2-poster.png"), 1920, 1080);
png(
  await readFile(new URL("../raw/lyric-card.png", import.meta.url)),
  1080,
  1350,
);
for (const name of newFeatures) {
  const base = "out/clips/" + name;
  video(base + ".mp4", 1280, 720, 7, false);
  await asset(base + ".mp4");
  webp(await asset(base + ".webp"));
  const gif = await asset(base + ".gif");
  assert.equal(gif.toString("ascii", 0, 6), "GIF89a");
  const fallback = probe(base + ".gif").streams[0];
  assert.equal(fallback.width, 480);
  assert.equal(fallback.height, 270);
  assert(Math.abs(Number(fallback.duration) - 7) < 0.15);
  execFileSync(
    "ffmpeg",
    [
      "-y",
      "-v",
      "error",
      "-i",
      base + ".mp4",
      "-vf",
      "fps=2,scale=384:216,tile=4x4",
      "-frames:v",
      "1",
      "out/qa-" + name + ".jpg",
    ],
    { cwd, stdio: "inherit" },
  );
}
execFileSync(
  "ffmpeg",
  [
    "-y",
    "-v",
    "error",
    "-i",
    "out/glowtune-0.2-hero.mp4",
    "-vf",
    "fps=1/5,scale=384:216,tile=5x3",
    "-frames:v",
    "1",
    "out/qa-hero.jpg",
  ],
  { cwd, stdio: "inherit" },
);
await writeFile(
  new URL("../out/manifest-0.2.json", import.meta.url),
  JSON.stringify(files, null, 2) + "\n",
);
await writeFile(
  new URL("../out/review-0.2.html", import.meta.url),
  `<!doctype html><meta charset="utf-8"><title>Glowtune 0.2 video review</title><style>body{background:#101118;color:#f7f5ed;font:18px system-ui;max-width:1400px;margin:40px auto;padding:0 20px}video,img{display:block;width:100%;border-radius:16px}section{margin:48px 0}a{color:#ffc674}.grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(440px,1fr));gap:30px}h1,h2{font-weight:600}small{color:#bbb}</style><h1>Glowtune 0.2</h1><p>75-second film and nine silent 7-second loops. Captured app UI uses fictional demo data. Native hotkeys and shell placement are staged with fixtures.</p><video controls preload="metadata" poster="glowtune-0.2-poster.png" src="glowtune-0.2-hero.mp4"></video><p><a href="glowtune-0.2-poster.png">Poster</a> · <a href="manifest-0.2.json">Sizes and checksums</a> · <a href="../README.md">Credits and capture notes</a></p><div class="grid">${newFeatures.map((name) => `<section><h2>${name}</h2><video controls loop muted playsinline preload="metadata" src="clips/${name}.mp4"></video><p><a href="clips/${name}.webp">Animated WebP</a> · <a href="clips/${name}.gif">GIF fallback</a> · <a href="qa-${name}.jpg">Contact sheet</a></p></section>`).join("")}</div><p>“Green” by Cavetown (Robin Skinner), piano by Rachel Lyon. Video directed by Guy Larsen and Taha Khan, produced by Penny4. Trimmed, reframed and edited under <a href="https://creativecommons.org/licenses/by/3.0/">CC BY 3.0</a>. <a href="https://commons.wikimedia.org/wiki/File:Cavetown_-_Green.webm">Source</a>. Original demo lyrics and artwork: Glowtune video fixture, MIT. Full credits in video/README.md.</p>`,
);
for (const file of files)
  console.log(
    `${file.path}: ${(file.bytes / 1e6).toFixed(3)} MB (${file.bytes} bytes)`,
  );
console.log(
  "Verified film, poster, PNG card, all nine MP4/WebP/GIF loops; wrote manifest and review gallery.",
);
