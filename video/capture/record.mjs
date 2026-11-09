import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile } from "node:fs/promises";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { chromium } from "playwright-core";
import { installFixture, lyrics, songs, themes } from "./fixture.mjs";
import { installGlow } from "./glow.mjs";
import { greenMedia, songStart, wallpaperShots } from "./green.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const origin = "http://127.0.0.1:15136";
const raw = new URL("../raw/", import.meta.url);
await mkdir(raw, { recursive: true });
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
const scene = "/stage";
const shots = {
  glow: [
    scene,
    10,
    async (page, at) => {
      await at(1, () => page.keyboard.press("Space"));
      await at(6, () =>
        page.getByRole("button", { name: "Pause", exact: true }).waitFor(),
      );
    },
  ],
  scene: [scene, 25, async () => {}],
  cinema: [
    scene,
    9,
    async (page, at) => {
      await at(0.2, () => page.keyboard.press("l"));
    },
  ],
  lyrics: [
    scene,
    9,
    async (page, at) => {
      await page.keyboard.press("l");
      await at(3, () =>
        page.getByRole("radio", { name: "Lyrics in the middle" }).click(),
      );
      await at(5.8, () =>
        page.getByRole("radio", { name: "Lyrics on the right" }).click(),
      );
    },
  ],
  layout: [
    scene,
    9,
    async (page, at) => {
      await at(0.8, () => page.keyboard.press("e"));
      await at(1.65, () =>
        drag(page, page.getByRole("slider", { name: "Move Album" }), -94, -84),
      );
      await at(3.7, () => drag(page, page.locator(".card .rz-se"), 99, 58));
      await at(6.7, () =>
        page.getByRole("button", { name: "Done", exact: true }).click(),
      );
      const layout = await page.evaluate(() =>
        JSON.parse(localStorage.getItem("glowtune.layout")),
      );
      assert(
        layout.card.x && layout.card.w,
        "Both move and resize must be saved",
      );
    },
  ],
  search: [
    scene,
    9,
    async (page, at) => {
      await at(0.85, () =>
        page.getByRole("button", { name: "Find something to play" }).click(),
      );
      await page
        .getByRole("dialog")
        .getByText("Evening rotation", { exact: true })
        .first()
        .waitFor();
      await at(3, () =>
        page
          .getByRole("textbox", { name: "Search your music and the catalog" })
          .pressSequentially("Soft current", { delay: 85 }),
      );
      await at(6.1, () =>
        page
          .getByRole("dialog")
          .getByRole("button", { name: /Soft current/ })
          .first()
          .click(),
      );
      await page
        .locator(".dock .t")
        .getByText("Soft current", { exact: true })
        .waitFor();
    },
  ],
  services: [
    scene,
    8,
    async (page, at) => {
      await at(0.4, () =>
        page.getByRole("button", { name: "Find something to play" }).click(),
      );
      const filters = page.getByRole("group", {
        name: "Services",
        exact: true,
      });
      for (const [index, name] of [
        "Spotify",
        "Apple Music",
        "YouTube Music",
        "SoundCloud",
      ].entries()) {
        await at(1.4 + index * 1.25, () =>
          filters.getByRole("button", { name, exact: true }).click(),
        );
      }
    },
  ],
  upnext: [
    scene,
    8,
    async (page, at) => {
      await at(1.1, () =>
        page.getByRole("button", { name: "Refresh", exact: true }).click(),
      );
      await at(3.2, () =>
        page
          .locator(".upcoming")
          .getByText("Soft current", { exact: true })
          .click(),
      );
      await page
        .locator(".dock .t")
        .getByText("Soft current", { exact: true })
        .waitFor();
    },
  ],
  controls: [
    scene,
    7,
    async (page, at) => {
      await at(1, () => page.keyboard.press("Space"));
      await page.getByRole("button", { name: "Play", exact: true }).waitFor();
      await at(2.3, () => page.keyboard.press("F9"));
      await page.getByRole("button", { name: "Pause", exact: true }).waitFor();
      await at(3.7, () => page.keyboard.press("F10"));
      await at(5.2, () => page.keyboard.press("F8"));
    },
  ],
  themes: [
    "/settings?tab=theme",
    8,
    async (page, at) => {
      const choices = page.getByRole("radiogroup", { name: "Themes" });
      await at(1.5, () =>
        choices.getByRole("radio", { name: /Aurora/ }).click(),
      );
      await at(3.5, () =>
        choices.getByRole("radio", { name: /Ember/ }).click(),
      );
      await at(5.8, () =>
        choices.getByRole("radio", { name: /Midnight Glass/ }).click(),
      );
    },
  ],
  portrait: ["/stage?primary=0", 8, async () => {}],
};

async function drag(page, locator, dx, dy) {
  await locator.waitFor();
  const rect = await locator.boundingBox();
  assert(rect, "Drag target must be on screen");
  const x = rect.x + rect.width / 2;
  const y = rect.y + rect.height / 2;
  await page.mouse.move(x, y, { steps: 12 });
  await delay(110);
  await page.mouse.down();
  for (let step = 1; step <= 18; step++) {
    const p = 0.5 - 0.5 * Math.cos((Math.PI * step) / 18);
    await page.mouse.move(x + dx * p, y + dy * p);
    await delay(31);
  }
  await page.mouse.up();
}

