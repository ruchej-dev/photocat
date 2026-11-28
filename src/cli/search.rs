//! `photocat search` — semantic search over the vectors, or over recognized text.

use crate::db;
use crate::i18n;
use crate::{embed, flag_num, flag_str, has_flag};
use anyhow::Result;
use std::path::PathBuf;

use super::language;

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let lang = language(args)?;
    let query = match args.get(1) {
        Some(q) if !q.starts_with('-') => q.clone(),
        _ => {
            eprintln!(
                "{}",
                i18n::t(lang, "a query is needed: photocat search \"cake\"")
            );
            std::process::exit(2);
        }
    };
    let top = flag_num(args, "--top", 30);
    if has_flag(args, "--text") {
        // Search over recognized text — no model and no network.
        let conn = db::open(&db_path)?;
        let t0 = std::time::Instant::now();
        let hits = db::search_ocr(&conn, &query, top)?;
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        println!(
            "{}",
            i18n::fill(
                lang,
                "text on photos, query “{}”: {} files in {} ms",
                &[query.clone(), hits.len().to_string(), format!("{ms:.0}")]
            )
        );
        let rows = db::load_all(&conn)?;
        let by_id: std::collections::HashMap<i64, &str> =
            rows.iter().map(|r| (r.id, r.path.as_str())).collect();
        for (id, snip) in hits {
            if let Some(path) = by_id.get(&id) {
                println!("  {path}\n    {snip}");
            }
        }
        return Ok(());
    }
    let repeat = flag_num(args, "--repeat", 1).max(1);
    eprintln!("{}", i18n::t(lang, "loading the CLIP model…"));
    let settings = {
        let conn = db::open(&db_path)?;
        db::load_settings(&conn)
    };
    let mut model = embed::Model::load(&settings, flag_num(args, "--jobs", 8))?;
    let mut first = Vec::new();
    let mut total = 0f32;
    for n in 0..repeat {
        let t0 = std::time::Instant::now();
        let hits = embed::search(&db_path, &mut model, &query, top)?;
        let ms = t0.elapsed().as_secs_f32() * 1000.0;
        total += ms;
        if n == 0 {
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "query “{}”: {} results in {} ms",
                    &[query.clone(), hits.len().to_string(), format!("{ms:.0}")]
                )
            );
            first = hits;
        } else {
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "repeat {}: {} ms",
                    &[n.to_string(), format!("{ms:.0}")]
                )
            );
        }
    }
    if repeat > 1 {
        println!(
            "{}",
            i18n::fill(
                lang,
                "on average: {} ms",
                &[format!("{:.0}", total / repeat as f32)]
            )
        );
    }
    let conn = db::open(&db_path)?;
    let rows = db::load_all(&conn)?;
    let by_id: std::collections::HashMap<i64, &str> =
        rows.iter().map(|r| (r.id, r.path.as_str())).collect();
    for (id, score) in first {
        if let Some(path) = by_id.get(&id) {
            println!("  {score:.3}  {path}");
        }
    }
    Ok(())
}
