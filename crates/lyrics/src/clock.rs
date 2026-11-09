use std::time::{Duration, Instant};

/// An allocation-free playback clock driven by player reports and play/pause events.
#[derive(Debug)]
pub struct SyncClock {
    position: Duration,
    reported_at: Instant,
    rate: f64,
    playing: bool,
    offset_ms: i64,
}

impl SyncClock {
    /// Creates a paused clock at zero with normal playback rate and no offset.
    pub fn new() -> Self {
        Self {
            position: Duration::ZERO,
            reported_at: Instant::now(),
            rate: 1.0,
            playing: false,
            offset_ms: 0,
        }
    }

    /// Replaces the timeline anchor, including on backward seeks.
    /// Non-finite and negative rates are treated as normal speed; zero is valid.
    pub fn update(&mut self, position: Duration, reported_at: Instant, rate: f64) {
        self.position = position;
        self.reported_at = reported_at;
        self.rate = if rate.is_finite() && rate >= 0.0 {
            rate
        } else {
            1.0
        };
    }

    /// Freezes the interpolated position when pausing and re-anchors it when resuming.
    pub fn set_playing(&mut self, playing: bool) {
        self.set_playing_at(playing, Instant::now());
    }

    fn set_playing_at(&mut self, playing: bool, now: Instant) {
        if self.playing != playing {
            self.position = self.base_position_at(now);
            self.reported_at = now;
            self.playing = playing;
        }
    }

    /// Sets a user offset in milliseconds; positive values display lyrics earlier.
    pub fn set_offset_ms(&mut self, ms: i64) {
        self.offset_ms = ms;
    }

    /// Returns the current interpolated position with the user offset applied.
    pub fn position(&self) -> Duration {
        self.position_at(Instant::now())
    }

    /// Returns the offset-adjusted position at a given instant, clamped to the duration range.
    /// Instants before the report do not extrapolate backwards.
    pub fn position_at(&self, now: Instant) -> Duration {
        let position = self.base_position_at(now);
        let offset = Duration::from_millis(self.offset_ms.unsigned_abs());
        if self.offset_ms >= 0 {
            position.saturating_add(offset)
        } else {
            position.saturating_sub(offset)
        }
    }

    fn base_position_at(&self, now: Instant) -> Duration {
        if !self.playing {
            return self.position;
        }
        let elapsed = now
            .saturating_duration_since(self.reported_at)
            .as_secs_f64()
            * self.rate;
        let elapsed = Duration::try_from_secs_f64(elapsed).unwrap_or(Duration::MAX);
        self.position.saturating_add(elapsed)
    }
}

impl Default for SyncClock {
    /// Creates a paused clock at zero.
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interpolates_and_seeks_backwards() {
        let now = Instant::now();
        let mut clock = SyncClock::new();
        clock.set_playing_at(true, now);
        clock.update(Duration::from_secs(10), now, 1.5);
        assert_eq!(
            clock.position_at(now + Duration::from_secs(4)),
            Duration::from_secs(16)
        );
        clock.update(Duration::from_secs(2), now + Duration::from_secs(4), 1.0);
        assert_eq!(
            clock.position_at(now + Duration::from_secs(5)),
            Duration::from_secs(3)
        );
        assert_eq!(clock.position_at(now), Duration::from_secs(2));
    }

    #[test]
    fn pause_freezes_and_resume_reanchors_without_double_offset() {
        let now = Instant::now();
        let mut clock = SyncClock::new();
        clock.set_playing_at(true, now);
        clock.update(Duration::from_secs(10), now, 1.0);
        clock.set_offset_ms(500);
        clock.set_playing_at(false, now + Duration::from_secs(2));
        assert_eq!(
            clock.position_at(now + Duration::from_secs(20)),
            Duration::from_millis(12500)
        );
        clock.set_playing_at(false, now + Duration::from_secs(5));
        clock.set_playing_at(true, now + Duration::from_secs(20));
        clock.set_playing_at(true, now + Duration::from_secs(21));
        assert_eq!(
            clock.position_at(now + Duration::from_secs(23)),
            Duration::from_millis(15500)
        );
    }

    #[test]
    fn offsets_and_saturation() {
        let now = Instant::now();
        let mut clock = SyncClock::default();
        clock.update(Duration::from_secs(1), now, 1.0);
        clock.set_offset_ms(250);
        assert_eq!(clock.position_at(now), Duration::from_millis(1250));
        clock.set_offset_ms(-2000);
        assert_eq!(clock.position_at(now), Duration::ZERO);
        clock.set_offset_ms(i64::MIN);
        assert_eq!(clock.position_at(now), Duration::ZERO);
        clock.set_offset_ms(0);
        clock.set_playing_at(true, now);
        clock.update(Duration::MAX, now, f64::MAX);
        assert_eq!(
            clock.position_at(now + Duration::from_secs(2)),
            Duration::MAX
        );
    }

    #[test]
    fn invalid_rates_and_zero_rate() {
        let now = Instant::now();
        for rate in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, -1.0, 0.0] {
            let mut clock = SyncClock::new();
            clock.set_playing_at(true, now);
            clock.update(Duration::ZERO, now, rate);
            assert_eq!(
                clock.position_at(now + Duration::from_secs(2)),
                Duration::from_secs(if rate == 0.0 { 0 } else { 2 })
            );
        }
    }

    #[test]
    fn public_paused_clock_is_stable() {
        let mut clock = SyncClock::new();
        clock.set_playing(true);
        clock.set_playing(false);
        assert_eq!(
            clock.position(),
            clock.position_at(Instant::now() + Duration::from_secs(100))
        );
    }
}
