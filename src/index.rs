//! Индексация каталога: обход, параллельная обработка, запись в SQLite.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::tools;
use anyhow::Result;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

pub struct Stats {
    pub total: usize,
    pub indexed: usize,
    /// Файлы с тем же путём, размером и mtime: содержимое не читалось повторно.
    pub skipped: usize,
    pub errors: usize,
    pub bytes: i64,
    pub seconds: f64,
    /// Прогон прерван владельцем: остальные файлы не обработаны.
    pub cancelled: bool,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}проиндексировано {}/{} файлов ({}), без изменений {}, ошибок {}, за {:.1} с",
            if self.cancelled { "остановлено: " } else { "" },
            self.indexed,
            self.total,
            db::human(self.bytes),
            self.skipped,
            self.errors,
            self.seconds
        )
    }
}

pub fn run(
    root: &Path,
    jobs: usize,
    db_path: &Path,
    tools: &tools::Tools,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    let t0 = std::time::Instant::now();
    let root = std::fs::canonicalize(root)?;
    let thumbs_dir = db::cache_dir().join("thumbs");
    std::fs::create_dir_all(&thumbs_dir)?;
    // Повторная индексация не перечитывает неизменённые файлы: ключ — путь + размер + mtime.
    let existing: std::collections::HashMap<String, Existing> = {
        let conn = db::open(db_path)?;
        let mut rows = db::load_all(&conn)?;
        rows.extend(db::load_trashed(&conn)?);
        rows.into_iter()
            .map(|r| {
                (
                    r.path.clone(),
                    Existing {
                        size: r.size,
                        mtime: r.mtime,
                        sha_ok: !r.sha256.is_empty(),
                        thumb: r.thumb.is_some(),
                        duration: r.duration_s.is_some(),
                        kind: r.kind,
                    },
                )
            })
            .collect()
    };

    let mut files: Vec<PathBuf> = Vec::new();
    for e in walkdir::WalkDir::new(&root)
        .follow_links(false)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        if !e.file_type().is_file() {
            continue;
        }
        if derive::kind_of(e.path()).is_some() {
            files.push(e.path().to_path_buf());
        }
    }
    let total = files.len();
    if let Some(p) = &progress {
        p.set_total(total);
        p.set_msg("обход каталога завершён");
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()?;

    let (tx, rx) = mpsc::channel::<FileRow>();
    let writer = {
        let db_path = db_path.to_path_buf();
        std::thread::spawn(move || -> Result<usize> {
            let mut conn = db::open(&db_path)?;
            let tx = conn.transaction()?;
            let mut n = 0usize;
            for rec in rx {
                db::insert(&tx, &rec)?;
                n += 1;
            }
            tx.commit()?;
            Ok(n)
        })
    };

    let bytes = std::sync::atomic::AtomicI64::new(0);
    let errors = std::sync::atomic::AtomicUsize::new(0);
    let skipped = std::sync::atomic::AtomicUsize::new(0);
    pool.install(|| {
        files.par_iter().for_each_with(tx, |tx, path| {
            // «Стоп»: уже начатые файлы доводим, остальные не трогаем.
            if progress.as_ref().is_some_and(|p| p.cancelled()) {
                return;
            }
            let key = path.to_string_lossy().to_string();
            if let Ok(md) = std::fs::metadata(path) {
                let size = md.len() as i64;
                let mtime = md
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0);
                let kind = derive::kind_of(path).unwrap_or("other");
                if can_skip(existing.get(&key), size, mtime, kind) {
                    skipped.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    if let Some(p) = &progress {
                        p.tick();
                    }
                    return;
                }
            }
            let res = process(path, &root, &thumbs_dir, tools);
            match res {
                Ok((rec, size, soft_error)) => {
                    bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                    let _ = tx.send(rec);
                    if soft_error {
                        // Файл в индексе, но без превью/метаданных: ошибка видна в счётчике.
                        errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if let Some(p) = &progress {
                            p.error();
                        }
                    }
                }
                Err(e) => {
                    errors.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    eprintln!("\nошибка {}: {e}", path.display());
                    if let Some(p) = &progress {
                        p.error();
                    }
                }
            }
            if let Some(p) = &progress {
                p.tick();
            }
        })
    });

    let indexed = writer.join().map_err(|_| anyhow::anyhow!("поток записи упал"))??;
    if let Some(p) = &progress {
        p.set_msg("готово");
        p.finish();
    }
    Ok(Stats {
        total,
        indexed,
        skipped: skipped.load(std::sync::atomic::Ordering::Relaxed),
        errors: errors.load(std::sync::atomic::Ordering::Relaxed),
        bytes: bytes.load(std::sync::atomic::Ordering::Relaxed),
        seconds: t0.elapsed().as_secs_f64(),
        cancelled: progress.as_ref().is_some_and(|p| p.cancelled()),
    })
}

