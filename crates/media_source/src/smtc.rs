//! Windows backend: System Media Transport Controls (SMTC).
//!
//! ```text
//!  WinRT thread pool                    our worker thread ("media_source-smtc")
//!  ─────────────────                    ─────────────────────────────────────────
//!  CurrentSessionChanged ──┐
//!  MediaPropertiesChanged ─┼─ Cmd ──►  recv() (blocks: 0% CPU while idle)
//!  PlaybackInfoChanged ────┤            ├─ coalesce a burst of Cmds into one pass
//!  TimelinePropertiesChanged┘            ├─ query SMTC, compare with last state
//!                                       └─ send MediaEvent only if it changed
//! ```
//!
//! Event handlers do no work: they only send a tiny `Cmd`. All WinRT queries
//! happen on one worker thread, so there are no locks and no races.
//! Each session subscription gets a `generation` number; events queued by an
//! old session carry an old number and are ignored after a switch.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    CurrentSessionChangedEventArgs, GlobalSystemMediaTransportControlsSession as Session,
    GlobalSystemMediaTransportControlsSessionManager as Manager,
    GlobalSystemMediaTransportControlsSessionMediaProperties as MediaProperties,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus as WinStatus,
    MediaPropertiesChangedEventArgs, PlaybackInfoChangedEventArgs,
    TimelinePropertiesChangedEventArgs,
};
use windows::Media::MediaPlaybackAutoRepeatMode;
use windows::Storage::Streams::{Buffer, DataReader, InputStreamOptions};
use windows::Win32::System::WinRT::{RO_INIT_MULTITHREADED, RoInitialize, RoUninitialize};

use crate::types::{TIMELINE_TOLERANCE, modes_changed, sanitize_rate};
use crate::{
    AlbumArt, Control, Error, MediaEvent, PlayModes, PlaybackStatus, RepeatMode, Result, Timeline,
    TrackInfo,
};

/// Ignore album art bigger than this (SMTC thumbnails are normally < 1 MB).
const MAX_ART_BYTES: u64 = 16 * 1024 * 1024;

/// WinRT `DateTime` counts 100 ns ticks since 1601-01-01. This is 1970-01-01 in those ticks.
const UNIX_EPOCH_TICKS: i64 = 116_444_736_000_000_000;

enum Cmd {
    SessionChanged,
    Media(u64),
    Playback(u64),
    Timeline(u64),
    Control(Control),
    Volume(Sender<Option<f32>>),
    Stop,
}

pub(crate) struct Watcher {
    cmd: Sender<Cmd>,
    thread: Option<JoinHandle<()>>,
}

impl Watcher {
    pub(crate) fn start() -> Result<(Self, Receiver<MediaEvent>)> {
        let (event_tx, event_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (ready_tx, ready_rx) = mpsc::sync_channel::<Result<()>>(1);

        let worker_cmd = cmd_tx.clone();
        let thread = std::thread::Builder::new()
            .name("media_source-smtc".into())
            .spawn(move || {
                // WinRT needs the thread initialized. MTA = WinRT callbacks may run on
                // any thread-pool thread, which is fine because handlers only send a Cmd.
                let _apartment = match Apartment::init() {
                    Ok(guard) => guard,
                    Err(e) => {
                        let _ = ready_tx.send(Err(e.into()));
                        return;
                    }
                };
                // `worker` is dropped before `_apartment` (reverse declaration order),
                // so all WinRT objects are released before RoUninitialize.
                match Worker::init(worker_cmd, event_tx) {
                    Ok(mut worker) => {
                        let _ = ready_tx.send(Ok(()));
                        worker.run(cmd_rx);
                    }
                    Err(e) => {
                        let _ = ready_tx.send(Err(e));
                    }
                }
            })
            .map_err(|e| Error::Thread(e.to_string()))?;

        match ready_rx.recv() {
            Ok(Ok(())) => Ok((
                Self {
                    cmd: cmd_tx,
                    thread: Some(thread),
                },
                event_rx,
            )),
            Ok(Err(e)) => {
                let _ = thread.join();
                Err(e)
            }
            Err(_) => Err(Error::Thread("SMTC worker exited during startup".into())),
        }
    }
}

impl Watcher {
    pub(crate) fn control(&self, control: Control) {
        let _ = self.cmd.send(Cmd::Control(control));
    }

