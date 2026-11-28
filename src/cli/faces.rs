//! `photocat faces` — detection, groups, names and the quarantine.

use crate::db::{self, Progress};
use crate::i18n;
use crate::{faces, flag_num, flag_str, has_flag};
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
    match args.get(1).map(String::as_str).unwrap_or("") {
        "download" => {
            let progress = Arc::new(Progress::default());
            report::spinner(progress.clone());
            let report =
                faces::download_model(&settings, has_flag(args, "--yes"), Some(progress))?;
            println!("{report}");
            return Ok(());
        }
        "label" => {
            let id = flag_str(args, "--id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let label = flag_str(args, "--name").unwrap_or_default();
            if id == 0 || label.trim().is_empty() {
                eprintln!(
                    "{}",
                    i18n::t(lang, "a group number and a name are needed: faces label --id N --name NAME")
                );
                std::process::exit(2);
            }
            let matched = faces::label(&db_path, id, &label)?;
            println!(
                "{}",
                i18n::fill(lang, "group {}: “{}”", &[id.to_string(), label.trim().to_string()])
            );
            // Naming a person matches the free faces at once (M26.11).
            if matched.worth_telling() {
                println!("{matched}");
            }
            return Ok(());
        }
        "confirm" | "reject" => {
            let confirm = args.get(1).map(String::as_str) == Some("confirm");
            let ids: Vec<i64> = if has_flag(args, "--all") {
                let conn = db::open(&db_path)?;
                db::load_face_proposals(&conn)?
                    .iter()
                    .map(|p| p.face.id)
                    .collect()
            } else {
                let id = flag_str(args, "--id")
                    .and_then(|v| v.parse::<i64>().ok())
                    .unwrap_or(0);
                if id == 0 {
                    eprintln!(
                        "{}",
                        i18n::t(
                            lang,
                            "a face number is needed: faces confirm --id N"
                        )
                    );
                    std::process::exit(2);
                }
                vec![id]
            };
            if ids.is_empty() {
                println!("{}", i18n::t(lang, "nothing waits in quarantine"));
                return Ok(());
            }
            // The owner's answer to the quarantine (M26.17): rows only — the photos, the
            // crops and the groups of other faces stay as they are.
            let n = if confirm {
                faces::confirm_matches(&db_path, &ids)?
            } else {
                faces::reject_matches(&db_path, &ids)?
            };
            println!("{}", faces::answer_report(lang, n, confirm));
            return Ok(());
        }
        "split" => {
            let id = flag_str(args, "--id")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            if id == 0 {
                eprintln!(
                    "{}",
                    i18n::t(lang, "a face number is needed: faces split --id N")
                );
                std::process::exit(2);
            }
            let cid = faces::split(&db_path, id)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "face {} split into group {}",
                    &[id.to_string(), cid.to_string()]
                )
            );
            return Ok(());
        }
        _ => {}
    }
    let threshold = flag_str(args, "--threshold")
        .and_then(|v| v.parse().ok())
        .unwrap_or(settings.face_threshold);
    if has_flag(args, "--quarantine") {
        let conn = db::open(&db_path)?;
        let proposals = db::load_face_proposals(&conn)?;
        if proposals.is_empty() {
            println!("{}", i18n::t(lang, "nothing waits in quarantine"));
        }
        for p in proposals {
            println!(
                "{} → {} {:.2}  {}",
                p.face.id, p.target_label, p.similarity, p.face.path
            );
        }
        return Ok(());
    }
    if has_flag(args, "--dry-run") {
        let plan = faces::plan(&db_path)?;
        let model = if faces::model_present(&settings) {
            i18n::t(lang, "in place")
        } else {
            i18n::t(lang, "not downloaded — “Settings” or faces download --yes")
        };
        let mut line = i18n::fill(
            lang,
            "photos: {}, still without faces: {}; face model: {}",
            &[
                plan.images.to_string(),
                plan.todo.to_string(),
                model.to_string(),
            ],
        );
        if plan.pass_skipped > 0 {
            line.push_str(&i18n::fill(
                lang,
                " ({} skipped — their folders have faces off)",
                &[plan.pass_skipped.to_string()],
            ));
        }
        println!("{line}");
        // How many faces the matching step would move to a named person, nothing written
        // (M26.11).
        println!("{}", faces::plan_named(&db_path, &settings)?);
        return Ok(());
    }
    if has_flag(args, "--list") {
        let conn = db::open(&db_path)?;
        let clusters = db::load_face_clusters(&conn)?;
        if clusters.is_empty() {
            println!(
                "{}",
                i18n::t(lang, "no face groups yet: run photocat faces")
            );
        }
        for c in clusters {
            println!(
                "{} {:<24} {}",
                c.id,
                if c.label.is_empty() {
                    i18n::t(lang, "(no name)")
                } else {
                    c.label.as_str()
                },
                i18n::fill(
                    lang,
                    "{} faces, {} photos",
                    &[c.faces.to_string(), c.photos.to_string()]
                )
            );
        }
        return Ok(());
    }
    if has_flag(args, "--recluster") {
        let stats = faces::recluster(&db_path, threshold)?;
        println!("{stats}");
        return Ok(());
    }
    let jobs = flag_num(args, "--jobs", settings.index_jobs as u64);
    let progress = Arc::new(Progress::default());
    report::progress(lang, "people: {}/{} photos, {} errors — {}", true, progress.clone());
    let stats = faces::run(&db_path, jobs, threshold, Some(progress))?;
    println!("{stats}");
    Ok(())
}
