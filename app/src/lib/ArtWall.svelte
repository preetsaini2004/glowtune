<script lang="ts">
  // Wallpapers made from the album cover itself, drawn by one small WebGL
  // shader each. The pointer plays with all of them.
  //
  //   water     a thin sheet of water runs down the cover; the pointer wipes
  //             it clear and it flows back in
  //   frost     the cover behind frosted glass; the pointer wipes the fog off
  //             like a finger on a cold window, and the frost grows back
  //   mosaic    the cover as a wall of tiles; waves of tiles flip over, and
  //             the tiles under the pointer lift and light up
  //   halftone  the cover as moving print dots; a slow wave breathes through
  //             them and the pointer sends ripples
  //
  // On the beat (kick drum), each look answers in its own way: a drop lands
  // in the water and rings spread out, the frost crystals flash, the print
  // dots swell. Kicks come from Rust only while a look asks for them.
  //
  // Water and frost go transparent where they're wiped, so the full-resolution
  // <img> underneath shows sharp there. At most 30 frames a second, none while
  // paused / covered / motion off (unless the pointer is still playing).
  import { onMount } from "svelte";
  import { fade } from "svelte/transition";
  import { invoke } from "@tauri-apps/api/core";
  import { getCurrentWindow } from "@tauri-apps/api/window";

  export type Look = "water" | "frost" | "mosaic" | "halftone";
  let { src, running, look, beat = true }: { src: string | null; running: boolean; look: Look; beat?: boolean } = $props();

  const FRAME_MS = 1000 / 30;
  const TRAIL = 40;
  /** Seconds until a wiped spot is covered again, per look. */
  const REFILL: Record<Look, number> = { water: 2.8, frost: 4.5, mosaic: 1.6, halftone: 2.2 };
  /** Canvas size against the window (the sharp looks need more pixels). */
  const SCALE: Record<Look, number> = { water: 0.7, frost: 0.6, mosaic: 0.9, halftone: 0.85 };
  /** Looks that let the sharp image show through where they're wiped. */
  const SEE_THROUGH: Record<Look, boolean> = { water: true, frost: true, mosaic: false, halftone: false };

  const VERT = `
attribute vec2 aPos;
varying vec2 vUv;
void main() { vUv = aPos * 0.5 + 0.5; gl_Position = vec4(aPos, 0.0, 1.0); }`;

  // Everything the looks share: the cover (cross-fading between songs), noise,
  // and how recently the pointer passed each spot.
  const HEAD = `
precision highp float;
uniform vec2 uRes;
uniform float uTime;
uniform sampler2D uA;
uniform sampler2D uB;
uniform vec2 uFitA;
uniform vec2 uFitB;
uniform float uMix;
uniform vec3 uTrail[${TRAIL}];
uniform int uTrailN;
uniform float uRefill;
uniform float uKick;
uniform float uKickT;
varying vec2 vUv;

// The last kick: how strong (fading out) and a ring spreading from the middle.
float kickNow() { return uKick * exp(-uKickT * 4.5); }
float kickRing(vec2 p, float aspect, float speed, float width) {
  float d = length(p - vec2(aspect * 0.5, 0.5));
  float r = uKickT * speed;
  return sin((d - r) * 60.0) * exp(-abs(d - r) * width) * uKick * exp(-uKickT * 1.6);
}

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
vec2 fit(vec2 uv, vec2 f) { return 0.5 + (uv - 0.5) * f; }
vec3 art(vec2 uv) {
  return mix(texture2D(uA, fit(uv, uFitA)).rgb, texture2D(uB, fit(uv, uFitB)).rgb, uMix);
}
// x: how wiped this spot is (0..1), y: a ripple running out from the wipe.
vec2 wipe(vec2 p, float aspect, float radius) {
  float clear = 0.0;
  float ring = 0.0;
  for (int i = 0; i < ${TRAIL}; i++) {
    if (i >= uTrailN) break;
    vec3 tr = uTrail[i];
    float age = tr.z;
    vec2 c = vec2(tr.x * aspect, 1.0 - tr.y);
    float d = length(p - c);
    float r = radius + age * 0.012;
    float back = 1.0 - smoothstep(uRefill * 0.35, uRefill, age);
    clear = max(clear, (1.0 - smoothstep(r * 0.45, r, d)) * back);
    ring += sin((d - age * 0.16) * 70.0) * exp(-age * 2.2) * exp(-abs(d - r) * 28.0) * 0.5;
  }
  return vec2(clear, ring);
}
`;

  const MAIN: Record<Look, string> = {
    water: `
float water(vec2 p, float t) {
  vec2 q = p * vec2(1.0, 0.7);
  float h = noise(q * 3.0 + vec2(0.0, t * 0.22)) * 0.55;
  h += noise(q * 6.5 + vec2(t * 0.05, t * 0.48) + 3.1) * 0.3;
  h += noise(q * 15.0 + vec2(-t * 0.08, t * 0.9) + 7.7) * 0.15;
  return h;
}
void main() {
  float aspect = uRes.x / uRes.y;
  vec2 p = vec2(vUv.x * aspect, vUv.y);
  vec2 w = wipe(p, aspect, 0.085);
  float e = 1.5 / uRes.y;
  float h0 = water(p, uTime);
  float h = h0 + w.y * 0.04;
  vec2 slope = vec2(water(p + vec2(e, 0.0), uTime) - h0, water(p + vec2(0.0, e), uTime) - h0) / e;
  float wet = 1.0 - w.x;
  float ring = kickRing(p, aspect, 0.5, 16.0);
  vec2 away = normalize(p - vec2(aspect * 0.5, 0.5) + 1e-4);
  slope += away * ring * 2.6;
  vec3 col = art(vUv + slope * 0.0045 * wet + vec2(away.x / aspect, away.y) * ring * 0.007 * wet);
  vec3 n = normalize(vec3(-slope * 0.012, 1.0));
  vec3 L = normalize(vec3(-0.35, 0.6, 0.72));
  float spec = pow(max(dot(reflect(-L, n), vec3(0.0, 0.0, 1.0)), 0.0), 60.0);
  col *= 0.94 + 0.1 * h;
  col += spec * 0.28 * wet + max(ring, 0.0) * 0.1 * wet;
  gl_FragColor = vec4(col * wet, wet);
}`,

    frost: `
void main() {
  float aspect = uRes.x / uRes.y;
  vec2 p = vec2(vUv.x * aspect, vUv.y);
  vec2 w = wipe(p, aspect, 0.075);
  // Frost grows back in patches, not as a neat circle.
  float patchy = noise(p * 9.0 + 4.0) * 0.6 + noise(p * 23.0) * 0.4;
  float cleared = smoothstep(patchy * 0.55, patchy * 0.55 + 0.25, w.x);
  // Seen through frost: soft (a few taps around), paler, a little blue-cold.
  float b = 0.012;
  vec3 soft = art(vUv) * 0.28 + art(vUv + vec2(b, 0.0)) * 0.18 + art(vUv - vec2(b, 0.0)) * 0.18
            + art(vUv + vec2(0.0, b * aspect)) * 0.18 + art(vUv - vec2(0.0, b * aspect)) * 0.18;
  vec3 col = mix(soft, vec3(0.86, 0.9, 0.96), 0.32);
  // Crystals: fine sharp ridges, glinting slowly.
  float n1 = noise(p * 60.0);
  float n2 = noise(p * 140.0 + 7.0);
  float ridge = pow(1.0 - abs(n1 * 2.0 - 1.0), 6.0) * 0.5 + pow(1.0 - abs(n2 * 2.0 - 1.0), 10.0) * 0.4;
  float glint = pow(noise(p * 90.0 + uTime * 0.6), 18.0) * 3.0;
  float flash = 1.0 + kickNow() * 2.2;
  col += (ridge * 0.16 * flash + glint * 0.5 * flash) * vec3(0.95, 0.97, 1.0);
  col += kickNow() * 0.05 * vec3(0.9, 0.95, 1.0);
  // A dark wet rim where the fog was just wiped.
  float rim = smoothstep(0.0, 0.25, w.x) * (1.0 - smoothstep(0.25, 0.75, w.x));
  col *= 1.0 - rim * 0.25;
  float a = 1.0 - cleared;
  gl_FragColor = vec4(col * a, a);
}`,

    mosaic: `
void main() {
  float aspect = uRes.x / uRes.y;
  vec2 p = vec2(vUv.x * aspect, vUv.y);
  vec2 w = wipe(p, aspect, 0.11);
  float cols = 30.0;
  vec2 grid = vec2(cols, cols / aspect);
  vec2 id = floor(vUv * grid);
  vec2 local = fract(vUv * grid) - 0.5;
  // A wave of flips sweeps across the wall every few seconds.
  float sweep = fract(uTime * 0.07 - (id.x + id.y * 1.4) * 0.012);
  float flip = smoothstep(0.0, 0.06, sweep) * (1.0 - smoothstep(0.06, 0.12, sweep));
  float turn = cos(flip * 3.14159);
  // Tiles near the pointer lift (grow) and light up.
  float lift = w.x;
  float scale = 0.92 + lift * 0.1 + kickNow() * 0.05;
  vec2 q = local / vec2(max(abs(turn), 0.05), 1.0) / scale;
  float inside = step(abs(q.x), 0.5) * step(abs(q.y), 0.5);
  vec2 uv = (id + 0.5 + q) / grid;
  vec3 col = art(uv);
  // The back of a flipping tile is a single colour from its own patch.
  vec3 back = art((id + 0.5) / grid) * 0.7;
  col = mix(col, back, step(turn, 0.0));
  float shade = 0.72 + 0.28 * abs(turn) + lift * 0.25 + 0.06 * hash(id);
  col *= shade;
  col += w.y * 0.05;
  vec3 gap = vec3(0.03, 0.028, 0.04);
  gl_FragColor = vec4(mix(gap, col, inside), 1.0);
}`,

    halftone: `
void main() {
  float aspect = uRes.x / uRes.y;
  vec2 p = vec2(vUv.x * aspect, vUv.y);
  vec2 w = wipe(p, aspect, 0.12);
  // A dot grid turned 25 degrees, like a print screen.
  float cell = 11.0 / uRes.y;
  float s = sin(0.436), c = cos(0.436);
  mat2 rot = mat2(c, -s, s, c);
  vec2 g = rot * p / cell;
  vec2 id = floor(g);
  vec2 local = fract(g) - 0.5;
  vec2 centre = mat2(c, s, -s, c) * ((id + 0.5) * cell);
  vec2 uv = vec2(centre.x / aspect, centre.y);
  vec3 col = art(uv);
  float luma = dot(col, vec3(0.299, 0.587, 0.114));
  // A slow wave breathes across the dots; the pointer swells them.
  float breathe = 0.5 + 0.5 * sin(length(p - vec2(aspect * 0.5, 0.5)) * 9.0 - uTime * 0.9);
  float r = sqrt(luma) * (0.5 + 0.08 * breathe + kickNow() * 0.16) + w.x * 0.18 + w.y * 0.25
          + kickRing(p, aspect, 0.65, 10.0) * 0.06;
  float d = length(local);
  float inked = 1.0 - smoothstep(r - 0.06, r + 0.02, d);
  vec3 ink = mix(vec3(luma), col, 1.45);
  vec3 paper = vec3(0.04, 0.035, 0.05);
  gl_FragColor = vec4(mix(paper, clamp(ink, 0.0, 1.0), inked), 1.0);
}`,
  };

  let canvas: HTMLCanvasElement | undefined = $state();

  onMount(() => {
    const c = canvas!;
    let gl: WebGLRenderingContext | null = null;
    let loc: Record<string, WebGLUniformLocation | null> = {};
    let tex: (WebGLTexture | null)[] = [null, null];
    let fits: [number, number][] = [
      [1, 1],
      [1, 1],
    ];
    let mixAt = 1;
    let mixStart = 0;
    let raf = 0;
    let last = 0;
    let clock = 0;
    let shownSrc: string | null = null;
    let builtFor: Look | null = null;
    const trail: { x: number; y: number; t: number }[] = [];
    // The last kick: strength and when (performance.now ms).
    let kick = 0;
    let kickAt = -1e9;
    const KICK_FOR = 1600;

    function init() {
      gl = c.getContext("webgl", { alpha: true, premultipliedAlpha: true, antialias: false, depth: false, stencil: false });
      if (!gl) return;
      builtFor = null;
      tex = [blank(), blank()];
      shownSrc = null;
      program(look);
      load(src);
    }
    /** (Re)build the shader for a look. */
    function program(which: Look) {
      if (!gl || builtFor === which) return;
      builtFor = which;
      const sh = (type: number, text: string) => {
        const s = gl!.createShader(type)!;
        gl!.shaderSource(s, text);
        gl!.compileShader(s);
        if (!gl!.getShaderParameter(s, gl!.COMPILE_STATUS)) console.warn(`${which} shader:`, gl!.getShaderInfoLog(s));
        return s;
      };
      const prog = gl.createProgram()!;
      gl.attachShader(prog, sh(gl.VERTEX_SHADER, VERT));
      gl.attachShader(prog, sh(gl.FRAGMENT_SHADER, HEAD + MAIN[which]));
      gl.linkProgram(prog);
      gl.useProgram(prog);
      const buf = gl.createBuffer();
      gl.bindBuffer(gl.ARRAY_BUFFER, buf);
      gl.bufferData(gl.ARRAY_BUFFER, new Float32Array([-1, -1, 1, -1, -1, 1, 1, 1]), gl.STATIC_DRAW);
      const aPos = gl.getAttribLocation(prog, "aPos");
      gl.enableVertexAttribArray(aPos);
      gl.vertexAttribPointer(aPos, 2, gl.FLOAT, false, 0, 0);
      loc = Object.fromEntries(
        ["uRes", "uTime", "uA", "uB", "uFitA", "uFitB", "uMix", "uTrail", "uTrailN", "uRefill", "uKick", "uKickT"].map((n) => [
          n,
          gl!.getUniformLocation(prog, n),
        ]),
      );
      gl.uniform1i(loc.uA, 0);
      gl.uniform1i(loc.uB, 1);
    }
    function blank() {
      const t = gl!.createTexture();
      gl!.bindTexture(gl!.TEXTURE_2D, t);
      gl!.texImage2D(gl!.TEXTURE_2D, 0, gl!.RGBA, 1, 1, 0, gl!.RGBA, gl!.UNSIGNED_BYTE, new Uint8Array([8, 8, 12, 255]));
      return t;
    }
    /** Cover-fit, like object-fit: cover on the <img> below. */
    function fitOf(w: number, h: number): [number, number] {
      const ia = w / h;
      const sa = c.clientWidth / Math.max(1, c.clientHeight);
      return sa > ia ? [1, ia / sa] : [sa / ia, 1];
    }

    function load(url: string | null) {
      if (!gl || !url || url === shownSrc) return;
      shownSrc = url;
      const img = new Image();
      img.crossOrigin = "anonymous";
      img.src = url;
      img
        .decode()
        .then(() => {
          if (!gl || shownSrc !== url) return;
          // The old picture becomes A; the new one fades in as B.
          [tex[0], tex[1]] = [tex[1], tex[0]];
          fits[0] = fits[1];
          gl.bindTexture(gl.TEXTURE_2D, tex[1]);
          gl.pixelStorei(gl.UNPACK_FLIP_Y_WEBGL, true);
          gl.texImage2D(gl.TEXTURE_2D, 0, gl.RGBA, gl.RGBA, gl.UNSIGNED_BYTE, img);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MIN_FILTER, gl.LINEAR);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_MAG_FILTER, gl.LINEAR);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_S, gl.CLAMP_TO_EDGE);
          gl.texParameteri(gl.TEXTURE_2D, gl.TEXTURE_WRAP_T, gl.CLAMP_TO_EDGE);
          fits[1] = fitOf(img.naturalWidth, img.naturalHeight);
          mixAt = 0;
          mixStart = performance.now();
          wake();
        })
        .catch(() => {});
    }

    function size() {
      const k = SCALE[look];
      const w = Math.max(64, Math.round(c.clientWidth * k));
      const h = Math.max(64, Math.round(c.clientHeight * k));
      if (c.width !== w || c.height !== h) {
        c.width = w;
        c.height = h;
      }
    }

    function draw(now: number) {
      if (!gl) return;
      program(look);
      size();
      gl.viewport(0, 0, c.width, c.height);
      mixAt = Math.min(1, (now - mixStart) / 1200);
      const t = now / 1000;
      const refill = REFILL[look];
      while (trail.length && t - trail[0].t > refill) trail.shift();
      const flat = new Float32Array(TRAIL * 3);
      trail.forEach((p, i) => flat.set([p.x, p.y, t - p.t], i * 3));
      gl.activeTexture(gl.TEXTURE0);
      gl.bindTexture(gl.TEXTURE_2D, tex[0]);
      gl.activeTexture(gl.TEXTURE1);
      gl.bindTexture(gl.TEXTURE_2D, tex[1]);
      gl.uniform2f(loc.uRes, c.width, c.height);
      gl.uniform1f(loc.uTime, clock);
      gl.uniform2fv(loc.uFitA, fits[0]);
      gl.uniform2fv(loc.uFitB, fits[1]);
      gl.uniform1f(loc.uMix, mixAt);
      gl.uniform3fv(loc.uTrail, flat);
      gl.uniform1i(loc.uTrailN, trail.length);
      gl.uniform1f(loc.uRefill, refill);
      const since = now - kickAt;
      gl.uniform1f(loc.uKick, since < KICK_FOR ? kick : 0);
      gl.uniform1f(loc.uKickT, Math.max(0, since) / 1000);
      gl.drawArrays(gl.TRIANGLE_STRIP, 0, 4);
    }

    /** Frames are needed while the song plays, a wipe heals or a cover fades. */
    const busy = () =>
      (running && !reduced.matches) || trail.length > 0 || mixAt < 1 || performance.now() - kickAt < KICK_FOR;
    function frame(now: number) {
      raf = 0;
      if (now - last < FRAME_MS - 2) return wake();
      const dt = last ? Math.min(0.1, (now - last) / 1000) : 0;
      last = now;
      if (running && !reduced.matches) clock += dt;
      draw(now);
      wake();
    }
    function wake() {
      if (!raf && busy()) raf = requestAnimationFrame(frame);
      else if (!busy()) last = 0;
    }

    function move(e: PointerEvent) {
      const t = performance.now() / 1000;
      // Relative to the cover's square (it sits in the middle of the screen).
      const r = c.getBoundingClientRect();
      const x = (e.clientX - r.left) / Math.max(1, r.width);
      const y = (e.clientY - r.top) / Math.max(1, r.height);
      if (x < -0.2 || x > 1.2 || y < -0.2 || y > 1.2) return;
      const prev = trail[trail.length - 1];
      if (prev && Math.hypot((x - prev.x) * r.width, (y - prev.y) * r.height) < 14 && t - prev.t < 0.05) return;
      trail.push({ x, y, t });
      if (trail.length > TRAIL) trail.shift();
      wake();
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
      draw(performance.now());
      wake();
    };
    c.addEventListener("webglcontextlost", lost);
    c.addEventListener("webglcontextrestored", restored);
    addEventListener("pointermove", move, { passive: true });
    const ro = new ResizeObserver(() => draw(performance.now()));
    ro.observe(c);

    init();
    draw(performance.now());
    // Kicks, only while this look is moving and the beat is wanted.
    const kicks = getCurrentWindow().listen<number>("kick", (e) => {
      if (!beat || !running || reduced.matches) return;
      kick = Math.max(0.35, Math.min(1, e.payload));
      kickAt = performance.now();
      wake();
    });
    const stop = $effect.root(() => {
      $effect(() => {
        load(src);
      });
      $effect(() => {
        void look;
        draw(performance.now());
      });
      $effect(() => {
        void running;
        last = 0;
        wake();
      });
      $effect(() => {
        invoke("want_kicks", { on: beat && running && !reduced.matches }).catch(() => {});
      });
    });

    return () => {
      stop();
      invoke("want_kicks", { on: false }).catch(() => {});
      kicks.then((f) => f());
      cancelAnimationFrame(raf);
      ro.disconnect();
      removeEventListener("pointermove", move);
      c.removeEventListener("webglcontextlost", lost);
      c.removeEventListener("webglcontextrestored", restored);
      gl?.getExtension("WEBGL_lose_context")?.loseContext();
    };
  });
</script>

<!-- The cover at a comfortable size in the middle, not the whole screen. -->
<div class="box">
  {#if SEE_THROUGH[look]}
    {#key src}
      {#if src}
        <img class="sharp" {src} alt="" transition:fade={{ duration: 1200 }} />
      {/if}
    {/key}
  {/if}
  <canvas class="shader" bind:this={canvas} aria-hidden="true"></canvas>
</div>

<style>
  .box {
    position: absolute;
    left: 50%;
    top: 50%;
    width: min(78vh, 62vw);
    aspect-ratio: 1;
    translate: -50% -50%;
    border-radius: 6px;
    overflow: hidden;
    box-shadow: 0 40px 120px -30px rgba(0, 0, 0, 0.9);
  }
  .sharp,
  .shader {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    display: block;
  }
  .sharp {
    object-fit: cover;
  }
</style>
