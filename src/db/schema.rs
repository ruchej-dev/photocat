//! The database file and its schema: where it lives, how it opens, how it upgrades.
//!
//! `SCHEMA_VERSION`, the `CREATE`/`ALTER` statements and `init` live here and nowhere else
//! (M27.3): a table is found by its subject, the way it is created is found in this file.

use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags};
use std::path::{Path, PathBuf};

pub fn cache_dir() -> PathBuf {
    // A portable copy names its cache itself (the Windows release's launcher does): the database,
    // the previews, the faces and the models then lie next to the binary, and the folder is the
    // whole application. Without it the platform convention below applies.
    if let Some(dir) = std::env::var_os("PHOTOCAT_CACHE") {
        return PathBuf::from(dir);
    }
    // Linux — XDG, Windows — LOCALAPPDATA, macOS — the usual ~/.cache: no platform crates.
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("LOCALAPPDATA").map(PathBuf::from))
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .unwrap_or_else(std::env::temp_dir);
    base.join("photocat")
}

pub fn resolve_db(explicit: Option<&Path>) -> Result<PathBuf> {
    let p = match explicit {
        Some(p) => p.to_path_buf(),
        None => cache_dir().join("photocat.db"),
    };
    if let Some(dir) = p.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("cannot create {}", dir.display()))?;
    }
    Ok(p)
}

pub fn open(db: &Path) -> Result<Connection> {
    let c = Connection::open(db).with_context(|| format!("cannot open the DB {}", db.display()))?;
    init(&c)?;
    Ok(c)
}

/// The schema version. It changes together with the DDL below: old DBs catch up once.
/// While the version matches, `open` writes nothing — the DB can be read in parallel
/// with a long pass (indexing, OCR) without hitting a lock.
pub(super) const SCHEMA_VERSION: i64 = 10;

pub(super) fn init(c: &Connection) -> Result<()> {
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=2000;",
    )?;
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    create_tables(c)?;
    migrate(c, version)?;
    c.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// The tables and indexes of a fresh database (M27.3): `init` only decides when to run them.
