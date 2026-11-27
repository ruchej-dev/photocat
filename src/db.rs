//! SQLite: the schema, inserts, reads, statistics.

use crate::i18n::{self, Lang};
use anyhow::{Context, Result};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Indexing progress (atomics, so the GUI thread can read it).
#[derive(Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    errors: AtomicUsize,
    finished: AtomicBool,
    cancel: AtomicBool,
    msg: Mutex<String>,
}

impl Progress {
    pub fn set_total(&self, n: usize) {
        self.total.store(n, Ordering::Relaxed);
    }
    /// Start over (for instance, when moving to the next candidate model).
    pub fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
        self.cancel.store(false, Ordering::Relaxed);
        self.total.store(0, Ordering::Relaxed);
        self.errors.store(0, Ordering::Relaxed);
        self.finished.store(false, Ordering::Relaxed);
    }
    pub fn tick(&self) {
        self.done.fetch_add(1, Ordering::Relaxed);
    }
    pub fn error(&self) {
        self.errors.fetch_add(1, Ordering::Relaxed);
    }
    pub fn finish(&self) {
        self.finished.store(true, Ordering::Relaxed);
    }
    /// Ask the pass to stop: the workers check the flag between files.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }
    pub fn set_msg(&self, s: impl Into<String>) {
        *self.msg.lock().unwrap() = s.into();
    }
    pub fn total(&self) -> usize {
        self.total.load(Ordering::Relaxed)
    }
    pub fn done(&self) -> usize {
        self.done.load(Ordering::Relaxed)
    }
    pub fn errors(&self) -> usize {
        self.errors.load(Ordering::Relaxed)
    }
    pub fn finished(&self) -> bool {
        self.finished.load(Ordering::Relaxed)
    }
    pub fn msg(&self) -> String {
        self.msg.lock().unwrap().clone()
    }
}

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
const SCHEMA_VERSION: i64 = 8;

fn init(c: &Connection) -> Result<()> {
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA busy_timeout=2000;",
    )?;
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
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
            created_at INTEGER NOT NULL
         );
         CREATE INDEX IF NOT EXISTS idx_faces_file    ON faces(file_id);
         CREATE INDEX IF NOT EXISTS idx_faces_cluster ON faces(cluster_id);
         CREATE TABLE IF NOT EXISTS face_clusters(
            id         INTEGER PRIMARY KEY,
            label      TEXT NOT NULL DEFAULT '',
            created_at INTEGER NOT NULL
         );
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
    // The schema of older DBs is extended in place: no data is lost on an upgrade.
    ensure_column(c, "files", "sharpness", "REAL")?;
    ensure_column(c, "files", "decision", "TEXT")?;
    ensure_column(c, "files", "duration_s", "REAL")?;
    ensure_column(c, "files", "trashed_at", "INTEGER")?;
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
    // Tags of older versions had no model in the source; they move to the current model id.
    c.execute(
        "UPDATE tags SET source='clip:clip-vit-b32-multilingual' WHERE source='clip'",
        [],
    )?;
    c.pragma_update(None, "user_version", SCHEMA_VERSION)?;
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

/// How the grid shows files: grouped by months or one continuous ribbon (M17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridLayout {
    #[default]
    Months,
    Flat,
}

impl GridLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            GridLayout::Months => "months",
            GridLayout::Flat => "flat",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "flat" => GridLayout::Flat,
            _ => GridLayout::Months,
        }
    }
}

/// The interface theme (M22, ADR 0044): dark, light or follow the system.
/// The two palettes of the mock-up are `dark` and `light`; `system` takes the OS choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
            ThemeMode::System => "system",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "light" => ThemeMode::Light,
            "system" => ThemeMode::System,
            _ => ThemeMode::Dark,
        }
    }
}

/// How files are shown in the grid (M22): the mock-up tile, proportion rows or a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellStyle {
    /// The mock-up tile: a 4:3 frame, the snapshot filling it (the sides are cut), the caption
    /// below.
    #[default]
    Tiles,
    /// Proportion rows: the row height comes from the slider, the tile width from the frame.
    Rows,
    /// A list: thumbnail, name, date, size.
    List,
}

impl CellStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            CellStyle::Tiles => "tiles",
            CellStyle::Rows => "rows",
            CellStyle::List => "list",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "rows" => CellStyle::Rows,
            "list" => CellStyle::List,
            _ => CellStyle::Tiles,
        }
    }
}

/// The order of the album cards on the "Albums" screen (M22.20). It is a look, not data: the
/// album order (`position`) stays the order of the export whatever the cards show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlbumSort {
    /// The owner's own order — the order a sub-album takes on disk.
    #[default]
    Position,
    /// The album changed most recently first (a photo was added, the name changed).
    Updated,
    Name,
    Count,
    Size,
}

impl AlbumSort {
    pub const ALL: [AlbumSort; 5] = [
        AlbumSort::Position,
        AlbumSort::Updated,
        AlbumSort::Name,
        AlbumSort::Count,
        AlbumSort::Size,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AlbumSort::Position => "position",
            AlbumSort::Updated => "updated",
            AlbumSort::Name => "name",
            AlbumSort::Count => "count",
            AlbumSort::Size => "size",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "updated" => AlbumSort::Updated,
            "name" => AlbumSort::Name,
            "count" => AlbumSort::Count,
            "size" => AlbumSort::Size,
            _ => AlbumSort::Position,
        }
    }
}

/// The order of the tiles (M17). "Album order" (M19) works only inside the chosen
/// album; outside it reads as "date: newest first".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridSort {
    #[default]
    DateDesc,
    DateAsc,
    Name,
    SizeDesc,
    Album,
}

impl GridSort {
    pub fn as_str(self) -> &'static str {
        match self {
            GridSort::DateDesc => "date_desc",
            GridSort::DateAsc => "date_asc",
            GridSort::Name => "name",
            GridSort::SizeDesc => "size_desc",
            GridSort::Album => "album",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "date_asc" => GridSort::DateAsc,
            "name" => GridSort::Name,
            "size_desc" => GridSort::SizeDesc,
            "album" => GridSort::Album,
            _ => GridSort::DateDesc,
        }
    }
}

/// A tag in the dictionary: the label, the prompts and the order. It lives in the DB per
/// model (M15.4), because labels are the owner's configuration, not a constant in the code.
#[derive(Debug, Clone, PartialEq)]
pub struct TagDef {
    /// The model id (`embeddings.model`): every model has its own dictionary.
    pub model: String,
    /// The stable key of the row: the tag is renamed and removed by it.
    pub id: String,
    /// The label in Russian: it is also the key in `tags.tag`.
    pub label: String,
    /// 2–3 general templates; an empty list means the tag is off by definition.
    pub prompts: Vec<String>,
    pub enabled: bool,
    /// The order in the filter and the editor.
    pub position: i64,
    /// The minimum percentile for showing the tag (0 — whatever the slider picks).
    pub min_percentile: i32,
}

/// A pass an owner can switch off for one catalog (M26.9): the expensive analysis that says
/// something *about* a photo. Indexing itself is not a pass here — the grid, the duplicates and
/// the calendar need it for every catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pass {
    Faces,
    Tags,
    Text,
}

impl Pass {
    pub const ALL: [Pass; 3] = [Pass::Faces, Pass::Tags, Pass::Text];

    /// The key in the `root_passes` JSON: data, not a label.
    pub fn key(self) -> &'static str {
        match self {
            Pass::Faces => "faces",
            Pass::Tags => "tags",
            Pass::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Pass> {
        match s {
            "faces" => Some(Pass::Faces),
            "tags" => Some(Pass::Tags),
            "text" => Some(Pass::Text),
            _ => None,
        }
    }
}

/// The application settings. Kept in the DB (next to the index), not in a file by the archive.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Where to download models to and read them from.
    pub models_dir: PathBuf,
    /// The catalogue model id; empty means the first available one.
    pub model_id: String,
    /// The size ceiling of a downloaded model, MiB.
    pub max_model_mb: u64,
    /// Indexing threads by default.
    pub index_jobs: usize,
    /// The default dHash similarity threshold.
    pub max_dist: u32,
    /// The confident-tag threshold (raw cosine) and the margin over the second tag.
    /// This is about the card, OCR and `{tag}` in the showcase, not about the filter.
    pub tag_show: f32,
    pub tag_margin: f32,
    /// The threshold of the tag filter — a percentile 0–100 (ADR 0032).
    /// 100% — the strongest photo of this tag, 0% — all; 70% — the top 30%.
    pub tag_show_pct: i32,
    /// The tesseract languages for text recognition, for instance `rus+eng`.
    pub ocr_lang: String,
    /// Paths to external programs; empty means a lookup in `PATH` (ADR 0020).
    pub tools: crate::tools::Tools,
    /// The cosine threshold for merging faces into one person (ADR 0021).
    pub face_threshold: f32,
    /// The grid tile size, px (docs/UI.md).
    pub tile_size: f32,
    /// The grid layout: months or a flat ribbon (M17).
    pub grid_layout: GridLayout,
    /// The tile order: date, name or size (M17).
    pub grid_sort: GridSort,
    /// How files are shown: the mock-up tile, proportion rows or a list (M22).
    pub cell_style: CellStyle,
    /// The order of the album cards and the album look: cards or a list (M22.20).
    pub album_sort: AlbumSort,
    pub album_list: bool,
    /// Folders indexed with one button; empty means the path from the top bar.
    pub roots: Vec<String>,
    /// Folders switched off (M26.1): their files are hidden from every view, the rows, the
    /// previews and the files on disk stay. The same newline-separated form as `roots`.
    pub roots_off: Vec<String>,
    /// Passes switched off per catalog (M26.9): only the exceptions, keyed by the root path. A
    /// root that is not listed runs every pass; a root that is gone keeps no key.
    pub root_passes: std::collections::BTreeMap<String, Vec<Pass>>,
    /// The integrity reminder (M10): the folder and the manifest the term belongs to,
    /// the interval in days (0 — the reminder is off) and the result of the last pass.
    pub verify_root: String,
    pub verify_manifest: String,
    pub verify_interval_days: u32,
    /// RFC3339 UTC of the last complete pass over the archive (a manifest or a check).
    pub verify_last_at: String,
    pub verify_last_ok: bool,
    /// The same report text that goes to `<cache>/photocat/verify-last.txt`.
    pub verify_last_report: String,
    /// The interface language (M21, ADR 0040): `en` is the main one, `ru` the second.
    pub lang: crate::i18n::Lang,
    /// The interface theme (M22, ADR 0044): dark by default, light at the owner's choice.
    pub theme: ThemeMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models_dir: cache_dir().join("models"),
            model_id: String::new(),
            max_model_mb: 250,
            index_jobs: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(8)
                .clamp(2, 16),
            max_dist: 6,
            tag_show: 0.22,
            tag_margin: 0.01,
            tag_show_pct: 70,
            ocr_lang: "rus+eng".into(),
            tools: crate::tools::Tools::default(),
            face_threshold: 0.45,
            tile_size: 150.0,
            grid_layout: GridLayout::Months,
            grid_sort: GridSort::DateDesc,
            cell_style: CellStyle::Tiles,
            album_sort: AlbumSort::Position,
            album_list: false,
            roots: Vec::new(),
            roots_off: Vec::new(),
            root_passes: std::collections::BTreeMap::new(),
            verify_root: String::new(),
            verify_manifest: String::new(),
            // 30 days is a calm term for a family archive; 0 switches the reminder off.
            verify_interval_days: 30,
            verify_last_at: String::new(),
            verify_last_ok: false,
            verify_last_report: String::new(),
            // The project continues in English; Russian stays a choice in the settings.
            lang: crate::i18n::Lang::En,
            // The mock-up paints the dark palette as the main one: a family archive is looked at in the evening.
            theme: ThemeMode::Dark,
        }
    }
}

/// A root without its trailing separator; a lone `/` or `C:\` stays as it is, so the whole
/// drive or the file system can still be named as a folder.
fn trim_root(root: &str) -> &str {
    let trimmed = root.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() { root } else { trimmed }
}

/// Whether `path` lies inside `root` (M26.1). A component-wise prefix, not a string one:
/// `/archive2` is not inside `/archive`. A trailing separator on either side is ignored.
pub fn under_root(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    std::path::Path::new(trim_root(path)).starts_with(std::path::Path::new(trim_root(root)))
}

impl Settings {
    /// Whether a file path lies in a folder that is switched off (M26.1). The switch is about
    /// showing, not about deleting: the rows stay in the DB.
    pub fn path_hidden(&self, path: &str) -> bool {
        self.roots_off.iter().any(|root| under_root(path, root))
    }

    /// Whether this very folder is switched off — the folder list and the catalog block.
    pub fn root_off(&self, root: &str) -> bool {
        let root = trim_root(root);
        self.roots_off.iter().any(|r| trim_root(r) == root)
    }

    /// Switch a folder on or off (M26.1): only the setting changes, no row is touched.
    pub fn set_root_off(&mut self, root: &str, off: bool) {
        let root = trim_root(root).to_string();
        if off {
            if !self.root_off(&root) {
                self.roots_off.push(root);
            }
        } else {
            self.roots_off.retain(|r| trim_root(r) != root);
        }
    }

    /// Whether the pass runs over this file (M26.9). One helper for the GUI and the CLI: the most
    /// specific root that holds the path decides; a file outside every root and a root without an
    /// exception allow the pass. A catalog switched off for display (M26.1) allows no pass at all —
    /// switching it back on lets the passes catch up.
    pub fn pass_allows(&self, path: &str, pass: Pass) -> bool {
        if self.path_hidden(path) {
            return false;
        }
        self.root_passes
            .iter()
            .filter(|(root, _)| under_root(path, root))
            .max_by_key(|(root, _)| trim_root(root).len())
            .is_none_or(|(_, off)| !off.contains(&pass))
    }

    /// Whether this catalog runs a pass — the three boxes of the folder list. A path that is not
    /// a root itself is looked up by the same rule as `pass_allows`.
    pub fn root_pass(&self, root: &str, pass: Pass) -> bool {
        self.root_passes
            .iter()
            .filter(|(key, _)| trim_root(key) == trim_root(root))
            .max_by_key(|(key, _)| trim_root(key).len())
            .is_none_or(|(_, off)| !off.contains(&pass))
    }

    /// Switch one pass off or on for one catalog (M26.9): only the exception list changes, nothing
    /// derived is deleted — the vectors, tags, text and faces stay until the owner forgets them.
    pub fn set_root_pass(&mut self, root: &str, pass: Pass, on: bool) {
        let key = trim_root(root).to_string();
        let list = self.root_passes.entry(key.clone()).or_default();
        if on {
            list.retain(|p| *p != pass);
        } else if !list.contains(&pass) {
            list.push(pass);
        }
        if list.is_empty() {
            self.root_passes.remove(&key);
        }
    }

    /// Drop the exceptions of roots that are no longer in `roots` (M26.9): a catalog that is gone
    /// keeps no key. Returns whether anything left.
    pub fn prune_root_passes(&mut self) -> bool {
        let before = self.root_passes.len();
        self.root_passes.retain(|key, _| {
            self.roots
                .iter()
                .any(|root| trim_root(root) == trim_root(key))
        });
        before != self.root_passes.len()
    }
}

/// How many indexed files lie in each root, in the order of `roots`: the folder list says where
/// the archive is. A switched-off root is counted too — the switch hides, it does not delete.
pub fn root_file_counts(db: &Path, roots: &[String]) -> Result<Vec<(String, usize)>> {
    let conn = open(db)?;
    let paths: Vec<String> = {
        let mut st = conn.prepare("SELECT path FROM files WHERE trashed_at IS NULL")?;
        let rows = st.query_map([], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<String>>>()?
    };
    Ok(roots
        .iter()
        .map(|root| {
            let n = paths.iter().filter(|p| under_root(p, root)).count();
            (root.clone(), n)
        })
        .collect())
}

/// The path of every face's source photo, by face id: a pass that respects a catalog's switches
/// (M26.9) needs the path to ask `Settings::pass_allows`.
pub fn face_paths(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare(
        "SELECT f.id, fi.path FROM faces f JOIN files fi ON fi.id = f.file_id",
    )?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, path) = row?;
        out.insert(id, path);
    }
    Ok(out)
}

