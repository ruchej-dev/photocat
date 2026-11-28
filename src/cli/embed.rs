//! `photocat embed` — CLIP vectors and the tag weights.

use crate::db::{self, Progress};
use crate::{embed, flag_num, flag_str};
use anyhow::Result;
use std::path::PathBuf;
use std::sync::Arc;

use super::{language, report};

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let lang = language(args)?;
    let jobs = flag_num(args, "--jobs", 8);
    let batch = flag_num(args, "--batch", 8);
    if args.iter().any(|a| a == "--retag") {
        let stats = embed::retag(&db_path, jobs, Some(Arc::new(Progress::default())))?;
        println!("{stats}");
        return Ok(());
    }
    let progress = Arc::new(Progress::default());
    report::progress(lang, "CLIP: {}/{} files, {} errors — {}", true, progress.clone());
    let refresh = args.iter().any(|a| a == "--refresh");
    let stats = embed::embed_missing(&db_path, jobs, batch, Some(progress), refresh)?;
    println!("{stats}");
    Ok(())
}
