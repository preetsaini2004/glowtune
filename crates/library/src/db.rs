use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::{Album, AlbumId, AudioFormat, Cover, Error, LibraryStats, Result, Track, TrackId};

/// A local music index. File parsing happens outside the database mutex.
#[derive(Debug)]
pub struct Library {
    connection: Mutex<Connection>,
    _db_path: Option<PathBuf>,
}

impl Library {
    /// Opens or creates an index, migrates its schema, and enables WAL and foreign keys.
    pub fn open(db_path: &Path) -> Result<Self> {
        Self::initialize(Connection::open(db_path)?, Some(db_path.to_owned()))
    }

    /// Opens a temporary in-memory index for tests and previews.
    pub fn open_in_memory() -> Result<Self> {
        Self::initialize(Connection::open_in_memory()?, None)
    }

    fn initialize(mut connection: Connection, path: Option<PathBuf>) -> Result<Self> {
        connection.busy_timeout(Duration::from_secs(5))?;
        connection.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")?;
        let version: i64 = connection.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version > 1 {
            return Err(Error::Db(rusqlite::Error::InvalidQuery));
        }
        if version == 0 {
            let tx = connection.transaction()?;
            tx.execute_batch(include_str!("schema.sql"))?;
            tx.commit()?;
        }
        Ok(Self {
            connection: Mutex::new(connection),
            _db_path: path,
        })
    }

    pub(crate) fn db(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
    }

    /// Adds a canonical directory path once. Symlinks within it are never scanned.
    pub fn add_folder(&self, path: &Path) -> Result<()> {
        let path = std::fs::canonicalize(path)?;
        if !path.is_dir() {
            return Err(Error::NotAFolder(path));
        }
        self.db().execute(
            "INSERT OR IGNORE INTO folders(path) VALUES (?)",
            [path_text(&path)?],
        )?;
        Ok(())
    }

    /// Removes an added folder and its owned tracks, then deletes empty albums.
    /// A missing directory can still be removed using its original absolute path.
    pub fn remove_folder(&self, path: &Path) -> Result<()> {
        let path = normalize(path)?;
        let mut db = self.db();
        let tx = db.transaction()?;
        tx.execute("DELETE FROM folders WHERE path=?", [path_text(&path)?])?;
        cleanup(&tx)?;
        tx.commit()?;
        Ok(())
    }

    /// Lists canonical added folders in path order.
    pub fn folders(&self) -> Result<Vec<PathBuf>> {
        Ok(self.roots()?.into_iter().map(|(_, path)| path).collect())
    }