/// The path of every indexed file by id, the trashed ones included (M26.9): a pass must not fall
/// back to "allowed" for a row it did not load into its visible set.
pub fn file_paths(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT id, path FROM files")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out = HashMap::new();
    for row in rows {
        let (id, path) = row?;
        out.insert(id, path);
    }
    Ok(out)
}

/// What a "forget this catalog's analysis" action removed (M26.9): derived rows only.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Forgotten {
    pub files: usize,
    pub tags: usize,
    pub vectors: usize,
    pub ocr: usize,
    pub faces: usize,
}

impl Forgotten {
    pub fn is_empty(&self) -> bool {
        self.tags == 0 && self.vectors == 0 && self.ocr == 0 && self.faces == 0
    }
}

/// Remove the derived analysis of one catalog: CLIP tags, vectors, recognized text, faces and the
/// scan mark. The file rows, the owner's own tags, the previews and the archive stay (M26.9).
pub fn forget_catalog_derived(conn: &Connection, root: &str) -> Result<Forgotten> {
    let ids: Vec<i64> = {
        let mut st = conn.prepare("SELECT id, path FROM files")?;
        let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        let mut ids = Vec::new();
        for row in rows {
            let (id, path) = row?;
            if under_root(&path, root) {
                ids.push(id);
            }
        }
        ids
    };
    let mut out = Forgotten {
        files: ids.len(),
        ..Forgotten::default()
    };
    let tx = conn.unchecked_transaction()?;
    {
        let mut tags = tx.prepare("DELETE FROM tags WHERE file_id=?1")?;
        let mut vectors = tx.prepare("DELETE FROM embeddings WHERE file_id=?1")?;
        let mut ocr = tx.prepare("DELETE FROM ocr WHERE file_id=?1")?;
        let mut scans = tx.prepare("DELETE FROM face_scans WHERE file_id=?1")?;
        let mut faces = tx.prepare("DELETE FROM faces WHERE file_id=?1")?;
        for id in &ids {
            out.tags += tags.execute([id])?;
            out.vectors += vectors.execute([id])?;
            out.ocr += ocr.execute([id])?;
            scans.execute([id])?;
            out.faces += faces.execute([id])?;
        }
    }
    tx.commit()?;
    delete_empty_face_clusters(conn)?;
    Ok(out)
}

pub fn load_settings(conn: &Connection) -> Settings {
    let mut s = Settings::default();
    let mut has_tag_pct = false;
    let Ok(mut st) = conn.prepare("SELECT key, value FROM settings") else {
        return s;
    };
    let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    else {
        return s;
    };
    for row in rows.flatten() {
        let (k, v) = row;
        match k.as_str() {
            "models_dir" => s.models_dir = PathBuf::from(v),
            "model_id" => s.model_id = v,
            "max_model_mb" => s.max_model_mb = v.parse().unwrap_or(s.max_model_mb),
            "index_jobs" => s.index_jobs = v.parse().unwrap_or(s.index_jobs),
            "max_dist" => s.max_dist = v.parse().unwrap_or(s.max_dist),
            "tag_show" => s.tag_show = v.parse().unwrap_or(s.tag_show),
            "tag_margin" => s.tag_margin = v.parse().unwrap_or(s.tag_margin),
            "tag_show_pct" => {
                s.tag_show_pct = v.parse().unwrap_or(s.tag_show_pct).clamp(0, 100);
                has_tag_pct = true;
            }
            "ocr_lang" if !v.trim().is_empty() => s.ocr_lang = v,
            "tool_ffmpeg" => s.tools.ffmpeg = v,
            "tool_ffprobe" => s.tools.ffprobe = v,
            "tool_tesseract" => s.tools.tesseract = v,
            "face_threshold" => s.face_threshold = v.parse().unwrap_or(s.face_threshold),
            "tile_size" => s.tile_size = v.parse().unwrap_or(s.tile_size),
            "grid_layout" => s.grid_layout = GridLayout::parse(&v),
            "grid_sort" => s.grid_sort = GridSort::parse(&v),
            "cell_style" => s.cell_style = CellStyle::parse(&v),
            "album_sort" => s.album_sort = AlbumSort::parse(&v),
            "album_list" => s.album_list = v == "1" || v == "true",
            "roots_off" => {
                s.roots_off = v
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            // Passes switched off per catalog (M26.9): a JSON object root → disabled pass keys.
            "root_passes" => {
                let parsed = serde_json::from_str::<serde_json::Value>(&v)
                    .ok()
                    .and_then(|value| value.as_object().cloned());
                for (root, list) in parsed.unwrap_or_default() {
                    let off: Vec<Pass> = list
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().and_then(Pass::parse))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !off.is_empty() {
                        s.root_passes.insert(trim_root(&root).to_string(), off);
                    }
                }
            }
            "roots" => {
                s.roots = v
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            "verify_root" => s.verify_root = v,
            "verify_manifest" => s.verify_manifest = v,
            "verify_interval_days" => {
                s.verify_interval_days = v.parse().unwrap_or(s.verify_interval_days).min(3650)
            }
            "verify_last_at" => s.verify_last_at = v,
            "verify_last_ok" => s.verify_last_ok = v == "1" || v == "true",
            "verify_last_report" => s.verify_last_report = v,
            "lang" => s.lang = crate::i18n::Lang::parse(&v),
            "theme" => s.theme = ThemeMode::parse(&v),
            _ => {}
        }
    }
    if !has_tag_pct {
        // Migration M15.2: `tag_show` used to be both a filter and a confidence threshold,
        // so the slider drove it down to 0.50. The filter is in percentiles now (70% by
        // default) and confidence is back to ADR 0013 — 0.22 with a margin of 0.01.
        s.tag_show = 0.22;
        s.tag_show_pct = 70;
    }
    // A catalog that was removed from the index keeps no exception (M26.9).
    s.prune_root_passes();
    s
}

pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()> {
    let mut pairs = vec![
        ("models_dir", s.models_dir.to_string_lossy().to_string()),
        ("model_id", s.model_id.clone()),
        ("max_model_mb", s.max_model_mb.to_string()),
        ("index_jobs", s.index_jobs.to_string()),
        ("max_dist", s.max_dist.to_string()),
        ("tag_show", s.tag_show.to_string()),
        ("tag_margin", s.tag_margin.to_string()),
        ("tag_show_pct", s.tag_show_pct.to_string()),
        ("ocr_lang", s.ocr_lang.clone()),
        ("face_threshold", s.face_threshold.to_string()),
        ("tile_size", s.tile_size.to_string()),
        ("grid_layout", s.grid_layout.as_str().to_string()),
        ("grid_sort", s.grid_sort.as_str().to_string()),
        ("cell_style", s.cell_style.as_str().to_string()),
        ("album_sort", s.album_sort.as_str().to_string()),
        (
            "album_list",
            if s.album_list { "1" } else { "0" }.to_string(),
        ),
        ("roots", s.roots.join("\n")),
        ("roots_off", s.roots_off.join("\n")),
        (
            "root_passes",
            {
                // Only the exceptions, and only for roots that are still in the list.
                let mut obj = serde_json::Map::new();
                for (root, off) in &s.root_passes {
                    if off.is_empty()
                        || !s
                            .roots
                            .iter()
                            .any(|r| trim_root(r) == trim_root(root))
                    {
                        continue;
                    }
                    obj.insert(
                        root.clone(),
                        serde_json::Value::Array(
                            off.iter()
                                .map(|p| serde_json::Value::String(p.key().into()))
                                .collect(),
                        ),
                    );
                }
                serde_json::Value::Object(obj).to_string()
            },
        ),
        ("verify_root", s.verify_root.clone()),
        ("verify_manifest", s.verify_manifest.clone()),
        ("verify_interval_days", s.verify_interval_days.to_string()),
        ("verify_last_at", s.verify_last_at.clone()),
        (
            "verify_last_ok",
            if s.verify_last_ok { "1" } else { "0" }.to_string(),
        ),
        ("verify_last_report", s.verify_last_report.clone()),
        ("lang", s.lang.code().to_string()),
        ("theme", s.theme.as_str().to_string()),
    ];
    for tool in crate::tools::ALL {
        pairs.push((tool.setting_key(), s.tools.get(tool).to_string()));
    }
    for (k, v) in pairs {
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![k, v],
        )?;
    }
    Ok(())
}

/// Whether it is time to check the archive against the manifest (M10). Computed from the
/// settings, not from a timer: the application starts nothing itself, it only says "due".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyDue {
    /// No manifest is set — nothing to remind about.
    NoManifest,
    /// The interval is 0 — the owner switched the reminder off.
    Disabled,
    /// The term has not come yet; `days_left` is how many full days remain.
    NotYet { days_left: i64 },
    /// The term has passed; `days_over` is by how many days (0 — exactly due).
    Due { days_over: i64 },
}

/// RFC3339 → UTC. An empty or broken value means `None`.
pub fn parse_stamp(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// The "due / not due" decision in one function: the same for the banner and for the tests.
/// `now` is passed explicitly, so "exactly due" and "an hour ago" are checked without waiting.
///
/// * no manifest → `NoManifest`;
/// * interval 0 → `Disabled`;
/// * the manifest has never been checked (empty or broken date) → `Due`;
/// * a date in the future (the clock was turned back) → `NotYet`: not due yet, no panic;
/// * otherwise — by calendar days.
pub fn verify_due(s: &Settings, now: chrono::DateTime<chrono::Utc>) -> VerifyDue {
    if s.verify_manifest.trim().is_empty() {
        return VerifyDue::NoManifest;
    }
    if s.verify_interval_days == 0 {
        return VerifyDue::Disabled;
    }
    let interval = chrono::Duration::days(i64::from(s.verify_interval_days));
    let Some(last) = parse_stamp(&s.verify_last_at) else {
        return VerifyDue::Due { days_over: 0 };
    };
    let elapsed = now.signed_duration_since(last);
    if elapsed >= interval {
        VerifyDue::Due {
            days_over: (elapsed - interval).num_days(),
        }
    } else {
        let left = (interval - elapsed).num_seconds();
        VerifyDue::NotYet {
            days_left: (left + 86_399) / 86_400,
        }
    }
}

/// One value from `settings` — for service keys like the dictionary version.
/// The interface language on the first launch (M21, ADR 0040): if the owner has not chosen it
/// yet, the system locale is taken and stored — afterwards the language is read from the DB, not guessed again.
pub fn ensure_lang(conn: &Connection) -> Result<()> {
    if get_setting(conn, "lang").is_none() {
        set_setting(conn, "lang", crate::i18n::Lang::from_env().code())?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0))
        .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}

pub fn hide_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,?2,?3)
         ON CONFLICT(file_id,tag) DO NOTHING",
        params![file_id, tag, now()],
    )?;
    Ok(())
}

/// Bring back every hidden tag of a file (the original is untouched, the decision lives in the DB).
pub fn unhide_tags(db: &Path, file_id: i64) -> Result<usize> {
    let conn = open(db)?;
    Ok(conn.execute("DELETE FROM tag_hidden WHERE file_id=?1", params![file_id])?)
}

/// file_id → how many tags the owner has hidden.
pub fn load_hidden_counts(conn: &Connection) -> Result<HashMap<i64, usize>> {
    let mut st = conn.prepare("SELECT file_id, COUNT(*) FROM tag_hidden GROUP BY file_id")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, n) = r?;
        out.insert(id, n as usize);
    }
    Ok(out)
}

/// The owner's own tags: file_id → a list of labels.
pub fn load_user_tags(conn: &Connection) -> Result<HashMap<i64, Vec<String>>> {
    let mut st = conn.prepare("SELECT file_id, tag FROM user_tags ORDER BY tag")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    let mut out: HashMap<i64, Vec<String>> = HashMap::new();
    for r in rows {
        let (id, tag) = r?;
        out.entry(id).or_default().push(tag);
    }
    Ok(out)
}

pub fn add_user_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let tag = tag.trim();
    if tag.is_empty() {
        return Ok(());
    }
    let conn = open(db)?;
    conn.execute(
        "INSERT INTO user_tags(file_id,tag,created_at) VALUES(?1,?2,?3)
         ON CONFLICT(file_id,tag) DO NOTHING",
        params![file_id, tag, now()],
    )?;
    Ok(())
}

pub fn remove_user_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "DELETE FROM user_tags WHERE file_id=?1 AND tag=?2",
        params![file_id, tag],
    )?;
    Ok(())
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

/// One catalogue entry.
#[derive(Debug, Clone)]
pub struct FileRow {
    pub id: i64,
    pub path: String,
    pub size: i64,
    pub mtime: i64,
    pub sha256: String,
    pub kind: String,
    pub width: i64,
    pub height: i64,
    pub taken_at: Option<String>,
    pub taken_src: String,
    pub lat: Option<f64>,
    pub lon: Option<f64>,
    pub camera: Option<String>,
    pub dhash: Option<i64>,
    pub sharpness: Option<f64>,
/// The video duration in seconds; `NULL` for photos and for files without ffprobe.
    pub duration_s: Option<f64>,
    pub thumb: Option<String>,
/// The owner's decision on a duplicate: `keep` | `drop` | `NULL` (undecided).
    pub decision: Option<String>,
/// The time the file was put into the app trash; `NULL` means it shows in the usual views.
    #[allow(dead_code)]
    pub trashed_at: Option<i64>,
}

pub fn insert(conn: &Connection, r: &FileRow) -> Result<()> {
    conn.execute(
        "INSERT INTO files(path,size,mtime,sha256,kind,width,height,taken_at,taken_src,lat,lon,camera,dhash,sharpness,duration_s,thumb,decision,indexed_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)
         ON CONFLICT(path) DO UPDATE SET
            size=excluded.size, sha256=excluded.sha256, kind=excluded.kind,
            width=excluded.width, height=excluded.height, taken_at=excluded.taken_at,
            taken_src=excluded.taken_src, lat=excluded.lat, lon=excluded.lon,
            camera=excluded.camera, dhash=excluded.dhash, sharpness=excluded.sharpness,
            duration_s=excluded.duration_s,
            thumb=excluded.thumb, indexed_at=excluded.indexed_at,
            decision=CASE WHEN files.sha256 IS excluded.sha256 THEN files.decision ELSE NULL END",
        params![
            r.path,
            r.size,
            r.mtime,
            r.sha256,
            r.kind,
            r.width,
            r.height,
            r.taken_at,
            r.taken_src,
            r.lat,
            r.lon,
            r.camera,
            r.dhash,
            r.sharpness,
            r.duration_s,
            r.thumb,
            r.decision,
            now(),
        ],
    )?;
    Ok(())
}

/// Mark one file of a group "keep", the rest "spare". Originals are untouched: DB rows only.
pub fn mark_keep(db: &Path, keep: i64, drops: &[i64]) -> Result<()> {
    let mut conn = open(db)?;
    mark_keep_conn(&mut conn, keep, drops)
}

fn mark_keep_conn(conn: &mut Connection, keep: i64, drops: &[i64]) -> Result<()> {
    let tx = conn.transaction()?;
    tx.execute(
        "UPDATE files SET decision='keep' WHERE id=?1",
        params![keep],
    )?;
    for &id in drops {
        tx.execute("UPDATE files SET decision='drop' WHERE id=?1", params![id])?;
    }
    tx.commit()?;
    Ok(())
}

pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub fn load_all(conn: &Connection) -> Result<Vec<FileRow>> {
    load_where(conn, "trashed_at IS NULL")
}

/// The files of the app trash: still on disk, but hidden from the usual views.
pub fn load_trashed(conn: &Connection) -> Result<Vec<FileRow>> {
    load_where(conn, "trashed_at IS NOT NULL")
}

const FILE_COLUMNS: &str = "id,path,size,sha256,kind,COALESCE(width,0),COALESCE(height,0),
                taken_at,COALESCE(taken_src,''),lat,lon,camera,dhash,sharpness,duration_s,thumb,decision,trashed_at,
                COALESCE(mtime,0)";

fn file_from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FileRow> {
    Ok(FileRow {
        id: r.get(0)?,
        path: r.get(1)?,
        size: r.get(2)?,
        mtime: r.get(18)?,
        sha256: r.get::<_, Option<String>>(3)?.unwrap_or_default(),
        kind: r.get(4)?,
        width: r.get(5)?,
        height: r.get(6)?,
        taken_at: r.get(7)?,
        taken_src: r.get(8)?,
        lat: r.get(9)?,
        lon: r.get(10)?,
        camera: r.get(11)?,
        dhash: r.get(12)?,
        sharpness: r.get(13)?,
        duration_s: r.get(14)?,
        thumb: r.get(15)?,
        decision: r.get(16)?,
        trashed_at: r.get(17)?,
    })
}

