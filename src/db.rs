//! Работа с SQLite: схема, вставка, чтение, статистика.

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

/// Прогресс индексации (атомики, чтобы читать из GUI-потока).
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
    /// Начать заново (например, при переходе к следующей модели-кандидату).
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
    /// Попросить прогон остановиться: воркеры проверяют флаг между файлами.
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
    // Linux — XDG, Windows — LOCALAPPDATA, macOS — обычный ~/.cache: без платформенных крейтов.
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
        std::fs::create_dir_all(dir).with_context(|| format!("не создать {}", dir.display()))?;
    }
    Ok(p)
}

pub fn open(db: &Path) -> Result<Connection> {
    let c = Connection::open(db).with_context(|| format!("не открыть БД {}", db.display()))?;
    init(&c)?;
    Ok(c)
}

/// Версия схемы. Меняется вместе с DDL ниже: старые БД догоняют её один раз.
/// Пока версия совпадает, `open` не делает ни одной записи — читать БД можно
/// параллельно с долгим прогоном (индексация, OCR), не упираясь в блокировку.
const SCHEMA_VERSION: i64 = 4;

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
            PRIMARY KEY(file_id, tag)
         );
         CREATE INDEX IF NOT EXISTS idx_tag ON tags(tag);
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
            DELETE FROM tags WHERE file_id = new.id AND source = 'clip';
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
         END;",
    )?;
    // Схема старых БД дополняется на месте: данные не теряются при обновлении.
    ensure_column(c, "files", "sharpness", "REAL")?;
    ensure_column(c, "files", "decision", "TEXT")?;
    ensure_column(c, "files", "duration_s", "REAL")?;
    ensure_column(c, "files", "trashed_at", "INTEGER")?;
    // Теги старых версий были без модели в источнике; переводим на id текущей модели.
    c.execute(
        "UPDATE tags SET source='clip:clip-vit-b32-multilingual' WHERE source='clip'",
        [],
    )?;
    c.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Настройки приложения. Хранятся в БД (рядом с индексом), а не в файле у архива.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Куда скачивать модели и откуда их читать.
    pub models_dir: PathBuf,
    /// id модели из каталога; пусто — первая доступная.
    pub model_id: String,
    /// Потолок размера скачиваемой модели, МиБ.
    pub max_model_mb: u64,
    /// Потоков на индексацию по умолчанию.
    pub index_jobs: usize,
    /// Порог похожести dHash по умолчанию.
    pub max_dist: u32,
    /// Порог уверенного тега и отрыв от второго.
    pub tag_show: f32,
    pub tag_margin: f32,
    /// Языки tesseract для распознавания текста, например `rus+eng`.
    pub ocr_lang: String,
    /// Пути к внешним программам; пусто — берётся из `PATH` (ADR 0020).
    pub tools: crate::tools::Tools,
    /// Порог косинуса для склейки лиц в одного человека (ADR 0021).
    pub face_threshold: f32,
    /// Размер плитки сетки, px (docs/UI.md).
    pub tile_size: f32,
    /// Каталоги, которые индексируются одной кнопкой; пусто — путь из верхней панели.
    pub roots: Vec<String>,
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
            ocr_lang: "rus+eng".into(),
            tools: crate::tools::Tools::default(),
            face_threshold: 0.45,
            tile_size: 150.0,
            roots: Vec::new(),
        }
    }
}

pub fn load_settings(conn: &Connection) -> Settings {
    let mut s = Settings::default();
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
            "ocr_lang" if !v.trim().is_empty() => s.ocr_lang = v,
            "tool_ffmpeg" => s.tools.ffmpeg = v,
            "tool_ffprobe" => s.tools.ffprobe = v,
            "tool_tesseract" => s.tools.tesseract = v,
            "tool_curl" => s.tools.curl = v,
            "face_threshold" => s.face_threshold = v.parse().unwrap_or(s.face_threshold),
            "tile_size" => s.tile_size = v.parse().unwrap_or(s.tile_size),
            "roots" => {
                s.roots = v
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            _ => {}
        }
    }
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
        ("ocr_lang", s.ocr_lang.clone()),
        ("face_threshold", s.face_threshold.to_string()),
        ("tile_size", s.tile_size.to_string()),
        ("roots", s.roots.join("\n")),
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

pub fn hide_tag(db: &Path, file_id: i64, tag: &str) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,?2,?3)
         ON CONFLICT(file_id,tag) DO NOTHING",
        params![file_id, tag, now()],
    )?;
    Ok(())
}

/// Вернуть все скрытые теги файла (оригинал не трогается, решение живёт в БД).
pub fn unhide_tags(db: &Path, file_id: i64) -> Result<usize> {
    let conn = open(db)?;
    Ok(conn.execute("DELETE FROM tag_hidden WHERE file_id=?1", params![file_id])?)
}

/// file_id → сколько тегов скрыто владельцем.
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

