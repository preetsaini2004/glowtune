//! Renders album art next to its extracted glow palette, for eyeballing.
//!
//! `cargo run -p analysis --example palette_preview -- <art> [more art...]`
//! Writes `target/palette_preview.png` (one row per image).

use image::{Rgba, RgbaImage, imageops};

fn main() {
    let paths: Vec<String> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!("usage: palette_preview <image> [image...]");
        std::process::exit(2);
    }
    const ROW: u32 = 160;
    const SWATCH: u32 = 110;
    let mut out = RgbaImage::from_pixel(
        ROW + 4 * SWATCH + 40,
        ROW * paths.len() as u32,
        Rgba([16, 16, 20, 255]),
    );

    for (row, path) in paths.iter().enumerate() {
        let bytes = std::fs::read(path).expect("read image");
        let started = std::time::Instant::now();
        let palette = analysis::extract_palette(&bytes).expect("palette");
        println!("{path}: {:?} in {:?}", palette, started.elapsed());

        let art = image::load_from_memory(&bytes).unwrap().resize_exact(
            ROW - 10,
            ROW - 10,
            imageops::FilterType::Triangle,
        );
        let y = row as u32 * ROW;
        imageops::overlay(&mut out, &art.to_rgba8(), 5, (y + 5) as i64);
        for (i, c) in palette.colors.iter().enumerate() {
            let x0 = ROW + 20 + i as u32 * SWATCH;
            for yy in y + 20..y + ROW - 20 {
                for xx in x0..x0 + SWATCH - 10 {
                    out.put_pixel(xx, yy, Rgba([c[0], c[1], c[2], 255]));
                }
            }
        }
    }
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../target/palette_preview.png"
    );
    out.save(path).unwrap();
    println!("wrote {path}");
}
