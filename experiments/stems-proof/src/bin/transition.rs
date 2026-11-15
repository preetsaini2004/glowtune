//! Renders song A → song B transitions in several versions, to compare by ear.
//! Usage: transition <model.onnx> <song A> <song B> <out_dir>
//!
//! ```text
//!  A: last 60 s ─┐                       B: first 60 s ─┐
//!   separate ────┤ drums, bass, other, vocals            ├─ separate
//!   beat grid from A's drums, downbeat = strongest kick phase
//!   overlap of N beats ending at A's last full bar; B starts on its first
//!   downbeat, time-stretched so its beats land on A's, then eases back to
//!   its own tempo over 8 s.
//!   stems version, over the overlap (u = 0..1):
//!     B drums in by 0.25, B other in by 0.5, bass swapped on the downbeat at 0.5,
//!     A other out over 0.5..1, A drums out over 0.75..1,
//!     A vocals out at their first gap after 0.5 (or forced by 0.9),
//!     B vocals held back until A's are gone.
//! ```
use std::path::Path;

use audio_engine::tempo;
use stems_proof::{Frame, SR, Separator, decode, rms, write_wav};

const WINDOW_S: usize = 60;

struct Song {
    mix: Vec<Frame>,
    /// drums, bass, other, vocals
    stems: [Vec<Frame>; 4],
    /// Frames per beat, and the first downbeat (frames into the window).
    period: f64,
    downbeat: f64,
    bpm: f32,
}

fn main() -> ort::Result<()> {
    let a: Vec<String> = std::env::args().collect();
    let (model, out) = (&a[1], Path::new(&a[4]));
    std::fs::create_dir_all(out).unwrap();
    let full_a = trim(decode(&a[2]));
    let full_b = trim(decode(&a[3]));
    let w = WINDOW_S * SR as usize;
    let a_mix = full_a[full_a.len().saturating_sub(w)..].to_vec();
    let b_mix = full_b[..w.min(full_b.len())].to_vec();

    let mut sep = Separator::new(model, 4)?;
    let t = std::time::Instant::now();
    let song_a = analyse(a_mix, &mut sep, "A")?;
    let song_b = analyse(b_mix, &mut sep, "B")?;
    println!("separation + analysis: {:.1} s", t.elapsed().as_secs_f32());

    // B frames per output frame so B's beats land on A's (half/double time
    // allowed, closest to 1).
    let speed = [0.5, 1.0, 2.0]
        .iter()
        .map(|k| song_b.period * k / song_a.period)
        .min_by(|x, y| x.ln().abs().total_cmp(&y.ln().abs()))
        .unwrap();
    println!("B plays at {:.3}× during the blend", speed);

    let plain = Opts {
        beats: 16,
        stems: true,
        vocal_echo: false,
        filters: false,
        drop: false,
        b_drums_before_drop: 1.0,
    };
    let only = std::env::args().nth(5); // render just one version (by prefix)
    for (name, opts) in [
        (
            "2-beatmatch-32beats-nostems",
            Opts {
                beats: 32,
                stems: false,
                ..plain
            },
        ),
        ("3-stems-32beats", Opts { beats: 32, ..plain }),
        ("4-stems-16beats", plain),
        (
            "5-vocal-echo",
            Opts {
                vocal_echo: true,
                ..plain
            },
        ),
        (
            "6-filters",
            Opts {
                filters: true,
                ..plain
            },
        ),
        (
            "7-drop",
            Opts {
                drop: true,
                ..plain
            },
        ),
        (
            "8-all",
            Opts {
                vocal_echo: true,
                filters: true,
                drop: true,
                ..plain
            },
        ),
    ] {
        if only.as_ref().is_some_and(|o| !name.starts_with(o.as_str())) {
            continue;
        }
        let mix = render(&song_a, &song_b, speed, opts);
        write_wav(&out.join(format!("{name}.wav")), &mix, SR);
        println!("wrote {name}.wav ({:.1} s)", mix.len() as f32 / SR as f32);
    }
    // The same #8 without AI: "stems" are just EQ bands of the full mix, the
    // way a DJ mixer's bass / mid / treble knobs split a song.
    if only
        .as_ref()
        .is_none_or(|o| "9".starts_with(o.as_str()) || o.starts_with('9'))
    {
        let a_eq = eq_song(&song_a.mix, "A (no AI)", false);
        let b_eq = eq_song(&song_b.mix, "B (no AI)", false);
        let speed_eq = [0.5, 1.0, 2.0]
            .iter()
            .map(|k| b_eq.period * k / a_eq.period)
            .min_by(|x, y| x.ln().abs().total_cmp(&y.ln().abs()))
            .unwrap();
        let all = Opts {
            vocal_echo: true,
            filters: true,
            drop: true,
            ..plain
        };
        let mix = render(&a_eq, &b_eq, speed_eq, all);
        write_wav(&out.join("9-no-ai-eq.wav"), &mix, SR);
        println!("wrote 9-no-ai-eq.wav");
        // Preet: #9's drums were a little loud. Both songs' treble (hats,
        // cymbals) plays before the drop: bring B's in at half level until then.
        let softer = Opts {
            b_drums_before_drop: 0.5,
            ..all
        };
        let mix = render(&a_eq, &b_eq, speed_eq, softer);
        write_wav(&out.join("10-no-ai-softer-drums.wav"), &mix, SR);
        println!("wrote 10-no-ai-softer-drums.wav");
        // #10 with the engine's clean (Linkwitz–Riley) bands: the breath and
        // the bass swap really are bass-free / bass-only.
        let a_clean = eq_song(&song_a.mix, "A (clean bands)", true);
        let b_clean = eq_song(&song_b.mix, "B (clean bands)", true);
        let mix = render(&a_clean, &b_clean, speed_eq, softer);
        write_wav(&out.join("11-no-ai-clean-bands.wav"), &mix, SR);
        println!("wrote 11-no-ai-clean-bands.wav");
    }
    let old = old_crossfade(&song_a.mix, &song_b.mix);
    write_wav(&out.join("1-old-crossfade-6s.wav"), &old, SR);
    println!("wrote 1-old-crossfade-6s.wav");
    Ok(())
}

