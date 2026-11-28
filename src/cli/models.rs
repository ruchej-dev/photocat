//! `photocat models` — the catalogue, the check and the download.

use crate::db::{self, Progress};
use crate::i18n;
use crate::{embed, flag_str, has_flag};
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
    match args.get(1).map(String::as_str).unwrap_or("list") {
        "list" => {
            let active = embed::active_entry(&settings);
            for e in embed::catalog() {
                let present = embed::model_present_in(&settings.models_dir, e);
                let fits = e.fits(settings.max_model_mb);
                let state = if present {
                    i18n::t(lang, "in place")
                } else if fits {
                    i18n::t(lang, "not downloaded")
                } else {
                    i18n::t(lang, "over the limit")
                };
                println!(
                    "{} {:<22} {} {} · {} · {} · {} · {}",
                    if std::ptr::eq(e, active) { "*" } else { " " },
                    e.id,
                    e.dim,
                    i18n::t(lang, "dims"),
                    db::human_in(lang, e.bytes() as i64),
                    e.license,
                    state,
                    i18n::t(lang, e.note)
                );
            }
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "model folder: {}",
                    &[settings.models_dir.display().to_string()]
                )
            );
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "limit: {} MiB",
                    &[settings.max_model_mb.to_string()]
                )
            );
        }
        "check" => {
            embed::check_model(&settings)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "the model is in order: {}",
                    &[i18n::t(lang, embed::active_entry(&settings).title).to_string()]
                )
            );
        }
        "download" => {
            let id = flag_str(args, "--id").unwrap_or_default();
            let consent = has_flag(args, "--yes");
            let progress = Arc::new(Progress::default());
            report::spinner(progress.clone());
            let mut report = embed::download_model(&settings, &id, consent, Some(progress))?;
            report.lang = lang;
            println!("{report}");
        }
        other => {
            eprintln!(
                "{}",
                i18n::fill(
                    lang,
                    "unknown models subcommand: {} (models list|check|download)",
                    &[other.to_string()]
                )
            );
            std::process::exit(2);
        }
    }
    Ok(())
}
