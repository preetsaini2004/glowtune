//! Listening stats: your own small "Wrapped".
//!
//! ```text
//!  hub: new song ─► song_started ─┐   (the song before is written down if it
//!  hub: play/pause ─► playing ────┤    played 30 s or more, counted by the
//!  app quits ─► flush ────────────┘    time it was actually playing)
//!                                   ▼
//!        listening.jsonl (one line per play, in the app data folder)
//!                                   ▼
//!  listening_stats(range) ─► total time, plays, top songs, top artists,
//!                            time per day/week, the hour you listen most
//! ```
//! Nothing leaves the PC. Ads are not counted. Off = nothing is written.

use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// A song counts as played after this much listening.
const MIN_PLAY: Duration = Duration::from_secs(30);
const TOP: usize = 8;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Play {
    /// When it started (seconds since 1970).
    pub t: u64,
    /// How long it actually played.
    pub ms: u64,
    pub title: String,
    pub artist: String,
    #[serde(default)]
    pub source: String,
}

struct Current {
    title: String,
    artist: String,
    source: String,
    started: u64,
    played: Duration,
    since: Option<Instant>,
}

impl Current {
    fn played_now(&self) -> Duration {
        self.played + self.since.map_or(Duration::ZERO, |s| s.elapsed())
    }
    fn as_play(&self) -> Play {
        Play {
            t: self.started,
            ms: self.played_now().as_millis() as u64,
            title: self.title.clone(),
            artist: self.artist.clone(),
            source: self.source.clone(),
        }
    }
}

pub struct Listening {
    path: PathBuf,
    current: Mutex<Option<Current>>,
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Ads and other non-songs aren't plays.
fn is_ad(title: &str, artist: &str) -> bool {
    let t = title.trim().to_lowercase();
    let a = artist.trim().to_lowercase();
    t.is_empty() || t == "advertisement" || a == "advertisement" || a == "spotify"
}

fn enabled(app: &AppHandle) -> bool {
    crate::current_settings(app).listening_history
}

impl Listening {
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            current: Mutex::new(None),
        }
    }

    fn write(&self, play: &Play) {
        if let Some(dir) = self.path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let line = match serde_json::to_string(play) {
            Ok(l) => l,
            Err(_) => return,
        };
        let result = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)
            .and_then(|mut f| writeln!(f, "{line}"));
        if let Err(e) = result {
            tracing::warn!("listening history: {e}");
        }
    }

    /// Writes down the song so far (if it played long enough) and forgets it.
    fn finish(&self, cur: Option<Current>) {
        if let Some(c) = cur
            && c.played_now() >= MIN_PLAY
        {
            self.write(&c.as_play());
        }
    }

    fn song_started(&self, title: &str, artist: &str, source: &str, playing: bool, keep: bool) {
        let Ok(mut slot) = self.current.lock() else {
            return;
        };
        let old = slot.take();
        self.finish(old);
        if keep && !is_ad(title, artist) {
            *slot = Some(Current {
                title: title.to_string(),
                artist: artist.to_string(),
                source: source.to_string(),
                started: now_secs(),
                played: Duration::ZERO,
                since: playing.then(Instant::now),
            });
        }
    }

    fn playing(&self, on: bool) {
        let Ok(mut slot) = self.current.lock() else {
            return;
        };
        if let Some(c) = slot.as_mut() {
            match (on, c.since) {
                (true, None) => c.since = Some(Instant::now()),
                (false, Some(s)) => {
                    c.played += s.elapsed();
                    c.since = None;
                }
                _ => {}
            }
        }
    }

    pub fn flush(&self) {
        if let Ok(mut slot) = self.current.lock() {
            let old = slot.take();
            self.finish(old);
        }
    }

    fn read(&self) -> Vec<Play> {
        let mut plays: Vec<Play> = std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        // The song playing now counts once it has played long enough.
        if let Ok(slot) = self.current.lock()
            && let Some(c) = slot.as_ref()
            && c.played_now() >= MIN_PLAY
        {
            plays.push(c.as_play());
        }
        plays
    }
}

/// A new song started (hub).
pub fn song_started(app: &AppHandle, title: &str, artist: &str, source: &str, playing: bool) {
    if let Some(l) = app.try_state::<Listening>() {
        l.song_started(title, artist, source, playing, enabled(app));
    }
}