    pub(crate) fn volume(&self) -> Option<f32> {
        let (tx, rx) = mpsc::channel();
        self.cmd.send(Cmd::Volume(tx)).ok()?;
        rx.recv_timeout(Duration::from_millis(250)).ok().flatten()
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Don't join: if a WinRT call is stuck (e.g. a player that never answers
        // a thumbnail request), joining would hang the app on exit. The worker
        // unsubscribes and exits by itself as soon as it reads `Stop`.
        let _ = self.cmd.send(Cmd::Stop);
        drop(self.thread.take());
    }
}

/// Balances `RoInitialize` with `RoUninitialize` when the worker thread ends.
struct Apartment;

impl Apartment {
    fn init() -> windows::core::Result<Self> {
        // SAFETY: This worker initializes its own COM apartment, balanced by Drop.
        unsafe { RoInitialize(RO_INIT_MULTITHREADED)? };
        Ok(Self)
    }
}

impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: Apartment is created and dropped on the same initialized worker thread.
        unsafe { RoUninitialize() };
    }
}

/// The session we are currently subscribed to, plus its event tokens.
struct Subscription {
    session: Session,
    app_id: String,
    tokens: Vec<(EventKind, i64)>,
}

#[derive(Clone, Copy)]
enum EventKind {
    Media,
    Playback,
    Timeline,
}

/// Which refreshes a burst of commands asked for.
#[derive(Default)]
struct Pending {
    stop: bool,
    controls: Vec<Control>,
    volumes: Vec<Sender<Option<f32>>>,
    session: bool,
    media: bool,
    playback: bool,
    timeline: bool,
}

struct Worker {
    manager: Manager,
    manager_token: i64,
    cmd: Sender<Cmd>,
    events: Sender<MediaEvent>,
    /// Set when the event receiver is gone; the worker then shuts down.
    closed: bool,

    sub: Option<Subscription>,
    generation: u64,
    rate: f64,

    // Last emitted values, for dedup. Outer `None` = nothing emitted yet.
    last_session: Option<Option<String>>,
    last_track: Option<TrackInfo>,
    last_art: Option<Option<u64>>,
    last_status: Option<PlaybackStatus>,
    last_timeline: Option<Timeline>,
    last_modes: Option<PlayModes>,
}

impl Worker {
    fn init(cmd: Sender<Cmd>, events: Sender<MediaEvent>) -> Result<Self> {
        let manager = Manager::RequestAsync()?.join()?;
        let tx = cmd.clone();
        let manager_token = manager.CurrentSessionChanged(&TypedEventHandler::<
            Manager,
            CurrentSessionChangedEventArgs,
        >::new(move |_, _| {
            let _ = tx.send(Cmd::SessionChanged);
            Ok(())
        }))?;

        let mut worker = Self {
            manager,
            manager_token,
            cmd,
            events,
            closed: false,
            sub: None,
            generation: 0,
            rate: 1.0,
            last_session: None,
            last_track: None,
            last_art: None,
            last_status: None,
            last_timeline: None,
            last_modes: None,
        };
        // The initial snapshot is best effort: a session that is closing right now
        // must not fail startup (and the worker's Drop cleans up subscriptions).
        if let Err(e) = worker.on_session_changed() {
            tracing::debug!("initial SMTC snapshot failed: {e}");
        }
        Ok(worker)
    }

    fn run(&mut self, rx: Receiver<Cmd>) {
        while let Ok(first) = rx.recv() {
            // Coalesce: players often fire several events at once (e.g. a track
            // change = media + playback + timeline). Handle them in one pass.
            let mut pending = Pending::default();
            self.add(&mut pending, first);
            while let Ok(cmd) = rx.try_recv() {
                self.add(&mut pending, cmd);
            }
            if pending.stop {
                break;
            }
            for control in pending.controls.drain(..) {
                log_err("control", self.apply_control(control));
            }

            if pending.session {
                log_err("session", self.on_session_changed());
            } else {
                // Each refresh runs independently: a failed metadata query must not
                // swallow a pause that arrived in the same burst.
                if pending.media {
                    log_err("media", self.refresh_media());
                }
                if pending.playback {
                    log_err("playback", self.refresh_playback());
                }
                if pending.timeline {
                    log_err("timeline", self.refresh_timeline());
                }
            }
            if !pending.volumes.is_empty() {
                let volume = self
                    .sub
                    .as_ref()
                    .and_then(|sub| crate::volume_win::get(&sub.app_id));
                for reply in pending.volumes {
                    let _ = reply.send(volume);
                }
            }
            if self.closed {
                break;
            }
        }
    }

