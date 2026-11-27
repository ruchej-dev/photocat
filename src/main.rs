//! photocat — a media archive cataloguer: indexing, duplicates, arranging by dates, ML tags.
//!
//! Subcommands:
//!   photocat index <folder> [--jobs N] [--db PATH]
//!   photocat stats
//!   photocat dupes [--max-dist 6] [--csv FILE] [--plan FILE]
//!   photocat organize --out <folder> [--by day|month|event] [--name TEMPLATE]
//!                     [--exclude-dup] [--apply] [--copy|--symlink] [--rename]
//!   photocat verify <a> <b> [--jobs N] [--all]  |  verify --manifest FILE <dir>
//!   photocat manifest [folder] [--out FILE] [--jobs N]
//!   photocat embed [--jobs N] [--batch N] [--refresh] [--retag]
//!   photocat tags [--list] [--add LABEL] [--prompt TEXT] [--export FILE] [--import FILE]
//!   photocat ocr [--jobs N] [--all]
//!   photocat search <query> [--top N] [--text]
//!   photocat clusters [--k N] [--out FILE]
//!   photocat eval --folder DIR --out DIR [--sample N]   tag labelling on a sample
//!   photocat eval --labels FILE                         metrics over an existing labelling
//!   photocat albums list|new|rename|move|add|remove|delete|show
//!   photocat export --album N --out DIR [--name TEMPLATE] [--apply]
//!   photocat            (no arguments — the GUI)

mod app;
mod db;
mod derive;
mod dupes;
mod embed;
mod eval;
mod faces;
mod i18n;
mod index;
mod ocr;
mod organize;
mod player;
mod recycle;
mod storage;
mod tools;
mod verify;

use anyhow::{Context, Result};
use db::Progress;
use i18n::Lang;
use std::path::{Path, PathBuf};
use std::sync::Arc;

fn usage(lang: Lang) {
    println!("{}", i18n::t(lang, i18n::USAGE.0));
}

