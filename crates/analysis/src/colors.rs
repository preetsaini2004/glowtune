//! Album art → glow palette.
//!
//! ```text
//!  image bytes ─decode─► 64×64 thumbnail ─► OKLab pixels ─k-means (k=8)─► clusters
//!                                                                        │
//!   GlowPalette ◄─ "glowify" (lift lightness, boost chroma) ◄─ pick 2–4 ─┘
//!                                                              (vivid, varied, weighted)
//! ```
//!
//! Album art is often dark, and dark colors are invisible as light, so we keep
//! each color's *hue* but lift its lightness into a "glowing" range. Grey-ish
//! art gets a soft warm-white palette instead of muddy grey.
//! Runs once per track (~1–3 ms).

use std::f32::consts::PI;

use image::imageops::FilterType;

use crate::oklab::{self, Lab};

const THUMB: u32 = 64;
const K: usize = 8;
/// Hue/chroma count this much more than lightness when clustering, so a dark
/// maroon doesn't merge into black just because both are dark.
const CHROMA_EMPHASIS: f32 = 2.0;
const ITERATIONS: usize = 12;
const MAX_COLORS: usize = 4;

/// Colors for the edge glow, most important first (2–4 colors).
#[derive(Debug, Clone, PartialEq)]
pub struct GlowPalette {
    /// sRGB colors, already tuned to look good as light.
    pub colors: Vec<[u8; 3]>,
    /// How much of the image each color represents (sums to ~1).
    pub weights: Vec<f32>,
}