    fn add(&self, pending: &mut Pending, cmd: Cmd) {
        let current = |g: u64| g == self.generation;
        match cmd {
            Cmd::Stop => pending.stop = true,
            Cmd::Control(c) => pending.controls.push(c),
            Cmd::Volume(reply) => pending.volumes.push(reply),
            Cmd::SessionChanged => pending.session = true,
            Cmd::Media(g) if current(g) => pending.media = true,
            Cmd::Playback(g) if current(g) => pending.playback = true,
            Cmd::Timeline(g) if current(g) => pending.timeline = true,
            _ => {} // stale event from a previous session
        }
    }

    fn emit(&mut self, event: MediaEvent) {
        if self.events.send(event).is_err() {
            self.closed = true;
        }
    }

    fn on_session_changed(&mut self) -> windows::core::Result<()> {
        self.unsubscribe();
        self.generation += 1;

        // GetCurrentSession fails (or returns null) when no app is playing.
        let session = self.manager.GetCurrentSession().ok();
        let app_id = session
            .as_ref()
            .and_then(|s| s.SourceAppUserModelId().ok())
            .map(|id| id.to_string());

        if let (Some(session), Some(app_id)) = (session, app_id.clone()) {
            self.sub = Some(self.subscribe(session, app_id)?);
        }

        if self.last_session.as_ref() != Some(&app_id) {
            self.last_session = Some(app_id.clone());
            self.emit(MediaEvent::SessionChanged(app_id));
            // Contract: SessionChanged is always followed by a full snapshot
            // (track, art, status, timeline, modes), even if values match the old app.
            self.forget_state();
        }

        if self.sub.is_none() {
            // Nothing playing anywhere: forget the track so it is re-sent
            // if it comes back, and report Stopped once.
            let status = self.last_status;
            let modes = self.last_modes;
            self.forget_state();
            self.last_status = status;
            self.last_modes = modes;
            if self.last_status != Some(PlaybackStatus::Stopped) {
                self.last_status = Some(PlaybackStatus::Stopped);
                self.emit(MediaEvent::PlaybackChanged(PlaybackStatus::Stopped));
            }
            self.emit_modes(PlayModes::default());
            return Ok(());
        }

        log_err("media", self.refresh_media());
        log_err("playback", self.refresh_playback());
        log_err("timeline", self.refresh_timeline());
        Ok(())
    }

    /// Clears dedup memory so the next refresh re-sends everything.
    fn forget_state(&mut self) {
        self.last_track = None;
        self.last_art = None;
        self.last_status = None;
        self.last_timeline = None;
        self.last_modes = None;
    }

    fn emit_modes(&mut self, modes: PlayModes) {
        if let Some(event) = modes_changed(&mut self.last_modes, modes) {
            self.emit(event);
        }
    }

