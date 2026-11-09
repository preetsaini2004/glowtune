//! "Search everywhere" (desktop scene search box).
//!
//! ```text
//!  query ─► iTunes Search API (free, no key) ─► songs / artists / albums with artwork
//!  pick a result + a service ─► that service's own search page, inside Glowtune
//! ```
//! The catalog only supplies names and artwork for the results list; playback
//! always happens in the service the user picks (Spotify, Apple Music, YouTube
//! Music, SoundCloud), logged in with their own account.

use std::sync::OnceLock;
use std::time::Duration;

use serde::{Deserialize, Serialize};

const ENDPOINT: &str = "https://itunes.apple.com/search";
const USER_AGENT: &str = "Glowtune/0.1 (desktop music companion)";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchItem {
    /// "song" | "artist" | "album"
    pub kind: &'static str,
    pub title: String,
    pub subtitle: String,
    /// Square artwork (600 px), absent for most artists.
    pub artwork: Option<String>,
    /// What to type into a service's search box to find this item.
    pub query: String,
}

#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct SearchResults {
    pub songs: Vec<SearchItem>,
    pub artists: Vec<SearchItem>,
    pub albums: Vec<SearchItem>,
}

#[derive(Deserialize)]
struct Response {
    #[serde(default)]
    results: Vec<Entry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Entry {
    wrapper_type: Option<String>,
    kind: Option<String>,
    track_name: Option<String>,
    artist_name: Option<String>,
    collection_name: Option<String>,
    artwork_url100: Option<String>,
    primary_genre_name: Option<String>,
}

fn client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(Duration::from_secs(8))
            .build()
            .unwrap_or_default()
    })
}

