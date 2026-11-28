//! Arranging the archive by dates: plan → confirmation → apply (hardlink/copy/symlink).
//! The originals are neither deleted nor renamed: the showcase is new files or links next to
//! the archive, not over it. The names in the showcase come from a template (original by default).
//!
//! Split by step (M27.5): `plan.rs` builds the dry-run plan, `apply.rs` creates the links or
//! copies, `export.rs` copies an album; `cli` is the `organize` command that ties the first two
//! together.

mod apply;
mod export;
mod plan;

#[cfg(test)]
mod tests;

pub use apply::*;
pub use export::*;
pub use plan::*;

use crate::db;
use crate::i18n;
use crate::{flag_str, has_flag};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;

pub fn cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;
    let out = match flag_str(args, "--out") {
        Some(o) => PathBuf::from(o),
        None => {
            eprintln!(
                "{}",
                i18n::t(
                    lang,
                    "a destination folder is needed: organize --out DIR [--apply] [--by day|month|event]"
                )
            );
            std::process::exit(2);
        }
    };
    let mode = if has_flag(args, "--copy") {
        LinkMode::Copy
    } else if has_flag(args, "--symlink") {
        LinkMode::Symlink
    } else {
        LinkMode::Hard
    };
    let layout = match flag_str(args, "--by") {
        None => Layout::Day,
        Some(v) => match Layout::parse(&v) {
            Some(l) => l,
            None => {
                eprintln!(
                    "{}",
                    i18n::fill(
                        lang,
                        "--by: expected day, month or event, got “{}”",
                        &[v]
                    )
                );
                std::process::exit(2);
            }
        },
    };
    let name = match flag_str(args, "--name") {
        Some(tpl) => Some(tpl),
        None if has_flag(args, "--rename") => Some(RENAME_TEMPLATE.to_string()),
        None => None,
    };
    if let Some(tpl) = name.as_deref()
        && let Err(e) = validate_template(tpl)
    {
        eprintln!("{e}");
        std::process::exit(2);
    }
    let apply_it = has_flag(args, "--apply");

    let rows = db::load_all(&conn)?;
    let tags = if name.as_deref().is_some_and(|t| t.contains("{tag}")) {
        db::load_visible_tags(&conn)?
    } else {
        HashMap::new()
    };
    let mut opts = Options::new(&out);
    opts.layout = layout;
    opts.name = name;
    opts.exclude_dup = has_flag(args, "--exclude-dup");
    opts.tags = tags;
    opts.tag_show = settings.tag_show;
    opts.tag_margin = settings.tag_margin;
    let plan = build_plan(&rows, &opts);

    // Counted by the top folder of the showcase (a year or "no date"), not by the path depth:
    // events inside a day have one more folder for the place.
    let mut by_top: BTreeMap<String, usize> = BTreeMap::new();
    for it in &plan.items {
        let top = it
            .dst
            .strip_prefix(&out)
            .ok()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        *by_top.entry(top).or_insert(0) += 1;
    }
    println!(
        "{}",
        i18n::fill(
            lang,
            "showcase: {} → {}",
            &[layout.title(lang).to_string(), out.display().to_string()]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "files with a date: {}, without a date (not in the plan): {}, marked spare and left out: {}",
            &[
                plan.items.len().to_string(),
                plan.no_date.to_string(),
                plan.excluded.to_string(),
            ]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "showcase folders: {}",
            &[plan.dirs.to_string()]
        )
    );
    for (top, n) in &by_top {
        println!("  {top}: {n}");
    }
    println!("\n{}", i18n::t(lang, "examples:"));
    for it in plan.items.iter().take(5) {
        println!("  {}  →  {}", it.src.display(), it.dst.display());
    }
    if !apply_it {
        println!(
            "\n{}",
            i18n::fill(
                lang,
                "this is a plan (dry-run). Add --apply to create links/copies in {}",
                &[out.display().to_string()]
            )
        );
        return Ok(());
    }
    let (ok, fail) = apply(&plan.items, mode)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "created: {}, errors: {}",
            &[ok.to_string(), fail.to_string()]
        )
    );
    Ok(())
}