/// Что уже лежит в индексе по этому пути.
struct Existing {
    size: i64,
    mtime: i64,
    sha_ok: bool,
    thumb: bool,
    duration: bool,
    kind: String,
}

/// Файл не изменился — читать и декодировать заново не нужно.
/// Видео без кадра-обложки или длительности обрабатывается заново: это старый индекс.
fn can_skip(e: Option<&Existing>, size: i64, mtime: i64, kind: &str) -> bool {
    let Some(e) = e else {
        return false;
    };
    e.size == size
        && e.mtime == mtime
        && e.sha_ok
        && e.kind == kind
        && e.thumb
        && (kind != "video" || e.duration)
}

fn process(
    p: &Path,
    _root: &Path,
    thumbs_dir: &Path,
    tools: &tools::Tools,
) -> Result<(FileRow, i64, bool)> {
    let md = std::fs::metadata(p)?;
    let size = md.len() as i64;
    let mtime = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let kind = derive::kind_of(p).unwrap_or("other");
    let sha = derive::sha256_file(p)?;
    let key = &sha[..24.min(sha.len())];

    let mut width = 0u32;
    let mut height = 0u32;
    let mut dhash: Option<u64> = None;
    let mut sharpness: Option<f64> = None;
    let mut duration: Option<f64> = None;
    let mut thumb: Option<String> = None;
    let mut soft_error = false;

    if kind == "image" {
        match derive::analyze_image(p, thumbs_dir, key, 320) {
            Ok(info) => {
                width = info.width;
                height = info.height;
                dhash = Some(info.dhash);
                sharpness = Some(info.sharpness as f64);
                thumb = Some(info.thumb);
            }
            Err(e) => {
                // битый/нечитаемый файл: индексируем без хеша и превью
                soft_error = true;
                eprintln!("\nне декодируется {}: {e:#}", p.display());
            }
        }
    } else if kind == "video" {
        match derive::analyze_video(tools, p, thumbs_dir, key, 320) {
            Ok(info) => {
                width = info.width;
                height = info.height;
                dhash = Some(info.dhash);
                sharpness = Some(info.sharpness as f64);
                duration = Some(info.duration_s);
                thumb = Some(info.thumb);
            }
            Err(e) => {
                // Нет ffmpeg/битый файл: видео остаётся в индексе, но без превью и хеша.
                soft_error = true;
                eprintln!("\nвидео без превью {}: {e:#}", p.display());
            }
        }
    }

    let meta = derive::exif_meta(p);
    let (taken_at, taken_src) = match meta.taken_at {
        Some(d) => (Some(d), "exif"),
        None => match derive::filename_date(p) {
            Some(d) => (Some(d), "filename"),
            None => (derive::mtime_date(p), "mtime"),
        },
    };

    let row = FileRow {
        id: 0,
        path: p.to_string_lossy().to_string(),
        size,
        mtime,
        sha256: sha,
        kind: kind.to_string(),
        width: width as i64,
        height: height as i64,
        taken_at,
        taken_src: taken_src.to_string(),
        lat: meta.lat,
        lon: meta.lon,
        camera: meta.camera,
        dhash: dhash.map(|v| v as i64),
        sharpness,
        duration_s: duration,
        thumb,
        decision: None,
        trashed_at: None,
    };
    Ok((row, size, soft_error))
}

#[cfg(test)]
mod tests {
    use super::{Existing, can_skip};

    fn e() -> Existing {
        Existing {
            size: 10,
            mtime: 100,
            sha_ok: true,
            thumb: true,
            duration: true,
            kind: "image".into(),
        }
    }

    #[test]
    fn неизменённый_файл_пропускается() {
        assert!(can_skip(Some(&e()), 10, 100, "image"));
    }

    #[test]
    fn изменённый_размер_или_время_перечитывается() {
        assert!(!can_skip(Some(&e()), 11, 100, "image"));
        assert!(!can_skip(Some(&e()), 10, 101, "image"));
        assert!(!can_skip(None, 10, 100, "image"));
    }

    #[test]
    fn видео_без_обложки_или_длительности_перечитывается() {
        let mut v = e();
        v.kind = "video".into();
        v.thumb = false;
        assert!(!can_skip(Some(&v), 10, 100, "video"));
        v.thumb = true;
        v.duration = false;
        assert!(!can_skip(Some(&v), 10, 100, "video"));
        v.duration = true;
        assert!(can_skip(Some(&v), 10, 100, "video"));
    }
}