    fn subscribe(&self, session: Session, app_id: String) -> windows::core::Result<Subscription> {
        let mut sub = Subscription {
            session,
            app_id,
            tokens: Vec::with_capacity(3),
        };
        let generation = self.generation;
        let result = (|| {
            let tx = self.cmd.clone();
            let token = sub.session.MediaPropertiesChanged(&TypedEventHandler::<
                Session,
                MediaPropertiesChangedEventArgs,
            >::new(move |_, _| {
                let _ = tx.send(Cmd::Media(generation));
                Ok(())
            }))?;
            sub.tokens.push((EventKind::Media, token));

            let tx = self.cmd.clone();
            let token = sub.session.PlaybackInfoChanged(&TypedEventHandler::<
                Session,
                PlaybackInfoChangedEventArgs,
            >::new(move |_, _| {
                let _ = tx.send(Cmd::Playback(generation));
                Ok(())
            }))?;
            sub.tokens.push((EventKind::Playback, token));

            let tx = self.cmd.clone();
            let token =
                sub.session.TimelinePropertiesChanged(&TypedEventHandler::<
                    Session,
                    TimelinePropertiesChangedEventArgs,
                >::new(move |_, _| {
                    let _ = tx.send(Cmd::Timeline(generation));
                    Ok(())
                }))?;
            sub.tokens.push((EventKind::Timeline, token));
            Ok(())
        })();

        match result {
            Ok(()) => Ok(sub),
            Err(e) => {
                remove_tokens(&sub);
                Err(e)
            }
        }
    }

    fn unsubscribe(&mut self) {
        if let Some(sub) = self.sub.take() {
            remove_tokens(&sub);
        }
    }

    fn current(&self) -> Option<(Session, String)> {
        self.sub
            .as_ref()
            .map(|s| (s.session.clone(), s.app_id.clone()))
    }

    fn refresh_media(&mut self) -> windows::core::Result<()> {
        let Some((session, app_id)) = self.current() else {
            return Ok(());
        };
        let props = session.TryGetMediaPropertiesAsync()?.join()?;

        let title = props.Title()?.to_string();
        if title.is_empty() {
            // Players briefly publish empty metadata while switching tracks.
            return Ok(());
        }
        let artist = props.Artist().map(|s| s.to_string()).unwrap_or_default();
        let album = props
            .AlbumTitle()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let album_artist = props
            .AlbumArtist()
            .map(|s| s.to_string())
            .unwrap_or_default();

        let duration = session
            .GetTimelineProperties()
            .ok()
            .and_then(|tl| Some((tl.StartTime().ok()?.Duration, tl.EndTime().ok()?.Duration)))
            .and_then(|(start, end)| span(start, end));
        let track = TrackInfo {
            title,
            artist,
            album,
            album_artist,
            duration,
            source_app: app_id,
        };
        // Compare the whole TrackInfo, so a later correction (e.g. duration
        // filled in) is re-sent, but identical repeats are not.
        if self.last_track.as_ref() != Some(&track) {
            self.last_track = Some(track.clone());
            self.emit(MediaEvent::TrackChanged(track));
        }

        // Art often arrives in a *later* MediaPropertiesChanged than the title,
        // so it is checked on every pass and sent only when the image changes.
        let art = match read_thumbnail(&props) {
            Ok(art) => art,
            Err(e) => {
                tracing::debug!("could not read album art: {e}");
                None
            }
        };
        let hash = art.as_ref().map(|(data, _)| {
            let mut h = DefaultHasher::new();
            data.hash(&mut h);
            h.finish()
        });
        if self.last_art != Some(hash) {
            self.last_art = Some(hash);
            self.emit(MediaEvent::ArtChanged(art.map(|(data, mime)| {
                AlbumArt::Bytes {
                    data: Arc::from(data),
                    mime,
                }
            })));
        }
        Ok(())
    }

    /// Sends a transport command to the current session. The resulting state
    /// change comes back through the normal SMTC events.
    fn apply_control(&mut self, control: Control) -> windows::core::Result<()> {
        let Some((session, app_id)) = self.current() else {
            return Ok(());
        };
        let op = match control {
            Control::PlayPause => session.TryTogglePlayPauseAsync()?,
            Control::Play => session.TryPlayAsync()?,
            Control::Pause => session.TryPauseAsync()?,
            Control::Next => session.TrySkipNextAsync()?,
            Control::Previous => session.TrySkipPreviousAsync()?,
            Control::SetShuffle(shuffle) => session.TryChangeShuffleActiveAsync(shuffle)?,
            Control::SetRepeat(repeat) => {
                session.TryChangeAutoRepeatModeAsync(repeat_to_win(repeat))?
            }
            Control::SetVolume(volume) => return crate::volume_win::set(&app_id, volume),
            Control::SeekTo(pos) => {
                let start = session.GetTimelineProperties()?.StartTime()?.Duration;
                let ticks = i64::try_from(pos.as_nanos() / 100).unwrap_or(i64::MAX);
                session.TryChangePlaybackPositionAsync(start.saturating_add(ticks))?
            }
        };
        if !op.join()? {
            tracing::debug!("player refused {control:?}");
        }
        Ok(())
    }

