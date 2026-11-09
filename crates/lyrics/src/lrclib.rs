use std::{cmp::Ordering, time::Duration};

use reqwest::{Client, RequestBuilder, StatusCode, Url};
use serde::Deserialize;

use crate::{Error, Result, clean_query};

const USER_AGENT: &str = "Glowtune/0.1 (+https://lrclib.net client)";

/// A reusable HTTP client for LRCLIB, with an eight-second request timeout.
#[derive(Debug, Clone)]
pub struct LrclibClient {
    client: Client,
    base_url: Url,
}

/// A track returned by LRCLIB, including instrumental and untimed results.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LrclibRecord {
    /// LRCLIB record identifier.
    pub id: i64,
    /// Track title reported by LRCLIB.
    pub track_name: String,
    /// Artist reported by LRCLIB.
    pub artist_name: String,
    /// Album name, when available.
    pub album_name: Option<String>,
    /// Track duration in seconds, when available.
    pub duration: Option<f64>,
    /// Whether the track is confirmed to have no lyrics.
    pub instrumental: bool,
    /// Plain lyrics, when available.
    pub plain_lyrics: Option<String>,
    /// Synced LRC text, when available.
    pub synced_lyrics: Option<String>,
}

impl LrclibClient {
    /// Creates a client for `https://lrclib.net` with Glowtune's public User-Agent.
    pub fn new() -> Result<Self> {
        Self::with_base_url("https://lrclib.net")
    }

    /// Creates a client for an HTTP(S) base URL, optionally including a path prefix.
    /// Query strings, fragments, missing hosts, and embedded credentials are rejected.
    pub fn with_base_url(url: &str) -> Result<Self> {
        let parsed = Url::parse(url).map_err(|_| Error::InvalidBaseUrl(url.to_owned()))?;
        if !matches!(parsed.scheme(), "http" | "https")
            || parsed.host_str().is_none()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || !parsed.username().is_empty()
            || parsed.password().is_some()
        {
            return Err(Error::InvalidBaseUrl(url.to_owned()));
        }
        let client = Client::builder()
            .timeout(Duration::from_secs(8))
            .user_agent(USER_AGENT)
            .build()?;
        Ok(Self {
            client,
            base_url: parsed,
        })
    }

    /// Looks up exact metadata, sending optional album and whole-second duration parameters.
    /// A 404 is a normal miss; other unsuccessful statuses return an HTTP error.
    pub async fn get(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration: Option<Duration>,
    ) -> Result<Option<LrclibRecord>> {
        let response = self
            .get_request(title, artist, album, duration)
            .send()
            .await?;
        if response.status() == StatusCode::NOT_FOUND {
            return Ok(None);
        }
        Ok(Some(response.error_for_status()?.json().await?))
    }

    fn get_request(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration: Option<Duration>,
    ) -> RequestBuilder {
        let mut url = self.metadata_url("get", title, artist);
        if let Some(album) = album {
            url.query_pairs_mut().append_pair("album_name", album);
        }
        if let Some(duration) = duration {
            url.query_pairs_mut()
                .append_pair("duration", &duration.as_secs().to_string());
        }
        self.client.get(url)
    }

    fn metadata_request(&self, endpoint: &str, title: &str, artist: &str) -> RequestBuilder {
        self.client.get(self.metadata_url(endpoint, title, artist))
    }

    fn metadata_url(&self, endpoint: &str, title: &str, artist: &str) -> Url {
        let mut url = self.base_url.clone();
        let path = format!("{}/api/{endpoint}", url.path().trim_end_matches('/'));
        url.set_path(&path);
        // shortcut: use URL encoding until reqwest's `query` feature and lockfile update are allowed.
        url.query_pairs_mut()
            .append_pair("track_name", title)
            .append_pair("artist_name", artist);
        url
    }