/// A song split by EQ instead of AI: [high (≈ drums/cymbals), low (bass),
/// nothing, mid (where the vocals sit)]. The bands add back up exactly.
fn eq_song(mix: &[Frame], label: &str, clean: bool) -> Song {
    let lp = |x: &[Frame], hz: f32| -> Vec<Frame> {
        // Two one-pole stages (12 dB/oct), per channel.
        let k = 1.0 - (-std::f32::consts::TAU * hz / SR as f32).exp();
        let (mut y1, mut y2) = ([0f32; 2], [0f32; 2]);
        x.iter()
            .map(|f| {
                for ch in 0..2 {
                    y1[ch] += (f[ch] - y1[ch]) * k;
                    y2[ch] += (y1[ch] - y2[ch]) * k;
                }
                y2
            })
            .collect()
    };
    let low = lp(mix, 150.0);
    let below_high = lp(mix, 4000.0);
    let high: Vec<Frame> = mix
        .iter()
        .zip(&below_high)
        .map(|(f, l)| [f[0] - l[0], f[1] - l[1]])
        .collect();
    let mid: Vec<Frame> = (0..mix.len())
        .map(|i| {
            [
                mix[i][0] - low[i][0] - high[i][0],
                mix[i][1] - low[i][1] - high[i][1],
            ]
        })
        .collect();
    // Clean bands: the engine's Linkwitz–Riley crossover (dj::Bands).
    let (low, mid, high) = if clean {
        let mut bands = audio_engine::dj::Bands::new(SR);
        let (mut l, mut m, mut h) = (Vec::new(), Vec::new(), Vec::new());
        for f in mix {
            let (a, b, c) = bands.split(*f);
            l.push(a);
            m.push(b);
            h.push(c);
        }
        (l, m, h)
    } else {
        (low, mid, high)
    };
    // Beat grid from the full mix (as the live engine does today).
    let t = tempo::estimate(mix, SR).expect("no beat");
    let low_mono = lowpass(mix, 150.0);
    let around = (SR as f64 * 0.04) as usize;
    let mut punch = [0f64; 4];
    let mut k = 0;
    loop {
        let at = (t.first_beat + k as f64 * t.period) as usize;
        if at + around >= low_mono.len() {
            break;
        }
        punch[k % 4] += low_mono[at.saturating_sub(around)..at + around]
            .iter()
            .map(|v| (v * v) as f64)
            .sum::<f64>();
        k += 1;
    }
    let phase = (0..4)
        .max_by(|x, y| punch[*x].total_cmp(&punch[*y]))
        .unwrap();
    println!(
        "song {label}: {:.1} BPM (confidence {:.2})",
        t.bpm(SR),
        t.confidence
    );
    Song {
        mix: mix.to_vec(),
        stems: [high, low, vec![[0.0; 2]; mix.len()], mid],
        period: t.period,
        downbeat: t.first_beat + phase as f64 * t.period,
        bpm: t.bpm(SR),
    }
}

