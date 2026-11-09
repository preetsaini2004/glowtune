//! Linux backend: MPRIS over the D-Bus session bus.
//!
//! ```text
//!  D-Bus session bus                     listener threads (block on signals)
//!  ─────────────────                     ─────────────────────────────────────
//!  NameOwnerChanged (org.mpris.*) ──►  "player appeared / vanished" ─┐
//!  PropertiesChanged (…Player)    ──►  "player X changed"           ├─ Cmd ──► worker thread
//!  Seeked (…Player)               ──►  "player X seeked"            ┘          ├─ GetAll(X)
//!                                                                              ├─ pick active player
//!                                                                              └─ dedup → MediaEvent
//! ```
//!
//! Every MPRIS player owns a bus name like `org.mpris.MediaPlayer2.spotify`.
//! Signals arrive from the player's *unique* name (`:1.42`), so we keep a map
//! from unique name to bus name. The active player is the one that most
//! recently started playing (like Windows' "current session").
//!
//! MPRIS doesn't push `Position` (it would be too chatty), so we read it on
//! every change and on `Seeked`. Read time = report time, so `reported_at = now`.

use std::collections::HashMap;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use zbus::MatchRule;
use zbus::blocking::fdo::DBusProxy;
use zbus::blocking::{Connection, MessageIterator};
use zbus::message::Message;
use zbus::names::BusName;
use zbus::zvariant::{OwnedValue, Value};

use crate::types::{TIMELINE_TOLERANCE, clamp_volume, modes_changed, sanitize_rate};
use crate::{
    AlbumArt, Control, Error, MediaEvent, PlayModes, PlaybackStatus, RepeatMode, Result, Timeline,
    TrackInfo,
};

const MPRIS_PREFIX: &str = "org.mpris.MediaPlayer2.";
const MPRIS_PATH: &str = "/org/mpris/MediaPlayer2";
const PLAYER_IFACE: &str = "org.mpris.MediaPlayer2.Player";

const NAME_RULE: &str = "type='signal',sender='org.freedesktop.DBus',interface='org.freedesktop.DBus',\
                         member='NameOwnerChanged',arg0namespace='org.mpris.MediaPlayer2'";
const PROPS_RULE: &str = "type='signal',interface='org.freedesktop.DBus.Properties',\
                          member='PropertiesChanged',path='/org/mpris/MediaPlayer2',\
                          arg0='org.mpris.MediaPlayer2.Player'";
const SEEKED_RULE: &str = "type='signal',interface='org.mpris.MediaPlayer2.Player',member='Seeked',path='/org/mpris/MediaPlayer2'";

enum Cmd {
    NameOwner { name: String, new_owner: String },
    Changed(String),
    Seeked(String),
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
        let conn = Connection::session()?;
        let (event_tx, event_rx) = mpsc::channel();
        let (cmd_tx, cmd_rx) = mpsc::channel();

        // Register match rules *before* the initial scan so no change is missed.
        spawn_listener(&conn, NAME_RULE, cmd_tx.clone(), |msg| {
            let (name, _old, new_owner): (String, String, String) =
                msg.body().deserialize().ok()?;
            Some(Cmd::NameOwner { name, new_owner })
        })?;
        spawn_listener(&conn, PROPS_RULE, cmd_tx.clone(), |msg| {
            Some(Cmd::Changed(msg.header().sender()?.to_string()))
        })?;
        spawn_listener(&conn, SEEKED_RULE, cmd_tx.clone(), |msg| {
            Some(Cmd::Seeked(msg.header().sender()?.to_string()))
        })?;

        let mut worker = Worker::new(conn, event_tx)?;
        worker.scan()?;

