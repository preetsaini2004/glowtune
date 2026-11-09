import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, stat } from "node:fs/promises";
import { fileURLToPath } from "node:url";

const cwd = fileURLToPath(new URL("../", import.meta.url));
const clips = [
  "glow",
  "scene",
  "lyrics",
  "layout",
  "search",
  "services",
  "upnext",
  "controls",
  "themes",
  "screens",
];
await mkdir(new URL("../out/clips/", import.meta.url), { recursive: true });
let total = 0;
for (const name of clips) {
  const source = "out/clips/" + name + ".mp4";
  const webp = "out/clips/" + name + ".webp";
  const gif = "out/clips/" + name + ".gif";
  execFileSync(
    process.execPath,
    [
      "node_modules/@remotion/cli/remotion-cli.js",
      "render",
      "out/bundle",
      name,
      source,
      "--codec=h264",
      "--crf=17",
      "--x264-preset=fast",
      "--concurrency=3",
      "--log=error",
    ],
    { cwd, stdio: "inherit" },
  );
  execFileSync(
    "ffmpeg",
    [
      "-y",
      "-v",
      "error",
      "-i",
      source,
      "-an",
      "-vf",
      "fps=15",
      "-c:v",
      "libwebp_anim",
      "-quality",
      "76",
      "-compression_level",
      "5",
      "-loop",
      "0",
      webp,
    ],
    { cwd, stdio: "inherit" },
  );
  const data = await readFile(new URL("../" + webp, import.meta.url));
  assert(
    data.length <= 5_000_000 && data.toString("ascii", 8, 12) === "WEBP",
    "Expected WebP below 5 MB",
  );
  let frames = 0;
  let duration = 0;
  let loops = false;
  for (let i = 12; i + 8 <= data.length; ) {
    const type = data.toString("ascii", i, i + 4);
    const length = data.readUInt32LE(i + 4);
    assert(i + 8 + length <= data.length, "Incomplete WebP chunk");
    if (type === "VP8X") {
      assert.equal(data.readUIntLE(i + 12, 3) + 1, 1280);
      assert.equal(data.readUIntLE(i + 15, 3) + 1, 720);
    }
    if (type === "ANIM") loops = data.readUInt16LE(i + 12) === 0;
    if (type === "ANMF") {
      frames++;
      duration += data.readUIntLE(i + 20, 3);
    }
    i += 8 + length + (length % 2);
  }
  assert(
    loops && frames === 105 && Math.abs(duration - 7000) < 2,
    "Expected a 7-second looping WebP",
  );
  execFileSync(
    "ffmpeg",
    [
      "-y",
      "-v",
      "error",
      "-i",
      source,
      "-an",
      "-filter_complex_threads",
      "1",
      "-vf",
      "fps=8,scale=480:270:flags=lanczos,split[a][b];[a]palettegen=max_colors=192[p];[b][p]paletteuse=dither=bayer:bayer_scale=3:diff_mode=rectangle",
      "-loop",
      "0",
      gif,
    ],
    { cwd, stdio: "inherit" },
  );
  const gifSize = (await stat(new URL("../" + gif, import.meta.url))).size;
  assert(gifSize <= 5_000_000, "GIF fallback exceeds 5 MB");
  total += data.length + gifSize;
  console.log(
    name +
      " " +
      (data.length / 1e6).toFixed(2) +
      " MB WebP, " +
      (gifSize / 1e6).toFixed(2) +
      " MB GIF",
  );
}
assert(total < 35_000_000, "Clips exceed the README media budget");
