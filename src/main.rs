//! photocat — каталогизатор медиаархива: индексация, дубли, раскладка по датам, ML-теги.
//!
//! Подкоманды:
//!   photocat index <каталог> [--jobs N] [--db PATH]
//!   photocat stats
//!   photocat dupes [--max-dist 6] [--csv FILE] [--plan FILE]
//!   photocat organize --out <каталог> [--by day|month|event] [--name ШАБЛОН]
//!                     [--exclude-dup] [--apply] [--copy|--symlink] [--rename]
//!   photocat verify <a> <b> [--jobs N] [--all]  |  verify --manifest FILE <dir>
//!   photocat manifest [каталог] [--out FILE] [--jobs N]
//!   photocat embed [--jobs N] [--batch N] [--refresh] [--retag]
//!   photocat tags [--list] [--add МЕТКА] [--prompt ТЕКСТ] [--export FILE] [--import FILE]
//!   photocat ocr [--jobs N] [--all]
//!   photocat search <запрос> [--top N] [--text]
//!   photocat clusters [--k N] [--out FILE]
//!   photocat eval --folder DIR --out DIR [--sample N]   разметка тегов на выборке
//!   photocat eval --labels FILE                         метрики по готовой разметке
//!   photocat albums list|new|rename|move|add|remove|delete|show
//!   photocat export --album N --out DIR [--name ШАБЛОН] [--apply]
//!   photocat            (без аргументов — GUI)

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