    fn refresh_playback(&mut self) -> windows::core::Result<()> {
        let Some((session, _)) = self.current() else {
            return Ok(());
        };
        let result = session.GetPlaybackInfo();
        let modes = result
            .as_ref()
            .ok()
            .map(|info| {
                let controls = info.Controls().ok();
                PlayModes {
                    shuffle: controls
                        .as_ref()
                        .filter(|c| c.IsShuffleEnabled().unwrap_or(false))
                        .and_then(|_| info.IsShuffleActive().and_then(|v| v.Value()).ok()),
                    repeat: controls
                        .as_ref()
                        .filter(|c| c.IsRepeatEnabled().unwrap_or(false))
                        .and_then(|_| info.AutoRepeatMode().and_then(|v| v.Value()).ok())
                        .and_then(repeat_from_win),
                }
            })
            .unwrap_or_default();
        self.emit_modes(modes);
        let info = result?;
        let status = match info.PlaybackStatus()? {
            WinStatus::Playing => PlaybackStatus::Playing,
            WinStatus::Paused => PlaybackStatus::Paused,
            WinStatus::Changing => return Ok(()), // transient, wait for the next event
            _ => PlaybackStatus::Stopped,         // Closed / Opened / Stopped
        };
        let rate = sanitize_rate(info.PlaybackRate().and_then(|r| r.Value()).unwrap_or(1.0));

        let rate_changed = rate != self.rate;
        self.rate = rate;
        let status_changed = self.last_status != Some(status);
        if status_changed {
            self.last_status = Some(status);
            self.emit(MediaEvent::PlaybackChanged(status));
        }
        if status_changed || rate_changed {
            // Always send a fresh anchor after play/pause. Spotify may deliver the
            // resume timeline *before* the status change, while we still think
            // it's paused, so that report could have been skipped as redundant.
            self.read_timeline(true)?;
        }
        Ok(())
    }

    fn refresh_timeline(&mut self) -> windows::core::Result<()> {
        self.read_timeline(false)
    }

    fn read_timeline(&mut self, force: bool) -> windows::core::Result<()> {
        let Some((session, _)) = self.current() else {
            return Ok(());
        };
        let tl = session.GetTimelineProperties()?;
        let start = tl.StartTime()?.Duration;
        let end = tl.EndTime()?.Duration;
        let position = tl.Position()?.Duration;
        let timeline = Timeline {
            position: span(start, position).unwrap_or_default(),
            duration: span(start, end),
            rate: self.rate,
            reported_at: instant_from_winrt_datetime(tl.LastUpdatedTime()?.UniversalTime),
        };

        // Spotify re-sends the same position every ~4.5 s (even while paused).
        // Skip reports that only confirm what interpolation already predicts.
        let playing = self.last_status == Some(PlaybackStatus::Playing);
        if !force
            && let Some(last) = &self.last_timeline
            && last.predicts(&timeline, playing, TIMELINE_TOLERANCE)
        {
            return Ok(());
        }
        self.last_timeline = Some(timeline);
        self.emit(MediaEvent::TimelineChanged(timeline));
        Ok(())
    }
}

fn repeat_from_win(mode: MediaPlaybackAutoRepeatMode) -> Option<RepeatMode> {
    match mode {
        MediaPlaybackAutoRepeatMode::None => Some(RepeatMode::Off),
        MediaPlaybackAutoRepeatMode::Track => Some(RepeatMode::Track),
        MediaPlaybackAutoRepeatMode::List => Some(RepeatMode::List),
        _ => None,
    }
}

fn repeat_to_win(mode: RepeatMode) -> MediaPlaybackAutoRepeatMode {
    match mode {
        RepeatMode::Off => MediaPlaybackAutoRepeatMode::None,
        RepeatMode::Track => MediaPlaybackAutoRepeatMode::Track,
        RepeatMode::List => MediaPlaybackAutoRepeatMode::List,
    }
}