/// Play / pause (hub).
pub fn playing(app: &AppHandle, on: bool) {
    if let Some(l) = app.try_state::<Listening>() {
        l.playing(on);
    }
}

// ── Adding it up ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Top {
    pub title: String,
    pub artist: String,
    pub plays: u32,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Bar {
    /// Start of the day (or week), seconds since 1970, local midnight.
    pub start: u64,
    pub ms: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct StatsView {
    pub total_ms: u64,
    pub plays: u32,
    pub songs: Vec<Top>,
    pub artists: Vec<Top>,
    /// Days for a week or month, weeks for a year or all time.
    pub bars: Vec<Bar>,
    pub bar_days: u64,
    /// 0..23, local.
    pub busiest_hour: Option<u8>,
    /// The first play ever (seconds since 1970).
    pub first: Option<u64>,
}

/// "PARTYNEXTDOOR, Drake" / "A & B" / "A feat. B" → each artist.
fn artists(credit: &str) -> Vec<String> {
    let mut s = credit.to_string();
    for sep in [
        " feat. ",
        " ft. ",
        " featuring ",
        " Feat. ",
        " Ft. ",
        " & ",
        " x ",
        "; ",
        " / ",
    ] {
        s = s.replace(sep, ", ");
    }
    s.split(", ")
        .map(|a| a.trim().to_string())
        .filter(|a| !a.is_empty())
        .collect()
}

/// Range in days: week 7, month 30, year 365, all = everything.
pub fn aggregate(
    all: &[Play],
    now: u64,
    range_days: Option<u64>,
    tz_offset_secs: i64,
) -> StatsView {
    let from = range_days.map_or(0, |d| now.saturating_sub(d * 86_400));
    let plays: Vec<&Play> = all.iter().filter(|p| p.t >= from && p.t <= now).collect();
    let local = |t: u64| (t as i64 + tz_offset_secs).max(0) as u64;

    let mut songs: HashMap<(String, String), Top> = HashMap::new();
    let mut by_artist: HashMap<String, Top> = HashMap::new();
    let mut hours = [0u64; 24];
    let mut total = 0;
    for p in &plays {
        total += p.ms;
        hours[((local(p.t) % 86_400) / 3600) as usize] += p.ms;
        let key = (p.title.to_lowercase(), p.artist.to_lowercase());
        let s = songs.entry(key).or_insert_with(|| Top {
            title: p.title.clone(),
            artist: p.artist.clone(),
            plays: 0,
            ms: 0,
        });
        s.plays += 1;
        s.ms += p.ms;
        for a in artists(&p.artist) {
            let e = by_artist.entry(a.to_lowercase()).or_insert_with(|| Top {
                title: a.clone(),
                artist: a.clone(),
                plays: 0,
                ms: 0,
            });
            e.plays += 1;
            e.ms += p.ms;
        }
    }
    let rank = |mut v: Vec<Top>| {
        v.sort_by(|a, b| {
            b.plays
                .cmp(&a.plays)
                .then(b.ms.cmp(&a.ms))
                .then(a.title.cmp(&b.title))
        });
        v.truncate(TOP);
        v
    };

    // Bars: days for up to a month, weeks beyond that (at most two years).
    let first = all.iter().map(|p| p.t).min();
    let span_days = match range_days {
        Some(d) => d,
        None => first.map_or(7, |f| (now.saturating_sub(f) / 86_400 + 1).clamp(7, 728)),
    };
    let bar_days = if span_days <= 31 { 1 } else { 7 };
    let count = span_days.div_ceil(bar_days).max(1);
    // Local midnight today, as seconds since 1970 in UTC.
    let today = (local(now) / 86_400) * 86_400;
    let last_start = (today as i64 - tz_offset_secs).max(0) as u64;
    let first_start = last_start.saturating_sub((count - 1) * bar_days * 86_400);
    let mut bars: Vec<Bar> = (0..count)
        .map(|i| Bar {
            start: first_start + i * bar_days * 86_400,
            ms: 0,
        })
        .collect();
    for p in &plays {
        if p.t >= first_start {
            let i = ((p.t - first_start) / (bar_days * 86_400)) as usize;
            if let Some(b) = bars.get_mut(i) {
                b.ms += p.ms;
            }
        }
    }

    let busiest_hour =
        (total > 0).then(|| (0..24u8).max_by_key(|h| hours[*h as usize]).unwrap_or(0));
    StatsView {
        total_ms: total,
        plays: plays.len() as u32,
        songs: rank(songs.into_values().collect()),
        artists: rank(by_artist.into_values().collect()),
        bars,
        bar_days,
        busiest_hour,
        first,
    }
}

#[tauri::command]
pub fn listening_stats(
    range: String,
    tz_offset_min: i64,
    listening: tauri::State<'_, Listening>,
) -> StatsView {
    let days = match range.as_str() {
        "week" => Some(7),
        "month" => Some(30),
        "year" => Some(365),
        _ => None,
    };
    // JS getTimezoneOffset() is minutes *behind* UTC (India: -330).
    aggregate(
        &listening.read(),
        now_secs(),
        days,
        -tz_offset_min.clamp(-900, 900) * 60,
    )
}

#[tauri::command]
pub fn clear_listening(listening: tauri::State<'_, Listening>) -> Result<(), String> {
    if let Ok(mut slot) = listening.current.lock() {
        *slot = None;
    }
    match std::fs::remove_file(&listening.path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(format!("Couldn't clear the history: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn play(t: u64, ms: u64, title: &str, artist: &str) -> Play {
        Play {
            t,
            ms,
            title: title.into(),
            artist: artist.into(),
            source: "Spotify.exe".into(),
        }
    }

    const DAY: u64 = 86_400;
    // 2026-11-14 12:00 UTC.
    const NOW: u64 = 1_794_657_600;

    #[test]
    fn tops_totals_and_featured_artists() {
        let plays = vec![
            play(NOW - 100, 200_000, "Janice STFU", "Drake"),
            play(NOW - 500, 180_000, "janice stfu", "drake"),
            play(NOW - 900, 240_000, "Closer", "PARTYNEXTDOOR, Drake"),
            play(NOW - 40 * DAY, 240_000, "Old", "Someone"),
        ];
        let s = aggregate(&plays, NOW, Some(7), 0);
        assert_eq!(s.plays, 3);
        assert_eq!(s.total_ms, 620_000);
        assert_eq!(s.songs[0].title, "Janice STFU");
        assert_eq!(s.songs[0].plays, 2, "same song in any case");
        assert_eq!(s.artists[0].title, "Drake");
        assert_eq!(s.artists[0].plays, 3, "credited on the feature too");
        assert!(s.artists.iter().any(|a| a.title == "PARTYNEXTDOOR"));
        assert_eq!(s.bars.len(), 7);
        assert_eq!(s.bar_days, 1);
        assert_eq!(s.bars.last().unwrap().ms, 620_000, "all today");
        let all = aggregate(&plays, NOW, None, 0);
        assert_eq!(all.plays, 4);
        assert_eq!(all.bar_days, 7);
    }

    #[test]
    fn local_time_decides_the_hour_and_the_day() {
        // 18:00 UTC is 23:30 in India (+5:30).
        let t = NOW + 6 * 3600;
        let s = aggregate(&[play(t, 60_000, "A", "B")], t + 60, Some(7), 19_800);
        assert_eq!(s.busiest_hour, Some(23));
        // Bars start at a local midnight.
        assert_eq!((s.bars[0].start as i64 + 19_800) % DAY as i64, 0);
    }

    #[test]
    fn ads_and_short_plays_are_not_counted() {
        let dir = std::env::temp_dir().join(format!("glowtune-stats-{}", std::process::id()));
        let l = Listening::new(dir.join("listening.jsonl"));
        l.song_started("Advertisement", "", "Spotify.exe", true, true);
        l.song_started("Short", "Someone", "Spotify.exe", true, true);
        l.song_started("Long", "Someone", "Spotify.exe", false, true);
        if let Some(c) = l.current.lock().unwrap().as_mut() {
            c.played = Duration::from_secs(95);
        }
        l.flush();
        let plays = l.read();
        assert_eq!(plays.len(), 1);
        assert_eq!(plays[0].title, "Long");
        assert_eq!(plays[0].ms, 95_000);
        let _ = std::fs::remove_dir_all(dir);
    }
}