/// Свои теги владельца: file_id → список подписей.
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

/// Одна запись каталога.
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
    /// Длительность видео в секундах; `NULL` у фото и у файлов без ffprobe.
    pub duration_s: Option<f64>,
    pub thumb: Option<String>,
    /// Решение владельца по дублю: `keep` | `drop` | `NULL` (не решено).
    pub decision: Option<String>,
    /// Время помещения в корзину приложения; `NULL` — файл в обычных видах.
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

/// Пометить файл группы «оставить», остальные — «лишний». Оригиналы не трогаем: только БД.
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

/// Файлы корзины приложения: они ещё на диске, но скрыты из обычных видов.
pub fn load_trashed(conn: &Connection) -> Result<Vec<FileRow>> {
    load_where(conn, "trashed_at IS NOT NULL")
}

fn load_where(conn: &Connection, filter: &str) -> Result<Vec<FileRow>> {
    let sql = format!(
        "SELECT id,path,size,sha256,kind,COALESCE(width,0),COALESCE(height,0),
                taken_at,COALESCE(taken_src,''),lat,lon,camera,dhash,sharpness,duration_s,thumb,decision,trashed_at,
                COALESCE(mtime,0)
         FROM files WHERE {filter} ORDER BY COALESCE(taken_at,'9999') , path"
    );
    let mut st = conn.prepare(&sql)?;
    let rows = st.query_map([], |r| {
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
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
}

/// Пометить файл в корзину приложения: на диске ничего не меняется.
pub fn mark_trashed(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "UPDATE files SET trashed_at=?1 WHERE id=?2",
        params![now(), file_id],
    )?;
    Ok(())
}

/// Вернуть файл из корзины приложения.
pub fn untrash(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    conn.execute(
        "UPDATE files SET trashed_at=NULL WHERE id=?1",
        params![file_id],
    )?;
    Ok(())
}

/// Убрать файл из индекса после переноса в системную корзину. Оригинал уже не наш.
pub fn purge_file(db: &Path, file_id: i64) -> Result<()> {
    let conn = open(db)?;
    delete_file_rows(&conn, &[file_id])
}

/// Убрать каталог из индекса: строки БД, не файлы на диске.
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
    for id in ids {
        tx.execute("DELETE FROM ocr WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM embeddings WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM tags WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM tag_hidden WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM user_tags WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM faces WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM face_scans WHERE file_id=?1", params![id])?;
        tx.execute("DELETE FROM files WHERE id=?1", params![id])?;
    }
    delete_empty_face_clusters(tx)?;
    Ok(())
}

/// sha256 уже посчитанных векторов: file_id → sha256 файла на момент расчёта.
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

/// Все векторы модели: `(file_id, f32)` для косинусного поиска и кластеризации.
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

/// Одно найденное лицо: рамка, эмбеддинг и кластер. Вектор — f32 в BLOB.
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

/// Лицо для интерфейса: без эмбеддинга, зато с путём и датой исходного файла.
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

/// sha256 уже просканированных на лица файлов: file_id → sha256 на момент прохода.
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

/// Заменить лица одного файла и отметить его просканированным. Пустой список — тоже результат.
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

/// Все лица с эмбеддингами — для кластеризации.
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

/// Лица для сетки: `cluster_id=None` — все. Сначала крупные и уверенные.
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

/// Записать привязку лиц к кластерам. `manual` — владелец поставил её вручную.
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

/// Собрать несколько отмеченных лиц в группу с одним именем. Если имя уже есть,
/// используется существующая группа; пустое имя создаёт новую без подписи.
/// `manual=1` закрепляет решение при пересборке.
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

/// Отделить одно лицо в новую группу: ложная склейка разъединяется одним действием.
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
        anyhow::bail!("нет такого лица: {face_id}");
    }
    tx.commit()?;
    Ok(cluster_id)
}

/// Убрать кластеры без лиц. Кластер с подписью остаётся, даже если пока пуст.
pub fn delete_empty_face_clusters(conn: &Connection) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM face_clusters
         WHERE label='' AND id NOT IN (
            SELECT DISTINCT cluster_id FROM faces WHERE cluster_id IS NOT NULL
         )",
        [],
    )?)
}

/// План прохода по лицам: `(снимков всего, ещё не просканировано)`.
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

/// `(лиц, снимков с лицами, кластеров)` — для панели и отчёта.
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

/// Заменить теги одного источника у файла (CLIP пересчитывается целиком).
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

/// Все теги: file_id → [(тег, вес)] по убыванию веса.
/// Все видимые теги выбранного источника: file_id → [(тег, вес)] по убыванию.
/// Скрытые владельцем теги (`tag_hidden`) в результат не попадают.
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

/// sha256 текста, уже распознанного у файла: file_id → sha256 файла на момент OCR.
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

/// Записать распознанный текст. Текст живёт в БД: рядом с оригиналом ничего не пишется.
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

