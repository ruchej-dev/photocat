//! OCR документов и скриншотов: отдельный проход `tesseract` по кандидатам.
//!
//! Текст живёт в БД (`ocr`) и ищется через FTS5; рядом с оригиналом ничего не пишется.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// Длинная сторона кадра, который уходит в tesseract: больше не нужно, а декод дорожает.
pub const MAX_SIDE: u32 = 2000;

pub struct Stats {
    pub candidates: usize,
    pub done: usize,
    pub skipped: usize,
    pub errors: usize,
    pub chars: i64,
    pub seconds: f64,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.candidates == 0 {
            return f.write_str("кандидатов на текст нет: все снимки с камерой и без тега документа");
        }
        write!(
            f,
            "распознано {} из {} файлов, знаков {}, ошибок {}, за {:.1} с",
            self.done, self.candidates, self.chars, self.errors, self.seconds
        )?;
        if self.skipped > 0 {
            write!(f, " (пропущено {}, текст уже есть)", self.skipped)?;
        }
        Ok(())
    }
}

/// Уверенный верхний тег — та же формула, что `embed::confident_tag_with`.
/// Продублирована нарочно: OCR не зависит от модели и её модуля.
fn confident(tags: &[(String, f32)], show: f32, margin: f32) -> Option<(&str, f32)> {
    let (tag, top) = tags.first()?;
    if *top < show {
        return None;
    }
    if let Some((_, second)) = tags.get(1)
        && top - second < margin
    {
        return None;
    }
    Some((tag.as_str(), *top))
}

/// Нужен ли текст: фото без EXIF-камеры (скан или скриншот) либо файл, которому CLIP
/// уверенно дал тег «документ»/«скриншот». Решают метаданные и содержимое, а не имя файла.
pub fn wants_text(
    f: &FileRow,
    tags: &HashMap<i64, Vec<(String, f32)>>,
    show: f32,
    margin: f32,
) -> bool {
    if f.kind != "image" {
        return false;
    }
    if f.camera.is_none() {
        return true;
    }
    let Some(list) = tags.get(&f.id) else {
        return false;
    };
    matches!(
        confident(list, show, margin).map(|(tag, _)| tag),
        Some("документ" | "скриншот")
    )
}

/// Кандидаты прохода: снимки каталога и сколько из них попало под эвристику.
pub struct Plan {
    pub images: usize,
    pub candidates: usize,
}

/// Только план, без OCR: каталог и БД читаются, текст не распознаётся.
pub fn plan(db_path: &Path, all: bool) -> Result<Plan> {
    let conn = db::open(db_path)?;
    let (images, selected) = select(&conn, all)?;
    Ok(Plan {
        images,
        candidates: selected.len(),
    })
}

fn select(conn: &rusqlite::Connection, all: bool) -> Result<(usize, Vec<FileRow>)> {
    let settings = db::load_settings(conn);
    let rows = db::load_all(conn)?;
    let tags = db::load_visible_tags(conn)?;
    let images = rows.iter().filter(|r| r.kind == "image").count();
    let selected = rows
        .into_iter()
        .filter(|r| all || wants_text(r, &tags, settings.tag_show, settings.tag_margin))
        .collect();
    Ok((images, selected))
}

/// Пакетный проход по кандидатам: `all` — все изображения, а не только документы и скриншоты.
pub fn run(
    db_path: &Path,
    jobs: usize,
    all: bool,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    run_selected(db_path, jobs, all, None, progress)
}

/// Распознать конкретные файлы (кнопка у выбранного снимка). Явный запрос — всегда заново:
/// владелец мог попросить пересчитать текст, даже если sha256 не менялся.
pub fn run_files(
    db_path: &Path,
    ids: &[i64],
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    run_selected(db_path, 1, false, Some(ids), progress)
}

