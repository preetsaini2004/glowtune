// Glowtune edge glow.
//
// Every strip window (top / bottom / left / right of a monitor) runs this same
// shader in *monitor* coordinates, so the four strips form one seamless field:
//
//   ┌───────────────────── top strip ─────────────────────┐
//   │ left │                                       │ right │
//   │ strip│          (screen, untouched)          │ strip │
//   │      │                                       │       │
//   └──────────────────── bottom strip ───────────────────┘
//
// Glow strength depends on the distance to the screen edge (rounded-rect SDF);
// color depends on the position *around* the edge, where the album colors sit
// as soft blobs that slowly drift around the screen.

struct Uniforms {
    rect: vec4<f32>,     // strip x, y, w, h (physical px, monitor space)
    screen: vec4<f32>,   // monitor w, h, glow thickness px, corner radius px
    anim: vec4<f32>,     // time (s), fade 0..1, energy 0..1, intensity
    mixing: vec4<f32>,   // palette crossfade 0..1, n_from, n_to, drift phase (turns)
    effects: vec4<f32>,  // formation id, signed motion speed, strength, sparkle
    particles: vec4<f32>, // style, density, radius px, relative speed
    from_colors: array<vec4<f32>, 5>, // linear RGB
    to_colors: array<vec4<f32>, 5>,
};

@group(0) @binding(0) var<uniform> u: Uniforms;

const TAU: f32 = 6.28318530718;

@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    // One oversized triangle covers the whole window.
    let p = array(vec2(-1.0, -1.0), vec2(3.0, -1.0), vec2(-1.0, 3.0));
    return vec4(p[i], 0.0, 1.0);
}

// Signed distance to a rounded rectangle centered at 0 (negative inside).
fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2(r);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Circular distance between two positions on a loop of length 1.
fn loop_dist(a: f32, b: f32) -> f32 {
    let d = abs(fract(a) - fract(b));
    return min(d, 1.0 - d);
}

// Palette colors placed evenly around the loop, blended with soft gaussian
// bumps, so colors melt into each other instead of forming hard bands.
fn sample_palette(t: f32, from_side: bool) -> vec3<f32> {
    let n = select(u.mixing.z, u.mixing.y, from_side);
    let count = i32(max(n, 1.0));
    var acc = vec3(0.0);
    var total = 0.0;
    for (var i = 0; i < 5; i++) {
        if (i >= count) { break; }
        let center = f32(i) / f32(count);
        let d = loop_dist(t, center) * f32(count);
        let w = exp(-d * d * 2.2) + 1e-4;
        let c = select(u.to_colors[i].rgb, u.from_colors[i].rgb, from_side);
        acc += c * w;
        total += w;
    }
    return acc / total;
}

fn linear_to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

// Cheap per-pixel hash for dithering (kills banding in dark gradients).
fn hash(p: vec2<f32>) -> f32 {
    let q = fract(p * vec2(0.1031, 0.1030));
    let r = q + dot(q, q.yx + 33.33);
    return fract((r.x + r.y) * r.x);
}

// One seeded mote per cell keeps cost fixed at every density. Centers and
// their soft bounds stay inside each cell, avoiding neighbour searches.
fn particle_light(position: f32, depth: f32, time: f32, energy: f32, formation: f32) -> f32 {
    if (u.particles.x == 0.0 || u.particles.y <= 0.0) { return 0.0; }
    let radius = max(u.particles.z, 0.5);
    let spacing = max(48.0, radius * 12.0);
    let columns = max(8.0, floor(2.0 * (u.screen.x + u.screen.y) / spacing));
    let travel = select(0.012, 0.65 / TAU, u.effects.x == 3.0);
    let clock = time * u.particles.w;
    let arc = fract(position - clock * travel) * columns;
    let row = max(depth, 0.0) / spacing;
    let cell = vec2(floor(arc), floor(row));
    let seed = hash(cell + vec2(11.0, 53.0));
    if (seed >= u.particles.y) { return 0.0; }
    let center = vec2(
        0.25 + 0.5 * hash(cell + vec2(7.0, 17.0)),
        0.25 + 0.5 * hash(cell + vec2(31.0, 3.0))
    ) + 0.06 * vec2(sin(clock * 0.47 + seed * TAU), cos(clock * 0.31 + seed * TAU));
    let q = (vec2(fract(arc), fract(row)) - center) * spacing / radius;
    let r = length(q);
    var mote = exp(-r * r * 3.0);
    if (u.particles.x == 2.0) {
        let rays = exp(-abs(q.x) * 9.0 - abs(q.y) * 1.8)
            + exp(-abs(q.y) * 9.0 - abs(q.x) * 1.8);
        mote = max(mote, rays);
    } else if (u.particles.x == 3.0) {
        mote = 0.35 * exp(-r * r * 0.8);
    }
    let twinkle = 1.0 - u.effects.w * (0.5 + 0.5 * sin(time * 2.1 + seed * TAU));
    let edge = smoothstep(0.0, 3.0, depth)
        * (1.0 - smoothstep(0.75, 1.5, depth / u.screen.z));
    return mote * (1.0 - smoothstep(1.5, 2.0, r)) * twinkle
        * edge * formation * (0.6 + 0.4 * energy);
}