impl Drop for Worker {
    // Runs on every exit path (normal stop, receiver gone, startup error).
    fn drop(&mut self) {
        self.unsubscribe();
        let _ = self.manager.RemoveCurrentSessionChanged(self.manager_token);
    }
}

/// Sessions can disappear mid-query (app closed). Not fatal, just log.
fn log_err(what: &str, result: windows::core::Result<()>) {
    if let Err(e) = result {
        tracing::debug!("SMTC {what} query failed: {e}");
    }
}

fn remove_tokens(sub: &Subscription) {
    for &(kind, token) in &sub.tokens {
        let _ = match kind {
            EventKind::Media => sub.session.RemoveMediaPropertiesChanged(token),
            EventKind::Playback => sub.session.RemovePlaybackInfoChanged(token),
            EventKind::Timeline => sub.session.RemoveTimelinePropertiesChanged(token),
        };
    }
}

/// Reads the thumbnail stream into memory. `Ok(None)` = the player gave no art.
fn read_thumbnail(props: &MediaProperties) -> windows::core::Result<Option<(Vec<u8>, String)>> {
    let Ok(reference) = props.Thumbnail() else {
        return Ok(None);
    };
    let stream = reference.OpenReadAsync()?.join()?;
    let size = stream.Size()?;
    if size == 0 || size > MAX_ART_BYTES {
        return Ok(None);
    }
    let size = size as u32;
    let buffer = Buffer::Create(size)?;
    let filled = stream
        .ReadAsync(&buffer, size, InputStreamOptions::ReadAhead)?
        .join()?;
    let mut data = vec![0u8; filled.Length()? as usize];
    DataReader::FromBuffer(&filled)?.ReadBytes(&mut data)?;
    if data.is_empty() {
        return Ok(None);
    }

    let mut mime = stream
        .ContentType()
        .map(|s| s.to_string())
        .unwrap_or_default();
    if !mime.starts_with("image/") {
        mime = sniff_image_mime(&data).to_string();
    }
    Ok(Some((data, mime)))
}

fn sniff_image_mime(data: &[u8]) -> &'static str {
    match data {
        [0xFF, 0xD8, ..] => "image/jpeg",
        [0x89, b'P', b'N', b'G', ..] => "image/png",
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => "image/webp",
        [b'G', b'I', b'F', ..] => "image/gif",
        [b'B', b'M', ..] => "image/bmp",
        _ => "application/octet-stream",
    }
}

const TICKS_PER_SEC: u64 = 10_000_000;

/// WinRT `TimeSpan` (100 ns ticks) to `Duration`, clamping negatives to zero.
/// Split into seconds + remainder so huge values can't overflow.
fn ticks_to_duration(ticks: i64) -> Duration {
    let t = ticks.max(0) as u64;
    Duration::new(t / TICKS_PER_SEC, ((t % TICKS_PER_SEC) * 100) as u32)
}

/// `end - start` in ticks as a Duration, `None` if empty, negative or overflowing.
fn span(start: i64, end: i64) -> Option<Duration> {
    end.checked_sub(start)
        .filter(|&d| d > 0)
        .map(ticks_to_duration)
}