/// Сколько файлов с текстом и сколько всего знаков распознано.
pub fn ocr_summary(conn: &Connection) -> Result<(i64, i64)> {
    Ok(conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(chars),0) FROM ocr",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?)
}

/// Все видимые теги с весами: file_id → [(тег, вес)] по убыванию.
/// Один тег из нескольких моделей берётся по лучшему весу, чтобы догадки не дублировались.
/// Нужен проходу OCR: там тег «документ»/«скриншот» — повод искать текст.
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

/// Запрос для FTS5: каждое слово — префикс, кавычки снимают спецсимволы.
/// Пустая строка — `None`, а не ошибка синтаксиса FTS.
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

/// Полнотекстовый поиск по распознанному тексту: `(file_id, фрагмент с «…»)`.
/// Фрагмент уже подсвечен маркерами: UI красит совпадения, CLI печатает как есть.
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
    let b = bytes as f64;
    let units = ["Б", "КиБ", "МиБ", "ГиБ", "ТиБ"];
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

/// Длительность видео: `М:СС`, от часа — `Ч:ММ:СС`. Округление до секунды.
pub fn duration(secs: f64) -> String {
    let total = secs.round().max(0.0) as u64;
    let (h, m, s) = (total / 3600, (total % 3600) / 60, total % 60);
    if h > 0 {
        format!("{h}:{m:02}:{s:02}")
    } else {
        format!("{m}:{s:02}")
    }
}

pub fn print_stats(db_path: &Path) -> Result<()> {
    let conn = open(db_path)?;
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
    println!("{:<10} {:>10} {:>12}", "тип", "файлов", "объём");
    for r in rows {
        let (k, n, b) = r?;
        total_n += n;
        total_b += b;
        println!("{:<10} {:>10} {:>12}", k, n, human(b));
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
    println!("---");
    println!("всего: {total_n} файлов, {}", human(total_b));
    println!("без даты съёмки: {no_date}");
    println!("без перцептивного хеша: {no_hash}");
    println!(
        "уникальных sha256: {uniq_sha}  (точных копий: {})",
        total_n - uniq_sha
    );
    println!("уникальных dhash: {uniq_dhash}");
    let (vid_n, vid_dur): (i64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(duration_s),0) FROM files
         WHERE kind='video' AND duration_s IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    println!(
        "видео с длительностью: {vid_n}, суммарно {}",
        duration(vid_dur)
    );
    let (ocr_n, ocr_chars) = ocr_summary(&conn)?;
    if ocr_n > 0 {
        println!("файлов с распознанным текстом: {ocr_n}, знаков: {ocr_chars}");
    }
    Ok(())
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

        let split = split_face(&mut c, face_id).unwrap();
        let moved = &load_faces(&c).unwrap()[0];
        assert_eq!(moved.cluster_id, Some(split));
        assert!(moved.manual);
        assert_ne!(split, clustered);

        // Подмена содержимого стирает лица и отметку прохода — их нельзя показывать.
        insert(&c, &row("/a.jpg", "bb", 1.0)).unwrap();
        assert!(load_faces(&c).unwrap().is_empty());
        assert!(face_scan_shas(&c).unwrap().is_empty());
    }

    #[test]
    fn корзина_приложения_скрывает_файл_из_обычных_видов() {
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
    fn пакетное_назначение_лиц_собирает_группу_с_именем() {
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
        init(&c).unwrap(); // повторный вызов не падает
        let r = row("/old.jpg", "aa", 7.5);
        insert(&c, &r).unwrap();
        let loaded = load_all(&c).unwrap();
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0].sharpness, Some(7.5));
        assert_eq!(loaded[0].decision, None);
        assert_eq!(loaded[0].duration_s, None);
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
        assert_eq!(fts_query("  \"*  "), None); // одни спецсимволы — не ошибка FTS, пустой запрос
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
        // синтаксис FTS не должен превращать пользовательский ввод в ошибку
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
    fn settings_survive_roundtrip() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let s = Settings {
            model_id: "clip-vit-b32-en".into(),
            models_dir: PathBuf::from("/tmp/models"),
            max_model_mb: 160,
            tag_show: 0.27,
            tag_margin: 0.02,
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
        assert!((back.tag_margin - 0.02).abs() < 1e-6);
        assert_eq!(back.ocr_lang, "rus");
        assert_eq!(back.tools.ffmpeg, "/opt/ffmpeg");
        assert_eq!(back.tools.tesseract, "/opt/tesseract");
        assert_eq!(back.tools.curl, "");
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

        insert(&c, &r).unwrap(); // переиндексация того же файла
        assert_eq!(load_all(&c).unwrap()[0].decision.as_deref(), Some("keep"));

        let mut changed = r.clone();
        changed.sha256 = "cc".into();
        insert(&c, &changed).unwrap(); // файл подменили — решение устарело
        assert_eq!(load_all(&c).unwrap()[0].decision, None);
    }
}
