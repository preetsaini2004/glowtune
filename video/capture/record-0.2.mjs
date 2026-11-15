import assert from "node:assert/strict";
import { execFileSync } from "node:child_process";
import { mkdir, readFile, writeFile, copyFile } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import { performance } from "node:perf_hooks";
import { chromium } from "playwright-core";
import { installFixture, songs, lyrics, themes } from "./fixture.mjs";
import { installGlow } from "./glow.mjs";
import {
  newFeatures,
  captureSettings,
  installProduction,
  desktopHTML,
} from "./production.mjs";

const root = fileURLToPath(new URL("../", import.meta.url));
const origin = "http://127.0.0.1:15136";
const delay = (ms) => new Promise((resolve) => setTimeout(resolve, ms));
await mkdir(new URL("../raw/", import.meta.url), { recursive: true });
await mkdir(new URL("../public/capture/", import.meta.url), {
  recursive: true,
});
const selection = process.argv.slice(2);
const names = selection.length ? selection : newFeatures;
assert(
  names.every((name) => newFeatures.includes(name)),
  "Unknown 0.2 capture shot",
);
const shader = await readFile(
  new URL("../../crates/overlay/src/glow.wgsl", import.meta.url),
  "utf8",
);
const font = await readFile(
  new URL("../public/brand/instrument-sans.woff2", import.meta.url),
);
const pulse = JSON.parse(
  await readFile(
    new URL("../src/green-0.2-pulse.json", import.meta.url),
    "utf8",
  ),
);
const browser = await chromium.launch({
  channel: "chrome",
  headless: true,
  args: [
    "--enable-unsafe-webgpu",
    "--disable-background-timer-throttling",
    "--disable-renderer-backgrounding",
  ],
});

async function attachToast(page) {
  await page.evaluate(() => {
    const frame = document.createElement("iframe");
    frame.src = "/osd";
    frame.title = "Glowtune toast";
    Object.assign(frame.style, {
      position: "fixed",
      bottom: "72px",
      left: "calc(50% - 180px)",
      width: "360px",
      height: "84px",
      border: "0",
      zIndex: "100001",
      pointerEvents: "none",
    });
    document.body.append(frame);
  });
  await page
    .frameLocator('iframe[title="Glowtune toast"]')
    .locator("main")
    .waitFor();
}

