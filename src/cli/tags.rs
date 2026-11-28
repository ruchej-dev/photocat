//! `photocat tags` — the model tag dictionary: list, add, enable, remove, import, export.

use crate::db;
use crate::embed;
use crate::i18n;
use crate::{flag_str, has_flag};
use anyhow::{Context, Result};
use std::path::PathBuf;

/// The tag dictionary: show, add, enable or disable, remove, JSON import and export.
/// After the edits `embed --retag` recomputes the tag weights (without running CLIP over the photos again).
pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let settings = {
        let conn = db::open(&db_path)?;
        db::ensure_lang(&conn)?;
        db::load_settings(&conn)
    };
    let lang = settings.lang;
    let entry = embed::active_entry(&settings);
    let mut defs = embed::ensure_tag_defs(&db_path, entry)?;
    let source = embed::tag_source(entry);
    let list = || -> Result<()> {
        let conn = db::open(&db_path)?;
        let counts = db::tag_file_counts(&conn, &source)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "model dictionary {} — {} tags",
                &[entry.id.to_string(), defs.len().to_string()]
            )
        );
        for d in &defs {
            let threshold = if d.min_percentile > 0 {
                i18n::fill(
                    lang,
                    ", threshold {}%",
                    &[d.min_percentile.to_string()],
                )
            } else {
                String::new()
            };
            println!(
                "  {:<10} {:<16} {}{} · {}",
                d.id,
                d.label,
                i18n::t(lang, if d.enabled { "ok" } else { "off" }),
                threshold,
                i18n::fill(
                    lang,
                    "{} files · {} prompts",
                    &[
                        counts.get(&d.label).copied().unwrap_or(0).to_string(),
                        d.prompts.len().to_string(),
                    ]
                )
            );
            for p in &d.prompts {
                println!("             · {p}");
            }
        }
        Ok(())
    };

    if let Some(label) = flag_str(args, "--add") {
        let label = label.trim().to_string();
        if label.is_empty() {
            anyhow::bail!("{}", i18n::t(lang, "--add: an empty label is not allowed"));
        }
        if defs.iter().any(|d| d.label == label) {
            anyhow::bail!(
                "{}",
                i18n::fill(lang, "the tag “{}” is already in the dictionary", std::slice::from_ref(&label))
            );
        }
        let mut prompts: Vec<String> = Vec::new();
        let mut it = args.iter().peekable();
        while let Some(a) = it.next() {
            if a == "--prompt" {
                if let Some(p) = it.next() {
                    prompts.push(p.clone());
                }
            } else if let Some(rest) = a.strip_prefix("--prompt=") {
                prompts.push(rest.to_string());
            }
        }
        if prompts.is_empty() {
            anyhow::bail!(
                "{}",
                i18n::t(lang, "--add: at least one --prompt “a photo of …” is needed")
            );
        }
        let n = defs
            .iter()
            .filter_map(|d| d.id.strip_prefix("custom-")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let d = db::TagDef {
            model: entry.id.to_string(),
            id: format!("custom-{n}"),
            label,
            prompts,
            enabled: true,
            position: defs.iter().map(|d| d.position).max().unwrap_or(0) + 1,
            min_percentile: 0,
        };
        let conn = db::open(&db_path)?;
        db::save_tag_def(&conn, &d)?;
        defs.push(d.clone());
        println!(
            "{}",
            i18n::fill(
                lang,
                "tag “{}” added (id {}) — recompute the weights: photocat embed --retag",
                &[d.label.clone(), d.id.clone()]
            )
        );
        return Ok(());
    }

    if let Some(which) = flag_str(args, "--enable").or_else(|| flag_str(args, "--disable")) {
        let enable = has_flag(args, "--enable");
        let want = which.trim().to_string();
        let conn = db::open(&db_path)?;
        let d = defs
            .iter_mut()
            .find(|d| d.id == want || d.label == want)
            .with_context(|| format!("there is no tag “{want}”"))?;
        d.enabled = enable;
        db::save_tag_def(&conn, d)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "tag “{}” {}",
                &[
                    d.label.clone(),
                    i18n::t(lang, if enable { "enabled" } else { "disabled" }).to_string(),
                ]
            )
        );
        return Ok(());
    }

    if let Some(which) = flag_str(args, "--rm") {
        let want = which.trim().to_string();
        let conn = db::open(&db_path)?;
        let d = defs
            .iter()
            .find(|d| d.id == want || d.label == want)
            .with_context(|| format!("there is no tag “{want}”"))?;
        db::delete_tag_def(&conn, entry.id, &d.id)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "tag “{}” removed from the dictionary — the weights go away after photocat embed --retag",
                std::slice::from_ref(&d.label)
            )
        );
        return Ok(());
    }

    if let Some(path) = flag_str(args, "--export") {
        std::fs::write(&path, embed::tag_defs_to_json(&defs))
            .with_context(|| format!("cannot write {path}"))?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "dictionary exported: {} ({} tags)",
                &[path, defs.len().to_string()]
            )
        );
        return Ok(());
    }

    if let Some(path) = flag_str(args, "--import") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("cannot read {path}"))?;
        let defs = embed::tag_defs_from_json(entry.id, &text)?;
        embed::import_defs(&db_path, entry.id, &defs)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "dictionary loaded: {} tags — recompute the weights: photocat embed --retag",
                &[defs.len().to_string()]
            )
        );
        return Ok(());
    }

    if has_flag(args, "--reset") {
        let defs = embed::reset_to_defaults(&db_path, entry)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "dictionary restored to the default ({} tags) — photocat embed --retag",
                &[defs.len().to_string()]
            )
        );
        return Ok(());
    }

    list()
}
