use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

/// Metadata of the current track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub album_artist: String,
    /// Track length, if the player reports it.
    pub duration: Option<Duration>,
    /// Which app is playing: an AppUserModelId on Windows (e.g. `Spotify.exe`),
    /// a bus name on Linux (e.g. `org.mpris.MediaPlayer2.spotify`).
    pub source_app: String,
}

/// Album art as reported by the OS.
#[derive(Clone, PartialEq, Eq)]
pub enum AlbumArt {
    /// Encoded image bytes (usually JPEG or PNG). Windows SMTC gives us these.
    Bytes { data: Arc<[u8]>, mime: String },
    /// A URL (`file://` or `https://`). MPRIS gives us these; fetching happens later.
    Url(String),
}

impl fmt::Debug for AlbumArt {
    // Don't dump image bytes into logs.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes { data, mime } => write!(f, "Bytes({} bytes, {mime})", data.len()),
            Self::Url(url) => write!(f, "Url({url})"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
}

/// Repeat behavior of the current player.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RepeatMode {
    Off,
    Track,
    List,
}

/// Shuffle and repeat state of the current player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PlayModes {
    /// None = this player doesn't support / report it (hide the button).
    pub shuffle: Option<bool>,
    /// None = this player doesn't support / report it (hide the button).
    pub repeat: Option<RepeatMode>,
}

/// Deduplicates modes; callers reset `last` with the snapshot on a session change.
pub(crate) fn modes_changed(last: &mut Option<PlayModes>, modes: PlayModes) -> Option<MediaEvent> {
    if *last == Some(modes) {
        return None;
    }
    *last = Some(modes);
    Some(MediaEvent::ModesChanged(modes))
}

pub(crate) fn clamp_volume(volume: f32) -> Option<f32> {
    (!volume.is_nan()).then(|| volume.clamp(0.0, 1.0))
}

/// A position report from the player.
///
/// Reports are sparse, so consumers interpolate (PLAN.md section 8):
///
/// ```text
/// position_now = position + (Instant::now() - reported_at) * rate   // while Playing
/// position_now = position                                           // otherwise
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Timeline {
    /// Position at the moment `reported_at`.
    pub position: Duration,
    pub duration: Option<Duration>,
    /// Playback speed (1.0 = normal).
    pub rate: f64,
    /// When `position` was valid. Taken from the player's own timestamp when
    /// available (SMTC `LastUpdatedTime`), so stale reports are still accurate.
    pub reported_at: Instant,
}

impl Timeline {
    /// Estimated position right now, **assuming playback is running**.
    /// While paused, use `position` as-is.
    pub fn position_now(&self) -> Duration {
        let elapsed = self.reported_at.elapsed().as_secs_f64() * sanitize_rate(self.rate);
        let advanced = Duration::try_from_secs_f64(elapsed).unwrap_or(Duration::MAX);
        let pos = self.position.checked_add(advanced).unwrap_or(Duration::MAX);
        match self.duration {
            Some(d) => pos.min(d),
            None => pos,
        }
    }

    /// True if `next` only confirms what interpolating `self` already predicts
    /// (same duration and rate, position within `tolerance`). Players like
    /// Spotify re-send the same position every few seconds; those are skipped.
    pub(crate) fn predicts(&self, next: &Timeline, playing: bool, tolerance: Duration) -> bool {
        let predicted = if playing {
            self.position_now()
        } else {
            self.position
        };
        self.duration == next.duration
            && self.rate == next.rate
            && predicted.abs_diff(next.position) < tolerance
    }
}

/// Players can report nonsense rates (NaN, infinity, negative). Treat as 1.0.
pub(crate) fn sanitize_rate(rate: f64) -> f64 {
    if rate.is_finite() && rate >= 0.0 {
        rate
    } else {
        1.0
    }
}

/// Position reports closer than this to our prediction are not re-sent.
pub(crate) const TIMELINE_TOLERANCE: Duration = Duration::from_millis(250);

/// A transport command for the current player.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Control {
    PlayPause,
    Play,
    Pause,
    Next,
    Previous,
    /// Jump to this position in the current track.
    SeekTo(Duration),
    SetShuffle(bool),
    SetRepeat(RepeatMode),
    /// Set the player's volume, clamped to 0.0..=1.0. NaN is ignored.
    SetVolume(f32),
}

