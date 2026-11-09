//! Music shelves from saved service webviews. Remote pages remain outside Tauri capabilities.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

use crate::services::{self, Service};

const SERVICES: [Service; 4] = [
    Service::Spotify,
    Service::YoutubeMusic,
    Service::AppleMusic,
    Service::Soundcloud,
];
static REFRESH: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MusicItem {
    pub service: Service,
    pub kind: String,
    pub title: String,
    pub artist: String,
    pub artwork: Option<String>,
    pub url: String,
    /// The shelf it came from on the service page ("Made For You", "Recently played"…).
    #[serde(default)]
    pub section: Option<String>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Shelf {
    pub service: Service,
    pub name: String,
    pub status: String,
    pub items: Vec<MusicItem>,
    pub updated_at: u64,
}

#[derive(Default, Deserialize)]
#[serde(default, rename_all = "camelCase")]
struct PageSnapshot {
    page_url: String,
    items: Vec<MusicItem>,
    queue: Vec<MusicItem>,
    current: String,
    queue_visible: bool,
    sign_in: bool,
    /// Spotify only, when its queue couldn't be read: the panel's markers.
    debug: Option<String>,
    /// SoundCloud's "Autoplay station" switch is on.
    autoplay: bool,
}

#[derive(Serialize)]
pub struct Queue {
    pub items: Vec<MusicItem>,
    pub message: String,
    /// What the list is when it isn't the player's own queue (SoundCloud autoplay).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

const NOTHING_QUEUED: &str = "No upcoming songs exposed by this player.";

fn cache() -> &'static Mutex<HashMap<String, Shelf>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Shelf>>> = OnceLock::new();
    CACHE.get_or_init(Mutex::default)
}

fn valid_url(service: Service, value: &str) -> bool {
    let Ok(url) = tauri::Url::parse(value) else {
        return false;
    };
    if url.scheme() != "https"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.port().is_some()
    {
        return false;
    }
    let host = url.host_str().unwrap_or_default();
    match service {
        Service::Spotify => {
            host == "open.spotify.com"
                && ["/track/", "/album/", "/playlist/", "/artist/"]
                    .iter()
                    .any(|p| url.path().starts_with(p))
        }
        Service::YoutubeMusic => {
            host == "music.youtube.com"
                && ["/watch", "/playlist", "/browse/"]
                    .iter()
                    .any(|p| url.path().starts_with(p))
        }
        Service::AppleMusic => {
            host == "music.apple.com"
                && ["/album/", "/playlist/", "/song/", "/artist/"]
                    .iter()
                    .any(|p| url.path().contains(p))
        }
        Service::Soundcloud => {
            host == "soundcloud.com" && url.path().split('/').filter(|p| !p.is_empty()).count() >= 2
        }
    }
}

fn clean(items: Vec<MusicItem>, service: Service, deduplicate: bool) -> Vec<MusicItem> {
    let mut seen = HashSet::new();
    items
        .into_iter()
        .filter_map(|mut item| {
            if item.service != service
                || !valid_url(service, &item.url)
                || !["song", "album", "artist", "playlist"].contains(&item.kind.as_str())
                || (deduplicate && !seen.insert(item.url.clone()))
            {
                return None;
            }
            item.title = item.title.trim().chars().take(180).collect();
            item.artist = item.artist.trim().chars().take(180).collect();
            item.section = item
                .section
                .map(|s| s.trim().chars().take(60).collect::<String>())
                .filter(|s| !s.is_empty());
            if item.title.is_empty() {
                return None;
            }
            item.artwork = item.artwork.filter(|value| {
                tauri::Url::parse(value).is_ok_and(|u| {
                    u.scheme() == "https"
                        && u.host_str().is_some_and(|h| {
                            [
                                "scdn.co",
                                "spotifycdn.com",
                                "ytimg.com",
                                "googleusercontent.com",
                                "mzstatic.com",
                                "sndcdn.com",
                            ]
                            .iter()
                            .any(|d| h == *d || h.ends_with(&format!(".{d}")))
                        })
                })
            });
            Some(item)
        })
        .take(120)
        .collect()
}

async fn snapshot(window: &WebviewWindow) -> Option<PageSnapshot> {
    let (tx, rx) = std::sync::mpsc::channel();
    services::run_script(window, include_str!("music_snapshot.js").into(), tx).ok()?;
    let result =
        tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(2)).ok())
            .await
            .ok()??;
    if result.len() > 512_000 {
        return None;
    }
    serde_json::from_str(&result).ok()
}