fn create_tables(c: &Connection) -> Result<()> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS files(
            id         INTEGER PRIMARY KEY,
            path       TEXT NOT NULL UNIQUE,
            size       INTEGER NOT NULL,
            mtime      INTEGER,
            sha256     TEXT,
            kind       TEXT NOT NULL,
            width      INTEGER,
            height     INTEGER,
            taken_at   TEXT,
            taken_src  TEXT,
            lat        REAL,
            lon        REAL,
            camera     TEXT,
            dhash      INTEGER,
            sharpness  REAL,
            thumb      TEXT,
            decision   TEXT,
            indexed_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_sha   ON files(sha256);
         CREATE INDEX IF NOT EXISTS idx_taken ON files(taken_at);
         CREATE INDEX IF NOT EXISTS idx_dhash ON files(dhash);
         CREATE TABLE IF NOT EXISTS tags(
            file_id INTEGER NOT NULL,
            tag     TEXT NOT NULL,
            score   REAL,
            source  TEXT,
            PRIMARY KEY(file_id, tag, source)
         );
         CREATE INDEX IF NOT EXISTS idx_tag ON tags(tag);
         CREATE INDEX IF NOT EXISTS idx_tag_source ON tags(source, file_id);
         CREATE TABLE IF NOT EXISTS tag_hidden(
            file_id INTEGER NOT NULL,
            tag     TEXT NOT NULL,
            hidden_at INTEGER NOT NULL,
            PRIMARY KEY(file_id, tag)
         );
         CREATE TABLE IF NOT EXISTS user_tags(
            file_id INTEGER NOT NULL,
            tag     TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            PRIMARY KEY(file_id, tag)
         );
         CREATE INDEX IF NOT EXISTS idx_user_tag ON user_tags(tag);
         CREATE TABLE IF NOT EXISTS settings(
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS tag_defs(
            model     TEXT NOT NULL,
            tag_id    TEXT NOT NULL,
            label     TEXT NOT NULL,
            prompts   TEXT NOT NULL,
            enabled   INTEGER NOT NULL DEFAULT 1,
            position  INTEGER NOT NULL DEFAULT 0,
            min_pct   INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY(model, tag_id)
         );
         CREATE UNIQUE INDEX IF NOT EXISTS tag_defs_label ON tag_defs(model, label);
         CREATE TABLE IF NOT EXISTS embeddings(
            file_id INTEGER NOT NULL,
            model   TEXT NOT NULL,
            sha256  TEXT NOT NULL,
            dim     INTEGER NOT NULL,
            vec     BLOB NOT NULL,
            PRIMARY KEY(file_id, model)
         );
         CREATE INDEX IF NOT EXISTS idx_emb_model ON embeddings(model);
         CREATE TABLE IF NOT EXISTS ocr(
            file_id    INTEGER PRIMARY KEY,
            sha256     TEXT NOT NULL,
            lang       TEXT NOT NULL,
            text       TEXT NOT NULL,
            chars      INTEGER NOT NULL,
            created_at INTEGER NOT NULL
         );
         CREATE VIRTUAL TABLE IF NOT EXISTS ocr_fts USING fts5(
            text, content='ocr', content_rowid='file_id', tokenize='unicode61'
         );
         CREATE TRIGGER IF NOT EXISTS ocr_ai AFTER INSERT ON ocr BEGIN
            INSERT INTO ocr_fts(rowid, text) VALUES (new.file_id, new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS ocr_ad AFTER DELETE ON ocr BEGIN
            INSERT INTO ocr_fts(ocr_fts, rowid, text)
            VALUES('delete', old.file_id, old.text);
         END;
         CREATE TRIGGER IF NOT EXISTS ocr_au AFTER UPDATE ON ocr BEGIN
            INSERT INTO ocr_fts(ocr_fts, rowid, text)
            VALUES('delete', old.file_id, old.text);
            INSERT INTO ocr_fts(rowid, text) VALUES (new.file_id, new.text);
         END;
         CREATE TRIGGER IF NOT EXISTS invalidate_ocr_after_sha_change
         AFTER UPDATE OF sha256 ON files
         WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM ocr WHERE file_id = new.id;
         END;
         CREATE TRIGGER IF NOT EXISTS clear_hidden_tags_after_sha_change
         AFTER UPDATE OF sha256 ON files
         WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM tag_hidden WHERE file_id = new.id;
         END;
         CREATE TRIGGER IF NOT EXISTS invalidate_clip_after_sha_change
         AFTER UPDATE OF sha256 ON files
         WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM embeddings WHERE file_id = new.id;
            DELETE FROM tags WHERE file_id = new.id
               AND (source = 'clip' OR source LIKE 'clip:%');
         END;
         CREATE TABLE IF NOT EXISTS faces(
            id         INTEGER PRIMARY KEY,
            file_id    INTEGER NOT NULL,
            sha256     TEXT NOT NULL,
            x          INTEGER NOT NULL,
            y          INTEGER NOT NULL,
            w          INTEGER NOT NULL,
            h          INTEGER NOT NULL,
            score      REAL NOT NULL,
            quality    REAL NOT NULL,
            thumb      TEXT,
            embedding  BLOB NOT NULL,
            cluster_id INTEGER,
            manual     INTEGER NOT NULL DEFAULT 0,
            match_score REAL,
            created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_faces_file    ON faces(file_id);
         CREATE INDEX IF NOT EXISTS idx_faces_cluster ON faces(cluster_id);
         CREATE TABLE IF NOT EXISTS face_clusters(
            id         INTEGER PRIMARY KEY,
            label      TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL
         );
         CREATE TABLE IF NOT EXISTS face_matches(
            face_id    INTEGER PRIMARY KEY,
            cluster_id INTEGER NOT NULL,
            score      REAL NOT NULL,
            created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_face_matches_cluster ON face_matches(cluster_id);
         CREATE TABLE IF NOT EXISTS face_scans(
            file_id    INTEGER PRIMARY KEY,
            sha256     TEXT NOT NULL,
            faces      INTEGER NOT NULL,
            created_at INTEGER NOT NULL
         );
         CREATE TRIGGER IF NOT EXISTS invalidate_faces_after_sha_change
         AFTER UPDATE OF sha256 ON files
         WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM face_matches WHERE face_id IN (SELECT id FROM faces WHERE file_id = new.id);
            DELETE FROM faces WHERE file_id = new.id;
            DELETE FROM face_scans WHERE file_id = new.id;
         END;
         CREATE TABLE IF NOT EXISTS albums(
            id         INTEGER PRIMARY KEY,
            parent_id  INTEGER NOT NULL DEFAULT 0,
            position   INTEGER NOT NULL DEFAULT 0,
            name       TEXT NOT NULL,
            name_key   TEXT NOT NULL,
            note       TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
         );
         CREATE UNIQUE INDEX IF NOT EXISTS albums_sibling_name ON albums(parent_id, name_key);
         CREATE INDEX IF NOT EXISTS albums_tree ON albums(parent_id, position);
         CREATE TABLE IF NOT EXISTS album_files(
            album_id INTEGER NOT NULL,
            file_id  INTEGER NOT NULL,
            position INTEGER NOT NULL DEFAULT 0,
            added_at TEXT NOT NULL,
            PRIMARY KEY(album_id, file_id)
         );
         CREATE INDEX IF NOT EXISTS album_files_file ON album_files(file_id);",
    )?;
    Ok(())
}

/// Bring an older database up to `SCHEMA_VERSION` in place (M27.3): every step keeps the data
/// and touches no file on disk. A fresh database runs the same steps with `version = 0`.
fn migrate(c: &Connection, version: i64) -> Result<()> {
    // The schema of older DBs is extended in place: no data is lost on an upgrade.
    ensure_column(c, "files", "sharpness", "REAL")?;
    ensure_column(c, "files", "decision", "TEXT")?;
    ensure_column(c, "files", "duration_s", "REAL")?;
    ensure_column(c, "files", "trashed_at", "INTEGER")?;
    // Schema 9: the similarity that brought an automatically matched face into a named group
    // (M26.11). `NULL` means the face was not matched by the step — it is clustered or the owner
    // put it there by hand.
    ensure_column(c, "faces", "match_score", "REAL")?;
    // Schema 7: tags of different models do not mix — the key is (file_id, tag, source).
    // The old table is rebuilt in place and the data is kept; the trigger that resets
    // tags on a sha256 change is recreated, otherwise it would point at the old table
    // and delete only the legacy `clip` source.
    if version < 7 {
        c.execute_batch(
            "BEGIN;
             DROP TRIGGER IF EXISTS invalidate_clip_after_sha_change;
             ALTER TABLE tags RENAME TO tags_v6;
             CREATE TABLE tags(
                file_id INTEGER NOT NULL,
                tag     TEXT NOT NULL,
                score   REAL,
                source  TEXT,
                PRIMARY KEY(file_id, tag, source)
             );
             INSERT INTO tags(file_id,tag,score,source)
                SELECT file_id,tag,score,COALESCE(source,'') FROM tags_v6;
             DROP TABLE tags_v6;
             CREATE INDEX IF NOT EXISTS idx_tag ON tags(tag);
             CREATE INDEX IF NOT EXISTS idx_tag_source ON tags(source, file_id);
             COMMIT;",
        )?;
    }
    c.execute_batch(
        "DROP TRIGGER IF EXISTS invalidate_clip_after_sha_change;
         CREATE TRIGGER invalidate_clip_after_sha_change
         AFTER UPDATE OF sha256 ON files
         WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM embeddings WHERE file_id = new.id;
            DELETE FROM tags WHERE file_id = new.id
               AND (source = 'clip' OR source LIKE 'clip:%');
         END;",
    )?;
    // Schema 8: image previews follow the EXIF orientation (M22). Old previews of
    // phone portraits lie on their side, so the preview path is cleared: the next
    // `index` sees `thumb IS NULL` and recomputes the frame. Files, tags and albums are
    // untouched, nothing on disk changes.
    if version < 8 {
        c.execute("UPDATE files SET thumb = NULL WHERE kind = 'image'", [])?;
    }
    // Schema 10: the matching step no longer moves a face into a named group by itself — it
    // leaves the face where it is and writes a proposal into `face_matches` that waits for the
    // owner's confirmation (M26.17). The faces an older version placed automatically stay where
    // they are and are pinned, so nothing jumps on the upgrade and the quarantine starts empty.
    if version < 10 {
        c.execute(
            "UPDATE faces SET manual=1, match_score=NULL WHERE match_score IS NOT NULL",
            [],
        )?;
    }
    // The trigger of an older version does not know `face_matches`: it is replaced here, because
    // `CREATE TRIGGER IF NOT EXISTS` would keep the old body (as with the CLIP trigger above).
    c.execute_batch(
        "DROP TRIGGER IF EXISTS invalidate_faces_after_sha_change;
         CREATE TRIGGER invalidate_faces_after_sha_change
         AFTER UPDATE OF sha256 ON files
         WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM face_matches WHERE face_id IN (SELECT id FROM faces WHERE file_id = new.id);
            DELETE FROM faces WHERE file_id = new.id;
            DELETE FROM face_scans WHERE file_id = new.id;
         END;",
    )?;
    // Tags of older versions had no model in the source; they move to the current model id.
    c.execute(
        "UPDATE tags SET source='clip:clip-vit-b32-multilingual' WHERE source='clip'",
        [],
    )?;
    Ok(())
}

/// The size of the database file and the pages inside it that no row uses (M26.8). `VACUUM`
/// gives those pages back to the filesystem; `worth_compacting` is the honest "anything to
/// reclaim" test — under 1 MiB or under 5 % of the file it is not worth a rebuild.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DbSpace {
    pub file_bytes: u64,
    pub free_bytes: u64,
}

impl DbSpace {
    pub fn worth_compacting(&self) -> bool {
        const MIN_FREE_BYTES: u64 = 1024 * 1024;
        self.free_bytes >= MIN_FREE_BYTES && self.free_bytes * 20 >= self.file_bytes
    }
}

/// What a compaction did: the file before and after. A call with nothing to reclaim reports the
/// same numbers twice and touches nothing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Compaction {
    pub before: DbSpace,
    pub after: DbSpace,
}

/// Read the file's size and `freelist_count × page_size` without writing anything. A missing file
/// is an empty database rather than an error: the window asks before the first index.
pub fn db_space(path: &Path) -> Result<DbSpace> {
    if !path.exists() {
        return Ok(DbSpace::default());
    }
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .with_context(|| format!("cannot read the DB {}", path.display()))?;
    let free_pages: i64 = conn.query_row("PRAGMA freelist_count", [], |r| r.get(0))?;
    let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    Ok(DbSpace {
        file_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        free_bytes: free_pages.max(0) as u64 * page_size.max(0) as u64,
    })
}

/// Rebuild the database in place (M26.8): `VACUUM` writes a fresh file and moves every row in,
/// so the free pages leave the disk. The archive, the previews, the faces and the models are not
/// touched. It cannot be stopped once started; a second call with nothing to reclaim is a no-op.
pub fn vacuum(path: &Path) -> Result<Compaction> {
    let before = db_space(path)?;
    if before.free_bytes == 0 || !path.exists() {
        return Ok(Compaction {
            before,
            after: before,
        });
    }
    let conn = Connection::open(path)
        .with_context(|| format!("cannot open the DB {}", path.display()))?;
    // The window is told not to read while the pass runs; a CLI or another window may still hold
    // a reader, so the pass waits for it instead of failing at once.
    conn.busy_timeout(std::time::Duration::from_secs(30))?;
    conn.execute_batch("VACUUM; PRAGMA wal_checkpoint(TRUNCATE);")?;
    drop(conn);
    let after = db_space(path)?;
    Ok(Compaction { before, after })
}

fn ensure_column(c: &Connection, table: &str, column: &str, decl: &str) -> Result<()> {
    let exists = {
        let mut st = c.prepare(&format!("PRAGMA table_info({table})"))?;
        let mut names = st.query_map([], |r| r.get::<_, String>(1))?;
        let mut found = false;
        for n in names.by_ref() {
            if n? == column {
                found = true;
                break;
            }
        }
        found
    };
    if !exists {
        c.execute_batch(&format!("ALTER TABLE {table} ADD COLUMN {column} {decl}"))?;
    }
    Ok(())
}
