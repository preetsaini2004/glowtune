use std::{
    collections::HashSet,
    fs, io,
    path::{Path, PathBuf},
    sync::atomic::{AtomicBool, Ordering},
    time::UNIX_EPOCH,
};

use rusqlite::{OptionalExtension, params};

use crate::{
    Error, Library, Result, ScanProgress, ScanReport, Track,
    db::{cleanup, normalize, path_text},
};

enum Change {
    Put {
        folder: i64,
        track: Box<Track>,
        size: i64,
        mtime: String,
    },
    Delete(PathBuf),
}

impl Library {
    /// Recursively scans all added roots, or one added root, without following symlinks.
    /// Hidden subdirectories and nonaudio files are skipped. Unchanged size/mtime pairs
    /// avoid tag reads; failures are reported without losing prior metadata. Writes commit
    /// in batches of at most 500 files. Cancellation is checked between files; progress
    /// is delivered every 100 files, outside the database mutex.
    pub fn scan(
        &self,
        only: Option<&Path>,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(ScanProgress),
    ) -> Result<ScanReport> {
        let roots = self.roots()?;
        let seeds = if let Some(path) = only {
            let path = normalize(path)?;
            if !roots.iter().any(|(_, root)| *root == path) {
                return Err(Error::NotAFolder(path));
            }
            vec![path]
        } else {
            roots.iter().map(|(_, root)| root.clone()).collect()
        };
        self.scan_paths(&seeds, &roots, false, cancel, progress)
    }

    /// Applies changed files or directories inside added roots, including deleted paths.
    /// Cover-image changes refresh neighboring tracks. Duplicate paths are processed once.
    pub fn apply_changes(&self, paths: &[PathBuf]) -> Result<ScanReport> {
        let roots = self.roots()?;
        let mut seeds = Vec::new();
        let mut failed = Vec::new();
        for path in paths {
            let resolved = (|| -> Result<Option<PathBuf>> {
                if has_symlink(path)? {
                    return Ok(None);
                }
                Ok(Some(normalize(path)?))
            })();
            match resolved {
                Ok(Some(path)) if owner(&path, &roots).is_some() => {
                    if is_cover(&path) {
                        if let Some(parent) = path.parent() {
                            seeds.push(parent.to_owned());
                        }
                    } else {
                        seeds.push(path);
                    }
                }
                Ok(_) => (),
                Err(e) => failed.push((path.clone(), e.to_string())),
            }
        }
        let mut report =
            self.scan_paths(&seeds, &roots, true, &AtomicBool::new(false), &mut |_| {})?;
        report.failed.extend(failed);
        Ok(report)
    }

