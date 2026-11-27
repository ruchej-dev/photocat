//! Used space and cache cleanup. The archive is untouched: only `<cache>/photocat/` and the DB.
//!
//! This is the only module allowed to delete files, and only inside the application cache.
//! Paths are checked: anything outside `db::cache_dir()` is not deleted.

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
            "removed {} files, {}",
            self.files,
            db::human(self.bytes as i64)
        )
    }
}

/// The size of a folder without following symlinks; read errors are skipped.
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

/// Delete a folder only inside the photocat cache. Returns the number of files and bytes.
fn clear_cache_dir(dir: &Path) -> Result<Cleared> {
    if !dir.exists() {
        return Ok(Cleared::default());
    }
    let cache = db::cache_dir();
    let dir = dir
        .canonicalize()
        .with_context(|| format!("cannot find {}", dir.display()))?;
    let cache = cache
        .canonicalize()
        .with_context(|| format!("cannot find {}", cache.display()))?;
    if dir == cache || !dir.starts_with(&cache) {
        bail!("refused: {} is outside the photocat cache", dir.display());
    }
    let (files, bytes) = dir_size(&dir);
    std::fs::remove_dir_all(&dir).with_context(|| format!("cannot clear {}", dir.display()))?;
    std::fs::create_dir_all(&dir)?;
    Ok(Cleared { files, bytes })
}

/// Previews of photos and video covers. The rows lose their preview path too, so the next index
/// rebuilds every preview instead of skipping the file (M25.4).
pub fn clear_thumbs(db_path: &Path) -> Result<Cleared> {
    let cleared = clear_cache_dir(&db::cache_dir().join("thumbs"))?;
    forget_thumbs(db_path)?;
    Ok(cleared)
}

/// Null the `thumb` column: `index::can_skip` asks it, so a cleared preview comes back with the
/// next pass. Split from the folder removal, which touches the owner's cache and is not called in
/// tests (M25.4).
pub fn forget_thumbs(db_path: &Path) -> Result<()> {
    let conn = db::open(db_path)?;
    conn.execute("UPDATE files SET thumb = NULL", [])?;
    Ok(())
}

/// Face crops together with the face rows: a repeated pass finds the faces again.
pub fn clear_faces(db_path: &Path) -> Result<Cleared> {
    let cleared = clear_cache_dir(&db::cache_dir().join("faces"))?;
    let conn = db::open(db_path)?;
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM face_matches", [])?;
    tx.execute("DELETE FROM faces", [])?;
    tx.execute("DELETE FROM face_scans", [])?;
    tx.execute("DELETE FROM face_clusters", [])?;
    tx.commit()?;
    Ok(cleared)
}

/// Reset the index: file rows, tags, OCR, vectors and faces. The settings and the models stay.
pub fn reset_index(db_path: &Path) -> Result<()> {
    let conn = db::open(db_path)?;
    let tx = conn.unchecked_transaction()?;
    for table in [
        "ocr",
        "embeddings",
        "tags",
        "tag_hidden",
        "face_matches",
        "faces",
        "face_scans",
        "face_clusters",
        "album_files",
        "files",
    ] {
        tx.execute(&format!("DELETE FROM {table}"), [])
            .with_context(|| format!("cannot clear the table {table}"))?;
    }
    tx.commit()?;
    Ok(())
}
