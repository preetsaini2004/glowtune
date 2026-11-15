//! Splits a section of a song into 4 stems and writes them as WAVs.
//! Usage: separate <model.onnx> <song> <from_s> <len_s> <out_dir>
use std::time::Instant;

use stems_proof::{SR, STEMS, Separator, decode, rms, write_wav};

fn main() -> ort::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (model, song, out) = (&a[1], &a[2], std::path::Path::new(&a[5]));
    let from: f64 = a[3].parse().unwrap();
    let len: f64 = a[4].parse().unwrap();
    let all = decode(song);
    let s = ((from * SR as f64) as usize).min(all.len());
    let e = (((from + len) * SR as f64) as usize).min(all.len());
    let window = &all[s..e];
    let mut sep = Separator::new(model, 2)?;
    let t = Instant::now();
    let stems = sep.separate(window)?;
    println!(
        "separated {:.1} s of audio in {:.1} s",
        window.len() as f32 / SR as f32,
        t.elapsed().as_secs_f32()
    );
    std::fs::create_dir_all(out).unwrap();
    write_wav(&out.join("0-original.wav"), window, SR);
    for (name, stem) in STEMS.iter().zip(&stems) {
        println!("{name:>7}: level {:.3}", rms(stem));
        write_wav(&out.join(format!("{name}.wav")), stem, SR);
    }
    // The four stems should add back up to the original.
    let resid: Vec<[f32; 2]> = (0..window.len())
        .map(|i| {
            let mut f = window[i];
            for st in &stems {
                f[0] -= st[i][0];
                f[1] -= st[i][1];
            }
            f
        })
        .collect();
    println!(
        "original level {:.3}, left over after summing stems {:.4}",
        rms(window),
        rms(&resid)
    );
    Ok(())
}