async function menu(page, name) {
  if (!(await page.locator(".menu").isVisible()))
    await page
      .getByRole("button", { name: "Desktop scene options", exact: true })
      .click();
  await page
    .locator(".menu")
    .getByRole("button", { name, exact: true })
    .click();
}
async function closeMenu(page) {
  if (await page.locator(".menu").isVisible())
    await page.keyboard.press("Escape");
}
async function patch(page, values) {
  await page.evaluate((v) => window.capturePatch(v), values);
}
const actions = {
  presets: async (page, at) => {
    await at(0.2, () => menu(page, "Presets"));
    await at(0.7, () =>
      page.getByRole("textbox", { name: "Preset name" }).fill("Night"),
    );
    await at(1.1, () =>
      page.getByRole("button", { name: "Save", exact: true }).click(),
    );
    await page
      .locator('button[title="Switch to Night (Ctrl+Alt+2)"]')
      .waitFor();
    await at(2, () => closeMenu(page));
    await at(2.5, () => page.keyboard.press("Control+Alt+1"));
    await page
      .frameLocator('iframe[title="Glowtune toast"]')
      .getByText("Day", { exact: true })
      .waitFor();
    await at(4.5, () => page.keyboard.press("Control+Alt+2"));
    await page
      .frameLocator('iframe[title="Glowtune toast"]')
      .getByText("Night", { exact: true })
      .waitFor();
    assert.equal(
      (
        await page.evaluate(() =>
          window.__TAURI_INTERNALS__.invoke("get_settings"),
        )
      ).scenePresets.length,
      2,
    );
  },
  hotkeys: async (page, at) => {
    await at(0.8, () => page.keyboard.press("Control+Alt+ArrowRight"));
    await page
      .frameLocator('iframe[title="Glowtune toast"]')
      .getByText("Next song", { exact: true })
      .waitFor();
    await at(3.4, () => page.keyboard.press("Control+Alt+ArrowUp"));
    await page
      .frameLocator('iframe[title="Glowtune toast"]')
      .getByText("Volume 45 %", { exact: true })
      .waitFor();
  },
  "stars-lyrics": async (page, at) => {
    await page.locator(".stars").waitFor();
    await at(11.5, async () =>
      assert(await page.locator(".stars").isVisible()),
    );
  },
  "beat-walls": async (page, at) => {
    await at(0.3, async () => {
      for (let i = 0; i < 35; i++) {
        await page.mouse.move(590 + i * 21, 400 + Math.sin(i / 5) * 140);
        await delay(28);
      }
    });
    await at(2.5, () => patch(page, { stageWallpaper: "frost" }));
    await at(4.8, () => patch(page, { stageWallpaper: "halftone" }));
    assert(
      (await page.locator(".water canvas").count()) ||
        (await page.locator("canvas").count()),
      "Wallpaper shader must be present",
    );
  },
  "vinyl-dock": async (page, at) => {
    for (const [i, stageDockStyle] of [
      "bar",
      "pill",
      "islands",
      "line",
    ].entries()) {
      await at(i * 1.65, () => patch(page, { stageDockStyle }));
      await page.locator(".dock.dock-" + stageDockStyle).waitFor();
    }
    await page.locator(".art-vinyl.spinning").waitFor();
  },
  "lyric-card": async (page, at) => {
    await at(0.65, () => page.keyboard.press("Control+Alt+c"));
    await page.waitForFunction(() => window.captureCardSaved === true);
    await at(2, () =>
      page.evaluate(() => {
        const shade = document.createElement("div");
        shade.id = "saved-card-preview";
        Object.assign(shade.style, {
          position: "fixed",
          inset: "0",
          background: "#07090dd9",
          zIndex: "100002",
          display: "grid",
          placeItems: "center",
          backdropFilter: "blur(10px)",
        });
        const image = document.createElement("img");
        image.src = "/capture-card.png";
        image.alt = "Saved lyric card";
        Object.assign(image.style, {
          height: "940px",
          borderRadius: "16px",
          boxShadow: "0 30px 100px #000",
        });
        shade.append(image);
        document.body.append(shade);
      }),
    );
    await page.getByRole("img", { name: "Saved lyric card" }).waitFor();
  },
  stats: async (page, at) => {
    await page
      .getByRole("heading", { name: "Top songs", exact: true })
      .waitFor();
    await page
      .getByRole("heading", { name: "Top artists", exact: true })
      .waitFor();
    assert.equal(
      await page.locator(".bar-ends span").first().textContent(),
      await page.evaluate(() =>
        new Date(2026, 0, 10).toLocaleDateString(undefined, {
          weekday: "short",
          day: "numeric",
        }),
      ),
      "Listening bars must use Unix seconds, matching the app contract",
    );
    await at(3.5, () =>
      page.getByRole("radio", { name: "This month", exact: true }).click(),
    );
    await page.locator(".bars .bar").nth(29).waitFor();
  },
  "mini-player": async (page, at) => {
    await at(2, () =>
      page.frameLocator('iframe[title="disc"]').locator(".disc-wrap").hover(),
    );
    for (const name of ["glass", "disc", "card", "pill", "lyric"]) {
      await page
        .frameLocator(`iframe[title="${name}"]`)
        .locator(".s-" + name)
        .waitFor();
    }
    await page
      .frameLocator('iframe[title="Taskbar player"]')
      .getByRole("button", { name: "Pause", exact: true })
      .waitFor();
  },
  "glow-shapes": async (page, at) => {
    const changes = [
      ["comet", "embers"],
      ["aurora", "snow"],
      ["bars", "fireflies"],
      ["neon", "bubbles"],
      ["tide", "bubbles"],
    ];
    for (const [i, [effect, particles]] of changes.entries()) {
      await at(i * 1.4, () =>
        page.evaluate(
          async ({ effect, particles }) => {
            const theme = await window.__TAURI_INTERNALS__.invoke("get_theme");
            theme.glow = {
              ...theme.glow,
              effect,
              particles,
              particle_density: 0.65,
              particle_size: 1.5,
              particle_speed: 1,
              effect_speed: 1,
              effect_strength: 1,
            };
            await window.__TAURI_INTERNALS__.invoke("save_theme", { theme });
          },
          { effect, particles },
        ),
      );
    }
  },
};

