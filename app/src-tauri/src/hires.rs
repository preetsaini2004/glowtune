//! A big copy of the current song's cover, for backgrounds that show the art
//! sharp across the whole screen (Windows only hands Glowtune a 150 px
//! thumbnail).
//!
//! ```text
//!  service page's media session ─► its biggest cover (Spotify 640 px, YouTube
//!                                   Music any size)
//!  iTunes catalog (same title and artist) ─► up to 2000 px
//!  ─► downloaded once ─► the art store ─► glowart://<key> (CORS-readable, so
//!                                          WebGL can draw it)
//! ```
//! Precision first: a catalog cover is used only when both the title and the
//! artist match; otherwise the page's own cover, else nothing (the small one).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use tauri::{AppHandle, Manager};

use crate::hub::Shared;

/// Covers bigger than this are left at their own size (2000 px is plenty for
/// a 1440 p screen and stays well under a megabyte).
const CATALOG_PX: u32 = 2000;
/// Smaller covers from the page aren't worth the download.
const PAGE_MIN_PX: u32 = 500;
const MAX_BYTES: usize = 8 * 1024 * 1024;

/// Cache: song → art URL (or None = nothing better found).
fn cache() -> &'static Mutex<HashMap<String, Option<String>>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub async fn hires_art(app: &AppHandle, title: &str, artist: &str) -> Option<String> {
    let key = format!("{}\u{1f}{}", title.to_lowercase(), artist.to_lowercase());
    if let Some(hit) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        // Still in the art store (it keeps the last two dozen)?
        if hit.as_deref().is_none_or(|u| served(app, u)) {
            return hit;
        }
    }
    let page = page_cover(app, title).await;
    let candidates = match page {
        Some((url, px)) if px >= 1000 => vec![url],
        Some((url, px)) => {
            let mut list: Vec<String> = catalog_cover(title, artist).await.into_iter().collect();
            if px >= PAGE_MIN_PX {
                list.push(url);
            }
            list
        }
        None => catalog_cover(title, artist).await.into_iter().collect(),
    };
    let mut found = None;
    for url in candidates {
        if let Some(served) = download(app, &url).await {
            tracing::info!("big cover for {title}: {url}");
            found = Some(served);
            break;
        }
    }
    if found.is_none() {
        tracing::info!("big cover for {title}: none found, keeping the small one");
    }
    if let Ok(mut c) = cache().lock() {
        c.insert(key, found.clone());
    }
    found
}

fn served(app: &AppHandle, url: &str) -> bool {
    let Some(key) = url
        .rsplit('/')
        .next()
        .and_then(|k| u64::from_str_radix(k, 16).ok())
    else {
        return false;
    };
    app.try_state::<Arc<Shared>>()
        .and_then(|s| s.art.lock().ok().map(|a| a.get(key).is_some()))
        .unwrap_or(false)
}

/// The biggest cover the service page's media session offers, if the page
/// is playing this song: (URL, pixels across).
async fn page_cover(app: &AppHandle, title: &str) -> Option<(String, u32)> {
    let window = app.get_webview_window(crate::services::SERVICE)?;
    let js = r#"(() => {
  const m = navigator.mediaSession && navigator.mediaSession.metadata;
  if (!m) return null;
  return { title: m.title || "", art: (m.artwork || []).map(a => [a.src || "", a.sizes || ""]) };
})()"#
        .to_string();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    crate::services::run_script(&window, js, tx).ok()?;
    let out = tauri::async_runtime::spawn_blocking(move || {
        rx.recv_timeout(Duration::from_millis(1500)).ok()
    })
    .await
    .ok()
    .flatten()?;
    #[derive(serde::Deserialize)]
    struct Session {
        title: String,
        art: Vec<(String, String)>,
    }
    let s: Session = serde_json::from_str::<Option<Session>>(&out)
        .ok()
        .flatten()?;
    if norm(&s.title) != norm(title) {
        return None;
    }
    s.art
        .into_iter()
        .filter_map(|(src, sizes)| {
            let px = sizes
                .split_whitespace()
                .filter_map(|s| s.split(['x', 'X']).next()?.parse::<u32>().ok())
                .max()
                .unwrap_or(0);
            Some(bigger_page_url(&src, px)).filter(|(u, _)| allowed(u))
        })
        .max_by_key(|(_, px)| *px)
}

/// YouTube Music's covers come at any size: ask for a big one.
fn bigger_page_url(src: &str, px: u32) -> (String, u32) {
    if src.contains("googleusercontent.com/")
        && let Some(at) = src.rfind("=w")
    {
        return (format!("{}=w1600-h1600-l90-rj", &src[..at]), 1600);
    }
    (src.to_string(), px)
}

#[derive(serde::Deserialize)]
struct Response {
    #[serde(default)]
    results: Vec<Entry>,
}
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    track_name: Option<String>,
    artist_name: Option<String>,
    artwork_url100: Option<String>,
}