    /// Searches LRCLIB by title and artist, preserving the service's result order.
    pub async fn search(&self, title: &str, artist: &str) -> Result<Vec<LrclibRecord>> {
        Ok(self
            .metadata_request("search", title, artist)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?)
    }

    /// Cleans metadata, tries an exact lookup, retries without an album, then ranks search results.
    /// Request and decoding errors propagate instead of being treated as missing lyrics.
    pub async fn find(
        &self,
        title: &str,
        artist: &str,
        album: Option<&str>,
        duration: Option<Duration>,
    ) -> Result<Option<LrclibRecord>> {
        let query = clean_query(title, artist);
        if let Some(record) = self
            .get(&query.title, &query.artist, album, duration)
            .await?
        {
            return Ok(Some(record));
        }
        if album.is_some()
            && let Some(record) = self
                .get(&query.title, &query.artist, None, duration)
                .await?
        {
            return Ok(Some(record));
        }
        let records = self.search(&query.title, &query.artist).await?;
        Ok(pick_best(&records, duration).cloned())
    }
}

/// Selects a record within three seconds of a known target duration.
/// Prefers nonempty synced lyrics, then closest known duration, then original order.
/// Unknown durations remain eligible; instrumental results are valid.
pub fn pick_best(records: &[LrclibRecord], duration: Option<Duration>) -> Option<&LrclibRecord> {
    let target = duration.map(|duration| duration.as_secs_f64());
    let distance = |record: &LrclibRecord| match (target, record.duration) {
        (Some(target), Some(duration)) => (duration - target).abs(),
        (Some(_), None) => f64::INFINITY,
        (None, _) => 0.0,
    };
    let synced = |record: &LrclibRecord| {
        record
            .synced_lyrics
            .as_ref()
            .is_some_and(|text| !text.trim().is_empty())
    };
    records
        .iter()
        .filter(|record| target.is_none() || record.duration.is_none() || distance(record) <= 3.0)
        .min_by(|left, right| {
            synced(right).cmp(&synced(left)).then_with(|| {
                distance(left)
                    .partial_cmp(&distance(right))
                    .unwrap_or(Ordering::Equal)
            })
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> Vec<LrclibRecord> {
        serde_json::from_str(r#"[
            {"id":1,"trackName":"Blinding Lights","artistName":"The Weeknd","albumName":"After Hours","duration":200,"instrumental":false,"plainLyrics":"I've been tryna call","syncedLyrics":null},
            {"id":2,"trackName":"Blinding Lights","artistName":"The Weeknd","albumName":null,"duration":202,"instrumental":false,"plainLyrics":null,"syncedLyrics":"[00:01.00]I've been tryna call"},
            {"id":3,"trackName":"Blinding Lights","artistName":"The Weeknd","albumName":"After Hours","duration":201,"instrumental":false,"plainLyrics":"Text","syncedLyrics":"[00:01]Text"},
            {"id":4,"trackName":"Blinding Lights","artistName":"The Weeknd","albumName":null,"duration":201,"instrumental":false,"plainLyrics":null,"syncedLyrics":"[00:01]Tie"},
            {"id":5,"trackName":"Blinding Lights","artistName":"The Weeknd","albumName":null,"duration":210,"instrumental":false,"plainLyrics":null,"syncedLyrics":"[00:01]Wrong version"},
            {"id":6,"trackName":"Instrumental","artistName":"Artist","albumName":null,"duration":null,"instrumental":true,"plainLyrics":null,"syncedLyrics":null}
        ]"#).expect("valid fixture")
    }

    #[test]
    fn deserializes_realistic_camel_case_and_nulls() {
        let records = fixtures();
        assert_eq!(records[0].track_name, "Blinding Lights");
        assert_eq!(records[0].artist_name, "The Weeknd");
        assert_eq!(records[0].album_name.as_deref(), Some("After Hours"));
        assert_eq!(records[0].duration, Some(200.0));
        assert!(records[0].synced_lyrics.is_none());
        assert!(records[5].instrumental);
        assert!(records[5].duration.is_none());
        assert!(records[5].album_name.is_none());
        assert!(records[5].plain_lyrics.is_none());
    }

    #[test]
    fn ranking_prefers_synced_then_distance_then_first() {
        let records = fixtures();
        assert_eq!(
            pick_best(&records, Some(Duration::from_secs(200))).map(|r| r.id),
            Some(3)
        );
        assert_eq!(pick_best(&records, None).map(|r| r.id), Some(2));
        assert_eq!(
            pick_best(&records[..1], Some(Duration::from_secs(200))).map(|r| r.id),
            Some(1)
        );
        assert!(pick_best(&[], None).is_none());
    }

    #[test]
    fn duration_filter_boundary_unknown_and_instrumental() {
        let records = fixtures();
        assert!(pick_best(&records[..5], Some(Duration::from_secs(220))).is_none());
        assert!(
            pick_best(&records[5..], Some(Duration::from_secs(200)))
                .is_some_and(|r| r.instrumental)
        );
        assert!(pick_best(&records[..1], Some(Duration::from_secs(203))).is_some());
        assert!(pick_best(&records[..1], Some(Duration::from_millis(203001))).is_none());
        let mut records = fixtures();
        records[1].synced_lyrics = Some("  ".into());
        assert_eq!(
            pick_best(&records[..2], Some(Duration::from_secs(200))).map(|r| r.id),
            Some(1)
        );
        records[1].synced_lyrics = Some("[00:01]Text".into());
        records[1].duration = None;
        assert_eq!(
            pick_best(&records, Some(Duration::from_secs(200))).map(|r| r.id),
            Some(3)
        );
        records[1].duration = Some(f64::NAN);
        assert!(pick_best(&records[1..2], Some(Duration::from_secs(200))).is_none());
    }

    #[test]
    fn requests_encode_metadata_and_omit_missing_parameters() {
        let client = LrclibClient::with_base_url("http://localhost:1234/prefix/").expect("client");
        let request = client
            .get_request(
                "愛 & A/B?",
                "A + B",
                Some("Album #1"),
                Some(Duration::from_millis(200999)),
            )
            .build()
            .expect("request");
        assert_eq!(request.url().path(), "/prefix/api/get");
        let pairs: Vec<_> = request.url().query_pairs().collect();
        assert_eq!(
            pairs,
            [
                ("track_name".into(), "愛 & A/B?".into()),
                ("artist_name".into(), "A + B".into()),
                ("album_name".into(), "Album #1".into()),
                ("duration".into(), "200".into())
            ]
        );
        assert!(request.url().as_str().contains("%26"));
        let search = client
            .metadata_request("search", "Title", "Artist")
            .build()
            .expect("search request");
        assert_eq!(search.url().path(), "/prefix/api/search");
        assert_eq!(search.url().query_pairs().count(), 2);
        let get = client
            .get_request("Title", "Artist", None, None)
            .build()
            .expect("get request");
        assert_eq!(get.url().query_pairs().count(), 2);
    }

    #[test]
    fn rejects_invalid_base_urls() {
        for url in [
            "not a URL",
            "file:///tmp",
            "https://lrclib.net?q=x",
            "https://lrclib.net/#x",
            "http://user:pass@localhost/",
        ] {
            assert!(matches!(
                LrclibClient::with_base_url(url),
                Err(Error::InvalidBaseUrl(_))
            ));
        }
    }

    #[tokio::test]
    #[ignore = "requires a live LRCLIB connection"]
    async fn finds_blinding_lights_online() {
        let record = LrclibClient::new()
            .expect("client")
            .find(
                "Blinding Lights",
                "The Weeknd",
                None,
                Some(Duration::from_secs(200)),
            )
            .await
            .expect("LRCLIB response")
            .expect("matching record");
        assert!(
            record
                .synced_lyrics
                .is_some_and(|text| !text.trim().is_empty())
        );
    }
}