    pub(crate) fn roots(&self) -> Result<Vec<(i64, PathBuf)>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT id,path FROM folders ORDER BY path")?;
        Ok(stmt
            .query_map([], |r| {
                Ok((r.get(0)?, PathBuf::from(r.get::<_, String>(1)?)))
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Searches all tag words using literal prefixes and diacritic-insensitive FTS5.
    /// Empty queries return nothing; results follow BM25 rank and the limit is capped at 500.
    pub fn search(&self, query: &str, limit: usize) -> Result<Vec<Track>> {
        let query = crate::query::fts_query(query);
        if query.is_empty() || limit == 0 {
            return Ok(Vec::new());
        }
        let db = self.db();
        let mut stmt = db.prepare("SELECT tracks.* FROM track_search JOIN tracks ON tracks.id=track_search.rowid WHERE track_search MATCH ? ORDER BY bm25(track_search),tracks.id LIMIT ?")?;
        Ok(stmt
            .query_map(params![query, limit.min(500) as i64], read_track)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Looks up one indexed track by identifier.
    pub fn track(&self, id: TrackId) -> Result<Option<Track>> {
        Ok(self
            .db()
            .query_row("SELECT * FROM tracks WHERE id=?", [id.0], read_track)
            .optional()?)
    }

    /// Looks up an indexed file, accepting relative or canonical paths.
    pub fn track_by_path(&self, path: &Path) -> Result<Option<Track>> {
        let path = normalize(path)?;
        Ok(self
            .db()
            .query_row(
                "SELECT * FROM tracks WHERE path=?",
                [path_text(&path)?],
                read_track,
            )
            .optional()?)
    }

    /// Lists albums ordered by case-insensitive album artist and album title.
    pub fn albums(&self) -> Result<Vec<Album>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT a.id,a.title,a.artist,MIN(t.year),COUNT(*),MIN(CASE WHEN t.has_cover THEN t.id END) FROM albums a JOIN tracks t ON t.album_id=a.id GROUP BY a.id ORDER BY a.artist_key,a.title_key,a.id")?;
        Ok(stmt
            .query_map([], |r| {
                Ok(Album {
                    id: AlbumId(r.get(0)?),
                    title: r.get(1)?,
                    artist: r.get(2)?,
                    year: r.get(3)?,
                    track_count: r.get(4)?,
                    cover_track: r.get::<_, Option<i64>>(5)?.map(TrackId),
                })
            })?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Lists an album's tracks ordered by disc, track number, and title.
    pub fn album_tracks(&self, album: AlbumId) -> Result<Vec<Track>> {
        let db = self.db();
        let mut stmt = db.prepare("SELECT * FROM tracks WHERE album_id=? ORDER BY disc_no,track_no,title COLLATE NOCASE,id")?;
        Ok(stmt
            .query_map([album.0], read_track)?
            .collect::<rusqlite::Result<_>>()?)
    }

    /// Loads embedded art (front preferred), then adjacent cover art, on demand.
    pub fn cover(&self, id: TrackId) -> Result<Option<Cover>> {
        self.track(id)?
            .map_or(Ok(None), |t| crate::tags::cover(&t.path))
    }

    /// Returns track count, album count, and summed known duration.
    pub fn stats(&self) -> Result<LibraryStats> {
        let db = self.db();
        let tracks =
            db.query_row("SELECT COUNT(*) FROM tracks", [], |r| r.get::<_, i64>(0))? as u64;
        let albums =
            db.query_row("SELECT COUNT(*) FROM albums", [], |r| r.get::<_, i64>(0))? as u64;
        let mut stmt =
            db.prepare("SELECT duration_ns FROM tracks WHERE duration_ns IS NOT NULL")?;
        let mut total_duration = Duration::ZERO;
        for duration in stmt.query_map([], |r| r.get::<_, i64>(0))? {
            total_duration = total_duration.saturating_add(Duration::from_nanos(duration? as u64));
        }
        Ok(LibraryStats {
            tracks,
            albums,
            total_duration,
        })
    }
}

pub(crate) fn cleanup(db: &Connection) -> rusqlite::Result<usize> {
    db.execute(
        "DELETE FROM albums WHERE NOT EXISTS (SELECT 1 FROM tracks WHERE album_id=albums.id)",
        [],
    )
}

pub(crate) fn path_text(path: &Path) -> Result<&str> {
    path.to_str().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "Index paths must be valid Unicode",
        )
        .into()
    })
}

pub(crate) fn normalize(path: &Path) -> Result<PathBuf> {
    match std::fs::canonicalize(path) {
        Ok(path) => Ok(path),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Resolve the nearest existing ancestor, including Windows verbatim prefixes.
            let absolute = std::path::absolute(path)?;
            let mut ancestor = absolute.as_path();
            let mut tail = Vec::new();
            while !ancestor.exists() {
                if let Some(name) = ancestor.file_name() {
                    tail.push(name.to_owned());
                } else {
                    break;
                }
                ancestor = ancestor.parent().unwrap_or(ancestor);
            }
            let mut resolved = std::fs::canonicalize(ancestor)?;
            for name in tail.into_iter().rev() {
                resolved.push(name);
            }
            Ok(resolved)
        }
        Err(e) => Err(e.into()),
    }
}

fn read_track(r: &Row<'_>) -> rusqlite::Result<Track> {
    Ok(Track {
        id: TrackId(r.get("id")?),
        path: PathBuf::from(r.get::<_, String>("path")?),
        title: r.get("title")?,
        artist: r.get("artist")?,
        album: r.get("album")?,
        album_artist: r.get("album_artist")?,
        album_id: AlbumId(r.get("album_id")?),
        track_no: r.get("track_no")?,
        disc_no: r.get("disc_no")?,
        year: r.get("year")?,
        duration: r
            .get::<_, Option<i64>>("duration_ns")?
            .map(|d| Duration::from_nanos(d as u64)),
        format: AudioFormat {
            codec: r.get("codec")?,
            lossless: r.get("lossless")?,
            bit_depth: r.get("bit_depth")?,
            sample_rate: r.get("sample_rate")?,
            channels: r.get("channels")?,
            bitrate_kbps: r.get("bitrate_kbps")?,
        },
        has_cover: r.get("has_cover")?,
    })
}