/// Язык отчёта: `settings.lang` из БД (M21.6). Первый запуск определяет язык из
/// локали, как и GUI, — чтобы `photocat stats` и окно говорили на одном языке.
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
                        i18n::t(lang, "нужен каталог: photocat index <каталог>")
                    );
                    std::process::exit(2);
                }
            };
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
                            "индексация: {}/{} файлов, ошибок {}",
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
            let mut stats = index::run(&root, jobs, &db_path, &settings.tools, Some(progress))?;
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
                            "CLIP: {}/{} файлов, ошибок {} — {}",
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
                            i18n::t(lang, "нужны номер группы и имя: faces label --id N --name ИМЯ")
                        );
                        std::process::exit(2);
                    }
                    faces::label(&db_path, id, &label)?;
                    println!(
                        "{}",
                        i18n::fill(lang, "группа {}: «{}»", &[id.to_string(), label.trim().to_string()])
                    );
                    return Ok(());
                }
                "split" => {
                    let id = flag_str(&args, "--id")
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0);
                    if id == 0 {
                        eprintln!(
                            "{}",
                            i18n::t(lang, "нужен номер лица: faces split --id N")
                        );
                        std::process::exit(2);
                    }
                    let cid = faces::split(&db_path, id)?;
                    println!(
                        "{}",
                        i18n::fill(
                            lang,
                            "лицо {} отделено в группу {}",
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
                    i18n::t(lang, "на месте")
                } else {
                    i18n::t(lang, "не скачана — «Настройки» или faces download --yes")
                };
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "снимков: {}, ещё без лиц: {}; модель лиц: {}",
                        &[plan.images.to_string(), plan.todo.to_string(), model.to_string()]
                    )
                );
                return Ok(());
            }
            if has_flag(&args, "--list") {
                let conn = db::open(&db_path)?;
                let clusters = db::load_face_clusters(&conn)?;
                if clusters.is_empty() {
                    println!(
                        "{}",
                        i18n::t(lang, "групп лиц нет: запустите photocat faces")
                    );
                }
                for c in clusters {
                    println!(
                        "{} {:<24} {}",
                        c.id,
                        if c.label.is_empty() {
                            i18n::t(lang, "(без имени)")
                        } else {
                            c.label.as_str()
                        },
                        i18n::fill(
                            lang,
                            "лиц {}, фото {}",
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
                            "лица: {}/{} снимков, ошибок {} — {}",
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
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "кандидатов: {} из {} снимков; tesseract ({}): {}",
                        &[
                            plan.candidates.to_string(),
                            plan.images.to_string(),
                            settings.tools.source(tools::Tool::Tesseract, lang).to_string(),
                            settings
                                .tools
                                .probe(tools::Tool::Tesseract)
                                .map(|v| short_version(&v))
                                .unwrap_or_else(|| i18n::t(lang, "не найден").into()),
                        ]
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
                            "OCR: {}/{} файлов, ошибок {} — {}",
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
                    .map_err(|_| anyhow::anyhow!("файл не в индексе: {path}"))?;
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
                        i18n::t(lang, "нужен запрос: photocat search \"торт\"")
                    );
                    std::process::exit(2);
                }
            };
            let top = flag_num(&args, "--top", 30);
            if has_flag(&args, "--text") {
                // Поиск по распознанному тексту — без модели и без сети.
                let conn = db::open(&db_path)?;
                let t0 = std::time::Instant::now();
                let hits = db::search_ocr(&conn, &query, top)?;
                let ms = t0.elapsed().as_secs_f32() * 1000.0;
                println!(
                    "{}",
                    i18n::fill(
                        lang,
                        "текст на фото, запрос «{}»: файлов {} за {} мс",
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
            eprintln!("{}", i18n::t(lang, "загрузка модели CLIP…"));
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
                            "запрос «{}»: результатов {} за {} мс",
                            &[query.clone(), hits.len().to_string(), format!("{ms:.0}")]
                        )
                    );
                    first = hits;
                } else {
                    println!(
                        "{}",
                        i18n::fill(
                            lang,
                            "повтор {}: {} мс",
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
                        "в среднем: {} мс",
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
                            i18n::t(lang, "на месте")
                        } else if fits {
                            i18n::t(lang, "не скачана")
                        } else {
                            i18n::t(lang, "сверх лимита")
                        };
                        println!(
                            "{} {:<22} {} {} · {} · {} · {} · {}",
                            if std::ptr::eq(e, active) { "*" } else { " " },
                            e.id,
                            e.dim,
                            i18n::t(lang, "призн."),
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
                            "папка моделей: {}",
                            &[settings.models_dir.display().to_string()]
                        )
                    );
                    println!(
                        "{}",
                        i18n::fill(
                            lang,
                            "лимит: {} МиБ",
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
                            "модель в порядке: {}",
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
                            "неизвестная подкоманда models: {} (models list|check|download)",
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
            // Первый запуск: язык интерфейса берётся из системы (M21, ADR 0040).
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
                            "нет такой модели: {} (см. models list)",
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
            // Язык интерфейса (M21, ADR 0040): только коды из списка, иначе — ошибка,
            // чтобы опечатка не превращалась молча в английский.
            if let Some(code) = flag_str(&args, "--lang") {
                match code.trim().to_ascii_lowercase().as_str() {
                    "en" | "ru" => {
                        settings.lang = Lang::parse(&code);
                        // Смена языка действует сразу: отчёт печатается уже на новом.
                        lang = settings.lang;
                        changed = true;
                    }
                    other => {
                        eprintln!(
                            "{}",
                            i18n::fill(
                                lang,
                                "нет такого языка интерфейса: {} (en, ru)",
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
                ("--curl", tools::Tool::Curl),
            ] {
                let Some(path) = flag_str(&args, flag) else {
                    continue;
                };
                let mut probe = settings.tools.clone();
                *probe.get_mut(tool) = path.trim().to_string();
                // Путь проверяется до сохранения: ошибка — сообщением, а не падением.
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
                            i18n::fill(lang, "не сохранён: {}", &[format!("{e:#}")])
                        );
                        std::process::exit(2);
                    }
                }
            }
            if changed {
                db::save_settings(&conn, &settings)?;
                println!("{}", i18n::t(lang, "настройки сохранены"));
            }
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "модель: {}",
                    &[embed::active_entry(&settings).id.to_string()]
                )
            );
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "папка моделей: {}",
                    &[settings.models_dir.display().to_string()]
                )
            );
            println!(
                "{}",
                i18n::fill(lang, "лимит: {} МиБ", &[settings.max_model_mb.to_string()])
            );
            println!(
                "{}",
                i18n::fill(lang, "потоков: {}", &[settings.index_jobs.to_string()])
            );
            println!(
                "{}",
                i18n::fill(lang, "порог дублей: {}", &[settings.max_dist.to_string()])
            );
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "тег: уверенный ≥ {}, отрыв {}",
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
                    "фильтр по тегу: порог {}% (процентиль)",
                    &[settings.tag_show_pct.to_string()]
                )
            );
            println!(
                "{}",
                i18n::fill(lang, "языки OCR: {}", std::slice::from_ref(&settings.ocr_lang))
            );
            println!(
                "{}",
                i18n::fill(lang, "язык интерфейса: {}", &[settings.lang.code().to_string()])
            );
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "порог лиц: {}",
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
                        .unwrap_or_else(|| i18n::t(lang, "не найден").to_string()),
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
                        i18n::fill(lang, "разметки нет: заполните {} и повторите", &[file])
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
                        "строк в разметке: {} · размечено {} · «другое» {} · пропущено {} · не размечено {} · нет в индексе {}",
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
                        "словарь покрывает {} % снимков; у {} допустима не одна метка",
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
                        "top-1: {} из {} = {} % · top-3: {} из {} = {} %",
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
                        "уверенный тег: {} снимков, точность {} %; уверенно мимо («другое») {}",
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
                            "мебель: при {} % найдено {} из {} (порог {})",
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
                        "проверка {} · модель {}",
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
                        i18n::t(lang, "нужен каталог: photocat eval --folder DIR --out DIR")
                    );
                    std::process::exit(2);
                }
            };
            // По умолчанию — в кэш, а не в репозиторий: в labels.csv пути семейного архива.
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
                        "в {} нет снимков с тегами: проиндексируйте каталог и посчитайте теги",
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
                    "выборка: {} снимков из {} годных в {} · метки: {} · подсказки: {} · листов: {}",
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
                    "заполните столбец label в {} (other / skip / метки через |) и повторите с --labels",
                    &[report.labels.display().to_string()]
                )
            );
            println!(
                "{}",
                i18n::fill(lang, "модель {} · {}", &[entry.id.to_string(), source])
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
                    "кластеров: {} на {} фото, за {} с → {}",
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
                    "внешние программы (пусто в настройках — ищутся в PATH; без программы отключается только её работа):"
                )
            );
            for tool in tools::ALL {
                let state = match settings.tools.version(tool) {
                    Ok(v) => short_version(&v),
                    Err(e) => i18n::fill(
                        lang,
                        "не работает: {}",
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
                        "не найдена: {} ({})",
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
                        "зачем: {}; без неё: {}",
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
                                "языки OCR: {} — на месте",
                                std::slice::from_ref(&settings.ocr_lang)
                            )
                        );
                    } else {
                        println!(
                            "{}",
                            i18n::fill(
                                lang,
                                "языки OCR: {} — нет {}; установите языковые пакеты tesseract для них",
                                &[settings.ocr_lang.clone(), missing.join(", ")]
                            )
                        );
                    }
                    println!(
                        "{}",
                        i18n::fill(lang, "  доступны: {}", &[langs.join(", ")])
                    );
                }
                None => println!(
                    "{}",
                    i18n::t(lang, "языки OCR: не проверить — tesseract не найден")
                ),
            }

            let entry = embed::active_entry(&settings);
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "модель CLIP: {} · {} · {}",
                    &[
                        i18n::t(lang, entry.title).to_string(),
                        db::human_in(lang, entry.bytes() as i64),
                        if embed::model_present(&settings) {
                            i18n::t(lang, "на месте").to_string()
                        } else {
                            i18n::t(lang, "не скачана — смысловой поиск и теги недоступны")
                                .to_string()
                        },
                    ]
                )
            );
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "модель лиц: {} · {} · {}",
                    &[
                        faces::MODEL_ID.to_string(),
                        db::human_in(lang, faces::model_bytes(&settings)),
                        if faces::model_present(&settings) {
                            i18n::t(lang, "на месте").to_string()
                        } else {
                            i18n::t(lang, "не скачана — «Настройки» или faces download --yes")
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
                    "лица: {} на {} снимках, групп {}, ещё без лиц {} из {}",
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
                    "БД: {} · {} · файлов {}, с текстом {} (знаков {})",
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
                i18n::fill(lang, "неизвестная подкоманда: {}", &[other.to_string()])
            );
            eprintln!();
            usage(lang);
            std::process::exit(2);
        }
    }
}