/// The language of the report: `settings.lang` from the DB (M21.6). The first launch takes
/// the language from the locale, like the GUI does, so `photocat stats` and the window agree.
fn language(args: &[String]) -> Result<Lang> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    Ok(db::load_settings(&conn).lang)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("gui");
    match cmd {
        "index" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let settings = {
                let conn = db::open(&db_path)?;
                db::ensure_lang(&conn)?;
                db::load_settings(&conn)
            };
            let lang = settings.lang;
            let root = match args.get(1) {
                Some(p) if !p.starts_with('-') => PathBuf::from(p),
                _ => {
                    eprintln!(
                        "{}",
                        i18n::t(lang, "a folder is needed: photocat index <folder>")
                    );
                    std::process::exit(2);
                }
            };
            let jobs = flag_num(&args, "--jobs", settings.index_jobs as u64);
            // A switched-off folder is skipped (M26.1): no new rows are written, and switching it
            // back on needs no pass because the rows are still in the index.
            let full = std::fs::canonicalize(&root).unwrap_or_else(|_| root.clone());
            if settings.root_off(&full.to_string_lossy()) {
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "{} is switched off: its files are hidden and indexing skipped it — switch it on in the folder list",
                        &[root.display().to_string()]
                    )
                );
                return Ok(());
            }
            let progress = Arc::new(Progress::default());
            let show = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                loop {
                    let (d, t, e) = (show.done(), show.total(), show.errors());
                    if t > 0 {
                        let line = i18n::fill(
                            lang,
                            "indexing: {}/{} files, {} errors",
                            &[d.to_string(), t.to_string(), e.to_string()],
                        );
                        eprint!("\r{line}");
                    }
                    if show.finished() {
                        eprintln!();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let mut stats = index::run(
                &root,
                jobs,
                &db_path,
                &settings.tools,
                settings.lang,
                Some(progress),
            )?;
            stats.lang = lang;
            println!("{stats}");
            Ok(())
        }
        "stats" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            db::print_stats(language(&args)?, &db_path)
        }
        "dupes" => dupes::cli(&args[1..]),
        "organize" => organize::cli(&args[1..]),
        "verify" => verify::cli(&args[1..], language(&args)?),
        "manifest" => verify::manifest_cli(&args[1..], language(&args)?),
        "play" => player::cli(&args[1..]),
        "embed" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let lang = language(&args)?;
            let jobs = flag_num(&args, "--jobs", 8);
            let batch = flag_num(&args, "--batch", 8);
            if args.iter().any(|a| a == "--retag") {
                let stats = embed::retag(&db_path, jobs, Some(Arc::new(Progress::default())))?;
                println!("{stats}");
                return Ok(());
            }
            let progress = Arc::new(Progress::default());
            let show = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                loop {
                    let (d, t, e) = (show.done(), show.total(), show.errors());
                    if t > 0 {
                        let line = i18n::fill(
                            lang,
                            "CLIP: {}/{} files, {} errors — {}",
                            &[d.to_string(), t.to_string(), e.to_string(), show.msg()],
                        );
                        eprint!("\r{line}");
                    } else {
                        eprint!("\r{}", show.msg());
                    }
                    if show.finished() {
                        eprintln!();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let refresh = args.iter().any(|a| a == "--refresh");
            let stats = embed::embed_missing(&db_path, jobs, batch, Some(progress), refresh)?;
            println!("{stats}");
            Ok(())
        }
        "faces" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let settings = {
                let conn = db::open(&db_path)?;
                db::ensure_lang(&conn)?;
                db::load_settings(&conn)
            };
            let lang = settings.lang;
            match args.get(1).map(String::as_str).unwrap_or("") {
                "download" => {
                    let progress = Arc::new(Progress::default());
                    let show = progress.clone();
                    std::thread::spawn(move || {
                        while !show.finished() {
                            eprintln!("\r{}", show.msg());
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                    });
                    let report =
                        faces::download_model(&settings, has_flag(&args, "--yes"), Some(progress))?;
                    println!("{report}");
                    return Ok(());
                }
                "label" => {
                    let id = flag_str(&args, "--id")
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0);
                    let label = flag_str(&args, "--name").unwrap_or_default();
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
                "split" => {
                    let id = flag_str(&args, "--id")
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
            let threshold = flag_str(&args, "--threshold")
                .and_then(|v| v.parse().ok())
                .unwrap_or(settings.face_threshold);
            if has_flag(&args, "--dry-run") {
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
            if has_flag(&args, "--list") {
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
            if has_flag(&args, "--recluster") {
                let stats = faces::recluster(&db_path, threshold)?;
                println!("{stats}");
                return Ok(());
            }
            let jobs = flag_num(&args, "--jobs", settings.index_jobs as u64);
            let progress = Arc::new(Progress::default());
            let show = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                loop {
                    let (d, t, e) = (show.done(), show.total(), show.errors());
                    if t > 0 {
                        let line = i18n::fill(
                            lang,
                            "people: {}/{} photos, {} errors — {}",
                            &[d.to_string(), t.to_string(), e.to_string(), show.msg()],
                        );
                        eprint!("\r{line}");
                    } else {
                        eprint!("\r{}", show.msg());
                    }
                    if show.finished() {
                        eprintln!();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(300));
                }
            });
            let stats = faces::run(&db_path, jobs, threshold, Some(progress))?;
            println!("{stats}");
            Ok(())
        }
        "ocr" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let lang = language(&args)?;
            let jobs = flag_num(&args, "--jobs", 4);
            let all = has_flag(&args, "--all");
            if has_flag(&args, "--dry-run") {
                let plan = ocr::plan(&db_path, all)?;
                let settings = {
                    let conn = db::open(&db_path)?;
                    db::load_settings(&conn)
                };
                let mut line = i18n::fill(
                    lang,
                    "candidates: {} of {} photos; tesseract ({}): {}",
                    &[
                        plan.candidates.to_string(),
                        plan.images.to_string(),
                        settings.tools.source(tools::Tool::Tesseract, lang).to_string(),
                        settings
                            .tools
                            .probe(tools::Tool::Tesseract)
                            .map(|v| short_version(&v))
                            .unwrap_or_else(|| i18n::t(lang, "not found").into()),
                    ],
                );
                if plan.pass_skipped > 0 {
                    line.push_str(&i18n::fill(
                        lang,
                        " ({} skipped — their folders have text off)",
                        &[plan.pass_skipped.to_string()],
                    ));
                }
                println!("{line}");
                return Ok(());
            }
            let progress = Arc::new(Progress::default());
            let show = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                loop {
                    let (d, t, e) = (show.done(), show.total(), show.errors());
                    if t > 0 {
                        let line = i18n::fill(
                            lang,
                            "OCR: {}/{} files, {} errors — {}",
                            &[d.to_string(), t.to_string(), e.to_string(), show.msg()],
                        );
                        eprint!("\r{line}");
                    } else {
                        eprint!("\r{}", show.msg());
                    }
                    if show.finished() {
                        eprintln!();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            if let Some(file) = flag_str(&args, "--file") {
                let conn = db::open(&db_path)?;
                let path = std::fs::canonicalize(&file)
                    .map(|p| p.to_string_lossy().to_string())
                    .unwrap_or(file);
                let id: i64 = conn
                    .query_row("SELECT id FROM files WHERE path=?1", [&path], |r| r.get(0))
                    .map_err(|_| anyhow::anyhow!("the file is not in the index: {path}"))?;
                let stats = ocr::run_files(&db_path, &[id], Some(progress))?;
                println!("{stats}");
                return Ok(());
            }
            let stats = ocr::run(&db_path, jobs, all, Some(progress))?;
            println!("{stats}");
            Ok(())
        }
        "search" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let lang = language(&args)?;
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
            let top = flag_num(&args, "--top", 30);
            if has_flag(&args, "--text") {
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
            let repeat = flag_num(&args, "--repeat", 1).max(1);
            eprintln!("{}", i18n::t(lang, "loading the CLIP model…"));
            let settings = {
                let conn = db::open(&db_path)?;
                db::load_settings(&conn)
            };
            let mut model = embed::Model::load(&settings, flag_num(&args, "--jobs", 8))?;
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
        "models" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
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
                    let id = flag_str(&args, "--id").unwrap_or_default();
                    let consent = has_flag(&args, "--yes");
                    let progress = Arc::new(Progress::default());
                    let show = progress.clone();
                    std::thread::spawn(move || {
                        while !show.finished() {
                            eprintln!("\r{}", show.msg());
                            std::thread::sleep(std::time::Duration::from_millis(300));
                        }
                    });
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
        "settings" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let conn = db::open(&db_path)?;
            // The first launch: the interface language comes from the system (M21, ADR 0040).
            db::ensure_lang(&conn)?;
            let mut settings = db::load_settings(&conn);
            let mut lang = settings.lang;
            let mut changed = false;
            if let Some(dir) = flag_str(&args, "--models-dir") {
                settings.models_dir = PathBuf::from(dir);
                changed = true;
            }
            if let Some(id) = flag_str(&args, "--model") {
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
            if let Some(code) = flag_str(&args, "--ocr-lang") {
                settings.ocr_lang = code;
                changed = true;
            }
            // The interface language (M21, ADR 0040): only the listed codes, otherwise an error,
            // so a typo does not silently turn into English.
            if let Some(code) = flag_str(&args, "--lang") {
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
            if let Some(n) = flag_str(&args, "--max-model-mb") {
                settings.max_model_mb = n.parse().unwrap_or(settings.max_model_mb);
                changed = true;
            }
            if let Some(v) = flag_str(&args, "--face-threshold")
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
                let Some(path) = flag_str(&args, flag) else {
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
        "albums" => albums_cli(&args[1..]),
        "export" => organize::export_cli(&args[1..]),
        "tags" => tags_cli(&args[1..]),
        "eval" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let conn = db::open(&db_path)?;
            db::ensure_lang(&conn)?;
            let settings = db::load_settings(&conn);
            let lang = settings.lang;
            let entry = embed::active_entry(&settings);
            let source = embed::tag_source(entry);
            let defs = embed::ensure_tag_defs(&db_path, entry)?;
            if let Some(file) = flag_str(&args, "--labels") {
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
            let dir = match flag_str(&args, "--folder").or_else(|| args.get(1).cloned()) {
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
            let out = flag_str(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| db::cache_dir().join("eval"));
            let n = flag_num(&args, "--sample", eval::SAMPLE_DEFAULT);
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
        "clusters" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let lang = language(&args)?;
            let k = flag_num(&args, "--k", 30);
            let out = flag_str(&args, "--out")
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
        "doctor" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
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
        "gui" | "" => app::run(flag_str(&args, "--db").map(PathBuf::from)),
        "-h" | "--help" | "help" => {
            let lang = language(&args).unwrap_or_default();
            usage(lang);
            Ok(())
        }
        other => {
            let lang = language(&args).unwrap_or_default();
            eprintln!(
                "{}",
                i18n::fill(lang, "unknown subcommand: {}", &[other.to_string()])
            );
            eprintln!();
            usage(lang);
            std::process::exit(2);
        }
    }
}

/// `photocat albums …` — albums as data: the tree, the order and the rows in the DB.
/// No command opens a file by path or writes to disk: PATH is a `files.path` key, not a
/// file operation. Exit codes: 0 — done, 1 — a data error, 2 — wrong arguments or a refusal
/// without a confirmation.
fn albums_cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let lang = db::load_settings(&conn).lang;
    let cmd = args.first().map(String::as_str).unwrap_or("list");
    match cmd {
        "list" => {
            let tree = db::albums_tree(&conn)?;
            if tree.is_empty() {
                println!(
                    "{}",
                    i18n::t(
                        lang,
                        "no albums yet — create one: photocat albums new --name \"Summer 2024\""
                    )
                );
                return Ok(());
            }
            let by_tree = has_flag(args, "--tree");
            for node in &tree {
                let path = db::album_path(&conn, node.id)?;
                let indent = if by_tree {
                    "  ".repeat(node.depth)
                } else {
                    String::new()
                };
                let trashed = if node.trashed > 0 {
                    i18n::fill(
                        lang,
                        " · {} in the trash",
                        &[node.trashed.to_string()],
                    )
                } else {
                    String::new()
                };
                println!(
                    "{:>4}  {indent}{path}  · {}",
                    node.id,
                    i18n::fill(
                        lang,
                        "{} files · {} subalbums · {}{}",
                        &[
                            node.files.to_string(),
                            node.subalbums.to_string(),
                            db::human_in(lang, node.bytes),
                            trashed,
                        ]
                    )
                );
            }
            Ok(())
        }
        "new" => {
            let name = match flag_str(args, "--name") {
                Some(n) => n,
                None => {
                    eprintln!(
                        "{}",
                        i18n::t(
                            lang,
                            "a name is needed: photocat albums new --name \"Summer 2024\" [--parent ID]"
                        )
                    );
                    std::process::exit(2);
                }
            };
            let parent = flag_str(args, "--parent")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let note = flag_str(args, "--note").unwrap_or_default();
            let id = db::create_album(&conn, parent, &name, &note)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "album created: {} (id {})",
                    &[db::album_path(&conn, id)?, id.to_string()]
                )
            );
            Ok(())
        }
        "rename" => {
            let id = album_id_arg(args, lang);
            let name = match flag_str(args, "--name") {
                Some(n) => n,
                None => {
                    eprintln!(
                        "{}",
                        i18n::t(lang, "a name is needed: photocat albums rename --id N --name NAME")
                    );
                    std::process::exit(2);
                }
            };
            db::rename_album(&conn, id, &name)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "album renamed: {}",
                    &[db::album_path(&conn, id)?]
                )
            );
            Ok(())
        }
        "move" => {
            let id = album_id_arg(args, lang);
            let Some(parent) = flag_str(args, "--parent").and_then(|v| v.parse::<i64>().ok()) else {
                eprintln!(
                    "{}",
                    i18n::t(
                        lang,
                        "a parent is needed: photocat albums move --id N --parent ID [--position K]"
                    )
                );
                std::process::exit(2);
            };
            let position = flag_str(args, "--position").and_then(|v| v.parse::<i64>().ok());
            db::move_album(&conn, id, parent, position)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "album moved: {}",
                    &[db::album_path(&conn, id)?]
                )
            );
            Ok(())
        }
        "add" | "remove" => {
            let id = album_id_arg(args, lang);
            let paths = positional_paths(args, &["--id", "--db"]);
            if paths.is_empty() {
                eprintln!(
                    "{}",
                    i18n::fill(
                        lang,
                        "paths are needed: photocat albums {} --id N PATH...",
                        &[cmd.to_string()]
                    )
                );
                std::process::exit(2);
            }
            let file_ids = file_ids_for_paths(&conn, &paths)?;
            let path = db::album_path(&conn, id)?;
            if cmd == "add" {
                let (added, already) = db::add_to_album(&conn, id, &file_ids)?;
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "album “{}”: {} added, {} already there, {} files",
                        &[
                            path,
                            added.to_string(),
                            already.to_string(),
                            db::album_items(&conn, id)?.len().to_string(),
                        ]
                    )
                );
            } else {
                let removed = db::remove_from_album(&conn, id, &file_ids)?;
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "album “{}”: {} removed, {} left",
                        &[
                            path,
                            removed.to_string(),
                            db::album_items(&conn, id)?.len().to_string(),
                        ]
                    )
                );
            }
            Ok(())
        }
        "delete" => {
            let id = album_id_arg(args, lang);
            let tree = db::albums_tree(&conn)?;
            let node = tree
                .iter()
                .find(|n| n.id == id)
                .with_context(|| format!("there is no album {id}"))?;
            let mut subtree = vec![node.clone()];
            let mut seen = vec![id];
            loop {
                let mut grown = false;
                for candidate in &tree {
                    if seen.contains(&candidate.parent_id) && !seen.contains(&candidate.id) {
                        seen.push(candidate.id);
                        subtree.push(candidate.clone());
                        grown = true;
                    }
                }
                if !grown {
                    break;
                }
            }
            let rows: i64 = subtree.iter().map(|n| n.files).sum();
            if !has_flag(args, "--yes") {
                let names: Vec<&str> = subtree.iter().map(|n| n.name.as_str()).collect();
                eprintln!(
                    "{}",
                    i18n::fill(
                        lang,
                        "will be deleted: {} — {} albums, {} file rows; the files stay on disk. Repeat with --yes.",
                        &[
                            names.join(", "),
                            subtree.len().to_string(),
                            rows.to_string(),
                        ]
                    )
                );
                std::process::exit(2);
            }
            let albums = db::delete_album(&conn, id)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "deleted {} albums, {} file rows; the files on disk are untouched",
                    &[albums.to_string(), rows.to_string()]
                )
            );
            Ok(())
        }
        "show" => {
            let id = album_id_arg(args, lang);
            let path = db::album_path(&conn, id)?;
            let tree = db::albums_tree(&conn)?;
            let node = tree
                .iter()
                .find(|n| n.id == id)
                .with_context(|| format!("there is no album {id}"))?;
            let note = if node.note.trim().is_empty() {
                String::new()
            } else {
                format!(" · {}", node.note.trim())
            };
            println!(
                "{} (id {}) · {}",
                path,
                id,
                i18n::fill(
                    lang,
                    "{} files · {} subalbums · {}{}",
                    &[
                        node.files.to_string(),
                        node.subalbums.to_string(),
                        db::human_in(lang, node.bytes),
                        note,
                    ]
                )
            );
            if has_flag(args, "--tree") {
                for child in tree.iter().filter(|n| n.parent_id == id) {
                    println!(
                        "  {}",
                        i18n::fill(
                            lang,
                            "subalbum {} · {} files · {}",
                            &[
                                child.name.clone(),
                                child.files.to_string(),
                                db::human_in(lang, child.bytes),
                            ]
                        )
                    );
                }
            }
            for (position, row) in db::album_items(&conn, id)?.iter().enumerate() {
                let mark = if row.trashed_at.is_some() {
                    i18n::t(lang, "  [in the app trash]")
                } else {
                    ""
                };
                println!("{:>4}  {}{mark}", position + 1, row.path);
            }
            Ok(())
        }
        other => {
            eprintln!(
                "{}",
                i18n::fill(
                    lang,
                    "unknown albums command: {} (list [--tree] | new | rename | move | add | remove | delete | show)",
                    &[other.to_string()]
                )
            );
            std::process::exit(2);
        }
    }
}

