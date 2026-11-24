//! Работа с SQLite: схема, вставка, чтение, статистика.

use crate::i18n::{self, Lang};
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
const SCHEMA_VERSION: i64 = 7;

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
    // Схема старых БД дополняется на месте: данные не теряются при обновлении.
    ensure_column(c, "files", "sharpness", "REAL")?;
    ensure_column(c, "files", "decision", "TEXT")?;
    ensure_column(c, "files", "duration_s", "REAL")?;
    ensure_column(c, "files", "trashed_at", "INTEGER")?;
    // Схема 7: теги разных моделей не смешиваются — ключ (file_id, tag, source).
    // Старая таблица перестраивается на месте, данные сохраняются; триггер сброса
    // тегов при смене sha256 пересоздаётся, иначе он ссылался бы на старую таблицу
    // и удалял только legacy-источник `clip`.
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
    // Теги старых версий были без модели в источнике; переводим на id текущей модели.
    c.execute(
        "UPDATE tags SET source='clip:clip-vit-b32-multilingual' WHERE source='clip'",
        [],
    )?;
    c.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    Ok(())
}

/// Как сетка показывает файлы: группами по месяцам или одной непрерывной лентой (M17).
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

    /// Неизвестное значение из старой БД — значение по умолчанию, без падения.
    pub fn parse(value: &str) -> Self {
        match value {
            "flat" => GridLayout::Flat,
            _ => GridLayout::Months,
        }
    }
}

/// Порядок плиток (M17). «Порядок альбома» (M19) действует только внутри
/// выбранного альбома; вне его читается как «дата: новые сверху».
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