/// `photocat albums …` — альбомы как данные: дерево, порядок и строки в БД.
/// Ни одна команда не открывает файл по пути и не пишет на диск: PATH — ключ
/// `files.path`, а не файловая операция. Коды: 0 — сделано, 1 — ошибка данных,
/// 2 — неверные аргументы или отказ без подтверждения.
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
                        "альбомов нет — создайте: photocat albums new --name \"Дача 2024\""
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
                        " · в корзине {}",
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
                        "файлов {} · подальбомов {} · {}{}",
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
                            "нужно имя: photocat albums new --name \"Дача 2024\" [--parent ID]"
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
                    "альбом создан: {} (id {})",
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
                        i18n::t(lang, "нужно имя: photocat albums rename --id N --name ИМЯ")
                    );
                    std::process::exit(2);
                }
            };
            db::rename_album(&conn, id, &name)?;
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "альбом переименован: {}",
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
                        "нужен родитель: photocat albums move --id N --parent ID [--position K]"
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
                    "альбом перемещён: {}",
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
                        "нужны пути: photocat albums {} --id N PATH...",
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
                        "альбом «{}»: добавлено {}, уже было {}, файлов {}",
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
                        "альбом «{}»: убрано {}, осталось {}",
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
                .with_context(|| format!("альбома {id} нет"))?;
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
                        "будет удалено: {} — альбомов {}, строк файлов {}; файлы на диске останутся. Повторите с --yes.",
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
                    "удалено альбомов {}, строк файлов {}; файлы на диске не тронуты",
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
                .with_context(|| format!("альбома {id} нет"))?;
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
                    "файлов {} · подальбомов {} · {}{}",
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
                            "подальбом {} · файлов {} · {}",
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
                    i18n::t(lang, "  [в корзине приложения]")
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
                    "неизвестная команда albums: {} (list [--tree] | new | rename | move | add | remove | delete | show)",
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
            eprintln!("{}", i18n::t(lang, "нужен номер альбома: --id N"));
            std::process::exit(2);
        }
    }
}