impl GlowPalette {
    /// Shown when nothing (or no art) is known: a calm violet → blue.
    pub fn fallback() -> Self {
        Self {
            colors: vec![[139, 124, 255], [92, 160, 255]],
            weights: vec![0.5, 0.5],
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum PaletteError {
    #[error("could not decode album art: {0}")]
    Decode(#[from] image::ImageError),
    #[error("image has no visible pixels")]
    Empty,
}

/// Decodes encoded album art (JPEG/PNG/WebP/BMP/GIF) and extracts a glow palette.
pub fn extract_palette(image_bytes: &[u8]) -> Result<GlowPalette, PaletteError> {
    let img = image::load_from_memory(image_bytes)?;
    // Triangle filter averages neighbors, so small high-contrast details
    // (text, noise) don't produce fake clusters.
    let thumb = img
        .thumbnail(THUMB * 2, THUMB * 2)
        .resize(THUMB, THUMB, FilterType::Triangle)
        .to_rgba8();
    palette_from_rgba(thumb.width(), thumb.height(), thumb.as_raw())
}

/// Extracts a palette from raw RGBA8 pixels (`width * height * 4` bytes).
pub fn palette_from_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> Result<GlowPalette, PaletteError> {
    let pixels: Vec<Lab> = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .take((width * height) as usize)
        .filter(|p| p[3] >= 128)
        .map(|p| emphasize(oklab::from_srgb8([p[0], p[1], p[2]])))
        .collect();
    if pixels.is_empty() {
        return Err(PaletteError::Empty);
    }

    let clusters: Vec<Cluster> = kmeans(&pixels, K, ITERATIONS)
        .into_iter()
        .map(|c| Cluster {
            center: deemphasize(c.center),
            weight: c.weight,
        })
        .collect();
    Ok(build_palette(clusters))
}

fn emphasize(c: Lab) -> Lab {
    Lab {
        l: c.l,
        a: c.a * CHROMA_EMPHASIS,
        b: c.b * CHROMA_EMPHASIS,
    }
}

fn deemphasize(c: Lab) -> Lab {
    Lab {
        l: c.l,
        a: c.a / CHROMA_EMPHASIS,
        b: c.b / CHROMA_EMPHASIS,
    }
}

#[derive(Debug, Clone, Copy)]
struct Cluster {
    center: Lab,
    weight: f32,
}

/// Plain k-means with deterministic k-means++ seeding (same art → same colors).
fn kmeans(px: &[Lab], k: usize, iterations: usize) -> Vec<Cluster> {
    let k = k.min(px.len());
    let mut rng = XorShift(0x9E37_79B9_7F4A_7C15 ^ px.len() as u64);

    // k-means++: each next center is picked far from existing ones.
    let mut centers = vec![px[rng.below(px.len())]];
    let mut d2: Vec<f32> = px.iter().map(|p| p.dist2(centers[0])).collect();
    while centers.len() < k {
        let total: f32 = d2.iter().sum();
        if total <= f32::EPSILON {
            break; // all pixels identical
        }
        let mut target = rng.unit() * total;
        let idx = d2.iter().position(|&d| {
            target -= d;
            target <= 0.0
        });
        let c = px[idx.unwrap_or(px.len() - 1)];
        centers.push(c);
        for (d, p) in d2.iter_mut().zip(px) {
            *d = d.min(p.dist2(c));
        }
    }

    let mut assign = vec![0usize; px.len()];
    for _ in 0..iterations {
        let mut changed = false;
        for (a, p) in assign.iter_mut().zip(px) {
            let best = nearest(&centers, *p);
            changed |= *a != best;
            *a = best;
        }
        let mut sums = vec![(Lab::default(), 0usize); centers.len()];
        for (&a, p) in assign.iter().zip(px) {
            let s = &mut sums[a];
            s.0.l += p.l;
            s.0.a += p.a;
            s.0.b += p.b;
            s.1 += 1;
        }
        for (c, (sum, n)) in centers.iter_mut().zip(&sums) {
            if *n > 0 {
                let n = *n as f32;
                *c = Lab {
                    l: sum.l / n,
                    a: sum.a / n,
                    b: sum.b / n,
                };
            }
        }
        if !changed {
            break;
        }
    }

    let mut counts = vec![0usize; centers.len()];
    for &a in &assign {
        counts[a] += 1;
    }
    centers
        .into_iter()
        .zip(counts)
        .filter(|(_, n)| *n > 0)
        .map(|(center, n)| Cluster {
            center,
            weight: n as f32 / px.len() as f32,
        })
        .collect()
}

fn nearest(centers: &[Lab], p: Lab) -> usize {
    let mut best = (0, f32::MAX);
    for (i, c) in centers.iter().enumerate() {
        let d = p.dist2(*c);
        if d < best.1 {
            best = (i, d);
        }
    }
    best.0
}

/// Chroma below this is "grey" (OKLab chroma of vivid colors is ~0.1–0.3).
const GREY_CHROMA: f32 = 0.035;
/// Hues closer than this (radians, ~28°) count as the same color.
const MIN_HUE_GAP: f32 = 0.5;

fn build_palette(mut clusters: Vec<Cluster>) -> GlowPalette {
    let colorful: f32 = clusters
        .iter()
        .filter(|c| c.center.chroma() >= GREY_CHROMA)
        .map(|c| c.weight)
        .sum();

    // Grey-scale art (or a tiny colored speck): soft warm-white glow.
    if colorful < 0.04 {
        return GlowPalette {
            colors: vec![
                oklab::to_srgb8(Lab::from_lch(0.86, 0.025, 1.2)), // warm white
                oklab::to_srgb8(Lab::from_lch(0.74, 0.03, 4.3)),  // cool silver-blue
            ],
            weights: vec![0.6, 0.4],
        };
    }

    // Score: big areas matter, but vivid colors matter more for light.
    // sqrt(weight) keeps a small vivid accent from losing to a huge dull background.
    let score = |c: &Cluster| {
        let chroma = c.center.chroma();
        let vivid = if chroma < GREY_CHROMA {
            0.15
        } else {
            (chroma / 0.15).min(1.6)
        };
        let too_dark = if c.center.l < 0.18 { 0.4 } else { 1.0 };
        c.weight.sqrt() * vivid * too_dark
    };
    clusters.sort_by(|a, b| score(b).total_cmp(&score(a)));

    let mut picked: Vec<Cluster> = Vec::new();
    for c in &clusters {
        if picked.len() == MAX_COLORS {
            break;
        }
        if c.center.chroma() < GREY_CHROMA || score(c) < 0.05 {
            continue;
        }
        let distinct = picked.iter().all(|p| {
            hue_gap(p.center.hue(), c.center.hue()) > MIN_HUE_GAP
                || (p.center.l - c.center.l).abs() > 0.25
        });
        if distinct {
            picked.push(*c);
        }
    }

    // A single hue: add a lighter, hue-shifted companion so the glow still has movement.
    if picked.len() == 1 {
        let base = picked[0].center;
        let mut companion = base;
        companion = Lab::from_lch(
            (companion.l + 0.15).min(0.9),
            base.chroma() * 0.8,
            base.hue() + 0.35,
        );
        picked.push(Cluster {
            center: companion,
            weight: picked[0].weight * 0.5,
        });
    }

    let total: f32 = picked
        .iter()
        .map(|c| c.weight)
        .sum::<f32>()
        .max(f32::EPSILON);
    GlowPalette {
        colors: picked
            .iter()
            .map(|c| oklab::to_srgb8(glowify(c.center)))
            .collect(),
        weights: picked.iter().map(|c| c.weight / total).collect(),
    }
}

/// Makes a color work as *light*: lift dark colors into a glowing lightness
/// range and give a little extra saturation, keeping the hue.
fn glowify(c: Lab) -> Lab {
    let l = 0.66 + (c.l.clamp(0.0, 1.0) - 0.5) * 0.35; // maps 0..1 → ~0.49..0.84
    let l = l.clamp(0.6, 0.86);
    let chroma = (c.chroma() * 1.2 + 0.02).min(0.32);
    Lab::from_lch(l, chroma, c.hue())
}

fn hue_gap(a: f32, b: f32) -> f32 {
    let d = (a - b).abs() % (2.0 * PI);
    d.min(2.0 * PI - d)
}

/// Tiny deterministic RNG for k-means++ seeding.
struct XorShift(u64);

impl XorShift {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn unit(&mut self) -> f32 {
        (self.next() >> 40) as f32 / (1u64 << 24) as f32
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(regions: &[([u8; 3], usize)]) -> Vec<u8> {
        regions
            .iter()
            .flat_map(|&(rgb, n)| std::iter::repeat_n([rgb[0], rgb[1], rgb[2], 255], n))
            .flatten()
            .collect()
    }

    fn hue_of(rgb: [u8; 3]) -> f32 {
        oklab::from_srgb8(rgb).hue()
    }

    #[test]
    fn two_color_art_gives_both_hues() {
        let px = image(&[([20, 40, 200], 2048), ([230, 200, 20], 2048)]);
        let p = palette_from_rgba(64, 64, &px).unwrap();
        assert!(p.colors.len() >= 2, "{p:?}");
        let hues: Vec<f32> = p.colors.iter().map(|&c| hue_of(c)).collect();
        assert!(
            hues.iter()
                .any(|&h| hue_gap(h, hue_of([20, 40, 200])) < 0.3),
            "{p:?}"
        );
        assert!(
            hues.iter()
                .any(|&h| hue_gap(h, hue_of([230, 200, 20])) < 0.3),
            "{p:?}"
        );
    }

    #[test]
    fn dark_art_is_lifted_into_visible_light() {
        let px = image(&[([40, 5, 10], 4096)]); // very dark red
        let p = palette_from_rgba(64, 64, &px).unwrap();
        let first = oklab::from_srgb8(p.colors[0]);
        assert!(first.l >= 0.55, "too dark to glow: {p:?}");
        assert!(
            hue_gap(first.hue(), hue_of([40, 5, 10])) < 0.4,
            "hue drifted: {p:?}"
        );
        assert!(
            p.colors.len() >= 2,
            "single hue should get a companion: {p:?}"
        );
    }

    #[test]
    fn small_vivid_accent_beats_dull_background() {
        let px = image(&[([60, 60, 62], 3600), ([255, 30, 120], 496)]);
        let p = palette_from_rgba(64, 64, &px).unwrap();
        assert!(
            hue_gap(hue_of(p.colors[0]), hue_of([255, 30, 120])) < 0.3,
            "{p:?}"
        );
    }

    #[test]
    fn greyscale_art_gets_soft_white() {
        let px = image(&[([10, 10, 10], 2000), ([200, 200, 200], 2096)]);
        let p = palette_from_rgba(64, 64, &px).unwrap();
        for c in &p.colors {
            let lab = oklab::from_srgb8(*c);
            assert!(lab.chroma() < 0.06 && lab.l > 0.7, "{p:?}");
        }
    }

    #[test]
    fn deterministic() {
        let px: Vec<u8> = (0..64 * 64)
            .flat_map(|i: u32| {
                [
                    (i * 7 % 256) as u8,
                    (i * 13 % 256) as u8,
                    (i * 31 % 256) as u8,
                    255,
                ]
            })
            .collect();
        assert_eq!(
            palette_from_rgba(64, 64, &px).unwrap(),
            palette_from_rgba(64, 64, &px).unwrap()
        );
    }

    #[test]
    fn transparent_image_is_an_error() {
        assert!(matches!(
            palette_from_rgba(2, 1, &[0; 8]),
            Err(PaletteError::Empty)
        ));
    }

    #[test]
    fn real_art_decodes() {
        // Uses the art saved by the media_source demo, if present.
        let path = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../target/now_playing_art.png"
        );
        if let Ok(bytes) = std::fs::read(path) {
            let p = extract_palette(&bytes).unwrap();
            assert!((2..=MAX_COLORS).contains(&p.colors.len()), "{p:?}");
        }
    }
}
