import type { CSSProperties, ReactNode } from "react";
import {
  AbsoluteFill,
  Easing,
  Html5Audio,
  Img,
  OffthreadVideo,
  Sequence,
  interpolate,
  staticFile,
  useCurrentFrame,
  useVideoConfig,
} from "remotion";
import { DotField } from "./DotField";
import greenPulse from "./green-pulse.json";
import {
  BEAT_SECONDS,
  CLIP_SECONDS,
  FPS,
  features,
  timeline,
  type Feature,
} from "./edit";

const ink = "#f7f5ed";
const accent = "#ffc674";
const colors = ["#ffb347", "#57d6b9"] as const;
const logo = () => staticFile("brand/logo.png");
const capture = (name: string) => staticFile("capture/" + name + ".mp4");
const ease = Easing.bezier(0.22, 1, 0.36, 1);
const clamp = { extrapolateLeft: "clamp", extrapolateRight: "clamp" } as const;
const font = "Instrument Sans, sans-serif";
const eyebrow: CSSProperties = {
  fontSize: 19,
  fontWeight: 610,
  letterSpacing: "0.24em",
  color: accent,
  textTransform: "uppercase",
};

function Frame({ children }: { children: ReactNode }) {
  return (
    <AbsoluteFill style={{ color: ink, fontFamily: font, overflow: "hidden" }}>
      <style>
        {"@font-face{font-family:'Instrument Sans';src:url('" +
          staticFile("brand/instrument-sans.woff2") +
          "') format('woff2');font-weight:100 900;font-display:block}"}
      </style>
      {children}
    </AbsoluteFill>
  );
}

function Screen({
  name,
  trim = 0,
  style,
  focus,
}: {
  name: string;
  trim?: number;
  style?: CSSProperties;
  focus?: { scale: number; origin: string };
}) {
  const { fps } = useVideoConfig();
  return (
    <div
      style={{
        position: "absolute",
        width: 1472,
        aspectRatio: "16 / 9",
        left: "50%",
        top: 58,
        transform: "translateX(-50%)",
        overflow: "hidden",
        borderRadius: 20,
        border: "1px solid #ffffff3d",
        background: "#101118",
        boxShadow: "0 36px 80px #000b, 0 0 74px #f0a24a25, 0 0 130px #2ed3b71b",
        ...style,
      }}
    >
      <OffthreadVideo
        src={capture(name)}
        trimBefore={Math.round(trim * fps)}
        muted
        style={{
          width: "100%",
          height: "100%",
          objectFit: "cover",
          transform: focus ? "scale(" + focus.scale + ")" : undefined,
          transformOrigin: focus?.origin,
        }}
      />
    </div>
  );
}

function Center({ children }: { children: ReactNode }) {
  return (
    <AbsoluteFill
      style={{
        justifyContent: "center",
        alignItems: "center",
        textAlign: "center",
      }}
    >
      {children}
    </AbsoluteFill>
  );
}