/// Позиционные пути команды: всё, что не флаг и не значение известного флага.
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

/// Путь → id файла в индексе. Файл не открывается; если точного пути нет,
/// пробуем канонический вид (симлинк, `.`/`..`), но не содержимое.
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
        anyhow::bail!("нет в индексе: {}", missing.join(", "));
    }
    Ok(ids)
}

/// Словарь тегов: показать, добавить, включить/выключить, убрать, импорт/экспорт JSON.
/// После правок веса тегов пересчитывает `embed --retag` (без повторного CLIP по фото).
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
                "словарь модели {} — {} тегов",
                &[entry.id.to_string(), defs.len().to_string()]
            )
        );
        for d in &defs {
            let threshold = if d.min_percentile > 0 {
                i18n::fill(
                    lang,
                    ", порог {}%",
                    &[d.min_percentile.to_string()],
                )
            } else {
                String::new()
            };
            println!(
                "  {:<10} {:<16} {}{} · {}",
                d.id,
                d.label,
                i18n::t(lang, if d.enabled { "ок" } else { "выкл" }),
                threshold,
                i18n::fill(
                    lang,
                    "файлов {} · промптов {}",
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
            anyhow::bail!("{}", i18n::t(lang, "--add: нужна непустая метка"));
        }
        if defs.iter().any(|d| d.label == label) {
            anyhow::bail!(
                "{}",
                i18n::fill(lang, "тег «{}» уже есть в словаре", std::slice::from_ref(&label))
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
                i18n::t(lang, "--add: нужен хотя бы один --prompt «фотография …»")
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
                "тег «{}» добавлен (id {}) — пересчитайте веса: photocat embed --retag",
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
            .with_context(|| format!("нет тега «{want}»"))?;
        d.enabled = enable;
        db::save_tag_def(&conn, d)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "тег «{}» {}",
                &[
                    d.label.clone(),
                    i18n::t(lang, if enable { "включён" } else { "выключен" }).to_string(),
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
            .with_context(|| format!("нет тега «{want}»"))?;
        db::delete_tag_def(&conn, entry.id, &d.id)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "тег «{}» убран из словаря — веса исчезнут после photocat embed --retag",
                std::slice::from_ref(&d.label)
            )
        );
        return Ok(());
    }

    if let Some(path) = flag_str(args, "--export") {
        std::fs::write(&path, embed::tag_defs_to_json(&defs))
            .with_context(|| format!("не записать {path}"))?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "словарь выгружен: {} ({} тегов)",
                &[path, defs.len().to_string()]
            )
        );
        return Ok(());
    }

    if let Some(path) = flag_str(args, "--import") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("не прочитать {path}"))?;
        let defs = embed::tag_defs_from_json(entry.id, &text)?;
        embed::import_defs(&db_path, entry.id, &defs)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "словарь загружен: {} тегов — пересчитайте веса: photocat embed --retag",
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
                "словарь возвращён к стандартному ({} тегов) — photocat embed --retag",
                &[defs.len().to_string()]
            )
        );
        return Ok(());
    }

    list()
}