/// Something changed in the OS "now playing" state.
#[derive(Debug, Clone)]
pub enum MediaEvent {
    /// The active player changed. `None` = nothing is playing anywhere.
    ///
    /// Always followed by a full snapshot of the new player: `TrackChanged`,
    /// `ArtChanged`, `PlaybackChanged`, `TimelineChanged` (whatever it reports),
    /// and `ModesChanged` (unsupported modes are `None`),
    /// so consumers can safely reset their state on this event.
    /// For `None`, the snapshot is `PlaybackChanged(Stopped)` and
    /// `ModesChanged(PlayModes::default())`.
    SessionChanged(Option<String>),
    /// A different track started, or the current track's metadata was corrected
    /// (e.g. duration arrived later). Identical metadata never fires twice.
    TrackChanged(TrackInfo),
    /// Album art for the current track arrived or changed (`None` = no art).
    ArtChanged(Option<AlbumArt>),
    PlaybackChanged(PlaybackStatus),
    TimelineChanged(Timeline),
    /// Shuffle or repeat changed. Identical modes are skipped until a new session.
    ModesChanged(PlayModes),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volumes_clamp_and_nan_is_ignored() {
        for (input, expected) in [
            (-1.0, 0.0),
            (0.0, 0.0),
            (0.5, 0.5),
            (1.0, 1.0),
            (2.0, 1.0),
            (f32::NEG_INFINITY, 0.0),
            (f32::INFINITY, 1.0),
        ] {
            assert_eq!(clamp_volume(input), Some(expected));
        }
        assert_eq!(clamp_volume(f32::NAN), None);
    }

    #[test]
    fn modes_dedup_changes_and_session_reset() {
        let mut last = None;
        let unknown = PlayModes::default();
        let shuffle = PlayModes {
            shuffle: Some(true),
            ..unknown
        };
        let repeat = PlayModes {
            repeat: Some(RepeatMode::Track),
            ..shuffle
        };
        for modes in [unknown, shuffle, repeat, unknown] {
            assert!(
                matches!(modes_changed(&mut last, modes), Some(MediaEvent::ModesChanged(m)) if m == modes)
            );
            assert!(modes_changed(&mut last, modes).is_none());
        }
        last = None;
        assert!(modes_changed(&mut last, unknown).is_some());
    }

    fn tl(pos_ms: u64, age_ms: u64, rate: f64) -> Timeline {
        Timeline {
            position: Duration::from_millis(pos_ms),
            duration: Some(Duration::from_secs(180)),
            rate,
            reported_at: Instant::now() - Duration::from_millis(age_ms),
        }
    }

    #[test]
    fn position_now_interpolates_and_clamps() {
        let p = tl(10_000, 2_000, 1.0).position_now();
        assert!(
            p >= Duration::from_millis(11_990) && p < Duration::from_millis(12_100),
            "{p:?}"
        );
        assert_eq!(
            tl(179_000, 5_000, 1.0).position_now(),
            Duration::from_secs(180)
        );
        // Nonsense rates must not panic.
        let _ = tl(0, 1_000, f64::INFINITY).position_now();
        let _ = tl(0, 1_000, f64::NAN).position_now();
    }

    #[test]
    fn repeated_paused_report_is_redundant() {
        // Spotify paused: same position re-sent 4.5 s later.
        let last = tl(16_000, 4_500, 1.0);
        let next = tl(16_000, 0, 1.0);
        assert!(last.predicts(&next, false, TIMELINE_TOLERANCE));
        // ...but while playing, an unchanged position means a jump (e.g. a stall).
        assert!(!last.predicts(&next, true, TIMELINE_TOLERANCE));
    }

    #[test]
    fn playing_report_on_schedule_is_redundant_but_seek_is_not() {
        let last = tl(20_000, 4_500, 1.0);
        assert!(last.predicts(&tl(24_500, 0, 1.0), true, TIMELINE_TOLERANCE));
        assert!(!last.predicts(&tl(60_000, 0, 1.0), true, TIMELINE_TOLERANCE));
        assert!(!last.predicts(&tl(24_500, 0, 0.0), true, TIMELINE_TOLERANCE));
    }

    #[test]
    fn rates_are_sanitized() {
        assert_eq!(sanitize_rate(1.5), 1.5);
        assert_eq!(sanitize_rate(0.0), 0.0);
        assert_eq!(sanitize_rate(-1.0), 1.0);
        assert_eq!(sanitize_rate(f64::NAN), 1.0);
        assert_eq!(sanitize_rate(f64::INFINITY), 1.0);
    }
}
