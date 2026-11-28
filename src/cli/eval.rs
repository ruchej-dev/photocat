//! `photocat eval` — the labelled check: a sample, `labels.csv`, the metrics.

use crate::db;
use crate::i18n;
use crate::{embed, eval, flag_num, flag_str};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;
    let entry = embed::active_entry(&settings);
    let source = embed::tag_source(entry);
    let defs = embed::ensure_tag_defs(&db_path, entry)?;
    if let Some(file) = flag_str(args, "--labels") {
        let labels = eval::read_labels(Path::new(&file))?;
        if labels.is_empty() {
            println!(
                "{}",
                i18n::fill(lang, "no labels yet: fill in {} and run again", &[file])
            );
            return Ok(());
        }
        let by_id = db::load_tags(&conn, &source)?;
        let paths: std::collections::HashMap<i64, String> = db::load_all(&conn)?
            .into_iter()
            .map(|r| (r.id, r.path))
            .collect();
        let mut by_path: std::collections::HashMap<String, Vec<(String, f32)>> =
            std::collections::HashMap::new();
        for (id, tags) in &by_id {
            if let Some(p) = paths.get(id) {
                by_path.insert(p.clone(), tags.clone());
            }
        }
        let dists = embed::tag_distributions(&by_id);
        let s = eval::score(
            &labels,
            &by_path,
            &defs,
            settings.tag_show,
            settings.tag_margin,
            &dists,
            &[100, 70],
        )?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "rows in the labels: {} · labelled {} · “other” {} · skipped {} · not labelled {} · missing from the index {}",
                &[
                    s.rows.to_string(),
                    s.labelled.to_string(),
                    s.other.to_string(),
                    s.skipped.to_string(),
                    s.unlabelled.to_string(),
                    s.missing.to_string(),
                ]
            )
        );
        println!(
            "{}",
            i18n::fill(
                lang,
                "a dictionary label fits {} % of the shots; {} of them allow a second label",
                &[
                    format!(
                        "{:.1}",
                        if s.labelled + s.other == 0 {
                            0.0
                        } else {
                            s.labelled as f32 / (s.labelled + s.other) as f32 * 100.0
                        }
                    ),
                    s.multi.to_string(),
                ]
            )
        );
        println!(
            "{}",
            i18n::fill(
                lang,
                "top-1: {} of {} = {} % · top-3: {} of {} = {} %",
                &[
                    s.top1.to_string(),
                    s.labelled.to_string(),
                    format!("{:.1}", s.top1_share() * 100.0),
                    s.top3.to_string(),
                    s.labelled.to_string(),
                    format!("{:.1}", s.top3_share() * 100.0),
                ]
            )
        );
        println!(
            "{}",
            i18n::fill(
                lang,
                "confident tag: {} shots, accuracy {} %; confidently wrong (“other”) {}",
                &[
                    (s.confident_ok + s.confident_other).to_string(),
                    format!("{:.1}", s.confident_precision() * 100.0),
                    s.confident_other.to_string(),
                ]
            )
        );
        for t in s.per_tag.iter().filter(|t| t.n > 0) {
            println!(
                "  {:<20} {:<3} · top-1 {:>3} · top-3 {:>3}",
                t.label, t.n, t.top1, t.top3
            );
        }
        for f in &s.furniture {
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "furniture: at {} % found {} of {} (threshold {})",
                    &[
                        f.pct.to_string(),
                        f.found.to_string(),
                        f.n.to_string(),
                        format!("{:.3}", f.threshold),
                    ]
                )
            );
        }
        println!(
            "{}",
            i18n::fill(
                lang,
                "check {} · model {}",
                &[
                    chrono::Local::now().format("%Y-%m-%d").to_string(),
                    entry.id.to_string(),
                ]
            )
        );
        return Ok(());
    }
    let dir = match flag_str(args, "--folder").or_else(|| args.get(1).cloned()) {
        Some(p) if !p.starts_with('-') => PathBuf::from(p),
        _ => {
            eprintln!(
                "{}",
                i18n::t(lang, "a folder is needed: photocat eval --folder DIR --out DIR")
            );
            std::process::exit(2);
        }
    };
    // Into the cache by default, not the repository: labels.csv holds family archive paths.
    let out = flag_str(args, "--out")
        .map(PathBuf::from)
        .unwrap_or_else(|| db::cache_dir().join("eval"));
    let n = flag_num(args, "--sample", eval::SAMPLE_DEFAULT);
    let cand = eval::candidates(&conn, &dir, &source)?;
    if cand.is_empty() {
        return Err(anyhow::anyhow!(
            "{}",
            i18n::fill(
                lang,
                "no shots with tags in {}: index the folder and compute the tags",
                &[dir.display().to_string()]
            )
        ));
    }
    let picked = eval::pick(&cand, n);
    let tags = db::load_tags(&conn, &source)?;
    let mut rows = Vec::new();
    for r in &picked {
        let list = tags.get(&r.id).cloned().unwrap_or_default();
        rows.push((r.clone(), list));
    }
    let thumbs = db::cache_dir().join("thumbs");
    let report = eval::export(&out, &rows, &thumbs)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "sample: {} shots of {} eligible in {} · labels: {} · guesses: {} · sheets: {}",
            &[
                report.rows.to_string(),
                cand.len().to_string(),
                dir.display().to_string(),
                report.labels.display().to_string(),
                report.guesses.display().to_string(),
                report.sheets.len().to_string(),
            ]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "fill the label column in {} (other / skip / labels joined by |) and run again with --labels",
            &[report.labels.display().to_string()]
        )
    );
    println!(
        "{}",
        i18n::fill(lang, "model {} · {}", &[entry.id.to_string(), source])
    );
    Ok(())
}