/// Короткая подпись версии: «ffmpeg version 6.1.1» вместо строки с копирайтом.
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

    /// M21.6: отчёты CLI говорят на языке из `settings.lang`. Каждый отчёт — пара
    /// строк: английская и русская из одного и того же поля `lang`, чтобы было видно,
    /// что язык не глобальный и оба варианта живы.
    #[test]
    fn английский_язык_переводит_отчёты_команд() {
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
            faces: 20,
            clusters: 5,
            errors: 0,
            seconds: 3.0,
            cancelled: false,
            lang: Lang::En,
        };
        assert!(faces.to_string().contains("scanned 8 of 10 photos"));
        faces.lang = Lang::Ru;
        assert!(faces.to_string().contains("просканировано 8 из 10 снимков"));

        let mut clusters = faces::ClusterStats {
            faces: 20,
            clusters: 5,
            fixed: 2,
            seconds: 1.0,
            lang: Lang::En,
        };
        assert!(clusters.to_string().contains("20 faces, 5 groups"));
        clusters.lang = Lang::Ru;
        assert!(clusters.to_string().contains("лиц 20, групп 5"));

        // embed
        let mut embed = embed::EmbedStats {
            model: "clip-vit-b32",
            total: 10,
            computed: 9,
            skipped: 1,
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
        assert_eq!(tools::Tool::Curl.purpose(Lang::En), "downloading model weights on consent");
        assert_eq!(
            tools::Tool::Curl.purpose(Lang::Ru),
            "скачивание весов модели по согласию"
        );
        let t = tools::Tools::default();
        assert_eq!(t.source(tools::Tool::Curl, Lang::En), "from PATH");
        assert_eq!(t.source(tools::Tool::Curl, Lang::Ru), "из PATH");
    }

    /// M21.6: дубли отдают не готовую русскую фразу, а признак и части совета —
    /// один и тот же набор печатается на двух языках.
    #[test]
    fn английский_язык_переводит_советы_дублей() {
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

    /// M21.6: текст сверки, проверки по слепку и итог записи слепка — тот же
    /// источник слов, что и у CLI, и он переводится по полю `lang`.
    #[test]
    fn английский_язык_переводит_отчёты_сверки() {
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

    /// M21.6: `--help` — часть CLI и переводится вместе с ним; английский текст
    /// не содержит кириллицы, а русский остаётся русским.
    #[test]
    fn справка_cli_переведена() {
        let en = i18n::t(Lang::En, i18n::USAGE.0);
        assert!(en.starts_with("photocat — media archive cataloguer"), "{en}");
        assert!(en.contains("subcommands:"));
        assert!(en.contains("default: GUI"));
        assert!(
            !en.chars().any(|c| c.is_alphabetic() && ('\u{0400}'..='\u{04ff}').contains(&c)),
            "в английской справке осталась кириллица: {en}"
        );
        let ru = i18n::t(Lang::Ru, i18n::USAGE.0);
        assert!(ru.starts_with("photocat — каталогизатор медиаархива"), "{ru}");
    }
}