/// Drops leading and trailing near-silence.
fn trim(x: Vec<Frame>) -> Vec<Frame> {
    let loud = |f: &Frame| f[0].abs().max(f[1].abs()) > 0.01;
    let s = x.iter().position(loud).unwrap_or(0);
    let e = x.iter().rposition(loud).map_or(x.len(), |e| e + 1);
    x[s..e].to_vec()
}

fn analyse(mix: Vec<Frame>, sep: &mut Separator, label: &str) -> ort::Result<Song> {
    let stems = sep.separate(&mix)?;
    // Beat grid from the drums (+ bass): far steadier than the full mix.
    let rhythm: Vec<Frame> = stems[0]
        .iter()
        .zip(&stems[1])
        .map(|(d, b)| [d[0] + b[0] * 0.5, d[1] + b[1] * 0.5])
        .collect();
    let t = tempo::estimate(&rhythm, SR).expect("no beat in the drums");
    // Downbeat: the beat phase (of 4) with the most low-end punch (kick on 1).
    let low = lowpass(&rhythm, 150.0);
    let around = (SR as f64 * 0.04) as usize;
    let mut punch = [0f64; 4];
    let mut k = 0;
    loop {
        let at = (t.first_beat + k as f64 * t.period) as usize;
        if at + around >= low.len() {
            break;
        }
        let e: f64 = low[at.saturating_sub(around)..at + around]
            .iter()
            .map(|v| (v * v) as f64)
            .sum();
        punch[k % 4] += e;
        k += 1;
    }
    let phase = (0..4)
        .max_by(|x, y| punch[*x].total_cmp(&punch[*y]))
        .unwrap();
    let downbeat = t.first_beat + phase as f64 * t.period;
    let bpm = t.bpm(SR);
    println!(
        "song {label}: {bpm:.1} BPM (confidence {:.2}), downbeat at {:.2} s, vocals level {:.3}",
        t.confidence,
        downbeat / SR as f64,
        rms(&stems[3])
    );
    Ok(Song {
        mix,
        stems,
        period: t.period,
        downbeat,
        bpm,
    })
}

fn lowpass(x: &[Frame], hz: f32) -> Vec<f32> {
    let k = 1.0 - (-std::f32::consts::TAU * hz / SR as f32).exp();
    let mut y = 0.0;
    x.iter()
        .map(|f| {
            y += ((f[0] + f[1]) * 0.5 - y) * k;
            y
        })
        .collect()
}

fn ramp(u: f64, from: f64, to: f64) -> f32 {
    ((u - from) / (to - from)).clamp(0.0, 1.0) as f32
}

/// Equal-power shape of a 0..1 ramp.
fn eq_in(x: f32) -> f32 {
    (x * std::f32::consts::FRAC_PI_2).sin()
}

