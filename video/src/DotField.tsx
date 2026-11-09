import { useLayoutEffect, useRef } from "react";
import { AbsoluteFill, useCurrentFrame, useVideoConfig } from "remotion";

type Props = {
  colors: readonly [string, string];
  density: number;
  beat: number;
  drop?: number;
  quiet?: boolean;
};

export const DotField = ({
  colors,
  density,
  beat,
  drop = 0,
  quiet = false,
}: Props) => {
  const ref = useRef<HTMLCanvasElement>(null);
  const frame = useCurrentFrame();
  const { width, height, fps } = useVideoConfig();
  const time = frame / fps;

  useLayoutEffect(() => {
    const ctx = ref.current?.getContext("2d");
    if (!ctx) return;
    const scale = width / 1920;
    const spacing = (29 * scale) / Math.sqrt(density);
    ctx.clearRect(0, 0, width, height);
    for (let row = -3; row < height / spacing + 3; row++) {
      const y = row * spacing;
      for (let column = -1; column <= width / spacing + 1; column++) {
        const x = (column + (row % 2) * 0.48) * spacing;
        const nx = x / width;
        const ny = y / height;
        const dist = Math.hypot(nx - 0.48, (ny - 0.64) * 0.85);
        const front = 0.1 + (1 - beat) * 0.66;
        const ripple = Math.exp(-((dist - front) ** 2) / 0.0034) * beat;
        const wave = (Math.sin(nx * 10.4 - ny * 7.3 + time * 0.48) + 1) / 2;
        const drift = Math.sin(time * 0.48 + nx * 7 + ny * 6);
        const fall =
          drop * Math.max(0, Math.sin(nx * 5 + 0.7)) * height * 0.094;
        const radius = (0.9 + wave * 0.82 + ripple * 2.15) * scale;
        const vignetting =
          1 - Math.min(0.72, Math.hypot(nx - 0.5, ny - 0.5) * 0.66);
        ctx.fillStyle = colors[wave > 0.46 ? 0 : 1];
        ctx.globalAlpha =
          (0.16 + wave * 0.24 + ripple * 0.6 + drop * 0.17) *
          vignetting *
          (quiet ? 0.57 : 1);
        ctx.beginPath();
        ctx.arc(
          x + drift * 4 * scale,
          y + drift * 4 * scale + fall,
          radius,
          0,
          2 * Math.PI,
        );
        ctx.fill();
      }
    }
  }, [beat, colors, density, drop, height, quiet, time, width]);

  return (
    <AbsoluteFill
      style={{
        background:
          "radial-gradient(ellipse at 18% 82%, #35211988, transparent 60%), radial-gradient(ellipse at 87% 35%, #10312f99, transparent 58%), #08090c",
      }}
    >
      <canvas ref={ref} width={width} height={height} aria-hidden />
      <AbsoluteFill
        style={{
          background:
            "radial-gradient(ellipse at center, transparent 44%, #050608ad 100%)",
        }}
      />
    </AbsoluteFill>
  );
};
