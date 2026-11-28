//! `photocat settings` — the model, the paths, the thresholds and the language.

use crate::db;
use crate::i18n::{self, Lang};
use crate::{embed, flag_str, tools};
use anyhow::Result;
use std::path::PathBuf;

use super::report::short_version;

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    // The first launch: the interface language comes from the system (M21, ADR 0040).
    db::ensure_lang(&conn)?;
    let mut settings = db::load_settings(&conn);
    let mut lang = settings.lang;
    let mut changed = false;
    if let Some(dir) = flag_str(args, "--models-dir") {
        settings.models_dir = PathBuf::from(dir);
        changed = true;
    }
    if let Some(id) = flag_str(args, "--model") {
        if embed::entry(&id).is_none() {
            eprintln!(
                "{}",
                i18n::fill(
                    lang,
                    "no such model: {} (see models list)",
                    std::slice::from_ref(&id)
                )
            );
            std::process::exit(2);
        }
        settings.model_id = id;
        changed = true;
    }
    if let Some(code) = flag_str(args, "--ocr-lang") {
        settings.ocr_lang = code;
        changed = true;
    }
    // The interface language (M21, ADR 0040): only the listed codes, otherwise an error,
    // so a typo does not silently turn into English.
    if let Some(code) = flag_str(args, "--lang") {
        match code.trim().to_ascii_lowercase().as_str() {
            "en" | "ru" => {
                settings.lang = Lang::parse(&code);
                // The change applies at once: the report is printed in the new language already.
                lang = settings.lang;
                changed = true;
            }
            other => {
                eprintln!(
                    "{}",
                    i18n::fill(
                        lang,
                        "no such interface language: {} (en, ru)",
                        &[other.to_string()]
                    )
                );
                std::process::exit(2);
            }
        }
    }
    if let Some(n) = flag_str(args, "--max-model-mb") {
        settings.max_model_mb = n.parse().unwrap_or(settings.max_model_mb);
        changed = true;
    }
    if let Some(v) = flag_str(args, "--face-threshold")
        && let Ok(x) = v.parse::<f32>()
    {
        settings.face_threshold = x.clamp(0.0, 1.0);
        changed = true;
    }
    for (flag, tool) in [
        ("--ffmpeg", tools::Tool::Ffmpeg),
        ("--ffprobe", tools::Tool::Ffprobe),
        ("--tesseract", tools::Tool::Tesseract),
    ] {
        let Some(path) = flag_str(args, flag) else {
            continue;
        };
        let mut probe = settings.tools.clone();
        *probe.get_mut(tool) = path.trim().to_string();
        // The path is checked before saving: an error becomes a message, not a crash.
        match probe.version(tool) {
            Ok(v) => {
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "{}: {}",
                        &[tool.name().to_string(), short_version(&v)]
                    )
                );
                settings.tools = probe;
                changed = true;
            }
            Err(e) => {
                eprintln!(
                    "{}",
                    i18n::fill(lang, "not saved: {}", &[format!("{e:#}")])
                );
                std::process::exit(2);
            }
        }
    }
    if changed {
        db::save_settings(&conn, &settings)?;
        println!("{}", i18n::t(lang, "settings saved"));
    }
    println!(
        "{}",
        i18n::fill(
            lang,
            "model: {}",
            &[embed::active_entry(&settings).id.to_string()]
        )
    );
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
        i18n::fill(lang, "limit: {} MiB", &[settings.max_model_mb.to_string()])
    );
    println!(
        "{}",
        i18n::fill(lang, "threads: {}", &[settings.index_jobs.to_string()])
    );
    println!(
        "{}",
        i18n::fill(lang, "duplicate threshold: {}", &[settings.max_dist.to_string()])
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "tag: confident ≥ {}, margin {}",
            &[
                format!("{:.2}", settings.tag_show),
                format!("{:.2}", settings.tag_margin)
            ]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "tag filter: threshold {}% (percentile)",
            &[settings.tag_show_pct.to_string()]
        )
    );
    println!(
        "{}",
        i18n::fill(lang, "OCR languages: {}", std::slice::from_ref(&settings.ocr_lang))
    );
    println!(
        "{}",
        i18n::fill(lang, "interface language: {}", &[settings.lang.code().to_string()])
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "face threshold: {}",
            &[format!("{:.2}", settings.face_threshold)]
        )
    );
    for tool in tools::ALL {
        println!(
            "{:<10} {} ({})",
            tool.name(),
            settings
                .tools
                .probe(tool)
                .map(|v| short_version(&v))
                .unwrap_or_else(|| i18n::t(lang, "not found").to_string()),
            settings.tools.source(tool, lang)
        );
    }
    Ok(())
}
