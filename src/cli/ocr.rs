//! `photocat ocr` — text from documents and screenshots.

use crate::db::{self, Progress};
use crate::i18n;
use crate::{flag_num, flag_str, has_flag, ocr, tools};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use super::language;
use super::report::{self, short_version};

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let lang = language(args)?;
    let jobs = flag_num(args, "--jobs", 4);
    let all = has_flag(args, "--all");
    if has_flag(args, "--dry-run") {
        let plan = ocr::plan(&db_path, all)?;
        let settings = {
            let conn = db::open(&db_path)?;
            db::load_settings(&conn)
        };
        let mut line = i18n::fill(
            lang,
            "candidates: {} of {} photos; tesseract ({}): {}",
            &[
                plan.candidates.to_string(),
                plan.images.to_string(),
                settings.tools.source(tools::Tool::Tesseract, lang).to_string(),
                settings
                    .tools
                    .probe(tools::Tool::Tesseract)
                    .map(|v| short_version(&v))
                    .unwrap_or_else(|| i18n::t(lang, "not found").into()),
            ],
        );
        if plan.pass_skipped > 0 {
            line.push_str(&i18n::fill(
                lang,
                " ({} skipped — their folders have text off)",
                &[plan.pass_skipped.to_string()],
            ));
        }
        println!("{line}");
        return Ok(());
    }
    let progress = Arc::new(Progress::default());
    report::progress(lang, "OCR: {}/{} files, {} errors — {}", true, progress.clone());
    if let Some(file) = flag_str(args, "--file") {
        let conn = db::open(&db_path)?;
        let path = std::fs::canonicalize(&file)
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or(file);
        let id: i64 = conn
            .query_row("SELECT id FROM files WHERE path=?1", [&path], |r| r.get(0))
            .map_err(|_| anyhow::anyhow!("the file is not in the index: {path}"))?;
        let stats = ocr::run_files(&db_path, &[id], Some(progress))?;
        println!("{stats}");
        return Ok(());
    }
    let stats = ocr::run(&db_path, jobs, all, Some(progress))?;
    println!("{stats}");
    Ok(())
}