fn album_id_arg(args: &[String], lang: Lang) -> i64 {
    match flag_str(args, "--id").and_then(|v| v.parse::<i64>().ok()) {
        Some(id) => id,
        None => {
            eprintln!("{}", i18n::t(lang, "an album number is needed: --id N"));
            std::process::exit(2);
        }
    }
}

/// The positional paths of a command: everything that is not a flag or a known flag value.
fn positional_paths(args: &[String], flags: &[&str]) -> Vec<String> {
    let mut out = Vec::new();
    let mut it = args.iter().skip(1);
    while let Some(arg) = it.next() {
        if arg.starts_with("--") {
            let name = arg.split('=').next().unwrap_or(arg);
            if flags.contains(&name) && !arg.contains('=') {
                it.next();
            }
            continue;
        }
        out.push(arg.clone());
    }
    out
}

/// A path → the file id in the index. The file is not opened; if there is no exact path,
/// the canonical form is tried (a symlink, `.`/`..`), but never the content.
fn file_ids_for_paths(conn: &rusqlite::Connection, paths: &[String]) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    let mut missing = Vec::new();
    for path in paths {
        let mut found: Option<i64> = conn
            .query_row("SELECT id FROM files WHERE path=?1", [path], |r| r.get(0))
            .ok();
        if found.is_none()
            && let Ok(canonical) = std::fs::canonicalize(path)
        {
            found = conn
                .query_row(
                    "SELECT id FROM files WHERE path=?1",
                    [canonical.to_string_lossy().to_string()],
                    |r| r.get(0),
                )
                .ok();
        }
        match found {
            Some(id) => ids.push(id),
            None => missing.push(path.clone()),
        }
    }
    if !missing.is_empty() {
        anyhow::bail!("not in the index: {}", missing.join(", "));
    }
    Ok(ids)
}