/// Per-beat vocal loudness of a song (to find gaps between lines).
fn vocal_beats(song: &Song) -> Vec<f32> {
    let v = &song.stems[3];
    let mut out = Vec::new();
    let mut at = song.downbeat.rem_euclid(song.period);
    while (at + song.period) as usize <= v.len() {
        out.push(rms(&v[at as usize..(at + song.period) as usize]));
        at += song.period;
    }
    out
}

#[derive(Clone, Copy)]
struct Opts {
    beats: usize,
    stems: bool,
    /// A's last vocal line echoes out on the beat.
    vocal_echo: bool,
    /// A's instruments high-pass away; B's drums/other open from a low-pass.
    filters: bool,
    /// One beat without drums and bass before B's bass and kick land.
    drop: bool,
    /// Level of B's drums before the drop (both songs' drums play then).
    b_drums_before_drop: f32,
}

/// RBJ biquad, stereo.
#[derive(Clone, Copy)]
struct Biquad {
    b: [f32; 3],
    a: [f32; 2],
    z: [[f32; 2]; 2],
    on: bool,
}

impl Biquad {
    fn new() -> Self {
        Self {
            b: [1.0, 0.0, 0.0],
            a: [0.0; 2],
            z: [[0.0; 2]; 2],
            on: false,
        }
    }

    fn set(&mut self, high: bool, hz: f32) {
        let w = std::f32::consts::TAU * hz.clamp(15.0, SR as f32 * 0.45) / SR as f32;
        let (s, c) = w.sin_cos();
        let al = s / std::f32::consts::SQRT_2;
        let a0 = 1.0 + al;
        let (b0, b1) = if high {
            ((1.0 + c) / 2.0, -(1.0 + c))
        } else {
            ((1.0 - c) / 2.0, 1.0 - c)
        };
        self.b = [b0 / a0, b1 / a0, b0 / a0];
        self.a = [-2.0 * c / a0, (1.0 - al) / a0];
        self.on = true;
    }

    fn run(&mut self, x: Frame) -> Frame {
        if !self.on {
            return x;
        }
        let mut y = [0.0; 2];
        for ch in 0..2 {
            let o = self.b[0] * x[ch] + self.z[0][ch];
            self.z[0][ch] = self.b[1] * x[ch] - self.a[0] * o + self.z[1][ch];
            self.z[1][ch] = self.b[2] * x[ch] - self.a[1] * o;
            y[ch] = o;
        }
        y
    }
}