/// Тег в словаре: метка, промпты и порядок. Живёт в БД на каждую модель (M15.4),
/// потому что метки — конфигурация владельца, а не константа в коде.
#[derive(Debug, Clone, PartialEq)]
pub struct TagDef {
    /// id модели (`embeddings.model`): у каждой модели свой словарь.
    pub model: String,
    /// Стабильный ключ строки: по нему тег переименовывают и удаляют.
    pub id: String,
    /// Подпись по-русски: она же ключ в `tags.tag`.
    pub label: String,
    /// 2–3 общих шаблона; пустой список — тег выключен по смыслу.
    pub prompts: Vec<String>,
    pub enabled: bool,
    /// Порядок в фильтре и редакторе.
    pub position: i64,
    /// Минимальный процентиль показа тега (0 — сколько выбрано ползунком).
    pub min_percentile: i32,
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
    /// Порог уверенного тега (сырой косинус) и отрыв от второго.
    /// Это про карточку, OCR и `{tag}` в витрине, а не про фильтр.
    pub tag_show: f32,
    pub tag_margin: f32,
    /// Порог отбора в фильтре по тегу — процентиль 0–100 (ADR 0032).
    /// 100% — самый сильный снимок этого тега, 0% — все; 70% — верхние 30%.
    pub tag_show_pct: i32,
    /// Языки tesseract для распознавания текста, например `rus+eng`.
    pub ocr_lang: String,
    /// Пути к внешним программам; пусто — берётся из `PATH` (ADR 0020).
    pub tools: crate::tools::Tools,
    /// Порог косинуса для склейки лиц в одного человека (ADR 0021).
    pub face_threshold: f32,
    /// Размер плитки сетки, px (docs/UI.md).
    pub tile_size: f32,
    /// Представление сетки: месяцы или плоская лента (M17).
    pub grid_layout: GridLayout,
    /// Порядок плиток: дата, имя или размер (M17).
    pub grid_sort: GridSort,
    /// Каталоги, которые индексируются одной кнопкой; пусто — путь из верхней панели.
    pub roots: Vec<String>,
    /// Напоминание о проверке целостности (M10): каталог и слепок, к которым относится
    /// срок, интервал в днях (0 — напоминание выключено) и итог прошлого прохода.
    pub verify_root: String,
    pub verify_manifest: String,
    pub verify_interval_days: u32,
    /// RFC3339 UTC последнего полного прохода по архиву (слепок или проверка).
    pub verify_last_at: String,
    pub verify_last_ok: bool,
    /// Тот же текст отчёта, что уходит в `<кэш>/photocat/verify-last.txt`.
    pub verify_last_report: String,
    /// Язык интерфейса (M21, ADR 0040): `en` — основной, `ru` — второй.
    pub lang: crate::i18n::Lang,
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
            roots: Vec::new(),
            verify_root: String::new(),
            verify_manifest: String::new(),
            // 30 дней — спокойный срок для семейного архива; 0 выключает напоминание.
            verify_interval_days: 30,
            verify_last_at: String::new(),
            verify_last_ok: false,
            verify_last_report: String::new(),
            // Проект продолжается на английском; русский остаётся выбором в настройках.
            lang: crate::i18n::Lang::En,
        }
    }
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
            "tool_curl" => s.tools.curl = v,
            "face_threshold" => s.face_threshold = v.parse().unwrap_or(s.face_threshold),
            "tile_size" => s.tile_size = v.parse().unwrap_or(s.tile_size),
            "grid_layout" => s.grid_layout = GridLayout::parse(&v),
            "grid_sort" => s.grid_sort = GridSort::parse(&v),
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
            _ => {}
        }
    }
    if !has_tag_pct {
        // Миграция M15.2: раньше `tag_show` был и фильтром, и порогом уверенности,
        // поэтому ползунок уводил его до 0.50. Фильтр теперь в процентилях (70% по
        // умолчанию), а уверенность возвращается к ADR 0013 — 0.22 и отрыв 0.01.
        s.tag_show = 0.22;
        s.tag_show_pct = 70;
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
        ("tag_show_pct", s.tag_show_pct.to_string()),
        ("ocr_lang", s.ocr_lang.clone()),
        ("face_threshold", s.face_threshold.to_string()),
        ("tile_size", s.tile_size.to_string()),
        ("grid_layout", s.grid_layout.as_str().to_string()),
        ("grid_sort", s.grid_sort.as_str().to_string()),
        ("roots", s.roots.join("\n")),
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

/// Пора ли проверять архив по слепку (M10). Считается по настройкам, а не по таймеру:
/// приложение само ничего не запускает, только говорит «пора».
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyDue {
    /// Слепок не задан — напоминать не о чем.
    NoManifest,
    /// Интервал 0 — владелец выключил напоминание.
    Disabled,
    /// Срок ещё не вышел; `days_left` — сколько полных дней осталось.
    NotYet { days_left: i64 },
    /// Срок вышел; `days_over` — на сколько дней (0 — ровно срок).
    Due { days_over: i64 },
}

/// RFC3339 → UTC. Пустое или испорченное значение — `None`.
pub fn parse_stamp(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// Решение «пора / не пора» одной функцией: одинаково для баннера и для тестов.
/// `now` передаётся явно, чтобы «ровно срок» и «часы назад» проверялись без ожидания.
///
/// * нет слепка → `NoManifest`;
/// * интервал 0 → `Disabled`;
/// * слепка ещё не проверяли (пусто или мусор в дате) → `Due`;
/// * дата в будущем (часы перевели назад) → `NotYet`: срок не наступил, паники нет;
/// * иначе — по календарным суткам.
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

/// Одно значение из `settings` — для служебных ключей вроде версии словаря тегов.
/// Язык интерфейса на первом запуске (M21, ADR 0040): если владелец его ещё не выбирал,
/// берём локаль системы и сохраняем — дальше язык читается из БД, а не угадывается заново.
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
    // Альбомы хранят только id файлов: строку убираем вместе с файлом, иначе
    // осталась бы «висячая» ссылка, а порядок альбома — с дыркой.
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

/// Удалить векторы модели: пересчёт с новым входом модели. Архив и превью не трогаются.
pub fn clear_embeddings(conn: &Connection, model: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM embeddings WHERE model=?1", params![model])?)
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

/// Файл, годный в размеченную выборку M15.7: картинка не в корзине, с превью и с тегами
/// модели. Порядок — по sha256: выборка не зависит от имён и дат и повторяется один в один.
#[derive(Debug, Clone)]
pub struct TaggedFile {
    pub id: i64,
    pub path: String,
    pub sha256: String,
    /// Путь превью относительно `<кэш>/photocat/thumbs`.
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

/// Словарь тегов модели в порядке показа. Пусто — модель ещё не засеяна дефолтом.
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

/// Записать тег словаря на место (по `model`+`id`), не трогая соседей.
/// Метка уникальна внутри модели: иначе `tags` получит две строки с одним ключом.
pub fn save_tag_def(conn: &Connection, d: &TagDef) -> Result<()> {
    let taken: Option<String> = conn
        .query_row(
            "SELECT tag_id FROM tag_defs WHERE model=?1 AND label=?2 AND tag_id<>?3",
            params![d.model, d.label, d.id],
            |r| r.get(0),
        )
        .ok();
    if let Some(other) = taken {
        anyhow::bail!("метка «{}» уже занята тегом {other}", d.label);
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

/// Убрать тег из словаря. Строки `tags` с этой меткой удалит пересчёт тегов.
pub fn delete_tag_def(conn: &Connection, model: &str, id: &str) -> Result<usize> {
    Ok(conn.execute(
        "DELETE FROM tag_defs WHERE model=?1 AND tag_id=?2",
        params![model, id],
    )?)
}

/// Заменить словарь модели целиком (импорт JSON, «вернуть стандартные теги»).
pub fn replace_tag_defs(conn: &Connection, model: &str, defs: &[TagDef]) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM tag_defs WHERE model=?1", [model])?;
    for d in defs {
        save_tag_def(&tx, d)?;
    }
    tx.commit()?;
    Ok(())
}

/// Сколько файлов имеет каждый тег источника — для `tags --list` и пустого словаря.
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
    human_with(bytes, ["Б", "КиБ", "МиБ", "ГиБ", "ТиБ"])
}

/// То же, что `human`, но единицы на языке интерфейса (M21): `21.2 ГиБ` / `21.2 GiB`.
/// Формат числа один, меняются только подписи единиц, поэтому функция живёт здесь,
/// рядом с `human`, а не в `i18n`.
pub fn human_in(lang: crate::i18n::Lang, bytes: i64) -> String {
    human_with(
        bytes,
        match lang {
            crate::i18n::Lang::En => ["B", "KiB", "MiB", "GiB", "TiB"],
            crate::i18n::Lang::Ru => ["Б", "КиБ", "МиБ", "ГиБ", "ТиБ"],
        },
    )
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

/// Узел дерева альбомов (M19). Альбом — только строки в БД: ни один файл на диске
/// не создаётся, не меняется и не перемещается.
#[derive(Debug, Clone)]
pub struct AlbumNode {
    pub id: i64,
    /// 0 — корень дерева.
    pub parent_id: i64,
    pub position: i64,
    pub name: String,
    pub note: String,
    /// Глубина в дереве: 0 у корня. Нужна интерфейсу для отступа.
    pub depth: usize,
    /// Сколько файлов в альбоме (включая попавшие в корзину приложения).
    pub files: i64,
    /// Сколько из них в корзине приложения: в сетке они скрыты.
    pub trashed: i64,
    pub subalbums: i64,
    pub bytes: i64,
    /// Превью первого неудалённого файла по порядку; `None` — пустой альбом.
    pub cover: Option<String>,
}

fn album_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() {
        anyhow::bail!("имя альбома пустое");
    }
    if name.chars().count() > 80 {
        anyhow::bail!("имя альбома длиннее 80 символов");
    }
    if name.chars().any(char::is_control) {
        anyhow::bail!("в имени альбома есть управляющий символ");
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
        anyhow::bail!("альбом с таким именем уже есть среди соседей (id {id})");
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

/// Путь альбома в дереве: «Дача 2024 / Май».
pub fn album_path(conn: &Connection, id: i64) -> Result<String> {
    let map = album_rows(conn)?;
    if !map.contains_key(&id) {
        anyhow::bail!("альбома {id} нет");
    }
    Ok(path_in(&map, id))
}

/// Создать альбом в конце списка соседей. `parent_id = 0` — корень дерева.
pub fn create_album(conn: &Connection, parent_id: i64, name: &str, note: &str) -> Result<i64> {
    let name = album_name(name)?;
    let tx = conn.unchecked_transaction()?;
    if parent_id != 0 && !album_exists(&tx, parent_id)? {
        anyhow::bail!("родительского альбома {parent_id} нет");
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

/// Переименовать альбом. Имя уникально среди соседей без учёта регистра.
pub fn rename_album(conn: &Connection, id: i64, name: &str) -> Result<()> {
    let name = album_name(name)?;
    let tx = conn.unchecked_transaction()?;
    let parent: i64 = tx
        .query_row("SELECT parent_id FROM albums WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .context("альбома нет")?;
    let key = name.to_lowercase();
    sibling_name_free(&tx, parent, &key, id)?;
    tx.execute(
        "UPDATE albums SET name=?1, name_key=?2, updated_at=?3 WHERE id=?4",
        params![name, key, now().to_string(), id],
    )?;
    tx.commit()?;
    Ok(())
}

/// Переместить альбом к другому родителю. `position = None` — в конец.
/// Новый родитель не может быть самим альбомом или его потомком.
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
        .context("альбома нет")?;
    if new_parent == id {
        anyhow::bail!("альбом нельзя переместить в самого себя");
    }
    if new_parent != 0 && !album_exists(&tx, new_parent)? {
        anyhow::bail!("родительского альбома {new_parent} нет");
    }
    // Цикл: идём вверх от нового родителя; если дошли до id — отказ.
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
            anyhow::bail!("нельзя переместить альбом внутрь его подальбома");
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

/// Удалить альбом со всем поддеревом. Только строки: файлы на диске целы.
pub fn delete_album(conn: &Connection, id: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    let parent: i64 = tx
        .query_row("SELECT parent_id FROM albums WHERE id=?1", [id], |r| r.get(0))
        .optional()?
        .context("альбома нет")?;
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

/// Добавить файлы в конец альбома. Повтор не плодит строки: возвращает
/// `(добавлено, уже было)`. Файлов, которых нет в индексе, не бывает.
pub fn add_to_album(conn: &Connection, album_id: i64, file_ids: &[i64]) -> Result<(usize, usize)> {
    let tx = conn.unchecked_transaction()?;
    if !album_exists(&tx, album_id)? {
        anyhow::bail!("альбома {album_id} нет");
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
    tx.commit()?;
    Ok((added, already))
}

/// Убрать файлы из альбома и сжать порядок. Возвращает число удалённых строк.
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
    }
    tx.commit()?;
    Ok(removed)
}

/// «Выше/Ниже» внутри альбома: `delta` — −1 или +1. Порядок снова плотный 0..n−1.
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
    tx.commit()?;
    Ok(true)
}

/// Дерево альбомов в порядке обхода: корни по `position`, затем их подальбомы.
/// `depth` — отступ для интерфейса; счётчики — строки и объём файлов альбома.
pub fn albums_tree(conn: &Connection) -> Result<Vec<AlbumNode>> {
    let mut st = conn.prepare(
        "SELECT a.id, a.parent_id, a.position, a.name, a.note,
                (SELECT COUNT(*) FROM album_files af WHERE af.album_id=a.id),
                (SELECT COUNT(*) FROM album_files af JOIN files f ON f.id=af.file_id
                  WHERE af.album_id=a.id AND f.trashed_at IS NOT NULL),
                (SELECT COUNT(*) FROM albums c WHERE c.parent_id=a.id),
                (SELECT COALESCE(SUM(f.size),0) FROM album_files af
                  JOIN files f ON f.id=af.file_id WHERE af.album_id=a.id),
                (SELECT f.thumb FROM album_files af JOIN files f ON f.id=af.file_id
                  WHERE af.album_id=a.id AND f.trashed_at IS NULL
                  ORDER BY af.position, f.id LIMIT 1)
         FROM albums a ORDER BY a.parent_id, a.position, a.id",
    )?;
    let rows = st.query_map([], |r| {
        Ok(AlbumNode {
            id: r.get(0)?,
            parent_id: r.get(1)?,
            position: r.get(2)?,
            name: r.get(3)?,
            note: r.get(4)?,
            depth: 0,
            files: r.get(5)?,
            trashed: r.get(6)?,
            subalbums: r.get(7)?,
            bytes: r.get(8)?,
            cover: r.get(9)?,
        })
    })?;
    let mut by_parent: HashMap<i64, Vec<AlbumNode>> = HashMap::new();
    for row in rows {
        let node = row?;
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
    // Строка с потерянным родителем не должна пропасть из интерфейса.
    while let Some(&parent) = by_parent.keys().min() {
        walk(parent, 0, &mut by_parent, &mut out);
    }
    Ok(out)
}

/// Файлы альбома в его порядке. Включает строки корзины приложения: интерфейс
/// скрывает их в сетке и считает отдельно, а данные остаются.
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

/// Поддерево альбома в порядке дерева: корень, затем его подальбомы.
/// Нужно выгрузке (M20): структура на диске повторяет это дерево.
pub fn album_subtree(conn: &Connection, id: i64) -> Result<Vec<AlbumNode>> {
    let tree = albums_tree(conn)?;
    if !tree.iter().any(|n| n.id == id) {
        anyhow::bail!("альбома {id} нет");
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

/// Данные для выгрузки альбома (M20): узлы поддерева и их файлы в порядке
/// альбома. Только чтение; ни один файл на диске не открывается.
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

/// Альбомы, в которых лежит файл: id и путь в дереве. Для инспектора и карточки.
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

/// Только пути альбомов файла — для инспектора (M19). Карточка берёт
/// `album_refs_of_file`, потому что «×» нужен id; эта форма — для отчётов.
#[allow(dead_code)]
pub fn albums_of_file(conn: &Connection, file_id: i64) -> Result<Vec<String>> {
    Ok(album_refs_of_file(conn, file_id)?
        .into_iter()
        .map(|(_, path)| path)
        .collect())
}

/// Печать статистики индекса на языке интерфейса (M21.6).
pub fn print_stats(lang: Lang, db_path: &Path) -> Result<()> {
    print!("{}", stats_text(lang, db_path)?);
    Ok(())
}

/// Текст статистики отдельной функцией: его печатает CLI, а тест проверяет язык
/// и числа, не перехватывая stdout.
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
        i18n::t(lang, "тип"),
        i18n::t(lang, "файлов"),
        i18n::t(lang, "объём")
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
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "всего: {} файлов, {}",
            &[total_n.to_string(), human_in(lang, total_b)]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(lang, "без даты съёмки: {}", &[no_date.to_string()])
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "без перцептивного хеша: {}",
            &[no_hash.to_string()]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "уникальных sha256: {}  (точных копий: {})",
            &[uniq_sha.to_string(), (total_n - uniq_sha).to_string()]
        )
    );
    let _ = writeln!(
        out,
        "{}",
        i18n::fill(
            lang,
            "уникальных dhash: {}",
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
            "видео с длительностью: {}, суммарно {}",
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
                "файлов с распознанным текстом: {}, знаков: {}",
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

    /// Схема 7: теги разных моделей живут рядом, а смена sha256 снимает все CLIP-теги.
    #[test]
    fn миграция_7_не_смешивает_теги_моделей() {
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
        assert_eq!(version, 7);
        assert_eq!(load_tags(&c, "clip:m1").unwrap()[&1][0].0, "пляж");
        assert!(load_tags(&c, "clip:m2").unwrap().is_empty());
        // Теперь ключ допускает обе модели у одного файла — раньше это был конфликт.
        replace_tags(&c, 1, "clip:m1", &[("пляж".into(), 0.35)]).unwrap();
        replace_tags(&c, 1, "clip:m2", &[("пляж".into(), 0.45)]).unwrap();
        assert_eq!(load_tags(&c, "clip:m1").unwrap()[&1].len(), 1);
        assert_eq!(load_tags(&c, "clip:m2").unwrap()[&1].len(), 1);
        // Смена содержимого снимает теги всех моделей, а не только legacy `clip`.
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
    fn старый_порог_тегов_мигрирует_в_процентиль() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        // Старая база: ползунок фильтра увёл tag_show до 0.50, ключа процентиля нет.
        c.execute(
            "INSERT INTO settings(key,value) VALUES('tag_show','0.50')",
            [],
        )
        .unwrap();
        let s = load_settings(&c);
        assert_eq!(s.tag_show_pct, 70);
        assert!((s.tag_show - 0.22).abs() < 1e-6);
        // После сохранения миграция закреплена в БД и не повторяется.
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
        assert_eq!(back.tools.curl, "");
    }

    #[test]
    fn неизвестное_представление_сетки_берёт_умолчание() {
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

    /// «Пора / не пора» — все случаи из M10: нет слепка, выключено, ровно срок,
    /// просрочено, ещё рано и часы назад.
    #[test]
    fn напоминание_считает_срок_по_слепку() {
        let now = chrono::DateTime::parse_from_rfc3339("2026-11-24T12:00:00Z")
            .unwrap()
            .with_timezone(&chrono::Utc);
        let base = Settings {
            verify_root: "/tmp/архив".into(),
            verify_manifest: "/tmp/архив.sha256".into(),
            verify_interval_days: 30,
            ..Default::default()
        };
        // Слепка нет — напоминать не о чем, даже если срок задан.
        let no_manifest = Settings {
            verify_manifest: String::new(),
            ..base.clone()
        };
        assert_eq!(verify_due(&no_manifest, now), VerifyDue::NoManifest);
        // Интервал 0 — выключено владельцем.
        let off = Settings {
            verify_interval_days: 0,
            ..base.clone()
        };
        assert_eq!(verify_due(&off, now), VerifyDue::Disabled);
        // Прошлого прохода не было — сразу пора.
        assert_eq!(verify_due(&base, now), VerifyDue::Due { days_over: 0 });
        // Испорченная дата — тот же ответ: лучше напомнить, чем молчать.
        let broken = Settings {
            verify_last_at: "вчера".into(),
            ..base.clone()
        };
        assert_eq!(verify_due(&broken, now), VerifyDue::Due { days_over: 0 });
        // Ровно 30 дней — пора.
        let exactly = Settings {
            verify_last_at: (now - chrono::Duration::days(30)).to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&exactly, now), VerifyDue::Due { days_over: 0 });
        // Просрочено на 5 дней.
        let overdue = Settings {
            verify_last_at: (now - chrono::Duration::days(35)).to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&overdue, now), VerifyDue::Due { days_over: 5 });
        // Ещё рано: 29 дней и 12 часов из 30 — остался один день.
        let early = Settings {
            verify_last_at: (now - chrono::Duration::days(29) - chrono::Duration::hours(12))
                .to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&early, now), VerifyDue::NotYet { days_left: 1 });
        // Часы перевели назад: дата в будущем — не пора и без паники.
        let future = Settings {
            verify_last_at: (now + chrono::Duration::days(5)).to_rfc3339(),
            ..base.clone()
        };
        assert_eq!(verify_due(&future, now), VerifyDue::NotYet { days_left: 35 });
    }

    #[test]
    fn напоминание_переживает_перезапуск() {
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

        insert(&c, &r).unwrap(); // переиндексация того же файла
        assert_eq!(load_all(&c).unwrap()[0].decision.as_deref(), Some("keep"));

        let mut changed = r.clone();
        changed.sha256 = "cc".into();
        insert(&c, &changed).unwrap(); // файл подменили — решение устарело
        assert_eq!(load_all(&c).unwrap()[0].decision, None);
    }
    fn три_файла(c: &Connection) -> Vec<i64> {
        for (path, sha) in [("/a.jpg", "aa"), ("/b.jpg", "bb"), ("/c.jpg", "cc")] {
            insert(c, &row(path, sha, 1.0)).unwrap();
        }
        vec![id_of(c, "/a.jpg"), id_of(c, "/b.jpg"), id_of(c, "/c.jpg")]
    }

    #[test]
    fn дерево_альбомов_идёт_в_порядке_и_считает_файлы() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = три_файла(&c);
        let дача = create_album(&c, 0, "Дача 2024", "").unwrap();
        let май = create_album(&c, дача, "Май", "").unwrap();
        let июнь = create_album(&c, дача, "Июнь", "").unwrap();
        let год = create_album(&c, май, "1 год", "").unwrap();
        let печать = create_album(&c, 0, "На печать", "").unwrap();
        add_to_album(&c, дача, &[ids[0], ids[1]]).unwrap();
        add_to_album(&c, июнь, &[ids[2]]).unwrap();

        let tree = albums_tree(&c).unwrap();
        let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["Дача 2024", "Май", "1 год", "Июнь", "На печать"]);
        let depths: Vec<usize> = tree.iter().map(|n| n.depth).collect();
        assert_eq!(depths, [0, 1, 2, 1, 0]);
        let root = tree.iter().find(|n| n.id == дача).unwrap();
        assert_eq!((root.files, root.subalbums), (2, 2));
        assert!(tree.iter().any(|n| n.id == год && n.files == 0));
        assert_eq!(album_path(&c, год).unwrap(), "Дача 2024 / Май / 1 год");
        assert_eq!(
            albums_of_file(&c, ids[0]).unwrap(),
            vec!["Дача 2024".to_string()]
        );

        // Порядок подальбомов — данные: «Май» можно опустить ниже «Июня».
        move_album(&c, май, дача, Some(1)).unwrap();
        let tree = albums_tree(&c).unwrap();
        let siblings: Vec<&str> = tree
            .iter()
            .filter(|n| n.parent_id == дача)
            .map(|n| n.name.as_str())
            .collect();
        assert_eq!(siblings, ["Июнь", "Май"]);
        // Ветвь «На печать» осталась на месте.
        assert_eq!(
            tree.iter().find(|n| n.id == печать).unwrap().position,
            1
        );
    }

    #[test]
    fn имя_уникально_среди_соседей_и_не_ломается_на_пустом() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let дача23 = create_album(&c, 0, "Дача 2024", "").unwrap();
        let дача24 = create_album(&c, 0, "Дача 2025", "").unwrap();
        assert!(create_album(&c, 0, "дача 2024", "").is_err());
        // В разных ветвях одинаковые имена разрешены.
        create_album(&c, дача23, "Май", "").unwrap();
        create_album(&c, дача24, "Май", "").unwrap();
        assert!(create_album(&c, дача23, "  ", "").is_err());
        assert!(create_album(&c, 0, &"я".repeat(81), "").is_err());
        // 80 символов — уже допустимо; хвостовые пробелы срезаются.
        let id = create_album(&c, 0, &format!("  {}  ", "я".repeat(80)), "").unwrap();
        assert_eq!(album_path(&c, id).unwrap().chars().count(), 80);
        assert!(rename_album(&c, дача23, "Дача 2025").is_err());
        rename_album(&c, дача23, "Дача на даче").unwrap();
        assert_eq!(album_path(&c, дача23).unwrap(), "Дача на даче");
    }

    #[test]
    fn перемещение_запрещает_цикл_и_перенумеровывает_соседей() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let a = create_album(&c, 0, "A", "").unwrap();
        let b = create_album(&c, a, "B", "").unwrap();
        let d = create_album(&c, b, "D", "").unwrap();
        let e = create_album(&c, 0, "E", "").unwrap();
        assert!(move_album(&c, a, b, None).is_err());
        assert!(move_album(&c, a, a, None).is_err());
        // Перенос в конец и в начало: позиции плотные 0..n−1.
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
        // Имена по-прежнему уникальны среди новых соседей.
        assert!(move_album(&c, e, a, None).is_ok());
        assert!(create_album(&c, a, "E", "").is_err());
    }

    #[test]
    fn удаление_поддерева_трогает_только_строки() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = три_файла(&c);
        let дача = create_album(&c, 0, "Дача", "").unwrap();
        let май = create_album(&c, дача, "Май", "").unwrap();
        let июнь = create_album(&c, дача, "Июнь", "").unwrap();
        add_to_album(&c, май, &ids[..2]).unwrap();
        add_to_album(&c, июнь, &ids[2..]).unwrap();
        let печать = create_album(&c, 0, "На печать", "").unwrap();
        add_to_album(&c, печать, &ids).unwrap();

        assert_eq!(delete_album(&c, дача).unwrap(), 3);
        assert!(albums_tree(&c).unwrap().iter().all(|n| n.name != "Май"));
        assert_eq!(album_items(&c, май).unwrap().len(), 0);
        assert_eq!(album_items(&c, печать).unwrap().len(), 3);
        // Файлы на диске не наши: строки files целы.
        assert_eq!(load_all(&c).unwrap().len(), 3);
        // «На печать» остался единственным корнем с позицией 0.
        let root = &albums_tree(&c).unwrap()[0];
        assert_eq!((root.name.as_str(), root.position), ("На печать", 0));
    }

    #[test]
    fn добавление_идемпотентно_и_порядок_плотный() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = три_файла(&c);
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
        // «Выше/Ниже» меняет только порядок, не состав.
        assert!(move_in_album(&c, album, ids[2], -1).unwrap());
        let order: Vec<i64> = album_items(&c, album)
            .unwrap()
            .iter()
            .map(|r| r.id)
            .collect();
        assert_eq!(order, [ids[0], ids[2], ids[1]]);
        assert!(!move_in_album(&c, album, ids[0], -1).unwrap());
        // Убрали средний — позиции снова 0,1,2 без дырки.
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
    fn файл_в_корзине_остаётся_строкой_альбома() {
        let c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = три_файла(&c);
        let album = create_album(&c, 0, "Май", "").unwrap();
        add_to_album(&c, album, &ids).unwrap();
        c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [ids[1]])
            .unwrap();
        let node = &albums_tree(&c).unwrap()[0];
        assert_eq!((node.files, node.trashed), (3, 1));
        // Сетка берёт только неудалённые, данные альбома не теряются.
        let visible = album_items(&c, album)
            .unwrap()
            .into_iter()
            .filter(|r| r.trashed_at.is_none())
            .count();
        assert_eq!(visible, 2);
    }

    #[test]
    fn удаление_строк_файла_чистит_альбом_и_сжимает_порядок() {
        let mut c = Connection::open_in_memory().unwrap();
        init(&c).unwrap();
        let ids = три_файла(&c);
        let album = create_album(&c, 0, "Май", "").unwrap();
        add_to_album(&c, album, &ids).unwrap();
        // Так же, как «убрать каталог» и сброс индекса: строки files → album_files.
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

    /// Словарь тегов: правка сохраняется на месте, метка уникальна, удаление чистит одну строку.
    #[test]
    fn словарь_тегов_хранится_в_бд_по_модели() {
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
        // Правка по id: метка и промпты меняются, строка не дублируется.
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
        // Вторая модель — свой словарь, чужие метки не мешают.
        let mut other = mk("beach", "пляж", 0);
        other.model = "m2".into();
        save_tag_def(&c, &other).unwrap();
        assert_eq!(load_tag_defs(&c, "m2").unwrap().len(), 1);
        // Метка уникальна внутри модели: иначе две строки `tags` с одним ключом.
        let mut clash = mk("clash", "море", 2);
        clash.model = "m1".into();
        assert!(save_tag_def(&c, &clash).is_err());
        assert_eq!(delete_tag_def(&c, "m1", "sea").unwrap(), 1);
        assert_eq!(load_tag_defs(&c, "m1").unwrap().len(), 1);
    }

    #[test]
    fn старая_база_получает_альбомы_и_не_теряет_файлы() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch(
            "CREATE TABLE files(
                id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
                mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
                taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
                thumb TEXT, indexed_at INTEGER NOT NULL);
             INSERT INTO files(path,size,kind,indexed_at) VALUES('/old.jpg',10,'image',0);
             PRAGMA user_version=4;",
        )
        .unwrap();
        init(&c).unwrap();
        let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 7);
        assert_eq!(load_all(&c).unwrap().len(), 1);
        let album = create_album(&c, 0, "Старое", "").unwrap();
        add_to_album(&c, album, &[id_of(&c, "/old.jpg")]).unwrap();
        assert_eq!(album_items(&c, album).unwrap().len(), 1);
        init(&c).unwrap(); // повторный init не падает и не плодит таблиц/строк
        assert_eq!(albums_tree(&c).unwrap().len(), 1);
        assert_eq!(album_items(&c, album).unwrap().len(), 1);
    }

}