@fragment
fn fs(@builtin(position) frag: vec4<f32>) -> @location(0) vec4<f32> {
    let mode = u.effects.x;
    let time = select(u.anim.x * u.effects.y, 0.0, mode == 1.0);
    let fade = u.anim.y;
    var energy = u.anim.z;
    if (mode == 1.0) { energy = 0.0; }
    let intensity = u.anim.w;

    let size = u.screen.xy;
    let p = frag.xy + u.rect.xy;
    let center = size * 0.5;

    // --- How far inside the screen edge are we? ---------------------------
    let d = -sd_round_rect(p - center, center, u.screen.w);
    // Thickness breathes very slowly and swells with the beat (Phase 3).
    let rel = (p - center) / size;
    let angle = atan2(rel.y, rel.x);
    let position = angle / TAU + 0.5;
    var breathe = 1.0;
    var formation = 1.0;
    if (mode == 0.0) {
        breathe += 0.035 * sin(time * 0.9);
    } else if (mode == 2.0) {
        // Four fixed corner blooms, with a sharper musical attack.
        formation = 0.38 + 0.62 * pow(0.5 - 0.5 * cos(angle * 4.0), 2.0);
        energy *= energy;
    } else if (mode == 3.0) {
        formation = 0.26 + 0.74 * pow(0.5 + 0.5 * cos(angle - time * 0.65), 4.0);
    } else if (mode == 4.0) {
        let warp = sin(angle * 7.0 + time * 0.8 + energy * 1.8)
            + 0.5 * sin(angle * 11.0 - time * 0.57);
        formation = 0.7 + 0.3 * warp / 1.5;
    } else if (mode == 5.0) {
        // Soft lobes overlap and merge, like wax in a lava lamp.
        var blobs = 0.0;
        for (var i = 0; i < 3; i++) {
            let index = f32(i);
            let center_t = index / 3.0 + 0.09 * sin(time * 0.23 + index * 2.1)
                + time * 0.012 * select(1.0, -1.0, i == 1);
            let width = 0.07 + 0.025 * sin(time * 0.31 + index) + energy * 0.03;
            let distance = loop_dist(position, center_t) / width;
            blobs += exp(-distance * distance);
        }
        formation = 0.22 + 0.78 * smoothstep(0.05, 1.05, blobs);
    }
    formation = mix(1.0, formation, u.effects.z);
    breathe = mix(1.0, breathe, u.effects.z);
    // All effects stay inside the existing strip allocation (1.52 × thickness).
    let thickness = u.screen.z * min(breathe * (1.0 + 0.45 * energy) * formation, 1.51);
    let x = max(d, 0.0) / thickness;

    // Bright, tight core hugging the edge + a long soft tail. The final
    // smoothstep guarantees exactly zero before the strip window ends.
    let core = exp(-x * 7.0);
    let body = exp(-x * 2.6);
    var g = core * (0.55 + 0.35 * energy) + body * 0.75;
    // Guarantee exactly zero before the strip window ends.
    g *= 1.0 - smoothstep(0.55, 1.0, x);
    // Soft-saturate: strong near the edge without hard clipping.
    g = (1.0 - exp(-g * 1.6)) * formation;

    // --- Where around the screen are we? (0..1 loop) ----------------------
    // Aspect-corrected angle, so color blobs are evenly sized on wide screens.
    var t = position;
    // Gentle organic wobble so it never looks like a static gradient.
    if (mode == 0.0) {
        t += 0.035 * sin(t * TAU * 2.0 + time * 0.23) + 0.018 * sin(t * TAU * 5.0 - time * 0.41);
    }
    t += select(u.mixing.w, 0.0, mode == 1.0);

    let from_c = sample_palette(t, true);
    let to_c = sample_palette(t, false);
    let mix_t = smoothstep(0.0, 1.0, u.mixing.x);
    var color = mix(from_c, to_c, mix_t);

    // Slow light ripples travelling along the edge.
    let shimmer = select(1.0, 0.88 + 0.12 * sin(t * TAU * 3.0 - time * 0.7), mode == 0.0);
    // The core near the edge runs slightly hotter (whiter), like a real light.
    let hot = core * core * (0.06 + 0.22 * energy);
    color = mix(color, vec3(1.0), clamp(hot, 0.0, 0.35));

    // Quiet parts rest a little dimmer; kicks push the light outward and brighter.
    let punch = 0.78 + 0.42 * energy;
    let a = clamp(g * intensity * fade * shimmer * punch, 0.0, 1.0);

    // Premultiplied output in sRGB space. Alpha is lower than the color
    // strength, so the glow mostly *adds* light (like a real light source)
    // instead of painting a translucent film over the content.
    var rgb = linear_to_srgb(color) * a;
    var alpha = a * 0.62;

    // Dither (only where there's glow, so empty pixels stay exactly zero).
    let n = (hash(frag.xy + vec2(fract(time * 7.13) * 97.0)) - 0.5) / 255.0;
    let dither_mask = smoothstep(0.0, 0.03, a);
    rgb = max(rgb + vec3(n) * dither_mask, vec3(0.0));
    alpha = clamp(alpha + n * dither_mask, 0.0, 1.0);
    let motes = clamp(particle_light(position, d, time, energy, formation)
        * intensity * fade, 0.0, 1.0);
    let mote_color = linear_to_srgb(mix(color, vec3(1.0), 0.55));
    rgb = clamp(rgb + mote_color * motes, vec3(0.0), vec3(1.0));
    alpha = clamp(alpha + motes * 0.6 * (1.0 - alpha), 0.0, 1.0);
    return vec4(rgb, alpha);
}
