//! OKLab / OKLCh color math (Björn Ottosson, 2020).
//!
//! OKLab is *perceptually uniform*: equal distances look equally different,
//! which makes k-means clustering and "make this color brighter" behave the
//! way our eyes expect. sRGB distances don't.
//!
//! ```text
//! sRGB (0..1, gamma) ──decode──► linear RGB ──matrix + cbrt──► OKLab (L, a, b)
//!                                                         └──► OKLCh (L, C, h) = polar form
//! ```

/// A color in OKLab. `l` ≈ 0..1 lightness, `a` green↔red, `b` blue↔yellow.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Lab {
    pub l: f32,
    pub a: f32,
    pub b: f32,
}

impl Lab {
    pub fn chroma(self) -> f32 {
        self.a.hypot(self.b)
    }

    /// Hue angle in radians.
    pub fn hue(self) -> f32 {
        self.b.atan2(self.a)
    }

    pub fn from_lch(l: f32, c: f32, h: f32) -> Self {
        Self {
            l,
            a: c * h.cos(),
            b: c * h.sin(),
        }
    }

    pub fn dist2(self, o: Lab) -> f32 {
        (self.l - o.l).powi(2) + (self.a - o.a).powi(2) + (self.b - o.b).powi(2)
    }
}

pub fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

pub fn linear_to_srgb(c: f32) -> f32 {
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB bytes → OKLab.
pub fn from_srgb8(rgb: [u8; 3]) -> Lab {
    let [r, g, b] = rgb.map(|v| srgb_to_linear(v as f32 / 255.0));
    from_linear([r, g, b])
}

pub fn from_linear([r, g, b]: [f32; 3]) -> Lab {
    let l = 0.412_221_46 * r + 0.536_332_55 * g + 0.051_445_995 * b;
    let m = 0.211_903_5 * r + 0.680_699_5 * g + 0.107_396_96 * b;
    let s = 0.088_302_46 * r + 0.281_718_85 * g + 0.629_978_7 * b;
    let (l, m, s) = (l.cbrt(), m.cbrt(), s.cbrt());
    Lab {
        l: 0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        a: 1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        b: 0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    }
}

/// OKLab → linear RGB (may be out of 0..1 gamut).
pub fn to_linear(c: Lab) -> [f32; 3] {
    let l = c.l + 0.396_337_78 * c.a + 0.215_803_76 * c.b;
    let m = c.l - 0.105_561_346 * c.a - 0.063_854_17 * c.b;
    let s = c.l - 0.089_484_18 * c.a - 1.291_485_5 * c.b;
    let (l, m, s) = (l * l * l, m * m * m, s * s * s);
    [
        4.076_741_7 * l - 3.307_711_6 * m + 0.230_969_94 * s,
        -1.268_438 * l + 2.609_757_4 * m - 0.341_319_38 * s,
        -0.004_196_086_3 * l - 0.703_418_6 * m + 1.707_614_7 * s,
    ]
}

fn in_gamut(rgb: [f32; 3]) -> bool {
    rgb.iter().all(|&v| (-1e-4..=1.0 + 1e-4).contains(&v))
}

/// OKLab → sRGB bytes. Out-of-gamut colors keep their lightness and hue and
/// lose chroma (binary search), so they look like the "same color, a bit
/// less saturated" instead of hue-shifting like naive clipping does.
pub fn to_srgb8(c: Lab) -> [u8; 3] {
    let mut rgb = to_linear(c);
    if !in_gamut(rgb) {
        let (h, mut lo, mut hi) = (c.hue(), 0.0f32, c.chroma());
        for _ in 0..20 {
            let mid = (lo + hi) / 2.0;
            if in_gamut(to_linear(Lab::from_lch(c.l, mid, h))) {
                lo = mid;
            } else {
                hi = mid;
            }
        }
        rgb = to_linear(Lab::from_lch(c.l, lo, h));
    }
    rgb.map(|v| (linear_to_srgb(v.clamp(0.0, 1.0)) * 255.0).round() as u8)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip() {
        for rgb in [
            [0, 0, 0],
            [255, 255, 255],
            [255, 0, 0],
            [12, 200, 90],
            [30, 60, 250],
        ] {
            let back = to_srgb8(from_srgb8(rgb));
            for i in 0..3 {
                assert!(
                    (back[i] as i32 - rgb[i] as i32).abs() <= 1,
                    "{rgb:?} -> {back:?}"
                );
            }
        }
    }

    #[test]
    fn white_is_l1_no_chroma() {
        let w = from_srgb8([255, 255, 255]);
        assert!((w.l - 1.0).abs() < 1e-3 && w.chroma() < 1e-3);
    }

    #[test]
    fn gamut_mapping_keeps_hue() {
        let red = from_srgb8([255, 0, 0]);
        let boosted = Lab::from_lch(red.l, red.chroma() * 1.5, red.hue());
        let out = to_srgb8(boosted);
        assert!(out[0] > 200 && out[1] < 60 && out[2] < 60, "{out:?}");
    }
}