/// The tag dictionary: show, add, enable or disable, remove, JSON import and export.
/// After the edits `embed --retag` recomputes the tag weights (without running CLIP over the photos again).
fn tags_cli(args: &[String]) -> Result<()> {
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

/// A short version label: "ffmpeg version 6.1.1" instead of the line with the copyright.
fn short_version(line: &str) -> String {
    line.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
}

pub fn flag_str(args: &[String], name: &str) -> Option<String> {
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == name {
            return it.next().cloned();
        }
        if let Some(rest) = a.strip_prefix(&format!("{name}=")) {
            return Some(rest.to_string());
        }
    }
    None
}

pub fn flag_num(args: &[String], name: &str, default: u64) -> usize {
    flag_str(args, name)
        .and_then(|v| v.parse().ok())
        .unwrap_or(default as usize)
}

pub fn has_flag(args: &[String], name: &str) -> bool {
    args.iter().any(|a| a == name)
}

#[cfg(test)]
mod cli_i18n_tests {
    use super::*;

    /// M21.6: the CLI reports speak the language of `settings.lang`. Every report is a pair
    /// of lines, English and Russian, from the same `lang` field, so it is visible that the
    /// language is not global and both variants are alive.
    #[test]
    fn english_translates_the_command_reports() {
        // index
        let mut index = index::Stats {
            total: 3,
            indexed: 2,
            skipped: 1,
            errors: 0,
            bytes: 2048,
            seconds: 1.5,
            cancelled: false,
            lang: Lang::En,
        };
        let text = index.to_string();
        assert!(text.contains("indexed 2/3 files"), "{text}");
        assert!(text.contains("2.0 KiB"), "{text}");
        index.lang = Lang::Ru;
        assert!(index.to_string().contains("проиндексировано 2/3 файлов"));
        index.cancelled = true;
        assert!(index.to_string().starts_with("остановлено: "));
        index.lang = Lang::En;
        assert!(index.to_string().starts_with("stopped: "));

        // ocr
        let mut ocr = ocr::Stats {
            candidates: 4,
            done: 3,
            skipped: 1,
            pass_skipped: 0,
            errors: 0,
            chars: 120,
            seconds: 2.0,
            cancelled: false,
            lang: Lang::En,
        };
        let text = ocr.to_string();
        assert!(text.contains("recognised 3 of 4 files"), "{text}");
        assert!(text.contains("(1 skipped, text already there)"), "{text}");
        ocr.lang = Lang::Ru;
        assert!(ocr.to_string().contains("распознано 3 из 4 файлов"));

        // faces
        let mut faces = faces::Stats {
            images: 10,
            done: 8,
            skipped: 2,
            pass_skipped: 0,
            faces: 20,
            clusters: 5,
            matched: Default::default(),
            errors: 0,
            seconds: 3.0,
            cancelled: false,
            lang: Lang::En,
        };
        assert!(faces.to_string().contains("scanned 8 of 10 photos"));
        faces.lang = Lang::Ru;
        assert!(faces.to_string().contains("просканировано 8 из 10 снимков"));

        // The matching step of M26.11: one report, the same numbers in both languages, and its
        // reasons named — a dry run says "would match".
        let mut matched = faces::MatchReport {
            matched: 12,
            people: 3,
            named: 5,
            left_small: 4,
            left_far: 2,
            seconds: 0.07,
            dry_run: false,
            lang: Lang::En,
        };
        assert_eq!(
            matched.to_string(),
            "matched 12 faces to 3 named people; 4 left — the lead was too small; \
             2 left — no named person is close enough"
        );
        matched.lang = Lang::Ru;
        let ru = matched.to_string();
        assert!(
            ru.contains("сопоставлено 12 лиц 3 названным людям")
                && ru.contains("4 осталось — перевес слишком мал"),
            "{ru}"
        );
        matched.dry_run = true;
        matched.lang = Lang::En;
        assert!(matched.to_string().starts_with("would match 12 faces"));
        matched.matched = 0;
        matched.people = 0;
        assert!(matched.to_string().starts_with("no automatic match yet: 5 named"));
        matched.named = 0;
        assert!(matched
            .to_string()
            .contains("no named people yet: there is nothing to match to"));

        let mut clusters = faces::ClusterStats {
            faces: 20,
            clusters: 5,
            fixed: 2,
            pass_skipped: 0,
            matched: faces::MatchReport {
                matched: 12,
                people: 3,
                named: 5,
                lang: Lang::En,
                ..Default::default()
            },
            seconds: 1.0,
            lang: Lang::En,
        };
        assert!(clusters.to_string().contains("20 faces, 5 groups"));
        assert!(
            clusters.to_string().ends_with("; matched 12 faces to 3 named people"),
            "{clusters}"
        );
        clusters.lang = Lang::Ru;
        assert!(clusters.to_string().contains("лиц 20, групп 5"));
        clusters.matched.lang = Lang::Ru;
        assert!(clusters.to_string().contains("сопоставлено 12 лиц 3 названным людям"));

        // embed
        let mut embed = embed::EmbedStats {
            model: "clip-vit-b32",
            total: 10,
            computed: 9,
            skipped: 1,
            pass_skipped: 0,
            errors: 0,
            seconds: 4.0,
            model_bytes: 2048,
            cancelled: false,
            lang: Lang::En,
        };
        let text = embed.to_string();
        assert!(text.contains("computed 9/10"), "{text}");
        assert!(text.contains("2.0 KiB"), "{text}");
        embed.lang = Lang::Ru;
        assert!(embed.to_string().contains("посчитано 9/10"));

        let mut tags = embed::RetagStats {
            model: "clip-vit-b32",
            total: 10,
            tagged: 10,
            pass_skipped: 0,
            seconds: 1.0,
            lang: Lang::En,
        };
        assert!(tags.to_string().contains("tags (clip-vit-b32): recomputed 10/10"));
        tags.lang = Lang::Ru;
        assert!(tags.to_string().contains("теги (clip-vit-b32): пересчитано 10/10"));

        // organize: both the plan summary and the export report
        let mut export = organize::ExportReport {
            copied: 2,
            bytes: 2048,
            lang: Lang::En,
            ..Default::default()
        };
        let text = export.to_string();
        assert!(text.contains("exported 2 files"), "{text}");
        assert!(text.contains("2.0 KiB"), "{text}");
        export.lang = Lang::Ru;
        assert!(export.to_string().contains("выгружено 2 файлов"));
        assert_eq!(organize::Layout::Day.title(Lang::En), "by day");
        assert_eq!(organize::Layout::Day.title(Lang::Ru), "по дням");

        // tools: the same words `doctor` prints and Settings shows
        assert_eq!(
            tools::Tool::Tesseract.purpose(Lang::En),
            "OCR: text from documents and screenshots"
        );
        assert_eq!(
            tools::Tool::Tesseract.purpose(Lang::Ru),
            "OCR: текст с документов и скриншотов"
        );
        let t = tools::Tools::default();
        assert_eq!(t.source(tools::Tool::Tesseract, Lang::En), "from PATH");
        assert_eq!(t.source(tools::Tool::Tesseract, Lang::Ru), "из PATH");
    }