try {
  for (const shot of names) {
    const seconds = shot === "stars-lyrics" ? 13 : 7;
    const size =
      shot === "stats"
        ? { width: 1280, height: 720 }
        : { width: 1920, height: 1080 };
    const context = await browser.newContext({
      viewport: size,
      recordVideo: { dir: root + "raw", size },
      serviceWorkers: "block",
      timezoneId: "UTC",
      locale: "en-US",
    });
    const errors = [];
    const blocked = new Set();
    let card;
    let recording;
    let started;
    let elapsed;
    try {
      await context.route("**/*", async (route) => {
        const url = new URL(route.request().url());
        if (url.origin !== origin) {
          blocked.add(url.origin);
          return route.abort();
        }
        if (url.pathname === "/capture-desktop")
          return route.fulfill({
            contentType: "text/html",
            body: desktopHTML(shot === "mini-player"),
          });
        if (url.pathname === "/capture-brand.woff2")
          return route.fulfill({ contentType: "font/woff2", body: font });
        if (url.pathname === "/capture-card.png") {
          assert(card, "Card must be saved before preview");
          return route.fulfill({ contentType: "image/png", body: card });
        }
        return route.continue();
      });
      const settings = {
        ...captureSettings,
        stageClock: shot === "presets",
        stageWallpaper: shot === "beat-walls" ? "water" : "art",
        stageLyrics: !["beat-walls", "vinyl-dock", "glow-shapes"].includes(
          shot,
        ),
        stageArtStyle: shot === "presets" ? "glow" : "vinyl",
        stageDockStyle: "line",
      };
      if (shot === "presets") {
        const { scenePresets: _presets, ...look } = settings;
        settings.scenePresets = [
          {
            name: "Day",
            look: {
              ...look,
              artworkColors: false,
              customColors: ["#6dc9ea", "#97e2ca"],
              stageWallpaper: "cover",
              stageLyricsStyle: "rain",
              stageArtStyle: "print",
              stageDockStyle: "pill",
            },
            layout: null,
          },
        ];
      }
      const data = {
        songs,
        lyrics,
        themes,
        shot,
        pulse,
        production: true,
        captureSettings: settings,
      };
      await context.addInitScript({
        content: `const captureInput=${JSON.stringify(data)};window.captureData=captureInput;window.captureProduction=true;(${installFixture.toString()})(captureInput);(${installProduction.toString()})(captureInput);`,
      });
      await context.exposeFunction("captureSaveCard", async (bytes) => {
        card = Buffer.from(bytes);
        assert.equal(card.toString("hex", 0, 8), "89504e470d0a1a0a");
        assert.equal(card.readUInt32BE(16), 1080);
        assert.equal(card.readUInt32BE(20), 1350);
        await writeFile(root + "raw/lyric-card.png", card);
        await writeFile(root + "public/capture/lyric-card.png", card);
      });
      const page = await context.newPage();
      recording = page.video();
      page.setDefaultTimeout(10000);
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (/INVALID_VALUE|INVALID_OPERATION|shader:/.test(message.text()))
          errors.push(message.text());
      });
      // Mark completion only after the actual PNG bytes reach the capture backend.
      await page.addInitScript(() => {
        window.captureCardSaved = false;
        const original = window.captureSaveCard;
        window.captureSaveCard = async (...args) => {
          await original(...args);
          window.captureCardSaved = true;
        };
      });
      const path =
        shot === "stats"
          ? "/settings?tab=listening"
          : ["hotkeys", "mini-player"].includes(shot)
            ? "/capture-desktop"
            : "/stage";
      await page.goto(origin + path);
      if (path === "/stage") await page.locator(".dock").waitFor();
      if (["presets", "lyric-card"].includes(shot)) await attachToast(page);
      if (shot === "glow-shapes") await page.evaluate(installGlow, shader);
      if (shot === "beat-walls")
        await page.addStyleTag({ content: ".now { visibility:hidden }" });
      await page.evaluate(() => document.fonts.ready);
      if (shot === "stats") {
        await page.locator(".tops").waitFor();
        await page.addStyleTag({ content: "html { background:#101118 }" });
      }
      if (shot === "mini-player") {
        for (const title of [
          "glass",
          "disc",
          "card",
          "pill",
          "lyric",
          "Taskbar player",
        ])
          await page
            .frameLocator(`iframe[title="${title}"]`)
            .locator("main")
            .waitFor();
      }
      // A cropped calibration strip avoids relying on screencast duration under load.
      await page.evaluate(() => {
        const marker = document.createElement("div");
        marker.id = "capture-frame-marker";
        Object.assign(marker.style, {
          position: "fixed",
          left: "0",
          bottom: "0",
          width: "32px",
          height: "4px",
          background: "#ff00ff",
          zIndex: "300000",
          pointerEvents: "none",
        });
        document.body.append(marker);
      });
      await delay(900);
      await page.evaluate(async () => {
        await window.captureStart();
        document.getElementById("capture-frame-marker").style.background =
          "#00ff00";
      });
      started = performance.now();
      const at = async (sec, fn) => {
        await delay(Math.max(0, sec * 1000 - (performance.now() - started)));
        await fn();
      };
      await actions[shot](page, at);
      await at(seconds, async () => {});
      elapsed = (performance.now() - started) / 1000;
      assert(elapsed < seconds + 0.8, "Capture actions overran their shot");
      assert.deepEqual(errors, [], "App must render without page errors");
      assert.equal(
        blocked.size,
        0,
        "Production fixtures must not request external hosts",
      );
    } catch (error) {
      const page = context.pages()[0];
      if (page)
        await page.screenshot({ path: root + "out/failed-" + shot + ".png" });
      console.error("Capture page errors:", errors);
      throw error;
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
    const pixels = execFileSync("ffmpeg", [
      "-v",
      "error",
      "-i",
      source,
      "-vf",
      `fps=25,crop=16:2:8:${size.height - 2},scale=1:1`,
      "-f",
      "rawvideo",
      "-pix_fmt",
      "rgb24",
      "pipe:1",
    ]);
    let startFrame = -1;
    let armed = false;
    for (let i = 0; i < pixels.length; i += 3) {
      const [r, g, b] = pixels.subarray(i, i + 3);
      if (r > 120 && g < 90 && b > 120) armed = true;
      if (armed && g > 150 && r < 100 && b < 100) {
        startFrame = i / 3;
        break;
      }
    }
    assert(
      startFrame >= 0,
      "Capture must contain the armed/start frame marker",
    );
    const inPoint = startFrame / 25;
    assert(
      duration - inPoint >= seconds - 0.2,
      "Capture must hold the full timed shot",
    );
    execFileSync(
      "ffmpeg",
      [
        "-y",
        "-v",
        "error",
        "-ss",
        inPoint.toFixed(3),
        "-i",
        source,
        "-vf",
        "crop=iw:ih-4:0:0,scale=1920:1080:flags=lanczos,fps=60,tpad=stop_mode=clone:stop_duration=0.3",
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
        "raw/" + shot + ".mp4",
      ],
      { cwd: root, stdio: "inherit" },
    );
    await copyFile(
      root + "raw/" + shot + ".mp4",
      root + "public/capture/" + shot + ".mp4",
    );
    console.log(
      "Captured " +
        shot +
        " (" +
        seconds +
        " s), no external requests or page errors",
    );
  }
} finally {
  await browser.close();
}