fn load_where(conn: &Connection, filter: &str) -> Result<Vec<FileRow>> {
    let sql = format!(
        "SELECT {FILE_COLUMNS} FROM files WHERE {filter}
         ORDER BY COALESCE(taken_at,'9999') , path"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([], file_from_row)?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Put a file into the app trash: nothing on disk changes.
pub fn mark_trashed(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "UPDATE files SET trashed_at=?1 WHERE id=?2",
        params![now(), file_id],
    )?;
    Ok(())
}

/// Bring a file back from the app trash.
pub fn untrash(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "UPDATE files SET trashed_at=NULL WHERE id=?1",
        params![file_id],
    )?;
    Ok(())
}

/// Drop a file from the index after it moved to the OS trash. The original is not ours any more.
pub fn purge_file(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    delete_file_rows(&conn, &[file_id])
}

/// Drop a folder from the index: DB rows, not files on disk.
pub fn remove_root(db: &Path, root: &str) -> Result<usize> {
    let mut conn = open(db)?;
    let root = root.trim_end_matches(['/', '\\']);
    let root = std::fs::canonicalize(root)
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_else(|_| root.to_string());
    let like = format!("{}{}%", escape_like(&root), std::path::MAIN_SEPARATOR);
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare("SELECT id FROM files WHERE path=?1 OR path LIKE ?2 ESCAPE '\\'")?;
        let rows = st.query_map(params![root, like], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<i64>>>()?
    };
    if ids.is_empty() {
        return Ok(0);
    }
    let tx = conn.transaction()?;
    delete_file_rows_tx(&tx, &ids)?;
    tx.commit()?;
    Ok(ids.len())
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn delete_file_rows(conn: &Connection, ids: &[i64]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    delete_file_rows_tx(&tx, ids)?;
    tx.commit()?;
    Ok(())
}

fn delete_file_rows_tx(tx: &Connection, ids: &[i64]) -> Result<()> {
    // Albums keep file ids only: the row is removed together with the file, otherwise
    // a dangling reference would stay and the album order would keep a hole.
    let mut albums: Vec<i64> = Vec::new();
    for id in ids {
        let file_albums: Vec<i64> = {
            let mut st =
                tx.prepare("SELECT DISTINCT album_id FROM album_files WHERE file_id=?1")?;
            let rows = st.query_map([id], |r| r.get(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        for album in file_albums {
            if !albums.contains(&album) {
                albums.push(album);
            }
        }
        tx.execute("DELETE FROM album_files WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM ocr WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM embeddings WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM tags WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM tag_hidden WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM user_tags WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM faces WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM face_scans WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM files WHERE id=?1", params![id])?;
    }
    for album in albums {
        resequence_album_files(tx, album)?;
    }
    delete_empty_face_clusters(tx)?;
    Ok(())
}

/// sha256 of the vectors already computed: file_id → the file's sha256 at the time of the run.
pub fn embedding_shas(conn: &Connection, model: &str) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT file_id, sha256 FROM embeddings WHERE model=?1")?;
    let rows = st.query_map([model], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, sha) = r?;
        out.insert(id, sha);
    }
    Ok(out)
}

/// Every vector of a model: `(file_id, f32)` for cosine search and clustering.
pub fn load_embeddings(conn: &Connection, model: &str) -> Result<Vec<(i64, Vec<f32>)>> {
    let mut st = conn.prepare("SELECT file_id, vec FROM embeddings WHERE model=?1")?;
    let rows = st.query_map([model], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id, blob) = r?;
        let vec: Vec<f32> = blob
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| f32::from_le_bytes(*b))
            .collect();
        out.push((id, vec));
    }
    Ok(out)
}

/// Drop the vectors of a model: a recompute with a new model input. The archive and previews are untouched.
/// Drop the vectors of one model only for the given files (M26.9): a pass switched off for a
/// catalog must not lose what a previous pass produced.
pub fn clear_embeddings_for(conn: &Connection, model: &str, ids: &[i64]) -> Result<usize> {
    let mut st = conn.prepare("DELETE FROM embeddings WHERE model=?1 AND file_id=?2")?;
    let mut n = 0;
    for id in ids {
        n += st.execute(params![model, id])?;
    }
    Ok(n)
}

pub fn save_embedding(
    conn: &Connection,
    file_id: i64,
    model: &str,
    sha256: &str,
    vec: &[f32],
) -> Result<()> {
    let mut blob = Vec::with_capacity(vec.len() * 4);
    for v in vec {
        blob.extend_from_slice(&v.to_le_bytes());
    }
    conn.execute(
        "INSERT INTO embeddings(file_id,model,sha256,dim,vec) VALUES(?1,?2,?3,?4,?5)
         ON CONFLICT(file_id,model) DO UPDATE SET
            sha256=excluded.sha256, dim=excluded.dim, vec=excluded.vec",
        params![file_id, model, sha256, vec.len() as i64, blob],
    )?;
    Ok(())
}

/// One detected face: the box, the embedding and the cluster. The vector is f32 in a BLOB.
#[derive(Debug, Clone)]
pub struct FaceRow {
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub score: f32,
    pub quality: f32,
    pub thumb: Option<String>,
    pub embedding: Vec<f32>,
    pub cluster_id: Option<i64>,
    pub manual: bool,
}

/// A face for the interface: no embedding, but with the path and the date of the source file.
#[derive(Debug, Clone)]
pub struct FaceView {
    pub id: i64,
    pub file_id: i64,
    pub path: String,
    pub taken_at: Option<String>,
    pub thumb: Option<String>,
    pub score: f32,
    pub cluster_id: Option<i64>,
}

/// A face box over the frame of its file (M22.7): the photo in the full screen marks who is
/// in it. The coordinates are image pixels of the file as it is seen (EXIF orientation applied).
#[derive(Debug, Clone)]
pub struct FaceBox {
    /// The row of `faces`: a click on the box opens the photos of this very face (M22.28).
    pub id: i64,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub score: f32,
    /// The group of the face; `None` while it is not clustered yet.
    pub cluster_id: Option<i64>,
    /// The name of the group; empty while the group has no name.
    pub label: String,
}

#[derive(Debug, Clone)]
pub struct FaceCluster {
    pub id: i64,
    pub label: String,
    pub faces: i64,
    pub photos: i64,
}

fn f32_blob(v: &[f32]) -> Vec<u8> {
    let mut blob = Vec::with_capacity(v.len() * 4);
    for x in v {
        blob.extend_from_slice(&x.to_le_bytes());
    }
    blob
}

fn f32_from_blob(blob: &[u8]) -> Vec<f32> {
    blob.as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect()
}

/// sha256 of the files already scanned for faces: file_id → sha256 at the time of the pass.
pub fn face_scan_shas(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT file_id, sha256 FROM face_scans")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, sha) = r?;
        out.insert(id, sha);
    }
    Ok(out)
}

