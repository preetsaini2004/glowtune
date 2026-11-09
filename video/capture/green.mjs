import { readFile } from "node:fs/promises";

const youtube = "https://www.youtube-nocookie.com";
const embed = "/embed/hjr4wfnVmpE";
const asset = "/glowtune-fixture/green-wallpaper.mp4";

export const songStart = 24;
export const wallpaperShots = {
  scene: 4,
  glow: 9,
  cinema: 28,
  layout: 36,
  search: 41.7,
  services: 45.2,
  controls: 52,
  upnext: 18,
  portrait: 4,
};

export async function greenMedia() {
  const [movie, still, pulse] = await Promise.all([
    readFile(new URL("../raw/green-wallpaper.mp4", import.meta.url)),
    readFile(new URL("../raw/green-cover.jpg", import.meta.url)),
    readFile(new URL("../src/green-pulse.json", import.meta.url), "utf8"),
  ]);
  return {
    pulse: JSON.parse(pulse),
    track: {
      title: "Green",
      artist: "Cavetown",
      album: "Lemon Boy",
      source: "Glowtune demo",
      durationMs: 270561,
      palette: ["#d5ae46", "#5f8e6b"],
      artUrl: "data:image/jpeg;base64," + still.toString("base64"),
    },
    async route(route, url, origin) {
      if (url.origin !== youtube) return false;
      if (url.pathname === embed) {
        await route.fulfill({ contentType: "text/html", body: player(origin) });
        return true;
      }
      if (url.pathname === asset) {
        const range = /^bytes=(\d+)-(\d*)$/.exec(
          route.request().headers().range ?? "",
        );
        const start = range ? Number(range[1]) : 0;
        const requestedEnd = range?.[2] ? Number(range[2]) : movie.length - 1;
        const end = Math.min(movie.length - 1, requestedEnd, start + 2_097_151);
        if (start > end) {
          await route.fulfill({
            status: 416,
            headers: { "content-range": "bytes */" + movie.length },
          });
          return true;
        }
        const headers = { "accept-ranges": "bytes" };
        if (range)
          headers["content-range"] =
            "bytes " + start + "-" + end + "/" + movie.length;
        await route.fulfill({
          status: range ? 206 : 200,
          contentType: "video/mp4",
          headers,
          body: range ? movie.subarray(start, end + 1) : movie,
        });
        return true;
      }
      return false;
    },
  };
}

function player(origin) {
  // Fulfill only the artist's embed URL with the archived licensed recording.
  // Glowtune still owns the background layout, seeking, pause, and resume.
  return (
    "<!doctype html><style>html,body{margin:0;height:100%;background:#090b08}video{width:100%;height:100%;object-fit:cover}</style>" +
    '<video muted playsinline preload="auto" src="' +
    asset +
    '"></video><script>' +
    'const video = document.querySelector("video");' +
    "const origin = " +
    JSON.stringify(origin) +
    ";" +
    'const start = Number(new URL(location.href).searchParams.get("start"));' +
    'video.addEventListener("loadedmetadata",()=>{video.currentTime=start;});' +
    'addEventListener("message", ({data, origin: sender}) => {' +
    'if(sender!==origin || typeof data!=="string")return;' +
    "const {func,args}=JSON.parse(data);" +
    'if(func==="seekTo")video.currentTime=args[0];' +
    'if(func==="playVideo")video.play().catch(()=>{});' +
    'if(func==="pauseVideo")video.pause();' +
    "});" +
    'setInterval(()=>parent.postMessage(JSON.stringify({event:"infoDelivery",info:{currentTime:video.currentTime,duration:video.duration,playerState:video.paused?2:1}}),origin),250);' +
    "</script>"
  );
}
