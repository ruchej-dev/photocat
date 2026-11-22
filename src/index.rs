//! Индексация каталога: обход, параллельная обработка, запись в SQLite.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use anyhow::Result;
use rayon::prelude::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

pub struct Stats {
    pub total: usize,
    pub indexed: usize,
    pub errors: usize,
    pub bytes: i64,
    pub seconds: f64,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "проиндексировано {}/{} файлов ({}), ошибок {}, за {:.1} с",
            self.indexed,
            self.total,
            db::human(self.bytes),
            self.errors,
            self.seconds
        )
    }
}

pub fn run(
    root: &Path,
    jobs: usize,
    db_path: &Path,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    let t0 = std::time::Instant::now();
    let root = std::fs::canonicalize(root)?;
    let thumbs_dir = db::cache_dir().join("thumbs");
    std::fs::create_dir_all(&thumbs_dir)?;

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
    pool.install(|| {
        files.par_iter().for_each_with(tx, |tx, path| {
            let res = process(path, &root, &thumbs_dir);
            match res {
                Ok((rec, size)) => {
                    bytes.fetch_add(size, std::sync::atomic::Ordering::Relaxed);
                    let _ = tx.send(rec);
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
        errors: errors.load(std::sync::atomic::Ordering::Relaxed),
        bytes: bytes.load(std::sync::atomic::Ordering::Relaxed),
        seconds: t0.elapsed().as_secs_f64(),
    })
}

fn process(p: &Path, _root: &Path, thumbs_dir: &Path) -> Result<(FileRow, i64)> {
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
    let mut thumb: Option<String> = None;

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
                eprintln!("\nне декодируется {}: {e}", p.display());
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
        thumb,
        decision: None,
    };
    Ok((row, size))
}