const selection = process.argv.slice(2);
const names = selection.length ? selection : Object.keys(shots);
assert(
  names.every((name) => Object.hasOwn(shots, name)),
  "Unknown capture shot",
);

const green = names.some((name) => Object.hasOwn(wallpaperShots, name))
  ? await greenMedia()
  : null;
const browser = await chromium.launch({
  channel: "chrome",
  headless: true,
  args: [
    "--enable-unsafe-webgpu",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
  ],
});
try {
  for (const name of names) {
    const [path, seconds, action] = shots[name];
    const realTrack = green && Object.hasOwn(wallpaperShots, name);
    const size =
      name === "portrait"
        ? { width: 1080, height: 1920 }
        : { width: 1920, height: 1080 };
    const context = await browser.newContext({
      viewport: size,
      recordVideo: { dir: fileURLToPath(raw), size },
      serviceWorkers: "block",
      timezoneId: "UTC",
      locale: "en-US",
    });
    const errors = [];
    let recording;
    let start;
    let end;
    try {
      await context.route("**/*", async (route) => {
        const url = new URL(route.request().url());
        if (realTrack && (await green.route(route, url, origin))) return;
        return url.origin === origin ? route.continue() : route.abort();
      });
      await context.addInitScript(installFixture, {
        songs: realTrack ? [green.track, ...songs.slice(1)] : songs,
        wallpaper: realTrack
          ? {
              startMs:
                (songStart + wallpaperShots[name] + (name === "glow" ? 1 : 0)) *
                1000,
              musicStartMs: songStart * 1000,
              pulse: green.pulse,
            }
          : null,
        lyrics,
        themes,
        shot: name,
      });
      const page = await context.newPage();
      page.setDefaultTimeout(8000);
      recording = page.video();
      page.on("pageerror", (error) => errors.push(error.message));
      await page.goto(origin + path);
      if (path.startsWith(scene)) {
        await page.locator(".ct, .card").first().waitFor();
      } else {
        await page.getByRole("radiogroup", { name: "Themes" }).waitFor();
        await page.addStyleTag({ content: "html { background: #101118 }" });
      }
      await page.evaluate(() => document.fonts.ready);
      if (name === "glow") {
        const shader = await readFile(
          new URL("../../crates/overlay/src/glow.wgsl", import.meta.url),
          "utf8",
        );
        await page.evaluate(installGlow, shader);
      }
      const movie = realTrack
        ? page.frameLocator("iframe.mv.ready").locator("video")
        : null;
      if (movie) {
        await movie.waitFor();
        await movie.evaluate(
          (video) =>
            new Promise((resolve, reject) => {
              if (video.readyState >= 2) return resolve();
              video.addEventListener("loadeddata", resolve, { once: true });
              setTimeout(
                () => reject(new Error("The wallpaper video did not load")),
                6000,
              );
            }),
        );
        const expected =
          songStart + wallpaperShots[name] + (name === "glow" ? 1 : 0);
        const position = await movie.evaluate((video) => video.currentTime);
        assert(
          Math.abs(position - expected) <= 0.8,
          "The wallpaper must cue at the soundtrack in point",
        );
      }
      await delay(450);
      if (movie && name !== "glow") await page.keyboard.press("Space");
      start = performance.now();
      const at = async (sec, fn) => {
        await delay(Math.max(0, sec * 1000 - (performance.now() - start)));
        await fn();
      };
      const motion =
        movie &&
        at(name === "glow" ? 1.6 : 0.25, async () => {
          const first = await movie.evaluate((video) => video.currentTime);
          await delay(450);
          assert(
            (await movie.evaluate((video) => video.currentTime)) > first + 0.12,
            "Music video wallpaper must advance",
          );
        });
      await Promise.all([action(page, at), motion]);
      await at(seconds, async () => {});
      end = performance.now();
      assert.deepEqual(errors, [], "The app must render without page errors");
    } finally {
      await context.close();
    }
    const source = await recording.path();
    const duration = Number(
      execFileSync(
        "ffprobe",
        [
          "-v",
          "error",
          "-show_entries",
          "format=duration",
          "-of",
          "csv=p=0",
          source,
        ],
        { encoding: "utf8" },
      ).trim(),
    );
    assert(duration >= seconds - 0.3, "Recording is shorter than the shot");
    const inPoint = Math.max(0, duration - (end - start) / 1000 - 0.07);
    execFileSync(
      "ffmpeg",
      [
        "-y",
        "-hide_banner",
        "-loglevel",
        "error",
        "-ss",
        inPoint.toFixed(3),
        "-i",
        source,
        "-vf",
        "fps=60,tpad=stop_mode=clone:stop_duration=0.3",
        "-t",
        String(seconds),
        "-an",
        "-c:v",
        "libx264",
        "-preset",
        "fast",
        "-crf",
        "16",
        "-pix_fmt",
        "yuv420p",
        "-movflags",
        "+faststart",
        "raw/" + name + ".mp4",
      ],
      { cwd: root, stdio: "inherit" },
    );
    console.log("Captured " + name + " at " + size.width + "×" + size.height);
  }
} finally {
  await browser.close();
}