/// Replace the faces of one file and mark it scanned. An empty list is a result too.
pub fn save_faces_for_file(
    conn: &Connection,
    file_id: i64,
    sha256: &str,
    faces: &[FaceRow],
) -> Result<()> {
    conn.execute("DELETE FROM faces WHERE file_id=?1", params![file_id])?;
    {
        let mut st = conn.prepare(
            "INSERT INTO faces(file_id,sha256,x,y,w,h,score,quality,thumb,embedding,created_at)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        )?;
        for f in faces {
            st.execute(params![
                file_id,
                sha256,
                f.x,
                f.y,
                f.w,
                f.h,
                f.score,
                f.quality,
                f.thumb,
                f32_blob(&f.embedding),
                now(),
            ])?;
        }
    }
    conn.execute(
        "INSERT INTO face_scans(file_id,sha256,faces,created_at) VALUES(?1,?2,?3,?4)
         ON CONFLICT(file_id) DO UPDATE SET
            sha256=excluded.sha256, faces=excluded.faces, created_at=excluded.created_at",
        params![file_id, sha256, faces.len() as i64, now()],
    )?;
    Ok(())
}

/// Every face with its embedding — for clustering.
pub fn load_faces(conn: &Connection) -> Result<Vec<FaceRow>> {
    let mut st = conn.prepare(
        "SELECT id,x,y,w,h,score,quality,thumb,embedding,cluster_id,manual
         FROM faces ORDER BY id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FaceRow {
            id: r.get(0)?,
            x: r.get(1)?,
            y: r.get(2)?,
            w: r.get(3)?,
            h: r.get(4)?,
            score: r.get(5)?,
            quality: r.get(6)?,
            thumb: r.get(7)?,
            embedding: f32_from_blob(&r.get::<_, Vec<u8>>(8)?),
            cluster_id: r.get(9)?,
            manual: r.get::<_, i64>(10)? != 0,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Faces for the grid: `cluster_id=None` means all. The large and confident ones come first.
pub fn load_face_views(conn: &Connection, cluster_id: Option<i64>) -> Result<Vec<FaceView>> {
    let mut st = conn.prepare(
        "SELECT f.id,f.file_id,fi.path,fi.taken_at,f.thumb,f.score,f.cluster_id
         FROM faces f JOIN files fi ON fi.id=f.file_id
         WHERE ?1 IS NULL OR f.cluster_id=?1
         ORDER BY f.quality DESC, f.id",
    )?;
    let rows = st.query_map([cluster_id], |r| {
        Ok(FaceView {
            id: r.get(0)?,
            file_id: r.get(1)?,
            path: r.get(2)?,
            taken_at: r.get(3)?,
            thumb: r.get(4)?,
            score: r.get(5)?,
            cluster_id: r.get(6)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// The faces of one file with the names of their groups: the chips over the frame in the full
/// screen. An unnamed group gives an empty label.
pub fn load_face_boxes(conn: &Connection, file_id: i64) -> Result<Vec<FaceBox>> {
    let mut st = conn.prepare(
        "SELECT f.id,f.x,f.y,f.w,f.h,f.score,f.cluster_id,COALESCE(c.label,'')
         FROM faces f LEFT JOIN face_clusters c ON c.id=f.cluster_id
         WHERE f.file_id=?1 ORDER BY f.quality DESC, f.id",
    )?;
    let rows = st.query_map([file_id], |r| {
        Ok(FaceBox {
            id: r.get(0)?,
            x: r.get(1)?,
            y: r.get(2)?,
            w: r.get(3)?,
            h: r.get(4)?,
            score: r.get(5)?,
            cluster_id: r.get(6)?,
            label: r.get(7)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn load_face_clusters(conn: &Connection) -> Result<Vec<FaceCluster>> {
    let mut st = conn.prepare(
        "SELECT c.id,c.label,COUNT(f.id),COUNT(DISTINCT f.file_id)
         FROM face_clusters c JOIN faces f ON f.cluster_id=c.id
         GROUP BY c.id ORDER BY c.label='', c.label, COUNT(DISTINCT f.file_id) DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FaceCluster {
            id: r.get(0)?,
            label: r.get(1)?,
            faces: r.get(2)?,
            photos: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn create_face_cluster(conn: &Connection) -> Result<i64> {
    conn.execute(
        "INSERT INTO face_clusters(label,created_at) VALUES('',?1)",
        params![now()],
    )?;
    Ok(conn.last_insert_rowid())
}

/// Store the assignment of faces to clusters. `manual` means the owner set it by hand.
pub fn save_face_assignments(
    conn: &mut Connection,
    assignments: &[(i64, Option<i64>)],
    manual: bool,
) -> Result<()> {
    let tx = conn.transaction()?;
    {
        let mut st = tx.prepare("UPDATE faces SET cluster_id=?1, manual=?2 WHERE id=?3")?;
        for &(face_id, cluster_id) in assignments {
            st.execute(params![cluster_id, manual as i64, face_id])?;
        }
    }
    tx.commit()?;
    Ok(())
}

pub fn label_face_cluster(conn: &Connection, cluster_id: i64, label: &str) -> Result<()> {
    conn.execute(
        "UPDATE face_clusters SET label=?1 WHERE id=?2",
        params![label.trim(), cluster_id],
    )?;
    Ok(())
}

/// Put several picked faces into one named group. If the name exists already,
/// the existing group is used; an empty name creates a new one without a label.
/// `manual=1` pins the decision through a rebuild.
pub fn group_faces(conn: &Connection, face_ids: &[i64], label: &str) -> Result<i64> {
    let label = label.trim();
    let existing: Option<i64> = if label.is_empty() {
        None
    } else {
        conn.query_row(
            "SELECT id FROM face_clusters WHERE label=?1 LIMIT 1",
            params![label],
            |r| r.get(0),
        )
        .optional()?
    };
    let cluster_id = match existing {
        Some(id) => id,
        None => {
            conn.execute(
                "INSERT INTO face_clusters(label,created_at) VALUES(?1,?2)",
                params![label, now()],
            )?;
            conn.last_insert_rowid()
        }
    };
    let tx = conn.unchecked_transaction()?;
    {
        let mut st = tx.prepare("UPDATE faces SET cluster_id=?1, manual=1 WHERE id=?2")?;
        for &id in face_ids {
            st.execute(params![cluster_id, id])?;
        }
    }
    tx.commit()?;
    delete_empty_face_clusters(conn)?;
    Ok(cluster_id)
}

/// Split one face into a new group: a false merge is undone with one action.
pub fn split_face(conn: &mut Connection, face_id: i64) -> Result<i64> {
    let tx = conn.transaction()?;
    tx.execute(
        "INSERT INTO face_clusters(label,created_at) VALUES('',?1)",
        params![now()],
    )?;
    let cluster_id = tx.last_insert_rowid();
    let n = tx.execute(
        "UPDATE faces SET cluster_id=?1, manual=1 WHERE id=?2",
        params![cluster_id, face_id],
    )?;
    if n == 0 {
        anyhow::bail!("no such face: {face_id}");
    }
    tx.commit()?;
    Ok(cluster_id)
}

/// Remove clusters without faces. A named cluster stays even while it is empty.
pub fn delete_empty_face_clusters(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM face_clusters
         WHERE label='' AND id NOT IN (
            SELECT DISTINCT cluster_id FROM faces WHERE cluster_id IS NOT NULL
         )",
        [],
    )?)
}

/// The plan of the face pass: `(photos in all, not scanned yet)`.
pub fn face_scan_plan(conn: &Connection) -> Result<(usize, usize)> {
    let images: i64 = conn.query_row("SELECT COUNT(*) FROM files WHERE kind='image'", [], |r| {
        r.get(0)
    })?;
    let todo: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files f LEFT JOIN face_scans s ON s.file_id=f.id
         WHERE f.kind='image' AND COALESCE(s.sha256,'') != COALESCE(f.sha256,'')",
        [],
        |r| r.get(0),
    )?;
    Ok((images as usize, todo as usize))
}

/// `(faces, photos with faces, clusters)` — for the panel and the report.
pub fn face_summary(conn: &Connection) -> Result<(i64, i64, i64)> {
    let faces: i64 = conn.query_row("SELECT COUNT(*) FROM faces", [], |r| r.get(0))?;
    let photos: i64 = conn.query_row("SELECT COUNT(DISTINCT file_id) FROM faces", [], |r| {
        r.get(0)
    })?;
    let clusters: i64 = conn.query_row(
        "SELECT COUNT(*) FROM face_clusters WHERE id IN (SELECT cluster_id FROM faces WHERE cluster_id IS NOT NULL)",
        [],
        |r| r.get(0),
    )?;
    Ok((faces, photos, clusters))
}

/// Replace the tags of one source on a file (CLIP is recomputed as a whole).
pub fn replace_tags(
    conn: &Connection,
    file_id: i64,
    source: &str,
    tags: &[(String, f32)],
) -> Result<()> {
    conn.execute(
        "DELETE FROM tags WHERE file_id=?1 AND source=?2",
        params![file_id, source],
    )?;
    let mut st = conn.prepare("INSERT INTO tags(file_id,tag,score,source) VALUES(?1,?2,?3,?4)")?;
    for (tag, score) in tags {
        st.execute(params![file_id, tag, score, source])?;
    }
    Ok(())
}

/// Every tag: file_id → [(tag, weight)] by descending weight.
/// Every visible tag of the chosen source: file_id → [(tag, weight)] by descending weight.
/// Tags hidden by the owner (`tag_hidden`) do not appear in the result.
pub fn load_tags(conn: &Connection, source: &str) -> Result<HashMap<i64, Vec<(String, f32)>>> {
    let mut st = conn.prepare(
        "SELECT t.file_id, t.tag, COALESCE(t.score,0)
         FROM tags t
         LEFT JOIN tag_hidden h ON h.file_id=t.file_id AND h.tag=t.tag
         WHERE t.source=?1 AND h.tag IS NULL
         ORDER BY t.file_id, t.score DESC",
    )?;
    let rows = st.query_map([source], |r| {
        Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?))
    })?;
    let mut out: HashMap<i64, Vec<(String, f32)>> = HashMap::new();
    for r in rows {
        let (id, tag, score) = r?;
        out.entry(id).or_default().push((tag, score));
    }
    Ok(out)
}

/// A file fit for the labelled sample M15.7: an image not in the trash, with a preview and with
/// the model's tags. The order is by sha256: the sample does not depend on names or dates and repeats exactly.
#[derive(Debug, Clone)]
pub struct TaggedFile {
    pub id: i64,
    pub path: String,
    pub sha256: String,
    /// The preview path relative to `<cache>/photocat/thumbs`.
    pub thumb: Option<String>,
}

pub fn tagged_files(conn: &Connection, source: &str) -> Result<Vec<TaggedFile>> {
    let mut st = conn.prepare(
        "SELECT f.id, f.path, f.sha256, f.thumb
         FROM files f
         WHERE f.kind='image' AND f.thumb IS NOT NULL AND f.trashed_at IS NULL
           AND f.sha256 IS NOT NULL AND f.sha256 <> ''
           AND EXISTS(SELECT 1 FROM tags t WHERE t.file_id=f.id AND t.source=?1)
         ORDER BY f.sha256",
    )?;
    let rows = st.query_map([source], |r| {
        Ok(TaggedFile {
            id: r.get(0)?,
            path: r.get(1)?,
            sha256: r.get(2)?,
            thumb: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// sha256 of the text already recognized for a file: file_id → the file's sha256 at the time of the OCR.
pub fn ocr_shas(conn: &Connection) -> Result<HashMap<i64, String>> {
    let mut st = conn.prepare("SELECT file_id, sha256 FROM ocr")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, sha) = r?;
        out.insert(id, sha);
    }
    Ok(out)
}

/// Store the recognized text. The text lives in the DB: nothing is written next to the original.
pub fn save_ocr(
    conn: &Connection,
    file_id: i64,
    sha256: &str,
    lang: &str,
    text: &str,
) -> Result<()> {
    conn.execute(
        "INSERT INTO ocr(file_id,sha256,lang,text,chars,created_at)
         VALUES(?1,?2,?3,?4,?5,?6)
         ON CONFLICT(file_id) DO UPDATE SET
            sha256=excluded.sha256, lang=excluded.lang, text=excluded.text,
            chars=excluded.chars, created_at=excluded.created_at",
        params![
            file_id,
            sha256,
            lang,
            text,
            text.chars().count() as i64,
            now()
        ],
    )?;
    Ok(())
}

pub fn load_ocr(conn: &Connection, file_id: i64) -> Result<Option<String>> {
    let mut st = conn.prepare("SELECT text FROM ocr WHERE file_id=?1")?;
    let mut rows = st.query([file_id])?;
    match rows.next()? {
        Some(r) => Ok(Some(r.get(0)?)),
        None => Ok(None),
    }
}

/// How many files have text and how many characters were recognized in all.
pub fn ocr_summary(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(chars),0) FROM ocr",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// The recognized text of every file, file_id → characters: the window counts the files with
/// text over the visible set (M26.1), not over the whole DB.
pub fn ocr_file_counts(conn: &Connection) -> Result<HashMap<i64, i64>> {
    let mut st = conn.prepare("SELECT file_id, chars FROM ocr")?;
    let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (id, chars) = r?;
        out.insert(id, chars);
    }
    Ok(out)
}

/// The tag dictionary of a model in display order. Empty means the model has not been seeded yet.
pub fn load_tag_defs(conn: &Connection, model: &str) -> Result<Vec<TagDef>> {
    let mut st = conn.prepare(
        "SELECT tag_id, label, prompts, enabled, position, min_pct
         FROM tag_defs WHERE model=?1 ORDER BY position, label",
    )?;
    let rows = st.query_map([model], |r| {
        Ok(TagDef {
            model: model.to_string(),
            id: r.get(0)?,
            label: r.get(1)?,
            prompts: r
                .get::<_, String>(2)?
                .lines()
                .map(str::trim)
                .filter(|p| !p.is_empty())
                .map(str::to_string)
                .collect(),
            enabled: r.get::<_, i64>(3)? != 0,
            position: r.get(4)?,
            min_percentile: r.get(5)?,
        })
    })?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// The labels every dictionary gives the tags `document` and `screenshot`: the ids are stable, the
/// labels are the owner's (and follow the language of the defaults). Every model's dictionary is
/// read on purpose — `settings.model_id` is empty until the owner picks a model, so resolving the
/// dictionary through the setting silently found nothing and OCR lost its content-based candidates
/// (M23.6). The labels of these two ids cannot widen the heuristic beyond documents.
pub fn text_tag_labels(conn: &Connection) -> Vec<String> {
    let Ok(mut st) = conn.prepare(
        "SELECT DISTINCT label FROM tag_defs
         WHERE enabled<>0 AND tag_id IN ('document','screenshot') ORDER BY label",
    ) else {
        return Vec::new();
    };
    let Ok(rows) = st.query_map([], |r| r.get::<_, String>(0)) else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// Store a dictionary tag in place (by `model`+`id`) without touching its neighbours.
/// The label is unique inside a model: otherwise `tags` would get two rows with one key.
pub fn save_tag_def(conn: &Connection, d: &TagDef) -> Result<()> {
    let taken: Option<String> = conn
        .query_row(
            "SELECT tag_id FROM tag_defs WHERE model=?1 AND label=?2 AND tag_id<>?3",
            params![d.model, d.label, d.id],
            |r| r.get(0),
        )
        .ok();
    if let Some(other) = taken {
        anyhow::bail!("the label “{}” is already taken by tag {other}", d.label);
    }
    let prompts = d
        .prompts
        .iter()
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    conn.execute(
        "INSERT INTO tag_defs(model,tag_id,label,prompts,enabled,position,min_pct)
         VALUES(?1,?2,?3,?4,?5,?6,?7)
         ON CONFLICT(model,tag_id) DO UPDATE SET
            label=excluded.label, prompts=excluded.prompts, enabled=excluded.enabled,
            position=excluded.position, min_pct=excluded.min_pct",
        params![
            d.model,
            d.id,
            d.label,
            prompts,
            d.enabled as i64,
            d.position,
            d.min_percentile.clamp(0, 100)
        ],
    )?;
    Ok(())
}

/// Remove a tag from the dictionary. The `tags` rows with this label are deleted by the tag recompute.
pub fn delete_tag_def(conn: &Connection, model: &str, id: &str) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM tag_defs WHERE model=?1 AND tag_id=?2",
        params![model, id],
    )?)
}

/// Replace the whole dictionary of a model (JSON import, "back to the default tags").
pub fn replace_tag_defs(conn: &Connection, model: &str, defs: &[TagDef]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM tag_defs WHERE model=?1", [model])?;
    for d in defs {
        save_tag_def(&tx, d)?;
    }
    tx.commit()?;
    Ok(())
}

/// How many files each tag of the source has — for `tags --list` and an empty dictionary.
pub fn tag_file_counts(conn: &Connection, source: &str) -> Result<HashMap<String, usize>> {
    let mut st = conn.prepare("SELECT tag, COUNT(*) FROM tags WHERE source=?1 GROUP BY tag")?;
    let rows = st.query_map([source], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)))?;
    let mut out = HashMap::new();
    for r in rows {
        let (tag, n) = r?;
        out.insert(tag, n as usize);
    }
    Ok(out)
}

/// Every visible tag with weights: file_id → [(tag, weight)] by descending weight.
/// One tag from several models is taken by the best weight, so guesses are not duplicated.
/// The OCR pass needs it: the tag "document"/"screenshot" is the reason to look for text.
pub fn load_visible_tags(conn: &Connection) -> Result<HashMap<i64, Vec<(String, f32)>>> {
    let mut st = conn.prepare(
        "WITH visible AS (
            SELECT t.file_id, t.tag, MAX(COALESCE(t.score,0)) AS score
            FROM tags t
            LEFT JOIN tag_hidden h ON h.file_id=t.file_id AND h.tag=t.tag
            WHERE h.tag IS NULL
            GROUP BY t.file_id, t.tag
         )
         SELECT file_id, tag, score FROM visible ORDER BY file_id, score DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, f32>(2)?,
        ))
    })?;
    let mut out: HashMap<i64, Vec<(String, f32)>> = HashMap::new();
    for r in rows {
        let (id, tag, score) = r?;
        out.entry(id).or_default().push((tag, score));
    }
    Ok(out)
}

/// An FTS5 query: every word is a prefix, quotes strip the special characters.
/// An empty string means `None`, not an FTS syntax error.
pub fn fts_query(q: &str) -> Option<String> {
    let terms: Vec<String> = q
        .split_whitespace()
        .map(|t| t.replace(['"', '*'], ""))
        .filter(|t| !t.is_empty())
        .map(|t| format!("\"{t}\"*"))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" AND "))
    }
}

/// Full-text search over recognized text: `(file_id, a fragment with "…")`.
/// The fragment is already marked up: the UI colours the matches, the CLI prints it as is.
pub fn search_ocr(conn: &Connection, q: &str, limit: usize) -> Result<Vec<(i64, String)>> {
    let Some(match_q) = fts_query(q) else {
        return Ok(Vec::new());
    };
    let mut st = conn.prepare(
        "SELECT o.file_id, snippet(ocr_fts, 0, '«', '»', '…', 12)
         FROM ocr_fts JOIN ocr o ON o.file_id = ocr_fts.rowid
         WHERE ocr_fts MATCH ?1
         ORDER BY rank LIMIT ?2",
    )?;
    let rows = st.query_map(params![match_q, limit as i64], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

pub fn human(bytes: i64) -> String {
    human_with(bytes, ["B", "KiB", "MiB", "GiB", "TiB"])
}

/// The same as `human`, but with the units of the interface language (M21).
/// The number format is one, only the unit labels change, so the function lives here,
/// next to `human`, and not in `i18n`.
pub fn human_in(lang: crate::i18n::Lang, bytes: i64) -> String {
    human_with(bytes, crate::i18n::units(lang))
}

fn human_with(bytes: i64, units: [&str; 5]) -> String {
    let b = bytes as f64;
    let mut v = b;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{v:.0} {}", units[i])
    } else {
        format!("{v:.1} {}", units[i])
    }
}

/// The video duration: `M:SS`, from an hour `H:MM:SS`. Rounded to the second.
pub fn duration(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

/// An album tree node (M19). An album is DB rows only: no file on disk is created,
/// changed or moved.
#[derive(Debug, Clone)]
pub struct AlbumNode {
    pub id: i64,
    /// 0 is the root of the tree.
    pub parent_id: i64,
    pub position: i64,
    pub name: String,
    pub note: String,
    /// The depth in the tree: 0 at the root. The interface needs it for the indent.
    pub depth: usize,
    /// How many files the album has (including those in the app trash).
    pub files: i64,
    /// How many of them are in the app trash: the grid hides those.
    pub trashed: i64,
    pub subalbums: i64,
    pub bytes: i64,
    /// Unix seconds of the last change of the album or of its contents: the card shows the date
    /// and "by update" sorts by it (M22.20).
    pub updated_at: String,
    /// Up to four previews of the first files in order that are not in the trash, in album order:
    /// the album card is a mosaic of them (M22.21). Empty for an album without previews.
    pub covers: Vec<String>,
}

/// How many previews of an album a card shows: one photo fills it, four split it into a 2×2
/// mosaic (M22.21). A larger album keeps the first photos in its order.
pub const ALBUM_COVERS: usize = 4;

/// The order of two sibling albums for the cards (M22.20). A tie keeps the album order, so the
/// screen does not shuffle on equal numbers.
pub fn album_order(a: &AlbumNode, b: &AlbumNode, sort: AlbumSort) -> std::cmp::Ordering {
    let by_position = || (a.position, a.id).cmp(&(b.position, b.id));
    let stamp = |n: &AlbumNode| n.updated_at.parse::<i64>().unwrap_or(0);
    match sort {
        AlbumSort::Position => by_position(),
        AlbumSort::Updated => stamp(b).cmp(&stamp(a)).then_with(by_position),
        AlbumSort::Name => a
            .name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(by_position),
        AlbumSort::Count => b.files.cmp(&a.files).then_with(by_position),
        AlbumSort::Size => b.bytes.cmp(&a.bytes).then_with(by_position),
    }
}

fn album_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("the album name is empty");
    }
    if name.chars().count() > 80 {
        anyhow::bail!("the album name is longer than 80 characters");
    }
    if name.chars().any(char::is_control) {
        anyhow::bail!("the album name contains a control character");
    }
    Ok(name.to_string())
}

fn album_exists(conn: &Connection, id: i64) -> Result<bool> {
    Ok(conn
        .query_row("SELECT 1 FROM albums WHERE id=?1", [id], |_| Ok(()))
        .optional()?
        .is_some())
}

fn sibling_name_free(conn: &Connection, parent: i64, name_key: &str, except: i64) -> Result<()> {
    let taken: Option<i64> = conn
        .query_row(
            "SELECT id FROM albums WHERE parent_id=?1 AND name_key=?2 AND id<>?3",
            params![parent, name_key, except],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = taken {
        anyhow::bail!("an album with this name already exists among the siblings (id {id})");
    }
    Ok(())
}

fn resequence_albums(conn: &Connection, parent: i64) -> Result<()> {
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare("SELECT id FROM albums WHERE parent_id=?1 ORDER BY position, id")?;
        let rows = st.query_map([parent], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE albums SET position=?1 WHERE id=?2",
            params![position as i64, id],
        )?;
    }
    Ok(())
}

fn resequence_album_files(conn: &Connection, album_id: i64) -> Result<()> {
    let ids: Vec<i64> = {
        let mut st = conn.prepare(
            "SELECT file_id FROM album_files WHERE album_id=?1 ORDER BY position, file_id",
        )?;
        let rows = st.query_map([album_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for (position, file_id) in ids.iter().enumerate() {
        conn.execute(
            "UPDATE album_files SET position=?1 WHERE album_id=?2 AND file_id=?3",
            params![position as i64, album_id, file_id],
        )?;
    }
    Ok(())
}

fn album_rows(conn: &Connection) -> Result<HashMap<i64, (i64, String)>> {
    let mut st = conn.prepare("SELECT id, parent_id, name FROM albums")?;
    let rows = st.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)))?;
    let mut map = HashMap::new();
    for row in rows {
        let (id, parent, name) = row?;
        map.insert(id, (parent, name));
    }
    Ok(map)
}

fn path_in(map: &HashMap<i64, (i64, String)>, id: i64) -> String {
    let mut names = Vec::new();
    let mut at = id;
    for _ in 0..256 {
        let Some((parent, name)) = map.get(&at) else {
            break;
        };
        names.push(name.clone());
        if *parent == 0 {
            break;
        }
        at = *parent;
    }
    names.reverse();
    names.join(" / ")
}

/// The path of an album in the tree: "Dacha 2024 / May".
pub fn album_path(conn: &Connection, id: i64) -> Result<String> {
    let map = album_rows(conn)?;
    if !map.contains_key(&id) {
        anyhow::bail!("there is no album {id}");
    }
    Ok(path_in(&map, id))
}

/// Create an album at the end of its siblings. `parent_id = 0` is the root of the tree.
pub fn create_album(conn: &Connection, parent_id: i64, name: &str, note: &str) -> Result<i64> {
    let name = album_name(name)?;
    let tx = conn.unchecked_transaction()?;
    if parent_id != 0 && !album_exists(&tx, parent_id)? {
        anyhow::bail!("there is no parent album {parent_id}");
    }
    let key = name.to_lowercase();
    sibling_name_free(&tx, parent_id, &key, 0)?;
    let position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position),-1)+1 FROM albums WHERE parent_id=?1",
        [parent_id],
        |r| r.get(0),
    )?;
    let stamp = now().to_string();
    tx.execute(
        "INSERT INTO albums(parent_id,position,name,name_key,note,created_at,updated_at)
         VALUES(?1,?2,?3,?4,?5,?6,?6)",
        params![parent_id, position, name, key, note.trim(), stamp],
    )?;
    let id = tx.last_insert_rowid();
    tx.commit()?;
    Ok(id)
}

/// Rename an album. The name is unique among the siblings, case-insensitively.
pub fn rename_album(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = album_name(name)?;
    let tx = conn.unchecked_transaction()?;
    let parent: i64 = tx
        .query_row("SELECT parent_id FROM albums WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .context("there is no album")?;
    let key = name.to_lowercase();
    sibling_name_free(&tx, parent, &key, id)?;
    tx.execute(
        "UPDATE albums SET name=?1, name_key=?2, updated_at=?3 WHERE id=?4",
        params![name, key, now().to_string(), id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Move an album to another parent. `position = None` means the end.
/// The new parent cannot be the album itself or its descendant.
pub fn move_album(
    conn: &Connection,
    id: i64,
    new_parent: i64,
    position: Option<i64>,
) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    let (old_parent, name_key): (i64, String) = tx
        .query_row(
            "SELECT parent_id, name_key FROM albums WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .context("there is no album")?;
    if new_parent == id {
        anyhow::bail!("an album cannot be moved into itself");
    }
    if new_parent != 0 && !album_exists(&tx, new_parent)? {
        anyhow::bail!("there is no parent album {new_parent}");
    }
    // A cycle: walk up from the new parent; reaching the id means a refusal.
    let mut at = new_parent;
    for _ in 0..1000 {
        if at == 0 {
            break;
        }
        let parent: Option<i64> = tx
            .query_row("SELECT parent_id FROM albums WHERE id=?1", [at], |r| {
                r.get(0)
            })
            .optional()?;
        let Some(parent) = parent else {
            break;
        };
        if parent == id {
            anyhow::bail!("an album cannot be moved into its own subalbum");
        }
        at = parent;
    }
    sibling_name_free(&tx, new_parent, &name_key, id)?;
    let mut siblings: Vec<i64> = {
        let mut st = tx.prepare(
            "SELECT id FROM albums WHERE parent_id=?1 AND id<>?2 ORDER BY position, id",
        )?;
        let rows = st.query_map(params![new_parent, id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let at = position
        .unwrap_or(siblings.len() as i64)
        .clamp(0, siblings.len() as i64) as usize;
    siblings.insert(at, id);
    tx.execute(
        "UPDATE albums SET parent_id=?1, updated_at=?2 WHERE id=?3",
        params![new_parent, now().to_string(), id],
    )?;
    for (p, sibling) in siblings.iter().enumerate() {
        tx.execute(
            "UPDATE albums SET position=?1 WHERE id=?2",
            params![p as i64, sibling],
        )?;
    }
    if old_parent != new_parent {
        resequence_albums(&tx, old_parent)?;
    }
    tx.commit()?;
    Ok(())
}

/// Delete an album with its whole subtree. Rows only: the files on disk are intact.
pub fn delete_album(conn: &Connection, id: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let parent: i64 = tx
        .query_row("SELECT parent_id FROM albums WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .context("there is no album")?;
    let ids: Vec<i64> = {
        let mut st = tx.prepare(
            "WITH RECURSIVE sub(id) AS (
                SELECT id FROM albums WHERE id=?1
                UNION ALL
                SELECT a.id FROM albums a JOIN sub ON a.parent_id=sub.id
             ) SELECT id FROM sub",
        )?;
        let rows = st.query_map([id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    for album in &ids {
        tx.execute("DELETE FROM album_files WHERE album_id=?1", [album])?;
        tx.execute("DELETE FROM albums WHERE id=?1", [album])?;
    }
    resequence_albums(&tx, parent)?;
    tx.commit()?;
    Ok(ids.len())
}

/// Append files to an album. A repeat does not duplicate rows: it returns
/// `(added, already there)`. Files missing from the index cannot appear here.
pub fn add_to_album(conn: &Connection, album_id: i64, file_ids: &[i64]) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;
    if !album_exists(&tx, album_id)? {
        anyhow::bail!("there is no album {album_id}");
    }
    let mut position: i64 = tx.query_row(
        "SELECT COALESCE(MAX(position),-1)+1 FROM album_files WHERE album_id=?1",
        [album_id],
        |r| r.get(0),
    )?;
    let mut added = 0usize;
    let mut already = 0usize;
    let mut seen = std::collections::HashSet::new();
    for &file_id in file_ids {
        if !seen.insert(file_id) {
            continue;
        }
        let in_index: bool = tx
            .query_row("SELECT 1 FROM files WHERE id=?1", [file_id], |_| Ok(()))
            .optional()?
            .is_some();
        if !in_index {
            continue;
        }
        let exists: bool = tx
            .query_row(
                "SELECT 1 FROM album_files WHERE album_id=?1 AND file_id=?2",
                params![album_id, file_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if exists {
            already += 1;
            continue;
        }
        tx.execute(
            "INSERT INTO album_files(album_id,file_id,position,added_at) VALUES(?1,?2,?3,?4)",
            params![album_id, file_id, position, now().to_string()],
        )?;
        position += 1;
        added += 1;
    }
    // The card of the album shows the date of its last change (M22.20).
    if added > 0 {
        tx.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params![now().to_string(), album_id],
        )?;
    }
    tx.commit()?;
    Ok((added, already))
}

/// Remove files from an album and compact the order. Returns the number of deleted rows.
pub fn remove_from_album(conn: &Connection, album_id: i64, file_ids: &[i64]) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let mut removed = 0usize;
    for &file_id in file_ids {
        removed += tx.execute(
            "DELETE FROM album_files WHERE album_id=?1 AND file_id=?2",
            params![album_id, file_id],
        )?;
    }
    if removed > 0 {
        resequence_album_files(&tx, album_id)?;
        tx.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params![now().to_string(), album_id],
        )?;
    }
    tx.commit()?;
    Ok(removed)
}

/// "Up/Down" inside an album: `delta` is −1 or +1. The order becomes dense 0..n−1 again.
pub fn move_in_album(
    conn: &Connection,
    album_id: i64,
    file_id: i64,
    delta: i64,
) -> Result<bool> {
    let tx = conn.unchecked_transaction()?;
    let mut ids: Vec<i64> = {
        let mut st = tx.prepare(
            "SELECT file_id FROM album_files WHERE album_id=?1 ORDER BY position, file_id",
        )?;
        let rows = st.query_map([album_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let Some(at) = ids.iter().position(|&id| id == file_id) else {
        return Ok(false);
    };
    let to = (at as i64 + delta).clamp(0, ids.len() as i64 - 1) as usize;
    if at == to {
        return Ok(false);
    }
    let moved = ids.remove(at);
    ids.insert(to, moved);
    for (position, id) in ids.iter().enumerate() {
        tx.execute(
            "UPDATE album_files SET position=?1 WHERE album_id=?2 AND file_id=?3",
            params![position as i64, album_id, id],
        )?;
    }
    tx.execute(
        "UPDATE albums SET updated_at=?1 WHERE id=?2",
        params![now().to_string(), album_id],
    )?;
    tx.commit()?;
    Ok(true)
}

/// The album tree in traversal order: the roots by `position`, then their subalbums.
/// `depth` is the indent for the interface; the counters are the rows and the size of the album.
pub fn albums_tree(conn: &Connection) -> Result<Vec<AlbumNode>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.parent_id, a.position, a.name, a.note, a.updated_at,
                (SELECT COUNT(*) FROM album_files af WHERE af.album_id=a.id),
                (SELECT COUNT(*) FROM album_files af JOIN files f ON f.id=af.file_id
                  WHERE af.album_id=a.id AND f.trashed_at IS NOT NULL),
                (SELECT COUNT(*) FROM albums c WHERE c.parent_id=a.id),
                (SELECT COALESCE(SUM(f.size),0) FROM album_files af
                  JOIN files f ON f.id=af.file_id WHERE af.album_id=a.id)
         FROM albums a ORDER BY a.parent_id, a.position, a.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(AlbumNode {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            position: r.get(2)?,
            name: r.get(3)?,
            note: r.get(4)?,
            updated_at: r.get(5)?,
            depth: 0,
            files: r.get(6)?,
            trashed: r.get(7)?,
            subalbums: r.get(8)?,
            bytes: r.get(9)?,
            covers: Vec::new(),
        })
    })?;
    // The previews of every album in one pass, at most `ALBUM_COVERS` per album and in the album
    // order. A file without a preview is skipped, not replaced by a placeholder.
    let mut covers: HashMap<i64, Vec<String>> = HashMap::new();
    let mut cover_st = conn.prepare(
        "SELECT af.album_id, f.thumb FROM album_files af JOIN files f ON f.id=af.file_id
         WHERE f.trashed_at IS NULL AND f.thumb IS NOT NULL
         ORDER BY af.album_id, af.position, f.id",
    )?;
    let cover_rows = cover_st.query_map([], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    for row in cover_rows {
        let (album, thumb) = row?;
        let list = covers.entry(album).or_default();
        if list.len() < ALBUM_COVERS {
            list.push(thumb);
        }
    }
    let mut by_parent: HashMap<i64, Vec<AlbumNode>> = HashMap::new();
    for row in rows {
        let mut node = row?;
        node.covers = covers.remove(&node.id).unwrap_or_default();
        by_parent.entry(node.parent_id).or_default().push(node);
    }
    fn walk(
        parent: i64,
        depth: usize,
        by_parent: &mut HashMap<i64, Vec<AlbumNode>>,
        out: &mut Vec<AlbumNode>,
    ) {
        let Some(nodes) = by_parent.remove(&parent) else {
            return;
        };
        for mut node in nodes {
            node.depth = depth;
            let id = node.id;
            out.push(node);
            walk(id, depth + 1, by_parent, out);
        }
    }
    let mut out = Vec::new();
    walk(0, 0, &mut by_parent, &mut out);
    // A row with a lost parent must not disappear from the interface.
    while let Some(&parent) = by_parent.keys().min() {
        walk(parent, 0, &mut by_parent, &mut out);
    }
    Ok(out)
}

/// The files of an album in its order. Includes the app trash rows: the interface
/// hides them in the grid and counts them separately, while the data stays.
pub fn album_items(conn: &Connection, album_id: i64) -> Result<Vec<FileRow>> {
    let sql = "SELECT f.id, f.path, f.size, f.sha256, f.kind, COALESCE(f.width,0),
                COALESCE(f.height,0), f.taken_at, COALESCE(f.taken_src,''), f.lat, f.lon,
                f.camera, f.dhash, f.sharpness, f.duration_s, f.thumb, f.decision,
                f.trashed_at, COALESCE(f.mtime,0)
         FROM album_files af JOIN files f ON f.id=af.file_id
         WHERE af.album_id=?1 ORDER BY af.position, f.id";
    let mut st = conn.prepare(sql)?;
    let rows = st.query_map([album_id], file_from_row)?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// The subtree of an album in tree order: the root, then its subalbums.
/// The export needs it (M20): the structure on disk repeats this tree.
pub fn album_subtree(conn: &Connection, id: i64) -> Result<Vec<AlbumNode>> {
    let tree = albums_tree(conn)?;
    if !tree.iter().any(|n| n.id == id) {
        anyhow::bail!("there is no album {id}");
    }
    let mut ids = vec![id];
    loop {
        let mut grown = false;
        for node in &tree {
            if ids.contains(&node.parent_id) && !ids.contains(&node.id) {
                ids.push(node.id);
                grown = true;
            }
        }
        if !grown {
            break;
        }
    }
    Ok(tree.into_iter().filter(|n| ids.contains(&n.id)).collect())
}

/// Data for the album export (M20): the nodes of the subtree and their files in album
/// order. Read-only; not a single file on disk is opened.
pub fn album_export_data(
    conn: &Connection,
    id: i64,
) -> Result<Vec<(AlbumNode, Vec<FileRow>)>> {
    let mut out = Vec::new();
    for node in album_subtree(conn, id)? {
        let files = album_items(conn, node.id)?;
        out.push((node, files));
    }
    Ok(out)
}

/// The albums a file lies in: the id and the path in the tree. For the inspector and the card.
pub fn album_refs_of_file(conn: &Connection, file_id: i64) -> Result<Vec<(i64, String)>> {
    let map = album_rows(conn)?;
    let ids: Vec<i64> = {
        let mut st =
            conn.prepare("SELECT album_id FROM album_files WHERE file_id=?1 ORDER BY album_id")?;
        let rows = st.query_map([file_id], |r| r.get(0))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut out: Vec<(i64, String)> = ids
        .into_iter()
        .filter(|id| map.contains_key(id))
        .map(|id| (id, path_in(&map, id)))
        .collect();
    out.sort_by(|a, b| a.1.cmp(&b.1));
    Ok(out)
}

/// Only the album paths of a file — for the inspector (M19). The card takes
/// `album_refs_of_file`, because the "×" needs the id; this form is for reports.
#[allow(dead_code)]
pub fn albums_of_file(conn: &Connection, file_id: i64) -> Result<Vec<String>> {
    Ok(album_refs_of_file(conn, file_id)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}

/// Print the index statistics in the interface language (M21.6).
pub fn print_stats(lang: Lang, db_path: &Path) -> Result<()> {
    print!("{}", stats_text(lang, db_path)?);
    Ok(())
}

/// The statistics text as a separate function: the CLI prints it, a test checks the language
/// and the numbers without capturing stdout.
pub fn stats_text(lang: Lang, db_path: &Path) -> Result<String> {
    use std::fmt::Write;
    let conn = open(db_path)?;
    let mut out = String::new();
    let mut st = conn.prepare(
        "SELECT kind, COUNT(*), SUM(size) FROM files GROUP BY kind ORDER BY SUM(size) DESC",
    )?;
    let rows = st.query_map([], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut total_n = 0;
    let mut total_b = 0;
    let _ = writeln!(
        out,
        "{:<10} {:>10} {:>12}",
        i18n::t(lang, "kind"),
        i18n::t(lang, "files"),
        i18n::t(lang, "size")
    );
    for r in rows {
        let (k, n, b) = r?;
        total_n += n;
        total_b += b;
        let _ = writeln!(out, "{:<10} {:>10} {:>12}", k, n, human_in(lang, b));
    }
    let no_date: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE taken_at IS NULL",
        [],
        |r| r.get(0),
    )?;
    let no_hash: i64 =
        conn.query_row("SELECT COUNT(*) FROM files WHERE dhash IS NULL", [], |r| {
            r.get(0)
        })?;
    let uniq_sha: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT sha256) FROM files WHERE sha256 IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let uniq_dhash: i64 = conn.query_row(
        "SELECT COUNT(DISTINCT dhash) FROM files WHERE dhash IS NOT NULL",
        [],
        |r| r.get(0),
    )?;
    let _ = writeln!(out, "---");
    // A switched-off folder is named with its own count (M26.1): the rows are still in the index
    // and the totals below cover the whole DB, while the window and `index` work over the shown set.
    let settings = load_settings(&conn);
    if !settings.roots_off.is_empty() {
        let all: Vec<(String, i64)> = {
            let mut st = conn.prepare("SELECT path, size FROM files")?;
            let rows = st.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?;
            rows.collect::<rusqlite::Result<Vec<(String, i64)>>>()?
        };
        let mut hidden_n = 0usize;
        let mut hidden_b = 0i64;
        for root in &settings.roots_off {
            let (n, b) = all
                .iter()
                .filter(|(p, _)| under_root(p, root))
                .fold((0usize, 0i64), |(n, b), (_, size)| (n + 1, b + size));
            hidden_n += n;
            hidden_b += b;
            let _ = writeln!(
                out,
                "{}",
                i18n::fill(
                    lang,
                    "switched off: {} — {}",
                    &[
                        root.clone(),
                        format!(
                            "{} · {}",
                            i18n::count(lang, n, "file"),
                            human_in(lang, b)
                        ),
                    ]
                )
            );
        }
        let shown_n = all.len().saturating_sub(hidden_n);
        let shown_b = all.iter().map(|(_, size)| size).sum::<i64>() - hidden_b;
        let _ = writeln!(
            out,
            "{}",
            i18n::fill(
                lang,
                "shown: {}",
                &[format!(
                    "{} · {}",
                    i18n::count(lang, shown_n, "file"),
                    human_in(lang, shown_b)
                )]
            )
        );
    }
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "total: {} files, {}",
            &[total_n.to_string(), human_in(lang, total_b)]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(lang, "without a capture date: {}", &[no_date.to_string()])
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "without a perceptual hash: {}",
            &[no_hash.to_string()]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "unique sha256: {}  (exact copies: {})",
            &[uniq_sha.to_string(), (total_n - uniq_sha).to_string()]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "unique dhash: {}",
            &[uniq_dhash.to_string()]
        )
    );
    let (vid_n, vid_dur): (i64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(duration_s),0) FROM files
         WHERE kind='video' AND duration_s IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "videos with a duration: {}, {} in total",
            &[vid_n.to_string(), duration(vid_dur)]
        )
    );
    let (ocr_n, ocr_chars) = ocr_summary(&conn)?;
    if ocr_n > 0 {
        let _ = writeln!(
            out,
            "{}",
            i18n::fill(
                lang,
                "files with recognised text: {}, characters: {}",
                &[ocr_n.to_string(), ocr_chars.to_string()]
            )
        );
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, sha: &str, sharpness: f64) -> FileRow {
        FileRow {
            id: 0,
            path: path.into(),
            size: 1000,
            mtime: 0,
            sha256: sha.into(),
            kind: "image".into(),
            width: 100,
            height: 100,
            taken_at: None,
            taken_src: "mtime".into(),
            lat: None,
            lon: None,
            camera: None,
            dhash: Some(1),
            sharpness: Some(sharpness),
            duration_s: None,
            thumb: None,
            decision: None,
            trashed_at: None,
        }
    }

    fn id_of(c: &Connection, path: &str) -> i64 {
        c.query_row("SELECT id FROM files WHERE path=?1", [path], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn faces_survive_split_and_are_reset_with_content() {
        let mut c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
        let file_id = id_of(&c, "/a.jpg");
        let face = FaceRow {
            id: 0,
            x: 10,
            y: 20,
            w: 30,
            h: 40,
            score: 0.9,
            quality: 0.8,
            thumb: Some("aa/a-0.jpg".into()),
            embedding: vec![1.0, 0.0],
            cluster_id: None,
            manual: false,
        };
        save_faces_for_file(&c, file_id, "aa", &[face]).unwrap();
        assert_eq!(load_faces(&c).unwrap().len(), 1);
        assert_eq!(
            face_scan_shas(&c).unwrap().get(&file_id),
            Some(&"aa".into())
        );
        let face_id = load_faces(&c).unwrap()[0].id;
        let clustered = create_face_cluster(&c).unwrap();
        save_face_assignments(&mut c, &[(face_id, Some(clustered))], false).unwrap();
        label_face_cluster(&c, clustered, "Лиза").unwrap();
        assert_eq!(load_face_clusters(&c).unwrap()[0].label, "Лиза");

        // The chips over the frame in the full screen: the box and the name of the group.
        let boxes = load_face_boxes(&c, file_id).unwrap();
        assert_eq!(boxes.len(), 1);
        assert_eq!((boxes[0].x, boxes[0].y, boxes[0].w, boxes[0].h), (10, 20, 30, 40));
        assert_eq!(boxes[0].label, "Лиза");
        assert_eq!(boxes[0].id, face_id);
        assert_eq!(boxes[0].cluster_id, Some(clustered));
        assert!((boxes[0].score - 0.9).abs() < 1e-6);

        let split = split_face(&mut c, face_id).unwrap();
        let moved = &load_faces(&c).unwrap()[0];
        assert_eq!(moved.cluster_id, Some(split));
        assert!(moved.manual);
        assert_ne!(split, clustered);

        // Replacing the content wipes the faces and the pass mark — they must not be shown.
        insert(&c, &row("/a.jpg", "bb", 1.0)).unwrap();
        assert!(load_faces(&c).unwrap().is_empty());
        assert!(face_scan_shas(&c).unwrap().is_empty());
    }

    #[test]
    fn the_app_trash_hides_a_file_from_the_usual_views() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
        let id = id_of(&c, "/a.jpg");
        c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [id])
            .unwrap();
        assert!(load_all(&c).unwrap().is_empty());
        assert_eq!(load_trashed(&c).unwrap().len(), 1);
        c.execute("UPDATE files SET trashed_at=NULL WHERE id=?1", [id])
            .unwrap();
        assert_eq!(load_all(&c).unwrap().len(), 1);
        assert!(load_trashed(&c).unwrap().is_empty());
    }

    #[test]
    fn the_batch_face_assignment_builds_a_named_group() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
        let file_id = id_of(&c, "/a.jpg");
        let mk = |x: i32| FaceRow {
            id: 0,
            x,
            y: 20,
            w: 30,
            h: 40,
            score: 0.9,
            quality: 0.8,
            thumb: None,
            embedding: vec![1.0, 0.0],
            cluster_id: None,
            manual: false,
        };
        save_faces_for_file(&c, file_id, "aa", &[mk(10), mk(60)]).unwrap();
        let ids: Vec<i64> = load_faces(&c).unwrap().iter().map(|f| f.id).collect();
        let cid = group_faces(&c, &ids, "Лиза").unwrap();
        let clusters = load_face_clusters(&c).unwrap();
        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].id, cid);
        assert_eq!(clusters[0].label, "Лиза");
        assert!(
            load_faces(&c)
                .unwrap()
                .iter()
                .all(|f| f.cluster_id == Some(cid) && f.manual)
        );
    }

    #[test]
    fn schema_upgrade_keeps_old_db() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE files(
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
                mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
                taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
                thumb TEXT, indexed_at INTEGER NOT NULL);",
        )
        .unwrap();
        init(&c).unwrap();
        init(&c).unwrap(); // a repeated call does not fail
        let r = row("/old.jpg", "aa", 7.5);
        insert(&c, &r).unwrap();
        let loaded = load_all(&c).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].sharpness, Some(7.5));
        assert_eq!(loaded[0].decision, None);
        assert_eq!(loaded[0].duration_s, None);
    }

    /// Schema 7: tags of different models live side by side, and a sha256 change drops every CLIP tag.
    #[test]
    fn migration_7_does_not_mix_the_tags_of_two_models() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE files(
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
                mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
                taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
                thumb TEXT, indexed_at INTEGER NOT NULL);
             CREATE TABLE tags(
                file_id INTEGER NOT NULL, tag TEXT NOT NULL, score REAL, source TEXT,
                PRIMARY KEY(file_id, tag));
             CREATE TRIGGER invalidate_clip_after_sha_change
             AFTER UPDATE OF sha256 ON files WHEN old.sha256 IS NOT new.sha256
             BEGIN
                DELETE FROM embeddings WHERE file_id = new.id;
                DELETE FROM tags WHERE file_id = new.id AND source = 'clip';
             END;
             INSERT INTO files(id,path,size,sha256,kind,indexed_at)
                VALUES(1,'/a.jpg',10,'aa','image',0);
             INSERT INTO tags(file_id,tag,score,source) VALUES
                (1,'пляж',0.30,'clip:m1');
             PRAGMA user_version=6;",
        )
        .unwrap();
        init(&c).unwrap();
        let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(load_tags(&c, "clip:m1").unwrap()[&1][0].0, "пляж");
        assert!(load_tags(&c, "clip:m2").unwrap().is_empty());
        // The key now allows both models on one file — it used to be a conflict.
        replace_tags(&c, 1, "clip:m1", &[("пляж".into(), 0.35)]).unwrap();
        replace_tags(&c, 1, "clip:m2", &[("пляж".into(), 0.45)]).unwrap();
        assert_eq!(load_tags(&c, "clip:m1").unwrap()[&1].len(), 1);
        assert_eq!(load_tags(&c, "clip:m2").unwrap()[&1].len(), 1);
        // A content change drops the tags of every model, not only the legacy `clip`.
        c.execute("UPDATE files SET sha256='bb' WHERE id=1", []).unwrap();
        assert!(load_tags(&c, "clip:m1").unwrap().is_empty());
        assert!(load_tags(&c, "clip:m2").unwrap().is_empty());
    }

    #[test]
    fn hidden_tag_disappears_from_filter() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let r = row("/a.jpg", "aa", 10.0);
        insert(&c, &r).unwrap();
        let id = id_of(&c, "/a.jpg");
        replace_tags(&c, id, "clip", &[("пляж".into(), 0.3), ("еда".into(), 0.1)]).unwrap();
        assert_eq!(load_tags(&c, "clip").unwrap()[&id].len(), 2);
        c.execute(
            "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,'пляж',0)",
            params![id],
        )
        .unwrap();
        let left = load_tags(&c, "clip").unwrap();
        assert_eq!(left[&id].len(), 1);
        assert_eq!(left[&id][0].0, "еда");
    }

    #[test]
    fn hidden_tag_cleared_when_file_content_changes() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let r = row("/a.jpg", "aa", 10.0);
        insert(&c, &r).unwrap();
        let id = id_of(&c, "/a.jpg");
        c.execute(
            "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,'пляж',0)",
            params![id],
        )
        .unwrap();
        let mut changed = r.clone();
        changed.sha256 = "bb".into();
        insert(&c, &changed).unwrap();
        assert_eq!(load_hidden_counts(&c).unwrap().len(), 0);
    }

    #[test]
    fn fts_query_is_prefix_only_and_safe() {
        assert_eq!(
            fts_query("  справка 2024 ").as_deref(),
            Some("\"справка\"* AND \"2024\"*")
        );
        assert_eq!(fts_query(""), None);
        assert_eq!(fts_query("  \"*  "), None); // only special characters — not an FTS error, an empty query
        assert_eq!(fts_query("a\"b*").as_deref(), Some("\"ab\"*"));
    }

    #[test]
    fn ocr_text_is_searchable_and_highlighted() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        insert(&c, &row("/doc.jpg", "aa", 1.0)).unwrap();
        let id = id_of(&c, "/doc.jpg");
        save_ocr(&c, id, "aa", "rus+eng", "Справка о доходах\nза 2024 год").unwrap();
        assert_eq!(ocr_summary(&c).unwrap(), (1, 29));
        assert_eq!(
            load_ocr(&c, id).unwrap().as_deref(),
            Some("Справка о доходах\nза 2024 год")
        );
        let hits = search_ocr(&c, "справ", 10).unwrap();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].0, id);
        assert!(
            hits[0].1.contains('«') && hits[0].1.contains('»'),
            "{}",
            hits[0].1
        );
        assert!(search_ocr(&c, "договор", 10).unwrap().is_empty());
        // the FTS syntax must not turn user input into an error
        assert!(search_ocr(&c, "\"),", 10).is_ok());
    }

    #[test]
    fn ocr_text_is_invalidated_when_content_changes() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let r = row("/doc.jpg", "aa", 1.0);
        insert(&c, &r).unwrap();
        let id = id_of(&c, "/doc.jpg");
        save_ocr(&c, id, "aa", "rus+eng", "квитанция").unwrap();
        let mut changed = r.clone();
        changed.sha256 = "bb".into();
        insert(&c, &changed).unwrap();
        assert_eq!(load_ocr(&c, id).unwrap(), None);
        assert!(search_ocr(&c, "квитанция", 10).unwrap().is_empty());
    }

    #[test]
    fn top_tag_ignores_hidden_and_low_scores() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
        let id = id_of(&c, "/a.jpg");
        replace_tags(
            &c,
            id,
            "clip:m",
            &[("документ".into(), 0.30), ("дети".into(), 0.10)],
        )
        .unwrap();
        assert_eq!(load_visible_tags(&c).unwrap()[&id][0].0, "документ");
        c.execute(
            "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,'документ',0)",
            params![id],
        )
        .unwrap();
        assert_eq!(load_visible_tags(&c).unwrap()[&id][0].0, "дети");
    }

    #[test]
    fn the_old_tag_threshold_migrates_to_a_percentile() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        // An old DB: the filter slider drove tag_show down to 0.50, there is no percentile key.
        c.execute(
            "INSERT INTO settings(key,value) VALUES('tag_show','0.50')",
            [],
        )
        .unwrap();
        let s = load_settings(&c);
        assert_eq!(s.tag_show_pct, 70);
        assert!((s.tag_show - 0.22).abs() < 1e-6);
        // Once saved, the migration is fixed in the DB and does not run again.
        c.execute("UPDATE settings SET value='0.31' WHERE key='tag_show'", []).unwrap();
        save_settings(&c, &s).unwrap();
        c.execute("UPDATE settings SET value='0.31' WHERE key='tag_show'", []).unwrap();
        let back = load_settings(&c);
        assert_eq!(back.tag_show_pct, 70);
        assert!((back.tag_show - 0.31).abs() < 1e-6);
    }

    #[test]
    fn settings_survive_roundtrip() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let s = Settings {
            model_id: "clip-vit-b32-en".into(),
            models_dir: PathBuf::from("/tmp/models"),
            max_model_mb: 160,
            tag_show: 0.27,
            tag_margin: 0.02,
            tag_show_pct: 42,
            tile_size: 210.0,
            grid_layout: GridLayout::Flat,
            grid_sort: GridSort::Name,
            lang: crate::i18n::Lang::Ru,
            ocr_lang: "rus".into(),
            tools: crate::tools::Tools {
                ffmpeg: "/opt/ffmpeg".into(),
                tesseract: "/opt/tesseract".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        save_settings(&c, &s).unwrap();
        let back = load_settings(&c);
        assert_eq!(back.model_id, "clip-vit-b32-en");
        assert_eq!(back.models_dir, PathBuf::from("/tmp/models"));
        assert_eq!(back.max_model_mb, 160);
        assert!((back.tag_show - 0.27).abs() < 1e-6);
        assert_eq!(back.tag_show_pct, 42);
        assert!((back.tile_size - 210.0).abs() < 1e-6);
        assert_eq!(back.grid_layout, GridLayout::Flat);
        assert_eq!(back.grid_sort, GridSort::Name);
        assert_eq!(back.lang, crate::i18n::Lang::Ru);
        assert!((back.tag_margin - 0.02).abs() < 1e-6);
        assert_eq!(back.ocr_lang, "rus");
        assert_eq!(back.tools.ffmpeg, "/opt/ffmpeg");
        assert_eq!(back.tools.tesseract, "/opt/tesseract");
    }

    #[test]
    fn an_unknown_grid_layout_takes_the_default() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        save_settings(&c, &Settings::default()).unwrap();
        c.execute(
            "UPDATE settings SET value='wide' WHERE key='grid_layout'",
            [],
        )
        .unwrap();
        c.execute("UPDATE settings SET value='?' WHERE key='grid_sort'", [])
            .unwrap();
        let back = load_settings(&c);
        assert_eq!(back.grid_layout, GridLayout::Months);
        assert_eq!(back.grid_sort, GridSort::DateDesc);
        assert_eq!(GridSort::parse("album"), GridSort::Album);
        assert_eq!(GridSort::Album.as_str(), "album");
    }

    /// "Due / not due" — every case from M10: no manifest, switched off, exactly due,
    /// overdue, too early, and the clock turned back.
    #[test]
    fn the_reminder_counts_the_due_date_from_the_manifest() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-24T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let base = Settings {
            verify_root: "/tmp/архив".into(),
            verify_manifest: "/tmp/архив.sha256".into(),
            verify_interval_days: 30,
            ..Default::default()
        };
        // No manifest — nothing to remind about, even with a term set.
        let no_manifest = Settings {
            verify_manifest: String::new(),
            ..base.clone()
        };
        assert_eq!(verify_due(&no_manifest, now), VerifyDue::NoManifest);
        // Interval 0 — switched off by the owner.
        let off = Settings {
            verify_interval_days: 0,
            ..base.clone()
        };
        assert_eq!(verify_due(&off, now), VerifyDue::Disabled);
        // No last pass — due straight away.
        assert_eq!(verify_due(&base, now), VerifyDue::Due { days_over: 0 });
        // A broken date gives the same answer: better to remind than to stay silent.
        let broken = Settings {
            verify_last_at: "вчера".into(),
            ..base.clone()
        };
        assert_eq!(verify_due(&broken, now), VerifyDue::Due { days_over: 0 });
        // Exactly 30 days — due.
        let exactly = Settings {
            verify_last_at: (now - chrono::Duration::days(30)).to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&exactly, now), VerifyDue::Due { days_over: 0 });
        // Overdue by 5 days.
        let overdue = Settings {
            verify_last_at: (now - chrono::Duration::days(35)).to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&overdue, now), VerifyDue::Due { days_over: 5 });
        // Too early: 29 days and 12 hours out of 30 — one day is left.
        let early = Settings {
            verify_last_at: (now - chrono::Duration::days(29) - chrono::Duration::hours(12))
                .to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&early, now), VerifyDue::NotYet { days_left: 1 });
        // The clock was turned back: a date in the future — not due, and no panic.
        let future = Settings {
            verify_last_at: (now + chrono::Duration::days(5)).to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&future, now), VerifyDue::NotYet { days_left: 35 });
    }

    #[test]
    fn the_reminder_survives_a_restart() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let s = Settings {
            verify_root: "/tmp/архив".into(),
            verify_manifest: "/tmp/архив.sha256".into(),
            verify_interval_days: 14,
            verify_last_at: "2026-11-24T12:00:00+00:00".into(),
            verify_last_ok: true,
            verify_last_report: "сверка: всё совпало\n".into(),
            ..Default::default()
        };
        save_settings(&c, &s).unwrap();
        let back = load_settings(&c);
        assert_eq!(back.verify_root, "/tmp/архив");
        assert_eq!(back.verify_manifest, "/tmp/архив.sha256");
        assert_eq!(back.verify_interval_days, 14);
        assert_eq!(back.verify_last_at, "2026-11-24T12:00:00+00:00");
        assert!(back.verify_last_ok);
        assert_eq!(back.verify_last_report, "сверка: всё совпало\n");
    }

    #[test]
    fn face_threshold_survives_roundtrip() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let s = Settings {
            face_threshold: 0.51,
            ..Default::default()
        };
        save_settings(&c, &s).unwrap();
        let back = load_settings(&c);
        assert!((back.face_threshold - 0.51).abs() < 1e-6);
    }

    #[test]
    fn video_duration_survives_roundtrip() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let mut r = row("/v.mp4", "bb", 0.0);
        r.kind = "video".into();
        r.duration_s = Some(42.4);
        insert(&c, &r).unwrap();
        let loaded = load_all(&c).unwrap();
        assert_eq!(loaded[0].duration_s, Some(42.4));
    }

    #[test]
    fn duration_is_readable_in_minutes_and_hours() {
        assert_eq!(duration(0.0), "0:00");
        assert_eq!(duration(12.4), "0:12");
        assert_eq!(duration(65.0), "1:05");
        assert_eq!(duration(3725.0), "1:02:05");
        assert_eq!(duration(-5.0), "0:00");
        assert_eq!(duration(f64::NAN), "0:00");
    }

    #[test]
    fn clip_data_is_invalidated_when_content_changes() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let r = row("/a.jpg", "aa", 10.0);
        insert(&c, &r).unwrap();
        let id = id_of(&c, "/a.jpg");
        save_embedding(&c, id, "m", "aa", &[1.0, 0.0]).unwrap();
        replace_tags(&c, id, "clip", &[("пляж".into(), 0.3)]).unwrap();
        assert_eq!(embedding_shas(&c, "m").unwrap().len(), 1);
        assert_eq!(load_tags(&c, "clip").unwrap().len(), 1);

        let mut changed = r.clone();
        changed.sha256 = "bb".into();
        insert(&c, &changed).unwrap();
        assert!(embedding_shas(&c, "m").unwrap().is_empty());
        assert!(load_tags(&c, "clip").unwrap().is_empty());
    }

    #[test]
    fn decision_survives_same_content_and_resets_on_change() {
        let mut c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let r = row("/a.jpg", "aa", 10.0);
        insert(&c, &r).unwrap();
        let id = id_of(&c, "/a.jpg");
        let other = row("/b.jpg", "bb", 20.0);
        insert(&c, &other).unwrap();
        let other_id = id_of(&c, "/b.jpg");
        mark_keep_conn(&mut c, id, &[other_id]).unwrap();

        let loaded = load_all(&c).unwrap();
        let a = loaded.iter().find(|r| r.path == "/a.jpg").unwrap();
        let b = loaded.iter().find(|r| r.path == "/b.jpg").unwrap();
        assert_eq!(a.decision.as_deref(), Some("keep"));
        assert_eq!(b.decision.as_deref(), Some("drop"));

        insert(&c, &r).unwrap(); // re-indexing the same file
        assert_eq!(load_all(&c).unwrap()[0].decision.as_deref(), Some("keep"));

        let mut changed = r.clone();
        changed.sha256 = "cc".into();
        insert(&c, &changed).unwrap(); // the file was replaced — the decision went stale
        assert_eq!(load_all(&c).unwrap()[0].decision, None);
    }
    fn three_files(c: &Connection) -> Vec<i64> {
        for (path, sha) in [("/a.jpg", "aa"), ("/b.jpg", "bb"), ("/c.jpg", "cc")] {
            insert(c, &row(path, sha, 1.0)).unwrap();
        }
        vec![id_of(c, "/a.jpg"), id_of(c, "/b.jpg"), id_of(c, "/c.jpg")]
    }

    #[test]
    fn the_album_tree_follows_the_order_and_counts_files() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let dacha = create_album(&c, 0, "Дача 2024", "").unwrap();
        let may = create_album(&c, dacha, "Май", "").unwrap();
        let june = create_album(&c, dacha, "Июнь", "").unwrap();
        let year = create_album(&c, may, "1 год", "").unwrap();
        let to_print = create_album(&c, 0, "На печать", "").unwrap();
        add_to_album(&c, dacha, &[ids[0], ids[1]]).unwrap();
        add_to_album(&c, june, &[ids[2]]).unwrap();

        let tree = albums_tree(&c).unwrap();
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Дача 2024", "Май", "1 год", "Июнь", "На печать"]);
        let depths: Vec<usize> = tree.iter().map(|n| n.depth).collect();
        assert_eq!(depths, [0, 1, 2, 1, 0]);
        let root = tree.iter().find(|n| n.id == dacha).unwrap();
        assert_eq!((root.files, root.subalbums), (2, 2));
        assert!(tree.iter().any(|n| n.id == year && n.files == 0));
        assert_eq!(album_path(&c, year).unwrap(), "Дача 2024 / Май / 1 год");
        assert_eq!(
            albums_of_file(&c, ids[0]).unwrap(),
            vec!["Дача 2024".to_string()]
        );

        // The order of subalbums is data: "May" can go below "June".
        move_album(&c, may, dacha, Some(1)).unwrap();
        let tree = albums_tree(&c).unwrap();
        let siblings: Vec<&str> = tree
            .iter()
            .filter(|n| n.parent_id == dacha)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(siblings, ["Июнь", "Май"]);
        // The "To print" branch stayed in place.
        assert_eq!(
            tree.iter().find(|n| n.id == to_print).unwrap().position,
            1
        );
    }

    #[test]
    fn an_album_carries_up_to_four_previews_in_its_order() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let mut ids = Vec::new();
        for i in 0..6 {
            let mut r = row(&format!("/{i}.jpg"), &format!("sha{i}"), 1.0);
            r.thumb = Some(format!("thumbs/{i}.jpg"));
            insert(&c, &r).unwrap();
            ids.push(id_of(&c, &format!("/{i}.jpg")));
        }
        let album = create_album(&c, 0, "Day", "").unwrap();
        // The album order decides which four previews the card gets, not the id order.
        add_to_album(&c, album, &[ids[3], ids[5], ids[1], ids[4], ids[0], ids[2]]).unwrap();
        let node = |c: &Connection| {
            albums_tree(c)
                .unwrap()
                .into_iter()
                .find(|n| n.id == album)
                .unwrap()
        };
        assert_eq!(
            node(&c).covers,
            ["thumbs/3.jpg", "thumbs/5.jpg", "thumbs/1.jpg", "thumbs/4.jpg"]
        );
        // A trashed file and a file without a preview drop out; the next ones fill the mosaic.
        c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [ids[3]])
            .unwrap();
        c.execute("UPDATE files SET thumb=NULL WHERE id=?1", [ids[5]])
            .unwrap();
        assert_eq!(
            node(&c).covers,
            ["thumbs/1.jpg", "thumbs/4.jpg", "thumbs/0.jpg", "thumbs/2.jpg"]
        );
    }

    #[test]
    fn the_name_is_unique_among_siblings_and_survives_an_empty_one() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let dacha_2024 = create_album(&c, 0, "Дача 2024", "").unwrap();
        let dacha_2025 = create_album(&c, 0, "Дача 2025", "").unwrap();
        assert!(create_album(&c, 0, "дача 2024", "").is_err());
        // Equal names are allowed in different branches.
        create_album(&c, dacha_2024, "Май", "").unwrap();
        create_album(&c, dacha_2025, "Май", "").unwrap();
        assert!(create_album(&c, dacha_2024, "  ", "").is_err());
        assert!(create_album(&c, 0, &"я".repeat(81), "").is_err());
        // 80 characters are allowed already; trailing spaces are trimmed.
        let id = create_album(&c, 0, &format!("  {}  ", "я".repeat(80)), "").unwrap();
        assert_eq!(album_path(&c, id).unwrap().chars().count(), 80);
        assert!(rename_album(&c, dacha_2024, "Дача 2025").is_err());
        rename_album(&c, dacha_2024, "Дача на даче").unwrap();
        assert_eq!(album_path(&c, dacha_2024).unwrap(), "Дача на даче");
    }

    #[test]
    fn moving_refuses_a_cycle_and_renumbers_the_siblings() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let a = create_album(&c, 0, "A", "").unwrap();
        let b = create_album(&c, a, "B", "").unwrap();
        let d = create_album(&c, b, "D", "").unwrap();
        let e = create_album(&c, 0, "E", "").unwrap();
        assert!(move_album(&c, a, b, None).is_err());
        assert!(move_album(&c, a, a, None).is_err());
        // Moving to the end and to the start: the positions stay dense 0..n−1.
        move_album(&c, d, 0, None).unwrap();
        let roots = |c: &Connection| -> Vec<String> {
            albums_tree(c)
                .unwrap()
                .into_iter()
                .filter(|n| n.parent_id == 0)
                .map(|n| n.name)
                .collect()
        };
        assert_eq!(roots(&c), ["A", "E", "D"]);
        assert_eq!(
            albums_tree(&c)
                .unwrap()
                .iter()
                .filter(|n| n.parent_id == 0)
                .map(|n| n.position)
                .collect::<Vec<_>>(),
            [0, 1, 2]
        );
        move_album(&c, d, 0, Some(0)).unwrap();
        assert_eq!(roots(&c), ["D", "A", "E"]);
        // The names stay unique among the new siblings.
        assert!(move_album(&c, e, a, None).is_ok());
        assert!(create_album(&c, a, "E", "").is_err());
    }

    #[test]
    fn deleting_a_subtree_touches_rows_only() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let dacha = create_album(&c, 0, "Дача", "").unwrap();
        let may = create_album(&c, dacha, "Май", "").unwrap();
        let june = create_album(&c, dacha, "Июнь", "").unwrap();
        add_to_album(&c, may, &ids[..2]).unwrap();
        add_to_album(&c, june, &ids[2..]).unwrap();
        let to_print = create_album(&c, 0, "На печать", "").unwrap();
        add_to_album(&c, to_print, &ids).unwrap();

        assert_eq!(delete_album(&c, dacha).unwrap(), 3);
        assert!(albums_tree(&c).unwrap().iter().all(|n| n.name != "Май"));
        assert_eq!(album_items(&c, may).unwrap().len(), 0);
        assert_eq!(album_items(&c, to_print).unwrap().len(), 3);
        // The files on disk are not ours: the `files` rows are intact.
        assert_eq!(load_all(&c).unwrap().len(), 3);
        // "To print" stayed the only root with position 0.
        let root = &albums_tree(&c).unwrap()[0];
        assert_eq!((root.name.as_str(), root.position), ("На печать", 0));
    }

    #[test]
    fn adding_is_idempotent_and_the_order_is_dense() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let album = create_album(&c, 0, "Май", "").unwrap();
        assert_eq!(add_to_album(&c, album, &ids).unwrap(), (3, 0));
        assert_eq!(add_to_album(&c, album, &ids).unwrap(), (0, 3));
        assert_eq!(add_to_album(&c, album, &[ids[0], ids[1]]).unwrap(), (0, 2));
        assert_eq!(add_to_album(&c, album, &[9999]).unwrap(), (0, 0));
        let order: Vec<i64> = album_items(&c, album)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(order, ids);
        // "Up/Down" changes the order only, not the content.
        assert!(move_in_album(&c, album, ids[2], -1).unwrap());
        let order: Vec<i64> = album_items(&c, album)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(order, [ids[0], ids[2], ids[1]]);
        assert!(!move_in_album(&c, album, ids[0], -1).unwrap());
        // The middle one was removed — the positions are 0,1,2 again without a hole.
        assert_eq!(remove_from_album(&c, album, &[ids[2]]).unwrap(), 1);
        let positions: Vec<i64> = {
            let mut st = c
                .prepare("SELECT position FROM album_files WHERE album_id=?1 ORDER BY position")
                .unwrap();
            let rows = st.query_map([album], |r| r.get(0)).unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        assert_eq!(positions, [0, 1]);
    }

    #[test]
    fn the_album_cards_follow_the_chosen_order_not_the_data() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let small = create_album(&c, 0, "Малый", "").unwrap();
        let big = create_album(&c, 0, "Большой", "").unwrap();
        let middle = create_album(&c, 0, "Средний", "").unwrap();
        add_to_album(&c, big, &ids).unwrap();
        add_to_album(&c, middle, &ids[..1]).unwrap();
        let order = |sort: AlbumSort| {
            let tree = albums_tree(&c).unwrap();
            let node = |id: i64| tree.iter().find(|n| n.id == id).unwrap().clone();
            let mut cards = vec![node(small), node(big), node(middle)];
            cards.sort_by(|a, b| album_order(a, b, sort));
            cards.into_iter().map(|n| n.name).collect::<Vec<_>>()
        };
        assert_eq!(order(AlbumSort::Position), ["Малый", "Большой", "Средний"]);
        assert_eq!(order(AlbumSort::Name), ["Большой", "Малый", "Средний"]);
        assert_eq!(order(AlbumSort::Count), ["Большой", "Средний", "Малый"]);
        assert_eq!(order(AlbumSort::Size), ["Большой", "Средний", "Малый"]);
        // "By update": the album that changed last goes first. The order on disk is untouched.
        c.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params!["1714557600", small],
        )
        .unwrap();
        c.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params!["1717236000", middle],
        )
        .unwrap();
        c.execute(
            "UPDATE albums SET updated_at=?1 WHERE id=?2",
            params!["1711965600", big],
        )
        .unwrap();
        assert_eq!(order(AlbumSort::Updated), ["Средний", "Малый", "Большой"]);
        let positions: Vec<i64> = albums_tree(&c).unwrap().iter().map(|n| n.position).collect();
        assert_eq!(positions, [0, 1, 2], "the album order is data, the cards are a look");
        // A parse of an unknown value means the default.
        assert_eq!(AlbumSort::parse("galleries"), AlbumSort::Position);
        assert_eq!(AlbumSort::parse("count"), AlbumSort::Count);
    }

    #[test]
    fn a_change_of_the_contents_moves_the_album_card_up() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let album = create_album(&c, 0, "Май", "").unwrap();
        let stamp = |c: &Connection| {
            albums_tree(c)
                .unwrap()
                .iter()
                .find(|n| n.id == album)
                .unwrap()
                .updated_at
                .clone()
        };
        // A known old stamp: the date moves only when the contents really change.
        c.execute(
            "UPDATE albums SET updated_at='1' WHERE id=?1",
            [album],
        )
        .unwrap();
        assert_eq!(stamp(&c), "1");
        // An add of nothing changes nothing.
        add_to_album(&c, album, &[9999]).unwrap();
        assert_eq!(stamp(&c), "1", "nothing was added, nothing changed");
        add_to_album(&c, album, &ids[..1]).unwrap();
        assert_ne!(stamp(&c), "1", "the content changed");
        c.execute(
            "UPDATE albums SET updated_at='1' WHERE id=?1",
            [album],
        )
        .unwrap();
        remove_from_album(&c, album, &ids[..1]).unwrap();
        assert_ne!(stamp(&c), "1", "a removal is a change too");
    }

    #[test]
    fn a_file_in_the_trash_stays_an_album_row() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let album = create_album(&c, 0, "Май", "").unwrap();
        add_to_album(&c, album, &ids).unwrap();
        c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [ids[1]])
            .unwrap();
        let node = &albums_tree(&c).unwrap()[0];
        assert_eq!((node.files, node.trashed), (3, 1));
        // The grid takes the undeleted ones only, the album data is not lost.
        let visible = album_items(&c, album)
            .unwrap()
            .into_iter()
            .filter(|r| r.trashed_at.is_none())
            .count();
        assert_eq!(visible, 2);
    }

    #[test]
    fn deleting_the_rows_of_a_file_clears_the_album_and_compacts_the_order() {
        let mut c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = three_files(&c);
        let album = create_album(&c, 0, "Май", "").unwrap();
        add_to_album(&c, album, &ids).unwrap();
        // The same as "drop a folder" and the index reset: `files` rows → `album_files`.
        delete_file_rows(&c, &[ids[1]]).unwrap();
        let left = album_items(&c, album).unwrap();
        assert_eq!(left.len(), 2);
        assert!(left.iter().all(|r| r.id != ids[1]));
        let positions: Vec<i64> = {
            let mut st = c
                .prepare("SELECT position FROM album_files WHERE album_id=?1 ORDER BY position")
                .unwrap();
            let rows = st.query_map([album], |r| r.get(0)).unwrap();
            rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
        };
        assert_eq!(positions, [0, 1]);
        let _ = &mut c;
    }

    /// The labels that make OCR look for text: every dictionary counts, a disabled tag does not.
    #[test]
    fn text_tag_labels_come_from_every_dictionary() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let mk = |model: &str, id: &str, label: &str, pos: i64| TagDef {
            model: model.into(),
            id: id.into(),
            label: label.into(),
            prompts: vec!["a photo".into()],
            enabled: true,
            position: pos,
            min_percentile: 0,
        };
        save_tag_def(&c, &mk("m1", "document", "document", 0)).unwrap();
        save_tag_def(&c, &mk("m1", "screenshot", "screenshot", 1)).unwrap();
        save_tag_def(&c, &mk("m1", "beach", "beach", 2)).unwrap();
        // The owner renamed the tag in a second model and switched one off in the first.
        save_tag_def(&c, &mk("m2", "document", "papers", 0)).unwrap();
        let mut off = mk("m1", "screenshot", "screenshot", 1);
        off.enabled = false;
        save_tag_def(&c, &off).unwrap();
        assert_eq!(text_tag_labels(&c), ["document", "papers"]);
    }

    /// The tag dictionary: an edit is saved in place, the label is unique, a removal clears one row.
    #[test]
    fn the_tag_dictionary_is_stored_in_the_db_per_model() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let mk = |id: &str, label: &str, pos: i64| TagDef {
            model: "m1".into(),
            id: id.into(),
            label: label.into(),
            prompts: vec!["фотография пляжа".into(), "море".into()],
            enabled: true,
            position: pos,
            min_percentile: 0,
        };
        save_tag_def(&c, &mk("beach", "пляж", 0)).unwrap();
        save_tag_def(&c, &mk("sea", "море", 1)).unwrap();
        let defs = load_tag_defs(&c, "m1").unwrap();
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].label, "пляж");
        assert_eq!(defs[0].prompts.len(), 2);
        // Editing by id: the label and the prompts change, the row is not duplicated.
        let mut d = defs[0].clone();
        d.label = "пляж и берег".into();
        d.prompts = vec!["фотография берега".into()];
        d.enabled = false;
        d.min_percentile = 30;
        save_tag_def(&c, &d).unwrap();
        let defs = load_tag_defs(&c, "m1").unwrap();
        assert_eq!(defs.len(), 2);
        assert_eq!(defs[0].label, "пляж и берег");
        assert!(!defs[0].enabled);
        assert_eq!(defs[0].min_percentile, 30);
        // The second model has its own dictionary, foreign labels do not interfere.
        let mut other = mk("beach", "пляж", 0);
        other.model = "m2".into();
        save_tag_def(&c, &other).unwrap();
        assert_eq!(load_tag_defs(&c, "m2").unwrap().len(), 1);
        // The label is unique inside a model: otherwise two `tags` rows with one key.
        let mut clash = mk("clash", "море", 2);
        clash.model = "m1".into();
        assert!(save_tag_def(&c, &clash).is_err());
        assert_eq!(delete_tag_def(&c, "m1", "sea").unwrap(), 1);
        assert_eq!(load_tag_defs(&c, "m1").unwrap().len(), 1);
    }

    #[test]
    fn an_old_db_gains_albums_and_keeps_its_files() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE files(
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
                mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
                taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
                thumb TEXT, indexed_at INTEGER NOT NULL);
             INSERT INTO files(path,size,kind,thumb,indexed_at)
                VALUES('/old.jpg',10,'image','aa/old.jpg',0);
             INSERT INTO files(path,size,kind,thumb,indexed_at)
                VALUES('/old.mp4',20,'video','bb/old.jpg',0);
             PRAGMA user_version=4;",
        )
        .unwrap();
        init(&c).unwrap();
        let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, SCHEMA_VERSION);
        assert_eq!(load_all(&c).unwrap().len(), 2);
        // Schema 8 (M22): image previews are recomputed with the EXIF rotation, so the path
        // is cleared; a video cover stays, no rows are lost.
        let rows = load_all(&c).unwrap();
        let image = rows.iter().find(|r| r.kind == "image").unwrap();
        assert!(image.thumb.is_none(), "the image preview must be recomputed");
        let video = rows.iter().find(|r| r.kind == "video").unwrap();
        assert_eq!(video.thumb.as_deref(), Some("bb/old.jpg"));
        let album = create_album(&c, 0, "Старое", "").unwrap();
        add_to_album(&c, album, &[id_of(&c, "/old.jpg")]).unwrap();
        assert_eq!(album_items(&c, album).unwrap().len(), 1);
        init(&c).unwrap(); // a repeated init does not fail and does not duplicate tables or rows
        assert_eq!(albums_tree(&c).unwrap().len(), 1);
        assert_eq!(album_items(&c, album).unwrap().len(), 1);
    }

    /// M26.1: a root is matched by path components, never by a string prefix.
    #[test]
    fn a_root_is_matched_by_path_components() {
        assert!(under_root("/archive/2024/a.jpg", "/archive/2024"));
        assert!(under_root("/archive/2024/", "/archive/2024"));
        assert!(!under_root("/archive/20242/a.jpg", "/archive/2024"));
        assert!(!under_root("/archive/2024/a.jpg", ""));
        assert!(under_root("/archive/2024/a.jpg", "/"));
        let mut s = Settings::default();
        s.set_root_off("/archive/2024/", true);
        assert!(s.root_off("/archive/2024"));
        assert!(s.path_hidden("/archive/2024/a.jpg"));
        assert!(!s.path_hidden("/archive/2025/a.jpg"));
        // A second switch adds no duplicate, a switch-on removes it.
        s.set_root_off("/archive/2024", true);
        assert_eq!(s.roots_off.len(), 1);
        s.set_root_off("/archive/2024", false);
        assert!(s.roots_off.is_empty());
    }

    /// M26.1: `stats` names the switched-off folders with their own counts, separately from the
    /// shown files, and says nothing when every folder is on.
    #[test]
    fn the_statistics_name_the_switched_off_folders() {
        let path = std::env::temp_dir().join(format!(
            "photocat-stats-off-{}-{:?}-{:?}.db",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        {
            let c = open(&path).unwrap();
            insert(&c, &row("/archive/on/a.jpg", "aa", 1.0)).unwrap();
            insert(&c, &row("/archive/off/b.jpg", "bb", 1.0)).unwrap();
            let mut s = load_settings(&c);
            s.roots_off = vec!["/archive/off".into()];
            save_settings(&c, &s).unwrap();
        }
        let text = stats_text(Lang::En, &path).unwrap();
        assert!(
            text.contains("switched off: /archive/off — 1 file ·"),
            "the switched-off folder is not named: {text}"
        );
        assert!(
            text.contains("shown: 1 file ·"),
            "the shown files are not counted apart: {text}"
        );
        let ru = stats_text(Lang::Ru, &path).unwrap();
        assert!(
            ru.contains("выключен: /archive/off"),
            "the Russian report is missing the switched-off folder: {ru}"
        );
        {
            let c = open(&path).unwrap();
            let mut s = load_settings(&c);
            s.roots_off.clear();
            save_settings(&c, &s).unwrap();
        }
        let text = stats_text(Lang::En, &path).unwrap();
        assert!(
            !text.contains("switched off") && !text.contains("shown:"),
            "a report with nothing switched off names switches: {text}"
        );
    }

    /// M26.8: rows that left the database leave free pages, and `vacuum` returns them to the
    /// filesystem. The rows that stay are intact, and a second call has nothing to do.
    #[test]
    fn a_vacuum_gives_the_free_pages_back_and_keeps_every_row() {
        let path = std::env::temp_dir().join(format!(
            "photocat-vacuum-{}-{:?}-{:?}.db",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        {
            let c = open(&path).unwrap();
            c.execute_batch("CREATE TABLE junk(id INTEGER PRIMARY KEY, data BLOB)")
                .unwrap();
            for i in 0..64u8 {
                c.execute(
                    "INSERT INTO junk(id, data) VALUES (?1, ?2)",
                    params![i as i64, vec![i; 64 * 1024]],
                )
                .unwrap();
            }
            c.execute("DELETE FROM junk WHERE id % 2 = 0", []).unwrap();
            // The free pages are in the main file before the test looks at them.
            c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
        }
        let before = db_space(&path).unwrap();
        assert!(
            before.free_bytes >= 1024 * 1024,
            "the deleted rows left no free pages: {before:?}"
        );
        assert!(before.worth_compacting(), "nothing to reclaim: {before:?}");

        let compaction = vacuum(&path).unwrap();
        assert_eq!(compaction.before, before);
        assert_eq!(
            compaction.after.free_bytes, 0,
            "free pages are left after the compaction: {:?}",
            compaction.after
        );
        assert!(
            compaction.after.file_bytes < before.file_bytes,
            "the file did not shrink: {} → {}",
            before.file_bytes,
            compaction.after.file_bytes
        );
        assert!(!compaction.after.worth_compacting());

        let c = open(&path).unwrap();
        let kept: i64 = c
            .query_row("SELECT COUNT(*) FROM junk", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 32, "the compaction lost rows");
        let odd: i64 = c
            .query_row(
                "SELECT COUNT(*) FROM junk WHERE id % 2 = 1 AND length(data) = ?1",
                params![64 * 1024],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(odd, 32, "the rows that stayed lost their data");
        drop(c);

        // A second press finds nothing to reclaim and touches nothing.
        let again = vacuum(&path).unwrap();
        assert_eq!(again.before, again.after, "a second vacuum is not a no-op");
    }

    /// The M26.8 measurement on a copy of the real database:
    /// `PHOTOCAT_BENCH_DB=<copy> cargo test --release -- --ignored a_real_database_gives_its_free_pages_back --nocapture`.
    #[test]
    #[ignore]
    fn a_real_database_gives_its_free_pages_back() {
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let path = std::path::PathBuf::from(path);
        let started = std::time::Instant::now();
        let compaction = vacuum(&path).unwrap();
        let elapsed = started.elapsed().as_secs_f32();
        let c = open(&path).unwrap();
        let files: i64 = c
            .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
            .unwrap();
        println!(
            "vacuum: {} → {} (free {} → {}), {files} files, {elapsed:.3} s",
            human(compaction.before.file_bytes as i64),
            human(compaction.after.file_bytes as i64),
            human(compaction.before.free_bytes as i64),
            human(compaction.after.free_bytes as i64),
        );
        assert_eq!(compaction.after.free_bytes, 0);
        assert!(files > 100, "{files} files — that is not the corpus");
    }

    /// M26.9: a catalog switches a pass off for itself only; the choice is data in the settings,
    /// survives a restart and leaves no key for a root that is gone.
    #[test]
    fn a_catalog_switches_the_passes_off_for_itself_only() {
        let mut s = Settings {
            roots: vec!["/archive/a".into(), "/archive/b".into()],
            ..Settings::default()
        };
        for pass in Pass::ALL {
            assert!(
                s.pass_allows("/archive/b/x.jpg", pass),
                "a default catalog must allow every pass"
            );
        }
        s.set_root_pass("/archive/b/", Pass::Tags, false);
        assert!(!s.pass_allows("/archive/b/x.jpg", Pass::Tags));
        assert!(s.pass_allows("/archive/b/x.jpg", Pass::Faces));
        assert!(s.pass_allows("/archive/a/x.jpg", Pass::Tags));
        assert!(!s.root_pass("/archive/b", Pass::Tags));
        assert!(s.root_pass("/archive/b", Pass::Faces));
        // A catalog switched off for display allows no pass at all (M26.1).
        s.set_root_off("/archive/b", true);
        for pass in Pass::ALL {
            assert!(!s.pass_allows("/archive/b/x.jpg", pass));
        }
        s.set_root_off("/archive/b", false);

        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        save_settings(&c, &s).unwrap();
        let back = load_settings(&c);
        assert_eq!(back.root_passes, s.root_passes);
        assert!(!back.pass_allows("/archive/b/x.jpg", Pass::Tags));
        let mut back = back;
        back.set_root_pass("/archive/b", Pass::Tags, true);
        assert!(back.root_passes.is_empty(), "switching on leaves an empty key");

        // The root is gone: the next read drops its exception.
        s.roots = vec!["/archive/a".into()];
        save_settings(&c, &s).unwrap();
        assert!(load_settings(&c).root_passes.is_empty());
    }

    /// M26.9: "forget this catalog's analysis" removes the derived rows of that root only — the
    /// file rows, the owner's own tags and the other catalog's analysis stay.
    #[test]
    fn forgetting_a_catalog_removes_only_its_analysis() {
        let path = std::env::temp_dir().join(format!(
            "photocat-forget-{}-{:?}-{:?}.db",
            std::process::id(),
            std::thread::current().id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let (a, b) = {
            let c = open(&path).unwrap();
            insert(&c, &row("/archive/a/one.jpg", "aa", 1.0)).unwrap();
            insert(&c, &row("/archive/b/two.jpg", "bb", 1.0)).unwrap();
            let ids = (id_of(&c, "/archive/a/one.jpg"), id_of(&c, "/archive/b/two.jpg"));
            for (id, sha) in [(ids.0, "aa"), (ids.1, "bb")] {
                replace_tags(&c, id, "clip:m", &[("beach".into(), 0.3)]).unwrap();
                save_embedding(&c, id, "m", sha, &[1.0, 0.0]).unwrap();
                save_ocr(&c, id, sha, "eng", "text").unwrap();
                c.execute(
                    "INSERT INTO user_tags(file_id,tag,created_at) VALUES(?1,'mine',0)",
                    params![id],
                )
                .unwrap();
                c.execute(
                    "INSERT INTO faces(file_id,sha256,x,y,w,h,score,quality,thumb,embedding,created_at)
                     VALUES(?1,?2,0,0,0,0,0.9,0.5,NULL,x'00000000',0)",
                    params![id, sha],
                )
                .unwrap();
            }
            ids
        };
        let c = open(&path).unwrap();
        let forgotten = forget_catalog_derived(&c, "/archive/b").unwrap();
        assert_eq!(forgotten.files, 1);
        assert_eq!(forgotten.tags, 1);
        assert_eq!(forgotten.vectors, 1);
        assert_eq!(forgotten.ocr, 1);
        assert_eq!(forgotten.faces, 1);
        let count = |sql: &str, id: i64| -> i64 {
            c.query_row(sql, params![id], |r| r.get(0)).unwrap()
        };
        for table in ["tags", "embeddings", "ocr", "faces"] {
            assert_eq!(
                count(&format!("SELECT COUNT(*) FROM {table} WHERE file_id=?1"), b),
                0,
                "{table} rows of the forgotten catalog are still there"
            );
            assert_eq!(
                count(&format!("SELECT COUNT(*) FROM {table} WHERE file_id=?1"), a),
                1,
                "{table} rows of the other catalog were removed"
            );
        }
        // The owner's own tags and the file rows are not derived data.
        assert_eq!(count("SELECT COUNT(*) FROM user_tags WHERE file_id=?1", b), 1);
        assert_eq!(count("SELECT COUNT(*) FROM files WHERE id=?1", b), 1);
        assert_eq!(forget_catalog_derived(&c, "/archive/b").unwrap(), Forgotten { files: 1, ..Forgotten::default() });
    }
}