async fn fetch(term: &str, entity: &str, limit: u32) -> Result<Vec<Entry>, String> {
    let url = format!(
        "{ENDPOINT}?term={}&media=music&entity={entity}&limit={limit}",
        encode(term)
    );
    let resp = client()
        .get(url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let body: Response = resp.json().await.map_err(|e| e.to_string())?;
    Ok(body.results)
}

/// 100 px artwork → 600 px (the API serves any size from the same path).
fn big_art(url: Option<String>) -> Option<String> {
    url.map(|u| u.replace("100x100bb", "600x600bb"))
}

pub async fn search(query: &str) -> Result<SearchResults, String> {
    let q = query.trim();
    if q.is_empty() {
        return Ok(SearchResults::default());
    }
    let q: String = q.chars().take(120).collect();
    let (songs, artists, albums) = tokio::join!(
        fetch(&q, "song", 8),
        fetch(&q, "musicArtist", 6),
        fetch(&q, "album", 6)
    );
    let songs = songs?;
    let mut out = SearchResults::default();
    for e in songs {
        if e.wrapper_type.as_deref() != Some("track") || e.kind.as_deref() != Some("song") {
            continue;
        }
        let (Some(title), Some(artist)) = (e.track_name, e.artist_name) else {
            continue;
        };
        out.songs.push(SearchItem {
            kind: "song",
            query: format!("{title} {artist}"),
            subtitle: artist,
            title,
            artwork: big_art(e.artwork_url100),
        });
    }
    // Artists have no artwork in the API: borrow it from one of their songs/albums.
    let albums = albums.unwrap_or_default();
    for e in artists.unwrap_or_default() {
        let Some(name) = e.artist_name else { continue };
        let artwork = out
            .songs
            .iter()
            .find(|s| s.subtitle.eq_ignore_ascii_case(&name))
            .and_then(|s| s.artwork.clone())
            .or_else(|| {
                albums
                    .iter()
                    .find(|a| a.artist_name.as_deref() == Some(name.as_str()))
                    .and_then(|a| big_art(a.artwork_url100.clone()))
            });
        out.artists.push(SearchItem {
            kind: "artist",
            subtitle: e.primary_genre_name.unwrap_or_default(),
            query: name.clone(),
            title: name,
            artwork,
        });
    }
    for e in albums {
        let (Some(title), Some(artist)) = (e.collection_name, e.artist_name) else {
            continue;
        };
        out.albums.push(SearchItem {
            kind: "album",
            query: format!("{title} {artist}"),
            subtitle: artist,
            title,
            artwork: big_art(e.artwork_url100),
        });
    }
    Ok(out)
}

/// Percent-encodes a query for a URL path segment or query value.
pub fn encode(q: &str) -> String {
    let mut out = String::with_capacity(q.len() * 3);
    for b in q.trim().bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_everything_unsafe() {
        assert_eq!(
            encode(" Beyoncé & Jay-Z/ "),
            "Beyonc%C3%A9%20%26%20Jay-Z%2F"
        );
        assert_eq!(encode("a?b#c"), "a%3Fb%23c");
    }

    #[test]
    fn upsizes_artwork() {
        assert_eq!(
            big_art(Some("https://x/100x100bb.jpg".into())).as_deref(),
            Some("https://x/600x600bb.jpg")
        );
    }
}

/// Finds the song's music video on YouTube (for the scene's video wallpaper).
/// Uses YouTube's own web search endpoint (no key); prefers official videos and
/// visualizers whose length matches the song, and skips loops, mashups, slowed
/// or reverb edits. `None` when nothing looks right.
pub async fn music_video(
    title: &str,
    artist: &str,
    duration_ms: Option<f64>,
) -> Result<Option<String>, String> {
    let query = format!("{artist} {title} official video");
    let body = serde_json::json!({
        "context": { "client": { "clientName": "WEB", "clientVersion": "2.20251101.00.00", "hl": "en" } },
        "query": query,
        "params": "EgIQAQ%3D%3D", // videos only
    });
    let resp = client()
        .post("https://www.youtube.com/youtubei/v1/search?prettyPrint=false")
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?;
    let json: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;
    let mut videos = Vec::new();
    collect_videos(&json, &mut videos);
    Ok(best_video(&videos, title, artist, duration_ms))
}

#[derive(Debug, Clone, PartialEq)]
struct Video {
    id: String,
    title: String,
    channel: String,
    secs: Option<u32>,
}

fn collect_videos(v: &serde_json::Value, out: &mut Vec<Video>) {
    match v {
        serde_json::Value::Object(map) => {
            if let Some(r) = map.get("videoRenderer") {
                let id = r.get("videoId").and_then(|x| x.as_str());
                let title = r
                    .pointer("/title/runs/0/text")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default();
                let channel = r
                    .pointer("/ownerText/runs/0/text")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default();
                let secs = r
                    .pointer("/lengthText/simpleText")
                    .and_then(|x| x.as_str())
                    .and_then(parse_length);
                // Shorts are vertical clips with their own overlay buttons.
                let short = r.pointer("/navigationEndpoint/reelWatchEndpoint").is_some()
                    || secs.is_some_and(|s| s < 60);
                if short {
                    // skip
                } else if let Some(id) = id {
                    out.push(Video {
                        id: id.to_owned(),
                        title: title.to_owned(),
                        channel: channel.to_owned(),
                        secs,
                    });
                }
            }
            for x in map.values() {
                collect_videos(x, out);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(|x| collect_videos(x, out)),
        _ => {}
    }
}

/// "3:21" / "1:02:03" → seconds.
fn parse_length(s: &str) -> Option<u32> {
    s.split(':').try_fold(0u32, |acc, p| {
        Some(acc * 60 + p.trim().parse::<u32>().ok()?)
    })
}

fn best_video(
    videos: &[Video],
    title: &str,
    artist: &str,
    duration_ms: Option<f64>,
) -> Option<String> {
    let want = title.to_lowercase();
    // Main artist: before "," "&" " x " " feat" (Spotify lists every artist).
    let lead = artist
        .to_lowercase()
        .split([',', '&'])
        .next()
        .unwrap_or_default()
        .split(" x ")
        .next()
        .unwrap_or_default()
        .split(" feat")
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    let core = want
        .split(['(', '[', '-'])
        .next()
        .unwrap_or(&want)
        .trim()
        .to_owned();
    let score = |v: &Video| -> i32 {
        let t = v.title.to_lowercase();
        let mut s = 0;
        // Same song name by another artist is the classic wrong pick.
        if !lead.is_empty() {
            // The artist's own channel is the strongest sign of the real video.
            if v.channel.to_lowercase().contains(&lead) {
                s += 6;
            } else if t.contains(&lead) {
                s += 3;
            } else {
                s -= 6;
            }
        }
        if !core.is_empty() && t.contains(&core) {
            s += 4;
        } else {
            s -= 4;
        }
        if t.contains("official video") || t.contains("official music video") {
            s += 3;
        } else if t.contains("visualizer")
            || t.contains("visualiser")
            || t.contains("official visual")
        {
            s += 2;
        } else if t.contains("video") {
            s += 1;
        }
        for bad in [
            "hour",
            "loop",
            "mashup",
            "slowed",
            "reverb",
            "8d",
            "cover",
            "karaoke",
            "nightcore",
            "sped up",
            "reaction",
            "react",
            "live",
            "stream",
            "podcast",
            "interview",
            "#shorts",
            "shorts",
        ] {
            if t.contains(bad) && !want.contains(bad) {
                s -= 5;
            }
        }
        if t.contains("lyric") {
            s -= 1;
        }
        if let (Some(secs), Some(ms)) = (v.secs, duration_ms) {
            let diff = (secs as f64 - ms / 1000.0).abs();
            if diff <= 25.0 {
                s += 2;
            } else if diff > 120.0 {
                s -= 3;
            }
        }
        s
    };
    videos
        .iter()
        .take(12)
        .map(|v| (score(v), v))
        .filter(|(s, _)| *s >= 3)
        .max_by_key(|(s, _)| *s)
        .map(|(_, v)| v.id.clone())
}

#[cfg(test)]
mod video_tests {
    use super::*;

    fn v(id: &str, title: &str, secs: u32) -> Video {
        Video {
            id: id.into(),
            title: title.into(),
            channel: String::new(),
            secs: Some(secs),
        }
    }

    #[test]
    fn picks_the_official_video_and_skips_edits() {
        let list = [
            v("loop", "IYKYK (Visualizer) - Jxggi | 1 HOUR LOOP", 3600),
            v("mash", "IYKYK - Jxggi x Someone | Mashup", 200),
            v(
                "real",
                "IYKYK (Visualizer) - Jxggi, Dishant, Sickboi | Inferno",
                156,
            ),
            v("other", "Some other song (Official Video)", 155),
        ];
        assert_eq!(
            best_video(&list, "IYKYK", "Jxggi, Dishant", Some(155_000.0)).as_deref(),
            Some("real")
        );
        assert_eq!(
            best_video(&list[..2], "IYKYK", "Jxggi", Some(155_000.0)),
            None
        );
    }

    #[test]
    fn lengths_parse() {
        assert_eq!(parse_length("3:21"), Some(201));
        assert_eq!(parse_length("1:02:03"), Some(3723));
        assert_eq!(parse_length("live"), None);
    }
}

/// [`music_video`] with an in-memory cache: one lookup per song per session.
pub async fn music_video_cached(
    title: &str,
    artist: &str,
    duration_ms: Option<f64>,
) -> Result<Option<String>, String> {
    use std::collections::HashMap;
    use std::sync::Mutex;
    static CACHE: OnceLock<Mutex<HashMap<String, Option<String>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = format!("{}\u{1f}{}", title.to_lowercase(), artist.to_lowercase());
    if let Some(hit) = cache.lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(hit);
    }
    let found = music_video(title, artist, duration_ms).await?;
    if let Ok(mut c) = cache.lock() {
        c.insert(key, found.clone());
    }
    Ok(found)
}

#[cfg(test)]
mod artist_tests {
    use super::*;

    #[test]
    fn same_title_by_another_artist_loses() {
        let list = [
            Video {
                id: "pump".into(),
                title: "Lil Pump - Racks on Racks [Official Music Video]".into(),
                channel: "Lil Pump".into(),
                secs: Some(149),
            },
            Video {
                id: "navaan".into(),
                title: "Racks on Racks (Official Visuals) Navaan Sandhu | Rxtro".into(),
                channel: "Navaan Sandhu".into(),
                secs: Some(192),
            },
        ];
        assert_eq!(
            best_video(&list, "Racks On Racks", "Navaan Sandhu", Some(191_000.0)).as_deref(),
            Some("navaan")
        );
        assert_eq!(
            best_video(
                &list[..1],
                "Racks On Racks",
                "Navaan Sandhu",
                Some(191_000.0)
            ),
            None
        );
    }
}

#[cfg(test)]
mod reaction_tests {
    use super::*;

    #[test]
    fn official_channel_beats_a_reaction_stream() {
        let list = [
            Video {
                id: "react".into(),
                title: "Gagan Kooner 26 Freestyle REACTION live stream".into(),
                channel: "Some Streamer".into(),
                secs: Some(160),
            },
            Video {
                id: "real".into(),
                title: "26 Freestyle".into(),
                channel: "Gagan Kooner".into(),
                secs: Some(157),
            },
        ];
        assert_eq!(
            best_video(&list, "26 Freestyle", "Gagan Kooner", Some(157_000.0)).as_deref(),
            Some("real")
        );
        assert_eq!(
            best_video(&list[..1], "26 Freestyle", "Gagan Kooner", Some(157_000.0)),
            None
        );
    }
}
