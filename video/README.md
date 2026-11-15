# Glowtune launch video

The 70-second launch film and ten seven-second feature loops share one Remotion project. Cavetown’s official music video for “Green” plays behind Glowtune’s desktop scene and fills the screen in cinema view. The soundtrack uses the audio from that same release, and the dot field reacts to its low-frequency energy.

Footage comes from Glowtune’s Svelte interface with a local Tauri fixture. The fixture serves the archived clip to the app’s wallpaper player in an isolated browser, and the app handles placement, seeking, and playback. The edge-glow shot uses the app’s WGSL shader. The browser blocks external requests. The separate lyrics clip demonstrates word timing with original demo lyrics, and the search and queue shots use local fixture results.

## Record and render

Install Node.js, Chrome, and FFmpeg, with `ffmpeg` and `ffprobe` on PATH. From the repository root, install the app dependencies and leave its dev server running.

```sh
pnpm -C app install --frozen-lockfile
pnpm -C app exec vite dev --host 127.0.0.1 --port 15136 --strictPort
```

Run the following from `video/` in Git Bash. The preparation script verifies the checksum of the license-reviewed source and produces the render audio and a local wallpaper copy.

```sh
npm ci
npx playwright-core install ffmpeg
mkdir -p raw public/capture
curl -fL -A 'GlowtuneVideo/1.0 (local media production)' \
  https://upload.wikimedia.org/wikipedia/commons/7/7f/Cavetown_-_Green.webm \
  -o raw/cavetown-green.webm
node capture/prepare-green.mjs
node capture/record.mjs
cp raw/{glow,scene,cinema,lyrics,layout,search,services,upnext,controls,themes,portrait}.mp4 raw/green.wav public/capture/
npm run lint
npx remotion bundle src/index.ts --out-dir=out/bundle
npx remotion render out/bundle GlowtuneHero out/glowtune-hero.mp4 --codec=h264 --crf=21 --x264-preset=fast --audio-bitrate=192k --concurrency=6
npx remotion still out/bundle GlowtunePoster out/hero.png --image-format=png
node capture/render-clips.mjs
```

Pass shot names to `node capture/record.mjs` to recapture them. Chromium captures at 25 fps and preparation normalizes the MP4 inputs to 60 fps. Rendered loops are 1280×720 animated WebP at 15 fps with smaller GIF fallbacks. Recordings, staged media, and renders stay outside Git.

The end card defaults to “GitHub Releases.” Once the repository address is available, pass `--props` with a JSON file containing the `repository` string to the hero render.

## Glowtune 0.2 production cut

The 0.2 film is 75 seconds at 1920×1080, 30 fps, H.264 with AAC audio. It retains the original opening, edge glow, and Cavetown wallpaper footage, then gives Stars lyrics an uninterrupted eleven-second feature sequence. All nine new features also have seven-second, silent 1280×720 loops: `presets`, `hotkeys`, `stars-lyrics`, `beat-walls`, `vinyl-dock`, `lyric-card`, `stats`, `mini-player`, and `glow-shapes`. WebP runs at 15 fps; GIF fallbacks retain the original smaller 480×270, 8 fps format. The 0.1 compositions, recordings, and outputs remain available.

With the same app dev server running, use these commands from `video/` (Git Bash). On PowerShell, use `npm.cmd` instead of `npm` if script execution is disabled.

```sh
# Verify the same archived source; prepare a separate 75-second soundtrack.
node capture/prepare-green.mjs --production
cp raw/green-0.2.wav public/capture/

# Names may be passed to recapture individual features.
node capture/record-0.2.mjs
npm run lint
npx remotion bundle src/index.ts --out-dir=out/bundle
npx remotion render out/bundle Glowtune02Hero out/glowtune-0.2-hero.mp4 --codec=h264 --crf=21 --x264-preset=fast --audio-bitrate=192k --concurrency=6
npx remotion still out/bundle Glowtune02Poster out/glowtune-0.2-poster.png --image-format=png
node capture/render-clips.mjs presets hotkeys stars-lyrics beat-walls vinyl-dock lyric-card stats mini-player glow-shapes
node capture/verify-0.2.mjs
```

