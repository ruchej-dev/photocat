//! `photocat clusters` — the k-means report.

use crate::db;
use crate::i18n;
use crate::{embed, flag_num, flag_str};
use anyhow::Result;
use std::path::PathBuf;

use super::language;

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let lang = language(args)?;
    let k = flag_num(args, "--k", 30);
    let out = flag_str(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("plan/M3-clusters.md"));
    let stats = embed::write_clusters(&db_path, &out, k)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "clusters: {} over {} photos, {} s → {}",
            &[
                stats.clusters.to_string(),
                stats.photos.to_string(),
                format!("{:.1}", stats.seconds),
                stats.out.display().to_string(),
            ]
        )
    );
    Ok(())
}
