//! `photocat index` — one folder into the cache.

use crate::db::{self, Progress};
use crate::i18n;
use crate::{flag_num, flag_str, index};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use super::report;

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let settings = {
        let conn = db::open(&db_path)?;
        db::ensure_lang(&conn)?;
        db::load_settings(&conn)
    };
    let lang = settings.lang;
    let root = match args.get(1) {
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => {
            eprintln!(
                "{}",
                i18n::t(lang, "a folder is needed: photocat index <folder>")
            );
            std::process::exit(2);
        }
    };
    let jobs = flag_num(args, "--jobs", settings.index_jobs as u64);
    // A switched-off folder is skipped (M26.1): no new rows are written, and switching it
    // back on needs no pass because the rows are still in the index.
    let full = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
    if settings.root_off(&full.to_string_lossy()) {
        println!(
            "{}",
            i18n::fill(
                lang,
                "{} is switched off: its files are hidden and indexing skipped it — switch it on in the folder list",
                &[root.display().to_string()]
            )
        );
        return Ok(());
    }
    let progress = Arc::new(Progress::default());
    report::progress(lang, "indexing: {}/{} files, {} errors", false, progress.clone());
    let mut stats = index::run(
        &root,
        jobs,
        &db_path,
        &settings.tools,
        settings.lang,
        Some(progress),
    )?;
    stats.lang = lang;
    println!("{stats}");
    Ok(())
}
