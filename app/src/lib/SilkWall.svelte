<script lang="ts">
  // "Silk" wallpaper: sheer silk in the song's colours folding slowly around
  // a hole, and the album cover showing through the hole (and, softly,
  // through the silk itself: it never hides the cover). The hole is cut around what's on
  // screen (lyrics, album, clock: wherever Edit layout put them), so words
  // always sit on the dark part.
  //
  //   art <img> (darkened)  ◄─ shows through the hole
  //   this canvas            ◄─ silk outside the hole, a smoky rim inside it
  //
  // One small WebGL shader at ~45 % resolution (the silk is soft, nobody can
  // tell), at most 30 frames a second, and none at all when the scene is
  // covered, the song is paused or motion is off (one still frame then).
  import { onMount } from "svelte";

  /** Screen fractions (0..1) of the things the hole must keep clear. */
  export type Hole = { x: number; y: number; w: number; h: number };

  let {
    colors,
    running,
    measure,
  }: {
    /** The album palette (hex); the silk is tinted a little toward it. */
    colors: string[];
    /** Animate (else hold a still frame). */
    running: boolean;
    /** Where the content is now; asked a couple of times a second. */
    measure: () => Hole[];
  } = $props();

  const MAX_HOLES = 4;
  const SCALE = 0.45;
  const FRAME_MS = 1000 / 30;

  const VERT = `
attribute vec2 aPos;
varying vec2 vUv;
void main() { vUv = aPos * 0.5 + 0.5; gl_Position = vec4(aPos, 0.0, 1.0); }`;

  const FRAG = `
precision highp float;
uniform vec2 uRes;
uniform float uTime;
uniform vec3 uLight;
uniform vec3 uShade;
uniform vec4 uHole[${MAX_HOLES}];
uniform int uHoles;
varying vec2 vUv;

float hash(vec2 p) {
  p = fract(p * vec2(123.34, 456.21));
  p += dot(p, p + 45.32);
  return fract(p.x * p.y);
}
float noise(vec2 p) {
  vec2 i = floor(p), f = fract(p);
  vec2 u = f * f * (3.0 - 2.0 * f);
  return mix(mix(hash(i), hash(i + vec2(1.0, 0.0)), u.x),
             mix(hash(i + vec2(0.0, 1.0)), hash(i + vec2(1.0, 1.0)), u.x), u.y);
}
float fbm(vec2 p) {
  float v = 0.0, a = 0.5;
  mat2 m = mat2(1.6, 1.2, -1.2, 1.6);
  for (int i = 0; i < 4; i++) { v += a * noise(p); p = m * p; a *= 0.5; }
  return v;
}
// The cloth's height: long smooth waves, bent by a slow warp.
float cloth(vec2 p, float t) {
  vec2 q = vec2(fbm(p * 0.9 + vec2(0.0, t * 0.025)), fbm(p * 0.9 + vec2(5.2, -t * 0.02)));
  vec2 w = p + 1.8 * q;
  return 0.6 * sin(w.x * 2.6 + w.y * 1.5 + q.y * 6.0) + 0.4 * fbm(w * 1.6 + t * 0.015);
}
float box(vec2 p, vec2 b, float r) {
  vec2 q = abs(p) - b + r;
  return length(max(q, 0.0)) + min(max(q.x, q.y), 0.0) - r;
}
float smin(float a, float b, float k) {
  float h = clamp(0.5 + 0.5 * (b - a) / k, 0.0, 1.0);
  return mix(b, a, h) - k * h * (1.0 - h);
}

void main() {
  float aspect = uRes.x / uRes.y;
  vec2 p = vec2(vUv.x * aspect, vUv.y);
  float t = uTime;

  // Satin: light falls on the cloth's folds (normal from the height's slope).
  float e = 2.0 / uRes.y;
  float h = cloth(p, t);
  float hx = cloth(p + vec2(e, 0.0), t) - h;
  float hy = cloth(p + vec2(0.0, e), t) - h;
  vec3 n = normalize(vec3(-hx / e * 0.1, -hy / e * 0.1, 1.0));
  vec3 L = normalize(vec3(-0.45, 0.55, 0.7));
  float diff = clamp(dot(n, L), 0.0, 1.0);
  float spec = pow(clamp(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0, 1.0), 22.0);
  vec3 silk = mix(uShade * 0.9, uLight, smoothstep(0.3, 1.0, diff)) + spec * 0.2;

  // The hole: soft boxes around the content, merged into one shape.
  float d = 10.0;
  for (int i = 0; i < ${MAX_HOLES}; i++) {
    if (i >= uHoles) break;
    vec4 hb = uHole[i];
    vec2 c = vec2((hb.x + hb.z * 0.5) * aspect, 1.0 - (hb.y + hb.w * 0.5));
    vec2 b = vec2(hb.z * 0.5 * aspect, hb.w * 0.5);
    d = smin(d, box(p - c, b, 0.06), 0.2);
  }
  d -= 0.03;
  // A living edge: the cloth's own flow eats into the hole.
  d += (fbm(p * 1.8 + vec2(h * 0.6, t * 0.03)) - 0.5) * 0.2;

  float silkA = smoothstep(-0.01, 0.09, d);
  // Smoke just inside the edge, fading toward the middle.
  float rim = (1.0 - silkA) * smoothstep(-0.22, 0.0, d);
  // Sheer: the folds catching the light are the most opaque, the shadows
  // let the cover through.
  float sheer = silkA * mix(0.32, 0.72, smoothstep(0.25, 1.0, diff));
  float a = sheer + rim * 0.55 * (1.0 - silkA);
  gl_FragColor = vec4(silk * sheer, a); // premultiplied: the rim is dark
}`;

  let canvas: HTMLCanvasElement | undefined = $state();

  function rgb(hex: string | undefined, fallback: [number, number, number]): [number, number, number] {
    const m = /^#?([0-9a-f]{6})$/i.exec(hex ?? "");
    if (!m) return fallback;
    const n = parseInt(m[1], 16);
    return [((n >> 16) & 255) / 255, ((n >> 8) & 255) / 255, (n & 255) / 255];
  }
  const mix3 = (a: number[], b: number[], k: number) => a.map((v, i) => v + (b[i] - v) * k);

  onMount(() => {
    const c = canvas!;
    let gl: WebGLRenderingContext | null = null;
    let prog: WebGLProgram | null = null;
    let loc: Record<string, WebGLUniformLocation | null> = {};
    let raf = 0;
    let last = 0;
    let clock = 0; // seconds of silk time (stands still while paused)
    let lastMeasure = 0;
    let holes: number[] = []; // shown (eased toward the target)
    let target: number[] = [];
    let count = 0;

    function init() {
      gl = c.getContext("webgl", { alpha: true, premultipliedAlpha: true, antialias: false, depth: false, stencil: false });
      if (!gl) return;
      const sh = (type: number, src: string) => {
        const s = gl!.createShader(type)!;
        gl!.shaderSource(s, src);
        gl!.compileShader(s);
        if (!gl!.getShaderParameter(s, gl!.COMPILE_STATUS)) console.warn("silk shader:", gl!.getShaderInfoLog(s));
        return s;
      };
      prog = gl.createProgram()!;
      gl.attachShader(prog, sh(gl.VERTEX_SHADER, VERT));
      gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, FRAG));
      gl.linkProgram(prog);
      gl.useProgram(prog);
      const buf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
      const aPos = gl.getAttribLocation(prog, "aPos");
      gl.enableVertexAttribArray(aPos);
      gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
      loc = Object.fromEntries(
        ["uRes", "uTime", "uLight", "uShade", "uHole", "uHoles"].map((n) => [n, gl!.getUniformLocation(prog!, n)]),
      );
    }

    function size() {
      const w = Math.max(64, Math.round(c.clientWidth * SCALE));
      const h = Math.max(64, Math.round(c.clientHeight * SCALE));
      if (c.width !== w || c.height !== h) {
        c.width = w;
        c.height = h;
      }
    }

    function remeasure() {
      let list = measure().filter((r) => r.w > 0.01 && r.h > 0.01);
      if (!list.length) list = [{ x: 0.3, y: 0.3, w: 0.4, h: 0.4 }];
      list = list.slice(0, MAX_HOLES);
      count = list.length;
      target = list.flatMap((r) => [r.x, r.y, r.w, r.h]);
      while (target.length < MAX_HOLES * 4) target.push(0);
      if (holes.length !== target.length) holes = [...target];
    }

    function draw(dt: number) {
      if (!gl || !prog) return;
      size();
      gl.viewport(0, 0, c.width, c.height);
      // Holes glide to where the content moved (no jump while editing).
      const k = Math.min(1, dt * 4);
      holes = holes.map((v, i) => v + (target[i] - v) * k);
      // The song's own colours, softened: never a white sheet over the cover.
      const light = mix3(rgb(colors[0], [0.6, 0.6, 0.7]), [0.92, 0.92, 0.94], 0.35).map((v) => v * 0.85);
      const shade = rgb(colors[1] ?? colors[0], [0.3, 0.3, 0.4]).map((v) => v * 0.22);
      gl.uniform2f(loc.uRes, c.width, c.height);
      gl.uniform1f(loc.uTime, clock);
      gl.uniform3fv(loc.uLight, light);
      gl.uniform3fv(loc.uShade, shade);
      gl.uniform4fv(loc.uHole, new Float32Array(holes));
      gl.uniform1i(loc.uHoles, count);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }

    function frame(now: number) {
      raf = 0;
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
      if (now - last < FRAME_MS - 2) return schedule();
      last = now;
      clock += dt;
      if (now - lastMeasure > 500) {
        lastMeasure = now;
        remeasure();
      }
      draw(dt);
      schedule();
    }
    function schedule() {
      if (!raf && running && !reduced.matches) raf = requestAnimationFrame(frame);
    }
    /** A still frame (paused, covered or motion off), with the hole in place. */
    function still() {
      remeasure();
      holes = [...target];
      draw(0);
    }

    const reduced = matchMedia("(prefers-reduced-motion: reduce)");
    const lost = (e: Event) => {
      e.preventDefault();
      cancelAnimationFrame(raf);
      raf = 0;
      gl = null;
    };
    const restored = () => {
      init();
      still();
      schedule();
    };
    c.addEventListener("webglcontextlost", lost);
    c.addEventListener("webglcontextrestored", restored);
    const ro = new ResizeObserver(() => still());
    ro.observe(c);

    init();
    still();
    const stop = $effect.root(() => {
      $effect(() => {
        void colors;
        if (running) {
          last = 0;
          schedule();
        } else {
          cancelAnimationFrame(raf);
          raf = 0;
          still();
        }
      });
    });

    return () => {
      stop();
      cancelAnimationFrame(raf);
      ro.disconnect();
      c.removeEventListener("webglcontextlost", lost);
      c.removeEventListener("webglcontextrestored", restored);
      gl?.getExtension("WEBGL_lose_context")?.loseContext();
    };
  });
</script>

<canvas class="silk" bind:this={canvas} aria-hidden="true"></canvas>

<style>
  .silk {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
</style>