pub fn shelves(app: &AppHandle) -> Vec<Shelf> {
    let cached = cache().lock().ok();
    SERVICES
        .into_iter()
        .map(|service| {
            cached
                .as_ref()
                .and_then(|c| c.get(service.id()).cloned())
                .unwrap_or_else(|| Shelf {
                    service,
                    name: service.title().into(),
                    status: if profile(app, service).is_some_and(|p| p.exists()) {
                        "refresh"
                    } else {
                        "connect"
                    }
                    .into(),
                    items: Vec::new(),
                    updated_at: 0,
                })
        })
        .collect()
}

fn profile(app: &AppHandle, service: Service) -> Option<std::path::PathBuf> {
    Some(
        app.path()
            .app_local_data_dir()
            .ok()?
            .join("services")
            .join(service.id()),
    )
}

/// The extra reader is short-lived and serial, so browsing adds at most one webview.
// shortcut: reads loaded Home/Library shelves only, add provider APIs for complete private-library pagination.
#[tauri::command]
pub async fn refresh_music_library(app: AppHandle) -> Vec<Shelf> {
    let Ok(_guard) = REFRESH.try_lock() else {
        return shelves(&app);
    };
    for service in SERVICES {
        let Some(profile) = profile(&app, service).filter(|p| p.exists()) else {
            continue;
        };
        let label = format!("music-reader-{}", service.id());
        let mut shelf = Shelf {
            service,
            name: service.title().into(),
            status: "unavailable".into(),
            items: Vec::new(),
            updated_at: 0,
        };
        let reader = WebviewWindowBuilder::new(&app, &label, WebviewUrl::External(service.url().parse().expect("service URL")))
            .data_directory(profile)
            .additional_browser_args(services::BROWSER_ARGS)
            .initialization_script("HTMLMediaElement.prototype.play = function(){ this.muted=true; return Promise.resolve(); }; if(window.MediaSession){MediaSession.prototype.setActionHandler=function(){}; Object.defineProperty(MediaSession.prototype, 'metadata', {set(){},get(){return null;}});} window.open = () => null;")
            .visible(false).focused(false).skip_taskbar(true).build()
            .map_err(|error| { tracing::warn!(%error, service = service.id(), "music shelf reader unavailable"); error });
        if let Ok(reader) = reader {
            for (page_index, url) in [service.url(), library_url(service)]
                .into_iter()
                .enumerate()
            {
                if page_index > 0
                    && reader
                        .navigate(url.parse().expect("service library URL"))
                        .is_err()
                {
                    break;
                }
                for _ in 0..8 {
                    tokio::time::sleep(Duration::from_millis(700)).await;
                    if let Some(page) = snapshot(&reader).await {
                        if page.sign_in {
                            shelf.status = "signin".into();
                            shelf.items.clear();
                            break;
                        }
                        if page_index > 0 {
                            let expected = tauri::Url::parse(url).expect("library URL");
                            if !tauri::Url::parse(&page.page_url).is_ok_and(|observed| {
                                observed
                                    .path()
                                    .trim_end_matches('/')
                                    .ends_with(expected.path().trim_end_matches('/'))
                            }) {
                                continue;
                            }
                        }
                        let items = clean(page.items, service, true);
                        if !items.is_empty() {
                            shelf.items.extend(items.into_iter().take(60));
                            shelf.status = "ready".into();
                            break;
                        }
                    }
                }
                if shelf.status == "signin" {
                    break;
                }
            }
            shelf.items = clean(shelf.items, service, true);
            if let Err(e) = reader.destroy() {
                tracing::warn!(%e, "music reader cleanup failed");
            }
        }
        shelf.updated_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if let Ok(mut cached) = cache().lock() {
            cached.insert(service.id().into(), shelf);
        }
        let _ = tauri::Emitter::emit(&app, "music-library", shelves(&app));
    }
    shelves(&app)
}

fn library_url(service: Service) -> &'static str {
    match service {
        Service::Spotify => "https://open.spotify.com/your-library",
        Service::YoutubeMusic => "https://music.youtube.com/library",
        Service::AppleMusic => "https://music.apple.com/library/albums",
        Service::Soundcloud => "https://soundcloud.com/you/likes",
    }
}