Retain the existing `public/capture/glow.mp4` and `scene.mp4` inputs from the original recording workflow. The verifier decodes every new MP4, checks video/audio formats and durations, validates PNG dimensions and WebP animation chunks, and writes `out/manifest-0.2.json`, contact sheets, and `out/review-0.2.html`. Open that gallery for the film, poster, all nine loops, and download links. Renders, recordings, saved cards, the gallery, and staged assets remain ignored by Git.

### Capture method and privacy

`capture/production.mjs` extends the existing local Tauri fixture. Each recording uses a fresh headless browser context, an original fictional track catalog, fictional listening history, a fixed evening clock, and blocked external HTTP requests. It never reads app settings, listening logs, service profiles, or the real desktop. Assertions check preset save/apply, both toast texts, five mini player styles, taskbar controls, shader errors, and the actual 1080×1350 card bytes.

The Svelte pages are the real app UI. The native Rust backend is replaced for capture: Ctrl+Alt keys replay the backend events, the real `/osd` page draws the toast, and the real mini player and `/taskbar` pages appear in an editorial desktop arrangement. “Field notes” is an original local stand-in for another app, with original text. The Windows shell, native shortcut registration, clipboard integration, acrylic, and native file picker are not recorded or verified. The five-player arrangement is a comparison, not a claim that five native players can be open together. Keycaps and look names are editorial labels outside the app. Playback starts from pause; a four-pixel calibration strip marks the actual first recorded frame and is cropped before export, keeping timed actions aligned with the edit under load.

Water/Frost/Halftone use the actual `ArtWall.svelte` shaders. The capture hides foreground song blocks to keep the wallpaper visible, retains the dock, and wipes Water with real browser pointer movement. Kicks are replayed from rising low-frequency energy in the licensed soundtrack at the corresponding film in-point. The edge lights run the app's current `glow.wgsl` through WebGPU, cycling Comets/Embers, Aurora/Snow, Equalizer/Fireflies, Neon/Bubbles, and Tide/Bubbles. Source footage is recorded at 25 fps, normalized to 60 fps, then edited at 30 fps, matching the existing workflow; this is not a native 60 fps performance demonstration.

The card is drawn by `app/src/lib/lyricCard.ts` after Ctrl+Alt+C. The fixture intercepts its PNG save, writes `raw/lyric-card.png`, and displays those exact bytes in an editorial preview. The words shown with fictional songs are original demo lyrics, not Cavetown lyrics. Depth and Orbit are omitted; Rain appears when the Day preset is applied. No published README media is replaced until Preet reviews the cut.

## Credits and licensing

- “Green” written and performed by Robin Skinner (Cavetown), with piano by Rachel Lyon. Official music video directed by Guy Larsen and Taha Khan, produced by Penny4. The excerpt was trimmed, reframed as wallpaper, and edited into the Glowtune film. The audio was level-matched and faded. The cover thumbnail is a cropped frame from the video. [Original music video](https://www.youtube.com/watch?v=hjr4wfnVmpE), [license-reviewed audiovisual file](https://commons.wikimedia.org/wiki/File:Cavetown_-_Green.webm), [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/), [full production credits](https://www.penny4.co.uk/green).
- The launch video uses an original dot field and the Glowtune logo from `docs/media/logo.png`.
- Instrument Sans is distributed under the SIL Open Font License. See `public/brand/OFL.txt`.
- Bricolage Grotesque, used by the app's lyric styles and card, is distributed under the SIL Open Font License. See `public/brand/Bricolage-OFL.txt`.
- All “Studies in light” fixture covers (including rotating record artwork), the fictional Night Signal / Paper Satellites / Low Meridian catalog, original demo lyric lines, and Field notes text are original Glowtune video assets, authored in `capture/fixture.mjs` and `capture/production.mjs`, distributed under this repository's MIT license. No commercial album artwork, service audio, or third-party lyrics are used in these new feature captures.
- The 0.2 soundtrack uses 00:24–01:39 of the same checksum-verified “Green” source, level-matched and faded. The reused moving-video shots and cover frame retain the original CC BY 3.0 attribution above. Preserve these credits, source and license links, and the statement of edits when publishing either cut.
- This project qualifies for Remotion’s [Free License](https://www.remotion.dev/docs/license/pricing), which covers individuals and companies of up to three people. Larger teams need a company license.
