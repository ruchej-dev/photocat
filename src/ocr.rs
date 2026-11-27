//! OCR of documents and screenshots: a separate `tesseract` pass over the candidates.
//!
//! The text lives in the DB (`ocr`) and is searched through FTS5; nothing is written next to the original.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use crate::tools::Tool;
use anyhow::Result;
use rayon::prelude::*;
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;

/// The long side of the frame handed to tesseract: more is not needed and the decode gets pricier.
pub const MAX_SIDE: u32 = 2000;

pub struct Stats {
    pub candidates: usize,
    pub done: usize,
    pub skipped: usize,
    /// The candidates the pass did not take because their catalog has text off (M26.9).
    pub pass_skipped: usize,
    pub errors: usize,
    pub chars: i64,
    pub seconds: f64,
    /// The pass was stopped by the owner: the remaining files are left for next time.
    pub cancelled: bool,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for Stats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.candidates == 0 {
            return f.write_str(i18n::t(
                self.lang,
                "no text candidates: every photo has a camera and no document tag",
            ));
        }
        let prefix = i18n::t(
            self.lang,
            if self.cancelled {
                "stopped: "
            } else {
                ""
            },
        );
        let body = i18n::fill(
            self.lang,
            "recognised {} of {} files, {} characters, {} errors, {} s",
            &[
                self.done.to_string(),
                self.candidates.to_string(),
                self.chars.to_string(),
                self.errors.to_string(),
                format!("{:.1}", self.seconds),
            ],
        );
        write!(f, "{prefix}{body}")?;
        if self.skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped, text already there)",
                &[self.skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped — their folders have text off)",
                &[self.pass_skipped.to_string()],
            );
            write!(f, "{note}")?;
        }
        Ok(())
    }
}

/// The confident top tag — the same formula as `embed::confident_tag_with`.
/// Duplicated on purpose: OCR does not depend on the model or its module.
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

/// Whether the text is wanted: a photo without an EXIF camera (a scan or a screenshot) or a file
/// the model confidently tagged "document"/"screenshot". Metadata and content decide, not the name.
pub fn wants_text(
    f: &FileRow,
    tags: &HashMap<i64, Vec<(String, f32)>>,
    show: f32,
    margin: f32,
    text_labels: &[String],
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
        Some(tag) if text_labels.iter().any(|l| l == tag)
    )
}

/// The candidates of the pass: the images of the folder and how many the heuristic picked.
pub struct Plan {
    pub images: usize,
    pub candidates: usize,
    /// Of the candidates: those without text yet (or whose file changed).
    pub todo: usize,
    /// The candidates the pass did not take because their catalog has text off (M26.9).
    pub pass_skipped: usize,
}

/// The plan only, no OCR: the folder and the DB are read, no text is recognized.
pub fn plan(db_path: &Path, all: bool) -> Result<Plan> {
    let conn = db::open(db_path)?;
    let (images, selected, pass_skipped) = select(&conn, all)?;
    let done = db::ocr_shas(&conn)?;
    let todo = selected
        .iter()
        .filter(|r| done.get(&r.id) != Some(&r.sha256))
        .count();
    Ok(Plan {
        images,
        candidates: selected.len(),
        todo,
        pass_skipped,
    })
}

/// The images of the pass and the candidates of the heuristic, split by `Settings::pass_allows`
/// (M26.9): the third number is what a catalog with text off kept out of the pass.
fn select(conn: &rusqlite::Connection, all: bool) -> Result<(usize, Vec<FileRow>, usize)> {
    let settings = db::load_settings(conn);
    // A switched-off folder takes no part in the pass (M26.1): its files are hidden, and the
    // owner switches it back on to let the pass catch up.
    let rows: Vec<FileRow> = db::load_all(conn)?
        .into_iter()
        .filter(|r| !settings.path_hidden(&r.path))
        .collect();
    let tags = db::load_visible_tags(conn)?;
    let images = rows.iter().filter(|r| r.kind == "image").count();
    // The labels of `document`/`screenshot` from every dictionary: the ids are stable, the words
    // are the owner's, so the heuristic never hard-codes a label (M23.6).
    let text = db::text_tag_labels(conn);
    let wanted: Vec<FileRow> = rows
        .into_iter()
        // `--all` widens the heuristic to every image but leaves videos alone: a cover frame is no document.
        .filter(|r| {
            r.kind == "image"
                && (all
                    || wants_text(r, &tags, settings.tag_show, settings.tag_margin, &text))
        })
        .collect();
    let pass_skipped = wanted
        .iter()
        .filter(|r| !settings.pass_allows(&r.path, db::Pass::Text))
        .count();
    let selected = wanted
        .into_iter()
        .filter(|r| settings.pass_allows(&r.path, db::Pass::Text))
        .collect();
    Ok((images, selected, pass_skipped))
}