fn render(a: &Song, b: &Song, speed: f64, o: Opts) -> Vec<Frame> {
    let beats = o.beats;
    let pa = a.period;
    // Overlap ends on A's last bar boundary before its end.
    let bars_in_a = ((a.mix.len() as f64 - a.downbeat) / (4.0 * pa)).floor() as usize;
    let end = a.downbeat + (bars_in_a.saturating_sub(1) * 4) as f64 * pa;
    let start = end - beats as f64 * pa;
    let (start_i, end_i) = (start.max(0.0) as usize, end as usize);
    let len = end_i - start_i;
    let lead = (15 * SR as usize).min(start_i);
    let one = 1.0 / beats as f64; // one beat, in blend progress

    // A's vocals: leave at the first quiet beat after the blend's middle.
    let vb = vocal_beats(a);
    let typical = {
        let mut s: Vec<f32> = vb.iter().copied().filter(|v| *v > 0.005).collect();
        s.sort_by(f32::total_cmp);
        s.get(s.len() / 2).copied().unwrap_or(0.0)
    };
    let beat_of = |frame: f64| ((frame - a.downbeat.rem_euclid(pa)) / pa).floor() as usize;
    let gap_beat = (beat_of(start + 0.5 * len as f64)..beat_of(end))
        .find(|k| vb.get(*k).is_some_and(|v| *v < typical * 0.25));
    let vocal_out = gap_beat
        .map(|k| ((k as f64 * pa + a.downbeat.rem_euclid(pa)) - start) / len as f64)
        .unwrap_or(0.9)
        .clamp(0.5, 0.9);
    // With the echo, the dry line stops a beat earlier and the echo fills
    // the gap instead of the next line.
    let vocal_out = if o.vocal_echo {
        (vocal_out - one).max(0.4)
    } else {
        vocal_out
    };

    let mut out: Vec<Frame> = a.mix[start_i - lead..start_i].to_vec();
    let b_overlap = beats as f64 * b.period;
    let gains_b = |u: f64| -> [f32; 4] {
        if !o.stems {
            return [eq_in(ramp(u, 0.0, 0.5)); 4];
        }
        let mut drums = eq_in(ramp(u, 0.0, 0.25)) * o.b_drums_before_drop;
        let other = eq_in(ramp(u, 0.1, 0.5));
        let bass = eq_in(ramp(u, 0.5, 0.5 + one * 0.25));
        let vocals = eq_in(ramp(u, vocal_out, vocal_out + 2.0 * one));
        if o.drop {
            // B's drums hold back during the breath too, then land with the bass.
            if (0.5 - one..0.5).contains(&u) {
                drums = 0.0;
            } else if u >= 0.5 {
                drums = 1.0;
            }
        }
        [drums, bass, other, vocals]
    };
    let gains_a = |u: f64| -> [f32; 4] {
        let fade =
            |from: f64, to: f64| ((1.0 - ramp(u, from, to)) * std::f32::consts::FRAC_PI_2).sin();
        if !o.stems {
            return [fade(0.5, 1.0); 4];
        }
        let mut g = [
            fade(0.75, 1.0),
            fade(0.5, 0.5 + one * 0.25),
            fade(0.5, 1.0),
            fade(vocal_out, vocal_out + 2.0 * one),
        ];
        if o.drop && u >= 0.5 - one {
            // The breath: A's drums and bass stop a beat before the swap.
            g[0] = 0.0;
            g[1] = 0.0;
        }
        if o.vocal_echo {
            // The dry vocal stops quickly; the echo carries it out.
            g[3] = fade(vocal_out, vocal_out + 0.25 * one);
        }
        g
    };

    // B's frames (in B time) with stem gains and the opening low-pass.
    let mut b_lp = Biquad::new();
    let mut b_frame = |i: f64| -> Frame {
        let idx = (b.downbeat + i) as usize;
        if idx >= b.mix.len() {
            return [0.0; 2];
        }
        let u = i / b_overlap;
        let g = gains_b(u);
        let mut rhythm = [0.0; 2];
        let mut rest = [0.0; 2];
        for (s, stem) in b.stems.iter().enumerate() {
            let target = if s == 0 || s == 2 {
                &mut rhythm
            } else {
                &mut rest
            };
            target[0] += stem[idx][0] * g[s];
            target[1] += stem[idx][1] * g[s];
        }
        if o.filters && u < 0.5 {
            if (i as usize).is_multiple_of(32) {
                // 400 Hz → wide open over the first half.
                b_lp.set(false, 400.0 * (18000.0f32 / 400.0).powf((u / 0.5) as f32));
            }
            rhythm = b_lp.run(rhythm);
        }
        [rhythm[0] + rest[0], rhythm[1] + rest[1]]
    };

    let mut dsp = signalsmith_stretch::Stretch::preset_default(2, SR);
    let il = dsp.input_latency();
    let ol = dsp.output_latency();
    let mut input: Vec<f32> = (0..il).flat_map(|i| b_frame(i as f64)).collect();
    dsp.seek(&input, speed);
    let mut fed = il as f64;
    let prime = (ol as f64 * speed).round() as usize;
    input = (0..prime).flat_map(|i| b_frame(fed + i as f64)).collect();
    let mut scratch = vec![0f32; ol * 2];
    dsp.process(&input, &mut scratch);
    fed += prime as f64;

    // A's effects.
    let mut a_hp = Biquad::new();
    let mut echo = vec![[0f32; 2]; ((pa * 0.75) as usize).max(1)];
    let mut echo_at = 0;
    let mut echo_hp = Biquad::new();
    let tail = (pa * 8.0) as usize; // the echo rings on past the blend

    let after = 15 * SR as usize;
    let recover = 8 * SR as usize;
    let block = 256;
    let mut produced = 0usize;
    let mut frac = 0.0;
    while produced < len + after {
        let s = if produced < len {
            speed
        } else {
            speed + (1.0 - speed) * ((produced - len) as f64 / recover as f64).min(1.0)
        };
        let exact = block as f64 * s + frac;
        let n = exact.floor() as usize;
        frac = exact - n as f64;
        input = (0..n).flat_map(|i| b_frame(fed + i as f64)).collect();
        fed += n as f64;
        let mut ob = vec![0f32; block * 2];
        dsp.process(&input, &mut ob);
        for (j, bf) in ob.chunks_exact(2).enumerate() {
            let p = produced + j;
            let mut f = [bf[0], bf[1]];
            if p < len + tail {
                let u = p as f64 / len as f64;
                let g = if p < len { gains_a(u) } else { [0.0; 4] };
                let at = |s: usize| {
                    if p < len {
                        a.stems[s][start_i + p]
                    } else {
                        [0.0; 2]
                    }
                };
                // Instruments (drums + other), high-passed away in the second half.
                let mut inst = [0.0; 2];
                for s in [0, 2] {
                    inst[0] += at(s)[0] * g[s];
                    inst[1] += at(s)[1] * g[s];
                }
                if o.filters && (0.5..1.0).contains(&u) {
                    if p.is_multiple_of(32) {
                        a_hp.set(
                            true,
                            30.0 * (1500.0f32 / 30.0).powf(((u - 0.5) / 0.5) as f32),
                        );
                    }
                    inst = a_hp.run(inst);
                }
                let (bass, voc) = (at(1), at(3));
                f[0] += inst[0] + bass[0] * g[1] + voc[0] * g[3];
                f[1] += inst[1] + bass[1] * g[1] + voc[1] * g[3];
                if o.vocal_echo {
                    // The vocal's last beat (until it is out) feeds a ¾-beat echo.
                    let send = if (vocal_out - one..vocal_out + 0.25 * one).contains(&u) {
                        1.0
                    } else {
                        0.0
                    };
                    let wet = echo[echo_at];
                    if u >= vocal_out && p.is_multiple_of(32) {
                        let span = 1.0 - vocal_out + tail as f64 / len as f64;
                        let t = ((u - vocal_out) / span).min(1.0);
                        echo_hp.set(true, 200.0 * 10f32.powf(t as f32));
                    }
                    let wet_f = echo_hp.run(wet);
                    for ch in 0..2 {
                        echo[echo_at][ch] = voc[ch] * send + wet[ch] * 0.55;
                        f[ch] += wet_f[ch] * 0.9;
                    }
                    echo_at = (echo_at + 1) % echo.len();
                }
            }
            out.push(f);
        }
        produced += block;
    }
    let _ = (a.bpm, b.bpm);
    out
}

/// What Glowtune did before: a 6 s equal-power crossfade, no beat matching.
fn old_crossfade(a: &[Frame], b: &[Frame]) -> Vec<Frame> {
    let len = 6 * SR as usize;
    let lead = 15 * SR as usize;
    let a_tail = &a[a.len() - len - lead..];
    let mut out: Vec<Frame> = a_tail[..lead].to_vec();
    for i in 0..len {
        let x = (i as f32 + 0.5) / len as f32;
        let (ga, gb) = (
            (x * std::f32::consts::FRAC_PI_2).cos(),
            (x * std::f32::consts::FRAC_PI_2).sin(),
        );
        let (fa, fb) = (a_tail[lead + i], b[i]);
        out.push([fa[0] * ga + fb[0] * gb, fa[1] * ga + fb[1] * gb]);
    }
    out.extend_from_slice(&b[len..len + 15 * SR as usize]);
    out
}