#[tauri::command]
pub async fn open_music_library(service: Service, app: AppHandle) -> Result<(), String> {
    services::show_at(&app, service, Some(library_url(service).into())).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn get_music_library(app: AppHandle) -> Vec<Shelf> {
    shelves(&app)
}

#[tauri::command]
pub async fn open_music_item(item: MusicItem, app: AppHandle) -> Result<(), String> {
    if !valid_url(item.service, &item.url) {
        return Err("Invalid service music link.".into());
    }
    if item.kind == "song" {
        let shared = app.state::<Arc<crate::hub::Shared>>();
        shared.media.control(media_source::Control::Pause);
        services::play_link(&app, item.service, item.url).map_err(|e| e.to_string())
    } else {
        services::show_at(&app, item.service, Some(item.url)).map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn get_music_queue(app: AppHandle) -> Queue {
    // Every read leaves a short trace of each step, so a player that stops
    // sharing its queue can be diagnosed from the file (no song data beyond titles).
    let mut log = Vec::new();
    let queue = read_music_queue(app.clone(), &mut log).await;
    log.push(format!(
        "result: {} items, message: {:?}",
        queue.items.len(),
        queue.message
    ));
    if let Ok(dir) = app.path().app_cache_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("queue-trace.txt"),
            log.join(
                "
",
            ),
        );
    }
    queue
}

async fn read_music_queue(app: AppHandle, log: &mut Vec<String>) -> Queue {
    let missing = |message: &str| Queue {
        items: Vec::new(),
        message: message.into(),
        label: None,
    };
    let Some(service) = services::open_service(&app) else {
        return missing("Open a service in Glowtune to see its playback queue.");
    };
    let Some(window) = app.get_webview_window(services::SERVICE) else {
        return missing("Player unavailable.");
    };
    log.push(format!("service: {}", service.id()));
    let Some(mut page) = snapshot(&window).await else {
        return missing("The player has not shared its queue yet.");
    };
    // Spotify, Apple Music and SoundCloud only build their queue list while the
    // Queue panel is open: open it once and leave it open (Up next re-reads it
    // every few seconds; toggling each time would make the player flicker).
    if !page.queue_visible && toggle_queue(&window, service).await {
        let _ =
            tauri::async_runtime::spawn_blocking(|| std::thread::sleep(Duration::from_millis(900)))
                .await;
        if let Some(read) = snapshot(&window).await {
            page = read;
        }
        // SoundCloud keeps the rows after its panel closes, so close it again
        // instead of leaving it over the page.
        if service == Service::Soundcloud {
            let (tx, _rx) = std::sync::mpsc::channel();
            let _ = services::run_script(
                &window,
                "document.querySelector('.queue.m-visible .queue__hide')?.click()".into(),
                tx,
            );
        }
    }
    log.push(format!(
        "page: queue_visible={} queue_rows={} current={:?} debug={}",
        page.queue_visible,
        page.queue.len(),
        page.current,
        page.debug.is_some()
    ));
    // Spotify changes its markup often: when its queue can't be read, keep a
    // description of the panel (markers + headings only) for fixing the reader.
    if let Some(debug) = page.debug.take()
        && let Ok(dir) = app.path().app_cache_dir()
    {
        let _ = std::fs::create_dir_all(&dir);
        let text: String = debug.chars().take(30_000).collect();
        let _ = std::fs::write(dir.join("queue-debug.txt"), text);
    }
    if services::open_service(&app) != Some(service) {
        return missing("Player changed. Refresh to read its queue.");
    }
    let track = app
        .state::<Arc<crate::hub::Shared>>()
        .snapshot
        .lock()
        .ok()
        .and_then(|s| s.track.clone());
    let Some(track) = track else {
        return missing("Play a song to see Up next.");
    };
    log.push(format!("track: {:?} from {:?}", track.title, track.source));
    let autoplay = page.autoplay;
    let mut queue = queue_for(page, service, &track.title, &track.source);
    // SoundCloud with nothing queued picks the next song from the current
    // song's station only when this one ends: show that station instead.
    if service == Service::Soundcloud
        && autoplay
        && queue.items.is_empty()
        && queue.message == NOTHING_QUEUED
    {
        let items = clean(soundcloud_station(&window).await, service, false);
        log.push(format!("autoplay station: {} items", items.len()));
        if !items.is_empty() {
            queue.items = items;
            queue.label = Some("Autoplay picks from these".into());
        }
    }
    queue
}

#[derive(Default, Deserialize)]
#[serde(default)]
struct StationRead {
    pending: bool,
    items: Vec<MusicItem>,
}

/// Reads the current SoundCloud song's station (see `soundcloud_station.js`),
/// waiting up to ~4 s for the page to fetch it. The page keeps the result, so
/// later refreshes for the same song return at once.
async fn soundcloud_station(window: &WebviewWindow) -> Vec<MusicItem> {
    for _ in 0..10 {
        let (tx, rx) = std::sync::mpsc::channel();
        if services::run_script(window, include_str!("soundcloud_station.js").into(), tx).is_err() {
            break;
        }
        let read = tauri::async_runtime::spawn_blocking(move || {
            let out = rx.recv_timeout(Duration::from_secs(2)).ok()?;
            (out.len() <= 512_000)
                .then(|| serde_json::from_str::<StationRead>(&out).ok())
                .flatten()
        })
        .await
        .ok()
        .flatten();
        match read {
            Some(read) if !read.pending => return read.items,
            Some(_) => {
                let _ = tauri::async_runtime::spawn_blocking(|| {
                    std::thread::sleep(Duration::from_millis(400))
                })
                .await;
            }
            None => break,
        }
    }
    Vec::new()
}

fn queue_for(page: PageSnapshot, service: Service, title: &str, source: &str) -> Queue {
    let missing = |message: &str| Queue {
        items: Vec::new(),
        message: message.into(),
        label: None,
    };
    if source != "Web player"
        || page.current.is_empty()
        || page.current.trim().to_lowercase() != title.trim().to_lowercase()
    {
        return missing("Play from a Glowtune service window to share its queue here.");
    }
    if !page.queue_visible {
        return missing("Open Queue in the service player, then refresh Up next.");
    }
    Queue {
        items: clean(page.queue, service, false),
        message: NOTHING_QUEUED.into(),
        label: None,
    }
}

/// Presses the player's own Queue button. True if it found one.
async fn toggle_queue(window: &WebviewWindow, service: Service) -> bool {
    let selector = match service {
        Service::Spotify => r#"[data-testid="control-button-queue"]"#,
        Service::AppleMusic => {
            r#"button[aria-label="Playing Next"], button.playback-controls__queue, [data-testid="queue-button"]"#
        }
        Service::Soundcloud => ".playbackSoundBadge__showQueue",
        Service::YoutubeMusic => return false, // its queue is always in the page
    };
    let js = format!(
        "(() => {{ const b = document.querySelector({}); if (!b) return false; b.click(); return true; }})()",
        serde_json::to_string(selector).unwrap_or_default()
    );
    let (tx, rx) = std::sync::mpsc::channel();
    if services::run_script(window, js, tx).is_err() {
        return false;
    }
    tauri::async_runtime::spawn_blocking(move || rx.recv_timeout(Duration::from_secs(2)).ok())
        .await
        .ok()
        .flatten()
        .is_some_and(|r| r == "true")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn queues_require_the_active_web_player_and_matching_song() {
        let make = || PageSnapshot {
            current: "Current song".into(),
            queue_visible: true,
            queue: vec![MusicItem {
                service: Service::Spotify,
                section: None,
                kind: "song".into(),
                title: "Next song".into(),
                artist: "Artist".into(),
                artwork: None,
                url: "https://open.spotify.com/track/next".into(),
            }],
            ..Default::default()
        };
        assert!(
            queue_for(make(), Service::Spotify, "Different song", "Web player")
                .items
                .is_empty()
        );
        assert!(
            queue_for(make(), Service::Spotify, "Current song", "Spotify")
                .items
                .is_empty()
        );
        assert_eq!(
            queue_for(make(), Service::Spotify, "current SONG", "Web player").items[0].title,
            "Next song"
        );
    }

    #[test]
    fn music_links_cannot_escape_the_service() {
        assert!(valid_url(
            Service::Spotify,
            "https://open.spotify.com/album/123"
        ));
        for bad in [
            "https://open.spotify.com.evil.test/track/123",
            "http://open.spotify.com/track/123",
            "https://user@open.spotify.com/track/123",
            "https://open.spotify.com:444/track/123",
            "javascript:alert(1)",
        ] {
            assert!(!valid_url(Service::Spotify, bad));
        }
        let item = MusicItem {
            service: Service::Spotify,
            section: None,
            kind: "song".into(),
            title: " Song ".into(),
            artist: "Artist".into(),
            artwork: Some("https://127.0.0.1/private".into()),
            url: "https://open.spotify.com/track/123".into(),
        };
        assert_eq!(
            clean(vec![item.clone(), item.clone()], Service::Spotify, false).len(),
            2
        );
        let rows = clean(vec![item.clone(), item], Service::Spotify, true);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Song");
        assert!(rows[0].artwork.is_none());
    }
}
