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

## Credits and licensing

- “Green” written and performed by Robin Skinner (Cavetown), with piano by Rachel Lyon. Official music video directed by Guy Larsen and Taha Khan, produced by Penny4. The excerpt was trimmed, reframed as wallpaper, and edited into the Glowtune film. The audio was level-matched and faded. The cover thumbnail is a cropped frame from the video. [Original music video](https://www.youtube.com/watch?v=hjr4wfnVmpE), [license-reviewed audiovisual file](https://commons.wikimedia.org/wiki/File:Cavetown_-_Green.webm), [CC BY 3.0](https://creativecommons.org/licenses/by/3.0/), [full production credits](https://www.penny4.co.uk/green).
- The launch video uses an original dot field and the Glowtune logo from `docs/media/logo.png`.
- Instrument Sans is distributed under the SIL Open Font License. See `public/brand/OFL.txt`.
- This project qualifies for Remotion’s [Free License](https://www.remotion.dev/docs/license/pricing), which covers individuals and companies of up to three people. Larger teams need a company license.