        let thread = std::thread::Builder::new()
            .name("media_source-mpris".into())
            .spawn(move || worker.run(cmd_rx))
            .map_err(|e| Error::Thread(e.to_string()))?;
        Ok((
            Self {
                cmd: cmd_tx,
                thread: Some(thread),
            },
            event_rx,
        ))
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
        // The worker only blocks on our channel, so it stops right away.
        // Known limitation: listener threads exit only on their next signal
        // (their send fails). Fine for one watcher per app lifetime; revisit
        // if watchers are ever created and dropped repeatedly.
        let _ = self.cmd.send(Cmd::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn spawn_listener(
    conn: &Connection,
    rule: &'static str,
    tx: Sender<Cmd>,
    map: fn(&Message) -> Option<Cmd>,
) -> Result<()> {
    let rule = MatchRule::try_from(rule)?;
    let messages = MessageIterator::for_match_rule(rule, conn, Some(64))?;
    std::thread::Builder::new()
        .name("media_source-mpris-signals".into())
        .stack_size(256 * 1024)
        .spawn(move || {
            for msg in messages.flatten() {
                if let Some(cmd) = map(&msg)
                    && tx.send(cmd).is_err()
                {
                    break;
                }
            }
        })
        .map_err(|e| Error::Thread(e.to_string()))?;
    Ok(())
}

struct Player {
    bus_name: String,
    unique: String,
    status: PlaybackStatus,
    /// When this player last switched to Playing (for picking the active one).
    playing_since: Option<Instant>,
}

type Props = HashMap<String, OwnedValue>;

struct Worker {
    conn: Connection,
    dbus: DBusProxy<'static>,
    events: Sender<MediaEvent>,
    closed: bool,

    players: Vec<Player>,
    active: Option<String>,

    // Last emitted values, for dedup. Outer `None` = nothing emitted yet.
    last_session: Option<Option<String>>,
    /// Track info plus `mpris:trackid` (distinguishes same-named tracks).
    last_track: Option<(TrackInfo, Option<String>)>,
    last_art: Option<Option<String>>,
    last_status: Option<PlaybackStatus>,
    last_timeline: Option<Timeline>,
    last_modes: Option<PlayModes>,
}

impl Worker {
    fn new(conn: Connection, events: Sender<MediaEvent>) -> Result<Self> {
        let dbus = DBusProxy::new(&conn)?;
        Ok(Self {
            conn,
            dbus,
            events,
            closed: false,
            players: Vec::new(),
            active: None,
            last_session: None,
            last_track: None,
            last_art: None,
            last_status: None,
            last_timeline: None,
            last_modes: None,
        })
    }

    fn scan(&mut self) -> Result<()> {
        for name in self.dbus.list_names()? {
            let name = name.to_string();
            if !name.starts_with(MPRIS_PREFIX) {
                continue;
            }
            let Ok(bus) = BusName::try_from(name.as_str()) else {
                continue;
            };
            if let Ok(owner) = self.dbus.get_name_owner(bus) {
                self.add_player(name, owner.to_string());
            }
        }
        let active = self.pick_active();
        self.set_active(active, true);
        Ok(())
    }

    fn run(&mut self, rx: Receiver<Cmd>) {
        while let Ok(cmd) = rx.recv() {
            match cmd {
                Cmd::Stop => break,
                Cmd::Control(c) => self.on_control(c),
                Cmd::Volume(reply) => {
                    let volume = self
                        .active
                        .as_ref()
                        .and_then(|name| self.get_all(name).ok())
                        .and_then(|props| props.get("Volume").and_then(|v| as_f64(v)))
                        .and_then(|v| clamp_volume(v as f32));
                    let _ = reply.send(volume);
                }
                Cmd::NameOwner { name, new_owner } => self.on_name_owner(name, new_owner),
                Cmd::Changed(unique) => self.on_changed(&unique),
                Cmd::Seeked(unique) => self.on_seeked(&unique),
            }
            if self.closed {
                break;
            }
        }
    }

    fn emit(&mut self, event: MediaEvent) {
        if self.events.send(event).is_err() {
            self.closed = true;
        }
    }

    fn get_all(&self, bus_name: &str) -> zbus::Result<Props> {
        let reply = self.conn.call_method(
            Some(bus_name),
            MPRIS_PATH,
            Some("org.freedesktop.DBus.Properties"),
            "GetAll",
            &(PLAYER_IFACE,),
        )?;
        reply.body().deserialize()
    }

    fn set_property(&self, bus_name: &str, property: &str, value: Value<'_>) -> zbus::Result<()> {
        self.conn
            .call_method(
                Some(bus_name),
                MPRIS_PATH,
                Some("org.freedesktop.DBus.Properties"),
                "Set",
                &(PLAYER_IFACE, property, value),
            )
            .map(|_| ())
    }

    fn add_player(&mut self, bus_name: String, unique: String) {
        self.players.retain(|p| p.bus_name != bus_name);
        let status = self
            .get_all(&bus_name)
            .map(|p| status_of(&p))
            .unwrap_or(PlaybackStatus::Stopped);
        let playing_since = (status == PlaybackStatus::Playing).then(Instant::now);
        self.players.push(Player {
            bus_name,
            unique,
            status,
            playing_since,
        });
    }

    fn on_name_owner(&mut self, name: String, new_owner: String) {
        if !name.starts_with(MPRIS_PREFIX) {
            return;
        }
        // A new owner of the *active* name is a different process: re-read it all.
        let was_active = self.active.as_deref() == Some(name.as_str());
        if new_owner.is_empty() {
            self.players.retain(|p| p.bus_name != name);
        } else {
            self.add_player(name, new_owner);
        }
        let active = self.pick_active();
        self.set_active(active, was_active);
    }

    fn on_changed(&mut self, unique: &str) {
        let Some(idx) = self.players.iter().position(|p| p.unique == unique) else {
            return;
        };
        let bus_name = self.players[idx].bus_name.clone();
        let props = match self.get_all(&bus_name) {
            Ok(props) => props,
            Err(e) => {
                tracing::debug!("MPRIS GetAll({bus_name}) failed: {e}");
                return;
            }
        };
        let status = status_of(&props);
        let player = &mut self.players[idx];
        if status == PlaybackStatus::Playing && player.status != PlaybackStatus::Playing {
            player.playing_since = Some(Instant::now());
        }
        player.status = status;

        let active = self.pick_active();
        if active != self.active {
            self.set_active(active, false);
        } else if self.active.as_deref() == Some(bus_name.as_str()) {
            self.apply(&bus_name, props);
        }
    }

    /// Calls the matching MPRIS method on the active player.
    fn on_control(&mut self, control: Control) {
        let Some(bus_name) = self.active.clone() else {
            return;
        };
        let call = |method: &str| {
            self.conn
                .call_method(
                    Some(bus_name.as_str()),
                    MPRIS_PATH,
                    Some(PLAYER_IFACE),
                    method,
                    &(),
                )
                .map(|_| ())
        };
        let result = match control {
            Control::PlayPause => call("PlayPause"),
            Control::Play => call("Play"),
            Control::Pause => call("Pause"),
            Control::Next => call("Next"),
            Control::Previous => call("Previous"),
            Control::SetShuffle(shuffle) => {
                self.set_property(&bus_name, "Shuffle", Value::Bool(shuffle))
            }
            Control::SetRepeat(repeat) => self.set_property(
                &bus_name,
                "LoopStatus",
                Value::from(repeat_to_mpris(repeat)),
            ),
            Control::SetVolume(volume) => {
                self.set_property(&bus_name, "Volume", Value::F64(f64::from(volume)))
            }
            Control::SeekTo(pos) => {
                // SetPosition needs the current track id (MPRIS spec).
                let track_id = self
                    .get_all(&bus_name)
                    .ok()
                    .and_then(|mut p| p.remove("Metadata"))
                    .and_then(|v| HashMap::<String, OwnedValue>::try_from(v).ok())
                    .and_then(|m| m.get("mpris:trackid").and_then(|v| as_string(v)));
                match track_id.and_then(|id| zbus::zvariant::ObjectPath::try_from(id).ok()) {
                    Some(path) => {
                        let us = i64::try_from(pos.as_micros()).unwrap_or(i64::MAX);
                        self.conn
                            .call_method(
                                Some(bus_name.as_str()),
                                MPRIS_PATH,
                                Some(PLAYER_IFACE),
                                "SetPosition",
                                &(path, us),
                            )
                            .map(|_| ())
                    }
                    None => Ok(()),
                }
            }
        };
        if let Err(e) = result {
            tracing::debug!("MPRIS {control:?} on {bus_name} failed: {e}");
        }
    }

    fn on_seeked(&mut self, unique: &str) {
        let Some(bus_name) = self.active.clone() else {
            return;
        };
        if !self
            .players
            .iter()
            .any(|p| p.unique == unique && p.bus_name == bus_name)
        {
            return;
        }
        if let Ok(props) = self.get_all(&bus_name) {
            let status = status_of(&props);
            self.apply_timeline(&props, status, None, true);
        }
    }

    /// Most recently started Playing player; else keep the current one; else any.
    fn pick_active(&self) -> Option<String> {
        let playing = self
            .players
            .iter()
            .filter(|p| p.status == PlaybackStatus::Playing)
            .max_by_key(|p| p.playing_since);
        if let Some(p) = playing {
            return Some(p.bus_name.clone());
        }
        if let Some(current) = &self.active
            && self.players.iter().any(|p| &p.bus_name == current)
        {
            return Some(current.clone());
        }
        self.players
            .iter()
            .find(|p| p.status == PlaybackStatus::Paused)
            .or_else(|| self.players.first())
            .map(|p| p.bus_name.clone())
    }

    /// Clears dedup memory so the next snapshot re-sends everything.
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

    fn set_active(&mut self, active: Option<String>, force_refresh: bool) {
        let changed = active != self.active;
        self.active = active.clone();
        if self.last_session.as_ref() != Some(&active) {
            self.last_session = Some(active.clone());
            self.emit(MediaEvent::SessionChanged(active.clone()));
            // Contract: SessionChanged is always followed by a full snapshot
            // (track, art, status, timeline, modes), even if values match the old player.
            self.forget_state();
        }
        match active {
            None => {
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
            }
            Some(bus_name) if changed || force_refresh => {
                if let Ok(props) = self.get_all(&bus_name) {
                    self.apply(&bus_name, props);
                } else {
                    self.emit_modes(PlayModes::default());
                }
            }
            Some(_) => {}
        }
    }

    /// Turns a full property snapshot into deduplicated events.
    fn apply(&mut self, bus_name: &str, mut props: Props) {
        self.emit_modes(modes_of(&props));
        let metadata: Props = props
            .remove("Metadata")
            .and_then(|v| HashMap::try_from(v).ok())
            .unwrap_or_default();
        let meta = |key: &str| metadata.get(key).map(|v| &**v);

        let title = meta("xesam:title").and_then(as_string).unwrap_or_default();
        // Per the MPRIS spec, `mpris:trackid` identifies a track; title is optional.
        let track_id = meta("mpris:trackid")
            .and_then(as_string)
            .filter(|id| !id.is_empty());
        let length = meta("mpris:length").and_then(as_micros);

        if !title.is_empty() || track_id.is_some() {
            let track = TrackInfo {
                title,
                artist: meta("xesam:artist")
                    .and_then(as_string_list)
                    .unwrap_or_default(),
                album: meta("xesam:album").and_then(as_string).unwrap_or_default(),
                album_artist: meta("xesam:albumArtist")
                    .and_then(as_string_list)
                    .unwrap_or_default(),
                duration: length,
                source_app: bus_name.to_string(),
            };
            let key = (track, track_id);
            if self.last_track.as_ref() != Some(&key) {
                self.last_track = Some(key.clone());
                self.emit(MediaEvent::TrackChanged(key.0));
            }

            let art_url = meta("mpris:artUrl")
                .and_then(as_string)
                .filter(|u| !u.is_empty());
            if self.last_art.as_ref() != Some(&art_url) {
                self.last_art = Some(art_url.clone());
                self.emit(MediaEvent::ArtChanged(art_url.map(AlbumArt::Url)));
            }
        }

        let status = status_of(&props);
        if self.last_status != Some(status) {
            self.last_status = Some(status);
            self.emit(MediaEvent::PlaybackChanged(status));
        }

        self.apply_timeline(&props, status, length, false);
    }

    fn apply_timeline(
        &mut self,
        props: &Props,
        status: PlaybackStatus,
        length: Option<Duration>,
        seeked: bool,
    ) {
        let Some(position) = props.get("Position").and_then(|v| as_micros(v)) else {
            return;
        };
        let rate = sanitize_rate(props.get("Rate").and_then(|v| as_f64(v)).unwrap_or(1.0));
        // A full snapshot's missing length means "unknown" (e.g. a live stream).
        // Only a Seeked update (same track, no metadata read) keeps the old one.
        let duration = if seeked {
            self.last_timeline.and_then(|t| t.duration)
        } else {
            length
        };
        let timeline = Timeline {
            position,
            duration,
            rate,
            reported_at: Instant::now(),
        };

        // Skip reports that only confirm what interpolation already predicts.
        let playing = status == PlaybackStatus::Playing;
        if !seeked
            && let Some(last) = &self.last_timeline
            && last.predicts(&timeline, playing, TIMELINE_TOLERANCE)
        {
            return;
        }
        self.last_timeline = Some(timeline);
        self.emit(MediaEvent::TimelineChanged(timeline));
    }
}

fn modes_of(props: &Props) -> PlayModes {
    PlayModes {
        shuffle: props.get("Shuffle").and_then(|v| bool::try_from(v).ok()),
        repeat: props
            .get("LoopStatus")
            .and_then(|v| as_string(v))
            .as_deref()
            .and_then(repeat_from_mpris),
    }
}

fn repeat_from_mpris(mode: &str) -> Option<RepeatMode> {
    match mode {
        "None" => Some(RepeatMode::Off),
        "Track" => Some(RepeatMode::Track),
        "Playlist" => Some(RepeatMode::List),
        _ => None,
    }
}

fn repeat_to_mpris(mode: RepeatMode) -> &'static str {
    match mode {
        RepeatMode::Off => "None",
        RepeatMode::Track => "Track",
        RepeatMode::List => "Playlist",
    }
}

fn status_of(props: &Props) -> PlaybackStatus {
    match props
        .get("PlaybackStatus")
        .and_then(|v| as_string(v))
        .as_deref()
    {
        Some("Playing") => PlaybackStatus::Playing,
        Some("Paused") => PlaybackStatus::Paused,
        _ => PlaybackStatus::Stopped,
    }
}

fn as_string(v: &Value<'_>) -> Option<String> {
    match v {
        Value::Str(s) => Some(s.to_string()),
        Value::ObjectPath(p) => Some(p.to_string()),
        Value::Value(inner) => as_string(inner),
        _ => None,
    }
}

/// `xesam:artist` is a list of strings; some players send a plain string.
fn as_string_list(v: &Value<'_>) -> Option<String> {
    match v {
        Value::Array(items) => {
            let parts: Vec<String> = items.iter().filter_map(as_string).collect();
            Some(parts.join(", "))
        }
        Value::Value(inner) => as_string_list(inner),
        other => as_string(other),
    }
}

/// MPRIS times are microseconds; players disagree on i64 vs u64.
fn as_micros(v: &Value<'_>) -> Option<Duration> {
    let us: i64 = match v {
        Value::I64(n) => *n,
        Value::U64(n) => i64::try_from(*n).ok()?,
        Value::I32(n) => (*n).into(),
        Value::U32(n) => (*n).into(),
        Value::F64(n) => *n as i64,
        Value::Value(inner) => return as_micros(inner),
        _ => return None,
    };
    Some(Duration::from_micros(us.max(0) as u64))
}

fn as_f64(v: &Value<'_>) -> Option<f64> {
    match v {
        Value::F64(n) => Some(*n),
        Value::Value(inner) => as_f64(inner),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_modes_round_trip() {
        for (mode, mpris) in [
            (RepeatMode::Off, "None"),
            (RepeatMode::Track, "Track"),
            (RepeatMode::List, "Playlist"),
        ] {
            assert_eq!(repeat_to_mpris(mode), mpris);
            assert_eq!(repeat_from_mpris(mpris), Some(mode));
        }
        assert_eq!(repeat_from_mpris(""), None);
        assert_eq!(repeat_from_mpris("List"), None);
    }

    #[test]
    fn optional_modes_from_properties() {
        let mut props = Props::new();
        assert_eq!(modes_of(&props), PlayModes::default());
        props.insert("Shuffle".into(), OwnedValue::from(false));
        props.insert(
            "LoopStatus".into(),
            Value::from("Playlist").try_into().unwrap(),
        );
        assert_eq!(
            modes_of(&props),
            PlayModes {
                shuffle: Some(false),
                repeat: Some(RepeatMode::List)
            }
        );
        props.insert("Shuffle".into(), OwnedValue::from(1u32));
        props.insert(
            "LoopStatus".into(),
            Value::from("invalid").try_into().unwrap(),
        );
        assert_eq!(modes_of(&props), PlayModes::default());
    }
}
