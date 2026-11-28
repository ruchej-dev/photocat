//! `photocat doctor` — what is present: ffmpeg, tesseract, the languages, the models.

use crate::db;
use crate::i18n;
use crate::{derive, embed, faces, flag_str, tools};
use anyhow::Result;
use std::path::PathBuf;

use super::report::short_version;

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;

    println!(
        "{}",
        i18n::t(
            lang,
            "external programs (empty in the settings — looked up in PATH; without a program only its own work is off):"
        )
    );
    for tool in tools::ALL {
        let state = match settings.tools.version(tool) {
            Ok(v) => short_version(&v),
            Err(e) => i18n::fill(
                lang,
                "does not work: {}",
                &[format!("{e:#}")],
            ),
        };
        let found = match settings.tools.locate(tool) {
            Some(p) => format!(
                "{} ({})",
                p.display(),
                settings.tools.source(tool, lang)
            ),
            None => i18n::fill(
                lang,
                "not found: {} ({})",
                &[
                    settings.tools.program(tool).to_string(),
                    settings.tools.source(tool, lang).to_string(),
                ],
            ),
        };
        println!("  {:<10} {}", tool.name(), state);
        println!("  {:<10} {}", "", found);
        println!(
            "  {:<10} {}",
            "",
            i18n::fill(
                lang,
                "why: {}; without it: {}",
                &[
                    tool.purpose(lang).to_string(),
                    tool.without(lang).to_string(),
                ]
            )
        );
    }

    match derive::tesseract_langs(&settings.tools) {
        Some(langs) => {
            let want: Vec<&str> = settings
                .ocr_lang
                .split('+')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();
            let missing: Vec<&str> = want
                .iter()
                .copied()
                .filter(|l| !langs.iter().any(|x| x.eq_ignore_ascii_case(l)))
                .collect();
            if missing.is_empty() {
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "OCR languages: {} — present",
                        std::slice::from_ref(&settings.ocr_lang)
                    )
                );
            } else {
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "OCR languages: {} — missing {}; install the tesseract language packs for them",
                        &[settings.ocr_lang.clone(), missing.join(", ")]
                    )
                );
            }
            println!(
                "{}",
                i18n::fill(lang, "  available: {}", &[langs.join(", ")])
            );
        }
        None => println!(
            "{}",
            i18n::t(lang, "OCR languages: cannot check — tesseract not found")
        ),
    }

    let entry = embed::active_entry(&settings);
    println!(
        "{}",
        i18n::fill(
            lang,
            "CLIP model: {} · {} · {}",
            &[
                i18n::t(lang, entry.title).to_string(),
                db::human_in(lang, entry.bytes() as i64),
                if embed::model_present(&settings) {
                    i18n::t(lang, "in place").to_string()
                } else {
                    i18n::t(lang, "not downloaded — semantic search and tags are unavailable")
                        .to_string()
                },
            ]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "face model: {} · {} · {}",
            &[
                faces::MODEL_ID.to_string(),
                db::human_in(lang, faces::model_bytes(&settings)),
                if faces::model_present(&settings) {
                    i18n::t(lang, "in place").to_string()
                } else {
                    i18n::t(lang, "not downloaded — “Settings” or faces download --yes")
                        .to_string()
                },
            ]
        )
    );
    let (face_faces, face_photos, face_clusters) = db::face_summary(&conn)?;
    let (face_images, face_todo) = db::face_scan_plan(&conn)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "people: {} on {} photos, {} groups, {} of {} still without faces",
            &[
                face_faces.to_string(),
                face_photos.to_string(),
                face_clusters.to_string(),
                face_todo.to_string(),
                face_images.to_string(),
            ]
        )
    );
    let (ocr_files, ocr_chars) = db::ocr_summary(&conn)?;
    let total: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
    let size = std::fs::metadata(&db_path).map(|m| m.len() as i64).unwrap_or(0);
    println!(
        "{}",
        i18n::fill(
            lang,
            "DB: {} · {} · {} files, {} with text ({} characters)",
            &[
                db_path.display().to_string(),
                db::human_in(lang, size),
                total.to_string(),
                ocr_files.to_string(),
                ocr_chars.to_string(),
            ]
        )
    );
    Ok(())
}