    /// M21.6: the duplicates give a feature and advice parts instead of a ready Russian phrase —
    /// the same set is printed in both languages.
    #[test]
    fn english_translates_the_duplicate_advice() {
        assert_eq!(dupes::Reason::Exact.label(Lang::En), "exact copy");
        assert_eq!(dupes::Reason::Similar.label(Lang::Ru), "похожий снимок");

        let advice = dupes::Advice {
            same_content: false,
            parts: vec![
                dupes::AdvicePart::SharperBy(150.0),
                dupes::AdvicePart::MorePixels,
            ],
        };
        assert_eq!(
            advice.label(Lang::En),
            "sharper than the rest by 150% · more pixels"
        );
        assert_eq!(
            advice.label(Lang::Ru),
            "резче остальных на 150% · больше по разрешению"
        );
        let same = dupes::Advice {
            same_content: true,
            parts: Vec::new(),
        };
        assert_eq!(same.label(Lang::En), "exact copies — no difference");
        assert_eq!(same.label(Lang::Ru), "точные копии — разницы нет");
    }

    /// M21.6: the comparison text, the manifest check and the manifest result are the same
    /// source of words as in the CLI, and they follow the `lang` field.
    #[test]
    fn english_translates_the_verification_reports() {
        let mut report = verify::Report {
            files_a: 2,
            files_b: 2,
            only_a: vec!["only.jpg".into()],
            only_b: Vec::new(),
            size_diff: vec![("size.jpg".into(), 10, 20)],
            content_diff: Vec::new(),
            read_errors: Vec::new(),
            same: 1,
            hashed: 1,
            hashed_bytes: 10,
            unchecked: 0,
            scan_errors: 0,
            skipped_a: 0,
            skipped_b: 0,
            cancelled: false,
            seconds: 1.5,
            lang: Lang::En,
        };
        let text = verify::verify_report(std::path::Path::new("/a"), std::path::Path::new("/b"), &report, true);
        assert!(text.contains("comparison: /a"), "{text}");
        assert!(text.contains("only in A: 1"), "{text}");
        assert!(text.contains("size differs:\n  size.jpg"), "{text}");
        assert!(text.contains("verdict: differences found"), "{text}");
        report.lang = Lang::Ru;
        let text = verify::verify_report(std::path::Path::new("/a"), std::path::Path::new("/b"), &report, true);
        assert!(text.contains("только в A: 1"), "{text}");
        assert!(text.contains("вердикт: найдены расхождения"), "{text}");

        let mut check = verify::CheckReport {
            missing: vec!["gone.jpg".into()],
            extra: Vec::new(),
            changed: Vec::new(),
            read_errors: Vec::new(),
            same: 4,
            unchecked: 0,
            scan_errors: 0,
            skipped: 1,
            cancelled: false,
            seconds: 0.2,
            lang: Lang::En,
        };
        let text = verify::check_report(
            std::path::Path::new("/tmp/list.sha256"),
            std::path::Path::new("/archive"),
            &check,
            true,
        );
        assert!(text.contains("manifest check: /tmp/list.sha256"), "{text}");
        assert!(text.contains("missing from the folder:\n  gone.jpg"), "{text}");
        assert!(text.contains("the folder does not match the manifest"), "{text}");
        check.lang = Lang::Ru;
        let text = verify::check_report(
            std::path::Path::new("/tmp/list.sha256"),
            std::path::Path::new("/archive"),
            &check,
            true,
        );
        assert!(text.contains("нет в каталоге:\n  gone.jpg"), "{text}");

        let mut stats = verify::ManifestStats {
            files: 3,
            bytes: 30,
            skipped: 0,
            errors: 0,
            unchecked: 0,
            cancelled: false,
            written: true,
            seconds: 0.5,
            lang: Lang::En,
        };
        let text = verify::manifest_report(std::path::Path::new("/archive"), None, &stats);
        assert!(text.contains("manifest /archive: 3 files"), "{text}");
        stats.lang = Lang::Ru;
        assert!(
            verify::manifest_report(std::path::Path::new("/archive"), None, &stats)
                .contains("слепок /archive: 3 файлов")
        );
    }

    /// M21.6: `--help` is part of the CLI and is translated with it; the English text holds no
    /// Cyrillic, and the Russian one stays Russian.
    #[test]
    fn the_cli_help_is_translated() {
        let en = i18n::t(Lang::En, i18n::USAGE.0);
        assert!(en.starts_with("photocat — media archive cataloguer"), "{en}");
        assert!(en.contains("subcommands:"));
        assert!(en.contains("default: GUI"));
        assert!(
            !en.chars().any(|c| c.is_alphabetic() && ('\u{0400}'..='\u{04ff}').contains(&c)),
            "the English help still holds Cyrillic: {en}"
        );
        let ru = i18n::t(Lang::Ru, i18n::USAGE.0);
        assert!(ru.starts_with("photocat — каталогизатор медиаархива"), "{ru}");
    }
}