    fn scan_paths(
        &self,
        seeds: &[PathBuf],
        roots: &[(i64, PathBuf)],
        force: bool,
        cancel: &AtomicBool,
        progress: &mut dyn FnMut(ScanProgress),
    ) -> Result<ScanReport> {
        let mut report = ScanReport::default();
        let mut pending = Vec::new();
        let mut seen = HashSet::new();
        let mut stack = seeds.to_vec();
        let mut count = 0;
        while let Some(path) = stack.pop() {
            if cancel.load(Ordering::Relaxed) {
                report.cancelled = true;
                break;
            }
            if !seen.insert(path.clone()) {
                continue;
            }
            let Some((folder, root)) = owner(&path, roots) else {
                continue;
            };
            let meta = match fs::symlink_metadata(&path) {
                Ok(meta) => meta,
                Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
                Err(e) => {
                    report.failed.push((path, e.to_string()));
                    continue;
                }
            };
            if meta.is_symlink() || hidden(&path, root, meta.is_dir()) {
                continue;
            }
            if meta.is_dir() {
                match fs::read_dir(&path) {
                    Ok(entries) => {
                        for entry in entries {
                            match entry {
                                Ok(entry) => stack.push(entry.path()),
                                Err(e) => report.failed.push((path.clone(), e.to_string())),
                            }
                        }
                    }
                    Err(e) => report.failed.push((path, e.to_string())),
                }
            } else if meta.is_file() && audio(&path) {
                count += 1;
                let result = (|| -> Result<()> {
                    let text = path_text(&path)?;
                    let size = i64::try_from(meta.len()).map_err(io::Error::other)?;
                    let modified = meta.modified()?;
                    let mtime = match modified.duration_since(UNIX_EPOCH) {
                        Ok(d) => d.as_nanos().to_string(),
                        Err(e) => format!("-{}", e.duration().as_nanos()),
                    };
                    let previous: Option<(i64, String)> = self
                        .db()
                        .query_row(
                            "SELECT size,mtime_ns FROM tracks WHERE path=?",
                            [text],
                            |r| Ok((r.get(0)?, r.get(1)?)),
                        )
                        .optional()?;
                    if !force
                        && previous
                            .as_ref()
                            .is_some_and(|p| p.0 == size && p.1 == mtime)
                    {
                        report.unchanged += 1;
                    } else {
                        let track = crate::tags::read_track(&path)?;
                        pending.push(Change::Put {
                            folder: *folder,
                            track: Box::new(track),
                            size,
                            mtime,
                        });
                        if previous.is_some() {
                            report.updated += 1;
                        } else {
                            report.added += 1;
                        }
                    }
                    Ok(())
                })();
                match result {
                    Err(Error::Db(e)) => return Err(e.into()),
                    Err(e) => report.failed.push((path, e.to_string())),
                    Ok(()) => (),
                }
                if pending.len() == 500 {
                    self.flush(&mut pending, &mut report)?;
                }
                if count % 100 == 0 {
                    progress(ScanProgress {
                        seen: count,
                        updated: report.added + report.updated,
                    });
                }
            }
        }
        // Probe indexed paths directly: inaccessible subtrees must never look deleted.
        if !report.cancelled {
            let candidates = {
                let db = self.db();
                let mut stmt = db.prepare("SELECT path FROM tracks")?;
                stmt.query_map([], |r| Ok(PathBuf::from(r.get::<_, String>(0)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            for path in candidates {
                if cancel.load(Ordering::Relaxed) {
                    report.cancelled = true;
                    break;
                }
                if !seeds.iter().any(|seed| path.starts_with(seed)) {
                    continue;
                }
                match fs::symlink_metadata(&path) {
                    Err(e) if e.kind() == io::ErrorKind::NotFound => {
                        pending.push(Change::Delete(path))
                    }
                    Ok(meta) if !meta.is_file() || meta.is_symlink() => {
                        pending.push(Change::Delete(path))
                    }
                    _ => (),
                }
                if pending.len() == 500 {
                    self.flush(&mut pending, &mut report)?;
                }
            }
        }
        self.flush(&mut pending, &mut report)?;
        // A callback may request cancellation on the final file.
        report.cancelled |= cancel.load(Ordering::Relaxed);
        Ok(report)
    }

    fn flush(&self, pending: &mut Vec<Change>, report: &mut ScanReport) -> Result<()> {
        if pending.is_empty() {
            return Ok(());
        }
        let mut db = self.db();
        let tx = db.transaction()?;
        for change in pending.iter() {
            match change {
                Change::Delete(path) => {
                    report.removed +=
                        tx.execute("DELETE FROM tracks WHERE path=?", [path_text(path)?])? as u64;
                }
                Change::Put {
                    folder,
                    track: t,
                    size,
                    mtime,
                } => {
                    // A folder removed during tag parsing must not be resurrected.
                    if !tx.query_row(
                        "SELECT EXISTS(SELECT 1 FROM folders WHERE id=?)",
                        [folder],
                        |r| r.get::<_, bool>(0),
                    )? {
                        continue;
                    }
                    let artist_key = t.album_artist.to_lowercase();
                    let title_key = t.album.to_lowercase();
                    tx.execute("INSERT OR IGNORE INTO albums(title,artist,title_key,artist_key) VALUES (?,?,?,?)", params![t.album,t.album_artist,title_key,artist_key])?;
                    let album: i64 = tx.query_row(
                        "SELECT id FROM albums WHERE artist_key=? AND title_key=?",
                        params![artist_key, title_key],
                        |r| r.get(0),
                    )?;
                    let duration = t
                        .duration
                        .map(|d| i64::try_from(d.as_nanos()))
                        .transpose()
                        .map_err(io::Error::other)?;
                    tx.execute("INSERT INTO tracks(folder_id,path,size,mtime_ns,title,artist,album,album_artist,album_id,track_no,disc_no,year,duration_ns,codec,lossless,bit_depth,sample_rate,channels,bitrate_kbps,has_cover)
                        VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19,?20)
                        ON CONFLICT(path) DO UPDATE SET folder_id=excluded.folder_id,size=excluded.size,mtime_ns=excluded.mtime_ns,title=excluded.title,artist=excluded.artist,album=excluded.album,album_artist=excluded.album_artist,album_id=excluded.album_id,track_no=excluded.track_no,disc_no=excluded.disc_no,year=excluded.year,duration_ns=excluded.duration_ns,codec=excluded.codec,lossless=excluded.lossless,bit_depth=excluded.bit_depth,sample_rate=excluded.sample_rate,channels=excluded.channels,bitrate_kbps=excluded.bitrate_kbps,has_cover=excluded.has_cover",
                        params![folder,path_text(&t.path)?,size,mtime,t.title,t.artist,t.album,t.album_artist,album,t.track_no,t.disc_no,t.year,duration,t.format.codec,t.format.lossless,t.format.bit_depth,t.format.sample_rate,t.format.channels,t.format.bitrate_kbps,t.has_cover])?;
                }
            }
        }
        cleanup(&tx)?;
        tx.commit()?;
        pending.clear();
        Ok(())
    }
}

fn owner<'a>(path: &Path, roots: &'a [(i64, PathBuf)]) -> Option<&'a (i64, PathBuf)> {
    roots
        .iter()
        .filter(|(_, root)| path.starts_with(root))
        .max_by_key(|(_, root)| root.components().count())
}

fn hidden(path: &Path, root: &Path, directory: bool) -> bool {
    let directory = if directory {
        path
    } else {
        path.parent().unwrap_or(path)
    };
    directory.strip_prefix(root).is_ok_and(|relative| {
        relative
            .components()
            .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
    })
}

fn has_symlink(path: &Path) -> io::Result<bool> {
    for ancestor in path.ancestors() {
        if ancestor.as_os_str().is_empty() {
            continue;
        }
        match fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.is_symlink() => return Ok(true),
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => (),
        }
    }
    Ok(false)
}

fn audio(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| {
            [
                "flac", "mp3", "m4a", "aac", "alac", "ogg", "opus", "wav", "aiff", "aif", "wv",
                "ape",
            ]
            .iter()
            .any(|e| ext.eq_ignore_ascii_case(e))
        })
}

fn is_cover(path: &Path) -> bool {
    path.file_stem().and_then(|s| s.to_str()).is_some_and(|s| {
        ["cover", "folder", "front", "album"]
            .iter()
            .any(|name| s.eq_ignore_ascii_case(name))
    }) && path.extension().and_then(|s| s.to_str()).is_some_and(|s| {
        ["jpg", "jpeg", "png"]
            .iter()
            .any(|ext| s.eq_ignore_ascii_case(ext))
    })
}
