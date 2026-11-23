//! Занятое место и очистка кэша. Архив не трогается: только `<кэш>/photocat/` и БД.
//!
//! Это единственный модуль, которому разрешено удалять файлы, и только внутри кэша
//! приложения. Пути проверяются: всё, что вне `db::cache_dir()`, не удаляется.

use crate::db;
use anyhow::{Context, Result, bail};
use std::path::Path;

#[derive(Debug, Clone, Default)]
pub struct Usage {
    pub db_bytes: u64,
    pub thumbs_bytes: u64,
    pub faces_bytes: u64,
    pub models_bytes: u64,
    pub total_bytes: u64,
}

#[derive(Debug, Clone, Default)]
pub struct Cleared {
    pub files: usize,
    pub bytes: u64,
}

impl std::fmt::Display for Cleared {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "удалено файлов {}, {}",
            self.files,
            db::human(self.bytes as i64)
        )
    }
}

/// Размер каталога без разыменования симлинков; ошибки чтения пропускаются.
pub fn dir_size(dir: &Path) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    for e in walkdir::WalkDir::new(dir)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if e.file_type().is_file()
            && let Ok(meta) = e.metadata()
        {
            files += 1;
            bytes += meta.len();
        }
    }
    (files, bytes)
}

pub fn usage(db_path: &Path, models_dir: &Path) -> Usage {
    let cache = db::cache_dir();
    let db_bytes = std::fs::metadata(db_path).map(|m| m.len()).unwrap_or(0);
    let (_, thumbs_bytes) = dir_size(&cache.join("thumbs"));
    let (_, faces_bytes) = dir_size(&cache.join("faces"));
    let (_, models_bytes) = dir_size(models_dir);
    let (_, total_bytes) = dir_size(&cache);
    Usage {
        db_bytes,
        thumbs_bytes,
        faces_bytes,
        models_bytes,
        total_bytes,
    }
}

/// Удалить каталог только внутри кэша photocat. Возвращает число файлов и байт.
fn clear_cache_dir(dir: &Path) -> Result<Cleared> {
    if !dir.exists() {
        return Ok(Cleared::default());
    }
    let cache = db::cache_dir();
    let dir = dir
        .canonicalize()
        .with_context(|| format!("не найти {}", dir.display()))?;
    let cache = cache
        .canonicalize()
        .with_context(|| format!("не найти {}", cache.display()))?;
    if dir == cache || !dir.starts_with(&cache) {
        bail!("отказ: {} вне кэша photocat", dir.display());
    }
    let (files, bytes) = dir_size(&dir);
    std::fs::remove_dir_all(&dir).with_context(|| format!("не очистить {}", dir.display()))?;
    std::fs::create_dir_all(&dir)?;
    Ok(Cleared { files, bytes })
}

/// Превью фото и обложек видео. После очистки сетка покажет «нет превью» до переиндексации.
pub fn clear_thumbs() -> Result<Cleared> {
    clear_cache_dir(&db::cache_dir().join("thumbs"))
}

/// Кропы лиц вместе со строками лиц: повторный проход снова найдёт лица.
pub fn clear_faces(db_path: &Path) -> Result<Cleared> {
    let cleared = clear_cache_dir(&db::cache_dir().join("faces"))?;
    let conn = db::open(db_path)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM faces", [])?;
    tx.execute("DELETE FROM face_scans", [])?;
    tx.execute("DELETE FROM face_clusters", [])?;
    tx.commit()?;
    Ok(cleared)
}

/// Сбросить индекс: строки файлов, теги, OCR, векторы и лица. Настройки и модели остаются.
pub fn reset_index(db_path: &Path) -> Result<()> {
    let conn = db::open(db_path)?;
    let tx = conn.unchecked_transaction()?;
    for table in [
        "ocr",
        "embeddings",
        "tags",
        "tag_hidden",
        "faces",
        "face_scans",
        "face_clusters",
        "album_files",
        "files",
    ] {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .with_context(|| format!("не очистить таблицу {table}"))?;
    }
    tx.commit()?;
    Ok(())
}
