CREATE TABLE folders (id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE);
CREATE TABLE albums (
    id INTEGER PRIMARY KEY, title TEXT NOT NULL, artist TEXT NOT NULL,
    title_key TEXT NOT NULL, artist_key TEXT NOT NULL,
    UNIQUE(artist_key, title_key)
);
CREATE TABLE tracks (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    folder_id INTEGER NOT NULL REFERENCES folders(id) ON DELETE CASCADE,
    path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL, mtime_ns TEXT NOT NULL,
    title TEXT NOT NULL, artist TEXT NOT NULL, album TEXT NOT NULL, album_artist TEXT NOT NULL,
    album_id INTEGER NOT NULL REFERENCES albums(id),
    track_no INTEGER, disc_no INTEGER, year INTEGER, duration_ns INTEGER,
    codec TEXT NOT NULL, lossless INTEGER NOT NULL, bit_depth INTEGER,
    sample_rate INTEGER, channels INTEGER, bitrate_kbps INTEGER, has_cover INTEGER NOT NULL
);
CREATE INDEX tracks_folder ON tracks(folder_id);
CREATE INDEX tracks_album ON tracks(album_id);
CREATE VIRTUAL TABLE track_search USING fts5(
    title, artist, album, album_artist, content='tracks', content_rowid='id',
    tokenize='unicode61 remove_diacritics 2'
);
CREATE TRIGGER tracks_insert AFTER INSERT ON tracks BEGIN
    INSERT INTO track_search(rowid,title,artist,album,album_artist)
    VALUES(new.id,new.title,new.artist,new.album,new.album_artist);
END;
CREATE TRIGGER tracks_delete AFTER DELETE ON tracks BEGIN
    INSERT INTO track_search(track_search,rowid,title,artist,album,album_artist)
    VALUES('delete',old.id,old.title,old.artist,old.album,old.album_artist);
END;
CREATE TRIGGER tracks_update AFTER UPDATE ON tracks BEGIN
    INSERT INTO track_search(track_search,rowid,title,artist,album,album_artist)
    VALUES('delete',old.id,old.title,old.artist,old.album,old.album_artist);
    INSERT INTO track_search(rowid,title,artist,album,album_artist)
    VALUES(new.id,new.title,new.artist,new.album,new.album_artist);
END;
PRAGMA user_version=1;