/// Converts SMTC's `LastUpdatedTime` (wall clock) to an `Instant` (monotonic clock).
///
/// ```text
/// age         = SystemTime::now() - last_updated
/// reported_at = Instant::now()    - age
/// ```
/// A missing or future timestamp counts as "just now".
fn instant_from_winrt_datetime(universal_time: i64) -> Instant {
    let now = Instant::now();
    let Some(unix_ticks) = universal_time
        .checked_sub(UNIX_EPOCH_TICKS)
        .filter(|&t| t >= 0)
    else {
        return now;
    };
    let Some(reported) = UNIX_EPOCH.checked_add(ticks_to_duration(unix_ticks)) else {
        return now;
    };
    let age = SystemTime::now()
        .duration_since(reported)
        .unwrap_or_default();
    now.checked_sub(age).unwrap_or(now)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_modes_round_trip() {
        for (mode, windows) in [
            (RepeatMode::Off, MediaPlaybackAutoRepeatMode::None),
            (RepeatMode::Track, MediaPlaybackAutoRepeatMode::Track),
            (RepeatMode::List, MediaPlaybackAutoRepeatMode::List),
        ] {
            assert_eq!(repeat_to_win(mode), windows);
            assert_eq!(repeat_from_win(windows), Some(mode));
        }
        assert_eq!(repeat_from_win(MediaPlaybackAutoRepeatMode(-1)), None);
    }

    #[test]
    fn public_volume_control_clamps_before_queueing_and_ignores_nan() {
        let (tx, rx) = mpsc::channel();
        let watcher = crate::MediaWatcher {
            _inner: Watcher {
                cmd: tx,
                thread: None,
            },
        };
        watcher.control(Control::SetVolume(-1.0));
        watcher.control(Control::SetVolume(2.0));
        watcher.control(Control::SetVolume(f32::NAN));
        assert!(matches!(
            rx.try_recv(),
            Ok(Cmd::Control(Control::SetVolume(0.0)))
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(Cmd::Control(Control::SetVolume(1.0)))
        ));
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn volume_uses_worker_reply_and_handles_missing_worker() {
        let (tx, rx) = mpsc::channel();
        let watcher = Watcher {
            cmd: tx,
            thread: None,
        };
        let worker = std::thread::spawn(move || {
            let Cmd::Volume(reply) = rx.recv().unwrap() else {
                panic!("expected volume query")
            };
            reply.send(Some(0.5)).unwrap();
        });
        assert_eq!(watcher.volume(), Some(0.5));
        worker.join().unwrap();
        assert_eq!(watcher.volume(), None);
    }

    #[test]
    fn volume_times_out_when_worker_is_busy() {
        let (tx, _rx) = mpsc::channel();
        let watcher = Watcher {
            cmd: tx,
            thread: None,
        };
        let started = Instant::now();
        assert_eq!(watcher.volume(), None);
        // Allow scheduling overhead; the actual receive deadline is 250 ms.
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn ticks_convert() {
        assert_eq!(ticks_to_duration(10_000_000), Duration::from_secs(1));
        assert_eq!(ticks_to_duration(15), Duration::from_nanos(1500));
        assert_eq!(ticks_to_duration(-5), Duration::ZERO);
        // Huge values must not overflow (found by Codex review).
        let _ = ticks_to_duration(i64::MAX);
        assert_eq!(span(i64::MIN, i64::MAX), None);
        assert_eq!(span(10, 5), None);
        assert_eq!(span(0, 10_000_000), Some(Duration::from_secs(1)));
    }

    #[test]
    fn extreme_datetimes_are_now() {
        for t in [i64::MIN, -1, 0, i64::MAX] {
            assert!(
                instant_from_winrt_datetime(t).elapsed() < Duration::from_millis(50),
                "{t}"
            );
        }
    }

    #[test]
    fn datetime_age_is_respected() {
        let two_secs_ago = SystemTime::now() - Duration::from_secs(2);
        let unix = two_secs_ago.duration_since(UNIX_EPOCH).unwrap();
        let ticks = UNIX_EPOCH_TICKS + (unix.as_nanos() / 100) as i64;
        let age = instant_from_winrt_datetime(ticks).elapsed();
        assert!(
            age >= Duration::from_millis(1900) && age < Duration::from_millis(2500),
            "{age:?}"
        );
    }

    #[test]
    fn bad_datetime_is_now() {
        assert!(instant_from_winrt_datetime(0).elapsed() < Duration::from_millis(50));
        let future = SystemTime::now() + Duration::from_secs(60);
        let unix = future.duration_since(UNIX_EPOCH).unwrap();
        let ticks = UNIX_EPOCH_TICKS + (unix.as_nanos() / 100) as i64;
        assert!(instant_from_winrt_datetime(ticks).elapsed() < Duration::from_millis(50));
    }

    #[test]
    fn sniff() {
        assert_eq!(sniff_image_mime(&[0xFF, 0xD8, 0xFF]), "image/jpeg");
        assert_eq!(sniff_image_mime(b"\x89PNG\r\n"), "image/png");
        assert_eq!(sniff_image_mime(b"xx"), "application/octet-stream");
    }
}