/// A batch pass over the candidates: `all` means every image, not only documents and screenshots.
pub fn run(
    db_path: &Path,
    jobs: usize,
    all: bool,
    progress: Option<Arc<Progress>>,
) -> Result<Stats> {
    run_selected(db_path, jobs, all, None, progress)
}

/// Recognize specific files (the button on the chosen photo). An explicit request always reruns:
/// the owner may have asked for the text again even though the sha256 did not change.
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
    let settings = db::load_settings(&conn);
    let lang = settings.ocr_lang.clone();
    let tools = settings.tools.clone();
    // An explicitly chosen file is recognized regardless of the heuristic.
    let (_, selected, pass_skipped) = select(&conn, all || only.is_some())?;
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

    // "Stop" was pressed before the start: nothing runs and tesseract is not probed for nothing.
    if progress.as_ref().is_some_and(|p| p.cancelled()) {
        if let Some(p) = &progress {
            p.finish();
        }
        return Ok(Stats {
            candidates,
            done: 0,
            skipped,
            pass_skipped,
            errors: 0,
            chars: 0,
            seconds: t0.elapsed().as_secs_f64(),
            cancelled: true,
            lang: Lang::default(),
        });
    }

    if !todo.is_empty() && tools.probe(Tool::Tesseract).is_none() {
        anyhow::bail!(
            "{}",
            i18n::fill(
                settings.lang,
                "tesseract does not start ({}) — install tesseract and the language packs ({}) or give its path in the settings; the rest works without it",
                &[
                    tools.source(Tool::Tesseract, settings.lang).to_string(),
                    settings.ocr_lang.clone()
                ]
            )
        );
    }

    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg(format!("OCR: {}", settings.ocr_lang));
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
            // "Stop": the file already started is finished, the rest wait for the next run.
            if progress.as_ref().is_some_and(|p| p.cancelled()) {
                return;
            }
            match derive::ocr_image(&tools, Path::new(&f.path), &lang, MAX_SIDE) {
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

    let (done, chars) = writer
        .join()
        .map_err(|_| anyhow::anyhow!(i18n::t(settings.lang, "the writing thread died")))??;
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "done"));
        p.finish();
    }
    Ok(Stats {
        candidates,
        done,
        skipped,
        pass_skipped,
        errors: errors.load(Ordering::Relaxed),
        chars,
        seconds: t0.elapsed().as_secs_f64(),
        cancelled: progress.as_ref().is_some_and(|p| p.cancelled()),
        lang: Lang::default(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The labels of the default dictionary that make OCR look for text.
    fn labels() -> Vec<String> {
        vec!["document".to_string(), "screenshot".to_string()]
    }

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
            trashed_at: None,
        }
    }

    fn tags(tag: &str, score: f32) -> HashMap<i64, Vec<(String, f32)>> {
        HashMap::from([(1, vec![(tag.to_string(), score), ("children".to_string(), score - 0.05)])])
    }

    const SHOW: f32 = 0.22;
    const MARGIN: f32 = 0.01;

    #[test]
    fn screenshots_and_scans_are_candidates_without_tags() {
        assert!(wants_text(&row("image", None), &HashMap::new(), SHOW, MARGIN, &labels()));
    }

    #[test]
    fn camera_photo_needs_confident_document_or_screenshot_tag() {
        let none = HashMap::new();
        assert!(!wants_text(&row("image", Some("Pixel")), &none, SHOW, MARGIN, &labels()));
        assert!(!wants_text(&row("image", Some("Pixel")), &tags("children", 0.30), SHOW, MARGIN, &labels()));
        assert!(wants_text(&row("image", Some("Pixel")), &tags("document", 0.30), SHOW, MARGIN, &labels()));
        assert!(wants_text(&row("image", Some("Pixel")), &tags("screenshot", 0.24), SHOW, MARGIN, &labels()));
        // no confidence: the weight is low or the margin over the second tag is under the threshold
        assert!(!wants_text(&row("image", Some("Pixel")), &tags("document", 0.20), SHOW, MARGIN, &labels()));
        let flat = HashMap::from([(1, vec![
            ("document".to_string(), 0.30),
            ("children".to_string(), 0.295),
        ])]);
        assert!(!wants_text(&row("image", Some("Pixel")), &flat, SHOW, MARGIN, &labels()));
    }

    #[test]
    fn cancelled_before_start_does_nothing() {
        // A unique name per run: the test file does not interfere and is not deleted (the bounds forbid it).
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "pc-ocr-cancel-{}-{stamp}.db",
            std::process::id()
        ));
        let conn = db::open(&path).unwrap();
        let mut r = row("image", None);
        r.path = "/nonexistent/нет-такого.jpg".into();
        db::insert(&conn, &r).unwrap();
        let id: i64 = conn
            .query_row("SELECT id FROM files", [], |r| r.get(0))
            .unwrap();
        drop(conn);

        let progress = Arc::new(Progress::default());
        progress.cancel();
        let stats = run_files(&path, &[id], Some(progress)).unwrap();
        assert!(stats.cancelled);
        assert_eq!(stats.done, 0);
        assert_eq!(stats.errors, 0);
    }

    #[test]
    fn a_confident_document_tag_counts_without_a_saved_model_choice() {
        // M23.6: `settings.model_id` stays empty until the owner picks a model, so the dictionary is
        // found by its tag ids, not by the setting. Before the fix the tag branch of the heuristic
        // was silently dead on every default DB and a document shot with a camera dropped out.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "pc-ocr-default-model-{}-{stamp}.db",
            std::process::id()
        ));
        let conn = db::open(&path).unwrap();
        let def = |id: &str| db::TagDef {
            model: "clip-vit-b32-multilingual".into(),
            id: id.into(),
            label: id.into(),
            prompts: vec!["a photo".into()],
            enabled: true,
            position: 0,
            min_percentile: 0,
        };
        db::save_tag_def(&conn, &def("document")).unwrap();
        db::save_tag_def(&conn, &def("screenshot")).unwrap();
        let mut image = row("image", Some("Pixel"));
        image.path = "/document.jpg".into();
        db::insert(&conn, &image).unwrap();
        let id: i64 = conn
            .query_row("SELECT id FROM files", [], |r| r.get(0))
            .unwrap();
        db::replace_tags(
            &conn,
            id,
            "clip:clip-vit-b32-multilingual",
            &[("document".to_string(), 0.30), ("children".to_string(), 0.10)],
        )
        .unwrap();
        assert!(db::load_settings(&conn).model_id.is_empty());
        drop(conn);

        let plan = plan(&path, false).unwrap();
        assert_eq!(plan.images, 1);
        assert_eq!(
            plan.candidates, 1,
            "a confident document tag is a candidate even though no model was ever chosen"
        );
    }

    #[test]
    fn video_is_never_an_ocr_candidate() {
        assert!(!wants_text(&row("video", None), &HashMap::new(), SHOW, MARGIN, &labels()));
    }

    #[test]
    fn all_still_skips_videos() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "pc-ocr-all-{}-{stamp}.db",
            std::process::id()
        ));
        let conn = db::open(&path).unwrap();
        let mut image = row("image", Some("Pixel"));
        image.path = "/a.jpg".into();
        db::insert(&conn, &image).unwrap();
        let mut video = row("video", None);
        video.path = "/v.mp4".into();
        video.id = 2;
        db::insert(&conn, &video).unwrap();
        drop(conn);

        let plan = plan(&path, true).unwrap();
        assert_eq!(plan.images, 1);
        assert_eq!(plan.candidates, 1);
    }
}