fn run_selected(
    db_path: &Path,
    jobs: usize,
    all: bool,
    only: Option<&[i64]>,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let lang = db::load_settings(&conn).ocr_lang;
    // Явно выбранный файл распознаём независимо от эвристики.
    let (_, selected) = select(&conn, all || only.is_some())?;
    let selected: Vec<FileRow> = match only {
        Some(ids) => selected.into_iter().filter(|r| ids.contains(&r.id)).collect(),
        None => selected,
    };
    let done_shas = db::ocr_shas(&conn)?;
    let candidates = selected.len();
    let todo: Vec<FileRow> = match only {
        Some(_) => selected,
        None => selected
            .into_iter()
            .filter(|r| done_shas.get(&r.id) != Some(&r.sha256))
            .collect(),
    };
    let skipped = candidates - todo.len();

    if !todo.is_empty() && derive::tesseract_version().is_none() {
        anyhow::bail!(
            "tesseract не найден в PATH — установите tesseract и языковые пакеты ({lang}); \
             остальной функционал работает без него"
        );
    }

    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg(format!("OCR: {lang}"));
    }

    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()?;

    let (tx, rx) = mpsc::channel::<(i64, String, String)>();
    let writer = {
        let db_path = db_path.to_path_buf();
        let lang = lang.clone();
        std::thread::spawn(move || -> Result<(usize, i64)> {
            let mut conn = db::open(&db_path)?;
            let tx = conn.transaction()?;
            let (mut n, mut chars) = (0usize, 0i64);
            for (id, sha, text) in rx {
                db::save_ocr(&tx, id, &sha, &lang, &text)?;
                chars += text.chars().count() as i64;
                n += 1;
            }
            tx.commit()?;
            Ok((n, chars))
        })
    };

    let errors = AtomicUsize::new(0);
    pool.install(|| {
        todo.par_iter().for_each_with(tx, |tx, f| {
            match derive::ocr_image(Path::new(&f.path), &lang, MAX_SIDE) {
                Ok(text) => {
                    let _ = tx.send((f.id, f.sha256.clone(), text));
                }
                Err(e) => {
                    errors.fetch_add(1, Ordering::Relaxed);
                    eprintln!("\nOCR {}: {e}", f.path);
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

    let (done, chars) = writer.join().map_err(|_| anyhow::anyhow!("поток записи упал"))??;
    if let Some(p) = &progress {
        p.set_msg("готово");
        p.finish();
    }
    Ok(Stats {
        candidates,
        done,
        skipped,
        errors: errors.load(Ordering::Relaxed),
        chars,
        seconds: t0.elapsed().as_secs_f64(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(kind: &str, camera: Option<&str>) -> FileRow {
        FileRow {
            id: 1,
            path: "/x.jpg".into(),
            size: 10,
            mtime: 0,
            sha256: "aa".into(),
            kind: kind.into(),
            width: 100,
            height: 100,
            taken_at: None,
            taken_src: "mtime".into(),
            lat: None,
            lon: None,
            camera: camera.map(str::to_string),
            dhash: Some(1),
            sharpness: None,
            duration_s: None,
            thumb: None,
            decision: None,
        }
    }

    fn tags(tag: &str, score: f32) -> HashMap<i64, Vec<(String, f32)>> {
        HashMap::from([(1, vec![(tag.to_string(), score), ("дети".to_string(), score - 0.05)])])
    }

    const SHOW: f32 = 0.22;
    const MARGIN: f32 = 0.01;

    #[test]
    fn screenshots_and_scans_are_candidates_without_tags() {
        assert!(wants_text(&row("image", None), &HashMap::new(), SHOW, MARGIN));
    }

    #[test]
    fn camera_photo_needs_confident_document_or_screenshot_tag() {
        let none = HashMap::new();
        assert!(!wants_text(&row("image", Some("Pixel")), &none, SHOW, MARGIN));
        assert!(!wants_text(&row("image", Some("Pixel")), &tags("дети", 0.30), SHOW, MARGIN));
        assert!(wants_text(&row("image", Some("Pixel")), &tags("документ", 0.30), SHOW, MARGIN));
        assert!(wants_text(&row("image", Some("Pixel")), &tags("скриншот", 0.24), SHOW, MARGIN));
        // уверенности нет: низкий вес или отрыв от второго меньше порога
        assert!(!wants_text(&row("image", Some("Pixel")), &tags("документ", 0.20), SHOW, MARGIN));
        let flat = HashMap::from([(1, vec![
            ("документ".to_string(), 0.30),
            ("дети".to_string(), 0.295),
        ])]);
        assert!(!wants_text(&row("image", Some("Pixel")), &flat, SHOW, MARGIN));
    }

    #[test]
    fn video_is_never_an_ocr_candidate() {
        assert!(!wants_text(&row("video", None), &HashMap::new(), SHOW, MARGIN));
    }
}