function Scene({
  id,
  duration,
  start,
  trim = 0,
  repository,
}: {
  id: (typeof timeline)[number]["id"];
  duration: number;
  start: number;
  trim?: number;
  repository: string;
}) {
  const frame = useCurrentFrame();
  const { fps } = useVideoConfig();
  const t = frame / fps;
  const beat = greenPulse[start * fps + frame] ?? 0;
  const drop = Math.sin(Math.PI * Math.min(1, t / 0.52)) ** 2;
  const entering = interpolate(t, [0, 0.72], [0, 1], {
    ...clamp,
    easing: ease,
  });
  const caption = interpolate(
    t,
    [0.3, 0.92, duration - 0.35, duration],
    [0, 1, 1, 0],
    clamp,
  );
  const opening = id === "opening";
  const ending = id === "end";
  const featured = id === "scene" || id === "glow" || id === "cinema";
  const detail =
    id === "cinema"
      ? {
          label: "Cinema view",
          caption: "Let the music video fill your screen.",
        }
      : features[id as Feature];
  const finish = ending
    ? interpolate(t, [duration - 2, duration], [1, 0.2], clamp)
    : 1;
  return (
    <AbsoluteFill>
      <AbsoluteFill
        style={{ opacity: opening ? entering : 1, background: "#07080a" }}
      >
        <DotField
          colors={colors}
          density={0.92}
          beat={beat * finish}
          drop={drop}
        />
      </AbsoluteFill>
      {(opening || ending) && (
        <Center>
          <div
            style={{
              opacity: entering,
              transform: "translateY(" + (1 - entering) * 26 + "px)",
              display: "grid",
              justifyItems: "center",
            }}
          >
            <Img
              src={logo()}
              style={{
                width: 240,
                height: 240,
                marginTop: -74,
                marginBottom: -30,
              }}
            />
            <div
              style={{
                fontSize: 138,
                fontWeight: 620,
                letterSpacing: "-0.085em",
                lineHeight: 0.98,
              }}
            >
              Glowtune
            </div>
            <div style={{ ...eyebrow, marginTop: 44 }}>
              {ending ? "Download for Windows" : "Desktop music companion"}
            </div>
            {ending && (
              <div style={{ fontSize: 26, color: "#e5e3d1bf", marginTop: 16 }}>
                {repository || "GitHub Releases"}
              </div>
            )}
          </div>
        </Center>
      )}
      {ending && (
        <div
          style={{
            position: "absolute",
            inset: "auto 0 42px",
            textAlign: "center",
            color: "#f7f5edb0",
            fontSize: 19,
            lineHeight: 1.45,
            opacity: entering,
          }}
        >
          “Green” · Cavetown (Robin Skinner), piano by Rachel Lyon
          <br />
          Official video by Guy Larsen &amp; Taha Khan · Excerpt adapted under
          CC BY 3.0
        </div>
      )}
      {id === "promise" && (
        <>
          <Screen
            name="scene"
            style={{
              top: 152,
              width: 1376,
              opacity: interpolate(t, [1.35, 3.6], [0, 1], {
                ...clamp,
                easing: ease,
              }),
              transform:
                "translateX(-50%) translateY(" +
                interpolate(t, [1.35, 3.6], [100, 0], {
                  ...clamp,
                  easing: ease,
                }) +
                "px)",
            }}
          />
          <Center>
            <div
              style={{
                opacity: interpolate(
                  t,
                  [0.12, 0.6, 2.55, 3.82],
                  [0, 1, 1, 0],
                  clamp,
                ),
                textShadow: "0 0 34px #08090c, 0 4px 18px #08090c",
              }}
            >
              <div style={eyebrow}>Meet Glowtune</div>
              <div
                style={{
                  fontSize: 104,
                  fontWeight: 580,
                  lineHeight: 0.99,
                  letterSpacing: "-0.073em",
                  marginTop: 24,
                }}
              >
                Your music,
                <br />
                all over your screen.
              </div>
            </div>
          </Center>
        </>
      )}
      {id === "privacy" && (
        <Center>
          <div
            style={{
              opacity: entering,
              transform: "translateY(" + (1 - entering) * 18 + "px)",
            }}
          >
            <div style={eyebrow}>Made for Windows</div>
            <div
              style={{
                fontSize: 102,
                lineHeight: 1,
                fontWeight: 600,
                letterSpacing: "-0.075em",
                marginTop: 30,
              }}
            >
              Free. Open source.
            </div>
            <div style={{ fontSize: 40, marginTop: 26, color: "#eeeadeb9" }}>
              Your settings stay on your PC.
            </div>
          </div>
        </Center>
      )}
      {detail && (
        <>
          <Screen
            name={id}
            trim={trim}
            focus={
              id === "search" || id === "services" || id === "controls"
                ? focusFor[id]
                : undefined
            }
            style={{
              opacity: entering,
              width: featured ? 1600 : 1472,
              top: featured ? 36 : 58,
              transform:
                "translateX(-50%) perspective(2100px) rotateX(" +
                (1.9 - t * 0.09) +
                "deg) translateY(" +
                (1 - entering) * 30 +
                "px) scale(" +
                (0.992 + t * 0.002) +
                ")",
            }}
          />
          <div
            style={{
              position: "absolute",
              left: featured ? 160 : 226,
              right: featured ? 160 : 226,
              bottom: 71,
              display: "flex",
              alignItems: "baseline",
              gap: 28,
              opacity: caption,
            }}
          >
            <div style={{ ...eyebrow, fontSize: 16, flexShrink: 0 }}>
              {detail.label}
            </div>
            <div
              style={{
                fontSize: 34,
                fontWeight: 510,
                letterSpacing: "-0.027em",
              }}
            >
              {detail.caption}
            </div>
            {id === "scene" && (
              <div style={{ marginLeft: "auto", fontSize: 22, color: "#fff9" }}>
                “Green” · Cavetown
              </div>
            )}
            {id === "controls" && (
              <div
                style={{
                  marginLeft: "auto",
                  fontSize: 22,
                  whiteSpace: "nowrap",
                  color: "#fff9",
                }}
              >
                F7 · F8 · F9 · Space
              </div>
            )}
          </div>
        </>
      )}
    </AbsoluteFill>
  );
}