/// The catalog's cover for this exact song (title and artist both match).
async fn catalog_cover(title: &str, artist: &str) -> Option<String> {
    let lead = lead_artist(artist);
    let url = format!(
        "https://itunes.apple.com/search?term={}&media=music&entity=song&limit=10",
        crate::search::encode(&format!("{} {lead}", core_title(title)))
    );
    let body: Response = client()
        .get(url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?
        .json()
        .await
        .ok()?;
    pick(&body.results, title, artist)
}

fn pick(results: &[Entry], title: &str, artist: &str) -> Option<String> {
    let want_title = norm(&core_title(title));
    let want_artist = norm(&lead_artist(artist));
    if want_title.is_empty() || want_artist.is_empty() {
        return None;
    }
    results.iter().find_map(|e| {
        let t = norm(&core_title(e.track_name.as_deref()?));
        let a = norm(e.artist_name.as_deref()?);
        let title_ok = t == want_title;
        let artist_ok = !a.is_empty() && (a.contains(&want_artist) || want_artist.contains(&a));
        if !(title_ok && artist_ok) {
            return None;
        }
        Some(
            e.artwork_url100
                .as_deref()?
                .replace("100x100bb", &format!("{CATALOG_PX}x{CATALOG_PX}bb")),
        )
    })
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent("Glowtune/0.1 (desktop music companion)")
            .timeout(Duration::from_secs(10))
            .build()
            .unwrap_or_default()
    })
}

/// Only image hosts of the services and the catalog.
fn allowed(url: &str) -> bool {
    let Some(rest) = url.strip_prefix("https://") else {
        return false;
    };
    let host = rest.split('/').next().unwrap_or("");
    host == "i.scdn.co"
        || host.ends_with(".mzstatic.com")
        || host.ends_with(".googleusercontent.com")
        || host == "i.ytimg.com"
        || host.ends_with(".sndcdn.com")
}

/// Downloads an image into the art store; its glowart URL.
async fn download(app: &AppHandle, url: &str) -> Option<String> {
    if !allowed(url) {
        return None;
    }
    let resp = client()
        .get(url)
        .send()
        .await
        .ok()?
        .error_for_status()
        .ok()?;
    let mime = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    if !mime.starts_with("image/") {
        return None;
    }
    let bytes = resp.bytes().await.ok()?;
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return None;
    }
    let key = {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        h.finish()
    };
    let shared = app.try_state::<Arc<Shared>>()?;
    shared
        .art
        .lock()
        .ok()?
        .insert(key, Arc::from(&bytes[..]), mime);
    Some(format!("{}/{key:x}", crate::ART_ORIGIN))
}

/// Lowercase letters and digits only.
fn norm(s: &str) -> String {
    s.chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

/// The title without "(feat. …)", "- Remastered" and the like.
fn core_title(title: &str) -> String {
    let cut = title
        .find(" (")
        .into_iter()
        .chain(title.find(" ["))
        .chain(title.find(" - "))
        .min()
        .unwrap_or(title.len());
    title[..cut].trim().to_string()
}

/// The first artist of "A, B & C feat. D".
fn lead_artist(artist: &str) -> String {
    let lower = artist.to_lowercase();
    let cut = [",", " & ", " feat", " ft.", " x ", " and "]
        .iter()
        .filter_map(|sep| lower.find(sep))
        .min()
        .unwrap_or(artist.len());
    artist[..cut].trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(t: &str, a: &str) -> Entry {
        Entry {
            track_name: Some(t.into()),
            artist_name: Some(a.into()),
            artwork_url100: Some(format!("https://is1-ssl.mzstatic.com/{t}/100x100bb.jpg")),
        }
    }

    #[test]
    fn the_catalog_cover_must_be_the_same_song_by_the_same_artist() {
        let results = [
            entry("Royal Oak", "Somebody Else"),
            entry("Royal Oak (feat. Guest)", "Lithe"),
        ];
        let got = pick(&results, "Royal Oak", "Lithe, Other").unwrap();
        assert!(got.contains("Royal Oak (feat. Guest)/2000x2000bb"), "{got}");
        assert_eq!(pick(&results[..1], "Royal Oak", "Lithe"), None);
        assert_eq!(
            pick(
                &[entry("Royal Oak Remix Pack", "Lithe")],
                "Royal Oak",
                "Lithe"
            ),
            None
        );
    }

    #[test]
    fn titles_and_artists_are_reduced_to_their_core() {
        assert_eq!(core_title("New Trip (feat. Yeat & BNYX®)"), "New Trip");
        assert_eq!(core_title("Song - Remastered 2011"), "Song");
        assert_eq!(lead_artist("Quavo, Yeat, BNYX®"), "Quavo");
        assert_eq!(lead_artist("Lil Baby & Travis Scott"), "Lil Baby");
        assert_eq!(lead_artist("Gunna"), "Gunna");
    }

    #[test]
    fn only_known_image_hosts_are_downloaded() {
        assert!(allowed("https://i.scdn.co/image/ab67616d0000b273"));
        assert!(allowed(
            "https://is3-ssl.mzstatic.com/image/x/2000x2000bb.jpg"
        ));
        assert!(!allowed("http://i.scdn.co/x"));
        assert!(!allowed("https://evil.example/i.scdn.co"));
        let (u, px) = bigger_page_url(
            "https://lh3.googleusercontent.com/abc=w544-h544-l90-rj",
            544,
        );
        assert_eq!(
            (u.as_str(), px),
            (
                "https://lh3.googleusercontent.com/abc=w1600-h1600-l90-rj",
                1600
            )
        );
    }
}
