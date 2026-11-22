//! Работа с SQLite: схема, вставка, чтение, статистика.

use anyhow::{Context, Result};
use rusqlite::{params, Connection};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Mutex;

/// Прогресс индексации (атомики, чтобы читать из GUI-потока).
#[derive(Default)]
pub struct Progress {
    done: AtomicUsize,
    total: AtomicUsize,
    errors: AtomicUsize,
    finished: AtomicBool,
    msg: Mutex<String>,
}

impl Progress {
    pub fn set_total(&self, n: usize) {
        self.total.store(n, Ordering::Relaxed);
    }
    /// Начать заново (например, при переходе к следующей модели-кандидату).
    pub fn reset(&self) {
        self.done.store(0, Ordering::Relaxed);
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

fn init(c: &Connection) -> Result<()> {
    c.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         CREATE TABLE IF NOT EXISTS files(
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
         END;",
    )?;
    // Схема старых БД дополняется на месте: данные не теряются при обновлении.
    ensure_column(c, "files", "sharpness", "REAL")?;
    ensure_column(c, "files", "decision", "TEXT")?;
    ensure_column(c, "files", "duration_s", "REAL")?;
    // Теги старых версий были без модели в источнике; переводим на id текущей модели.
    c.execute(
        "UPDATE tags SET source='clip:clip-vit-b32-multilingual' WHERE source='clip'",
        [],
    )?;
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
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models_dir: cache_dir().join("models"),
            model_id: String::new(),
            max_model_mb: 250,
            index_jobs: 8,
            max_dist: 6,
            tag_show: 0.22,
            tag_margin: 0.01,
        }
    }
}

pub fn load_settings(conn: &Connection) -> Settings {
    let mut s = Settings::default();
    let Ok(mut st) = conn.prepare("SELECT key, value FROM settings") else {
        return s;
    };
    let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))) else {
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
            _ => {}
        }
    }
    s
}

pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()> {
    let pairs = [
        ("models_dir", s.models_dir.to_string_lossy().to_string()),
        ("model_id", s.model_id.clone()),
        ("max_model_mb", s.max_model_mb.to_string()),
        ("index_jobs", s.index_jobs.to_string()),
        ("max_dist", s.max_dist.to_string()),
        ("tag_show", s.tag_show.to_string()),
        ("tag_margin", s.tag_margin.to_string()),
    ];
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
    tx.execute("UPDATE files SET decision='keep' WHERE id=?1", params![keep])?;
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
    let mut st = conn.prepare(
        "SELECT id,path,size,sha256,kind,COALESCE(width,0),COALESCE(height,0),
                taken_at,COALESCE(taken_src,''),lat,lon,camera,dhash,sharpness,duration_s,thumb,decision
         FROM files ORDER BY COALESCE(taken_at,'9999') , path",
    )?;
    let rows = st.query_map([], |r| {
        Ok(FileRow {
            id: r.get(0)?,
            path: r.get(1)?,
            size: r.get(2)?,
            mtime: 0,
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
        })
    })?;
    let mut out = Vec::new();
    for r in rows {
        out.push(r?);
    }
    Ok(out)
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
    let mut st = conn.prepare(
        "INSERT INTO tags(file_id,tag,score,source) VALUES(?1,?2,?3,?4)",
    )?;
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
    let rows = st.query_map([source], |r| Ok((r.get::<_, i64>(0)?, r.get(1)?, r.get(2)?)))?;
    let mut out: HashMap<i64, Vec<(String, f32)>> = HashMap::new();
    for r in rows {
        let (id, tag, score) = r?;
        out.entry(id).or_default().push((tag, score));
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
    let no_hash: i64 = conn.query_row(
        "SELECT COUNT(*) FROM files WHERE dhash IS NULL",
        [],
        |r| r.get(0),
    )?;
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
    println!("уникальных sha256: {uniq_sha}  (точных копий: {})", total_n - uniq_sha);
    println!("уникальных dhash: {uniq_dhash}");
    let (vid_n, vid_dur): (i64, f64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(duration_s),0) FROM files
         WHERE kind='video' AND duration_s IS NOT NULL",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    println!("видео с длительностью: {vid_n}, суммарно {}", duration(vid_dur));
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
        }
    }

    fn id_of(c: &Connection, path: &str) -> i64 {
        c.query_row("SELECT id FROM files WHERE path=?1", [path], |r| r.get(0))
            .unwrap()
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
        replace_tags(&c, id, "clip", &[("пляж".into(), 0.3), ("еда".into(), 0.1)])
            .unwrap();
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
    fn settings_survive_roundtrip() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let s = Settings {
            model_id: "clip-vit-b32-en".into(),
            models_dir: PathBuf::from("/tmp/models"),
            max_model_mb: 160,
            tag_show: 0.27,
            tag_margin: 0.02,
            ..Default::default()
        };
        save_settings(&c, &s).unwrap();
        let back = load_settings(&c);
        assert_eq!(back.model_id, "clip-vit-b32-en");
        assert_eq!(back.models_dir, PathBuf::from("/tmp/models"));
        assert_eq!(back.max_model_mb, 160);
        assert!((back.tag_show - 0.27).abs() < 1e-6);
        assert!((back.tag_margin - 0.02).abs() < 1e-6);
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