export function Hero({ repository }: { repository: string }) {
  return (
    <Frame>
      <Html5Audio src={staticFile("capture/green.wav")} />
      {timeline.map(({ id, start, end, ...shot }) => (
        <Sequence
          key={id}
          name={id}
          from={start * FPS}
          durationInFrames={(end - start) * FPS}
        >
          <Scene
            id={id}
            duration={end - start}
            start={start}
            trim={"trim" in shot ? shot.trim : 0}
            repository={repository}
          />
        </Sequence>
      ))}
    </Frame>
  );
}

const focusFor: Partial<Record<Feature, { scale: number; origin: string }>> = {
  services: { scale: 1.45, origin: "78% 82%" },
  search: { scale: 1.4, origin: "78% 82%" },
  upnext: { scale: 1.27, origin: "97% 61%" },
  controls: { scale: 1.55, origin: "40% 94%" },
  themes: { scale: 1.18, origin: "32% 20%" },
};

export function Clip({ feature }: { feature: Feature }) {
  const frame = useCurrentFrame();
  const beat = Math.exp(-4 * (((frame / FPS) % BEAT_SECONDS) / BEAT_SECONDS));
  const fade = interpolate(
    frame,
    [0, 7, FPS * CLIP_SECONDS - 8, FPS * CLIP_SECONDS - 1],
    [0, 1, 1, 0],
    clamp,
  );
  return (
    <Frame>
      <DotField colors={colors} density={0.75} beat={beat * 0.52} quiet />
      <AbsoluteFill style={{ opacity: fade }}>
        {feature === "screens" ? (
          <>
            <Screen
              name="scene"
              trim={14}
              style={{ width: 838, top: 107, left: 490, borderRadius: 14 }}
            />
            <Screen
              name="portrait"
              style={{
                width: 303,
                aspectRatio: "9 / 16",
                top: 72,
                left: 1048,
                borderRadius: 14,
              }}
            />
          </>
        ) : (
          <Screen
            name={feature}
            trim={feature === "scene" ? 14 : 0}
            style={{ width: 1064, top: 39, borderRadius: 14 }}
            focus={focusFor[feature]}
          />
        )}
      </AbsoluteFill>
      <div
        style={{
          position: "absolute",
          left: 108,
          right: 108,
          bottom: 30,
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
        }}
      >
        <div style={{ ...eyebrow, fontSize: 16 }}>
          {features[feature].label}
        </div>
        <div style={{ fontSize: 20, color: "#e6e1d9a8" }}>
          {feature === "controls"
            ? "F7 · F8 · F9 · Space"
            : feature === "scene"
              ? "Music video wallpaper · Green by Cavetown"
              : features[feature].caption}
        </div>
      </div>
    </Frame>
  );
}

export function Poster() {
  return (
    <Frame>
      <DotField colors={colors} density={0.95} beat={0.44} />
      <Screen
        name="scene"
        trim={20.2}
        style={{ top: 224, width: 1392, borderRadius: 24 }}
      />
      <div
        style={{
          position: "absolute",
          left: 259,
          top: 65,
          display: "flex",
          alignItems: "center",
          gap: 12,
        }}
      >
        <Img
          src={logo()}
          style={{ width: 115, height: 115, marginLeft: -18 }}
        />
        <span
          style={{
            fontWeight: 650,
            fontSize: 100,
            lineHeight: 1,
            letterSpacing: "-0.084em",
          }}
        >
          Glowtune
        </span>
      </div>
      <div style={{ ...eyebrow, position: "absolute", right: 266, top: 130 }}>
        Desktop music companion
      </div>
    </Frame>
  );
}
