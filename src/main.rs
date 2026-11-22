//! photocat — каталогизатор медиаархива: индексация, дубли, раскладка по датам, ML-теги.
//!
//! Подкоманды:
//!   photocat index <каталог> [--jobs N] [--db PATH]
//!   photocat stats
//!   photocat dupes [--max-dist 6] [--csv FILE] [--plan FILE]
//!   photocat organize --out <каталог> [--apply] [--copy|--symlink] [--rename]
//!   photocat embed [--jobs N] [--batch N]
//!   photocat ocr [--jobs N] [--all]
//!   photocat search <запрос> [--top N] [--text]
//!   photocat clusters [--k N] [--out FILE]
//!   photocat            (без аргументов — GUI)

mod app;
mod db;
mod derive;
mod dupes;
mod embed;
mod index;
mod ocr;
mod organize;

use anyhow::Result;
use db::Progress;
use std::path::PathBuf;
use std::sync::Arc;

fn usage() {
    println!(
        "photocat — каталогизатор медиаархива

подкоманды:
  index <каталог> [--jobs N] [--db PATH]   проиндексировать каталог
  stats [--db PATH]                        статистика по индексу
  dupes [--max-dist N] [--csv FILE] [--plan FILE]
                                            найти дубли и похожие снимки, выгрузить решения
  organize --out DIR [--apply] [--copy]    план (и применение) раскладки по датам
  embed [--jobs N] [--batch N]              CLIP-эмбеддинги и теги (модель локальная)
  ocr [--jobs N] [--all] [--dry-run]       текст с документов и скриншотов (tesseract)
  search <запрос> [--top N] [--text]        смысловой поиск или поиск по тексту на фото
  clusters [--k N] [--out FILE]             кластеры CLIP → отчёт с примерами
  models [list|check|download --id ID --yes]
                                            каталог моделей, проверка и загрузка весов
  settings [--models-dir DIR] [--model ID] [--max-model-mb N] [--ocr-lang L]
                                            показать или изменить настройки
  doctor                                   что есть на машине: программы, языки OCR, модель
  gui                                       графический интерфейс (по умолчанию)

по умолчанию: GUI"
    );
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("gui");
    match cmd {
        "index" => {
            let root = match args.get(1) {
                Some(p) if !p.starts_with('-') => PathBuf::from(p),
                _ => {
                    eprintln!("нужен каталог: photocat index <каталог>");
                    std::process::exit(2);
                }
            };
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let default_jobs = {
                let conn = db::open(&db_path)?;
                db::load_settings(&conn).index_jobs
            };
            let jobs = flag_num(&args, "--jobs", default_jobs as u64);
            let progress = Arc::new(Progress::default());
            let show = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                loop {
                    let (d, t, e) = (show.done(), show.total(), show.errors());
                    if t > 0 {
                        eprint!("\rиндексация: {d}/{t}, ошибок {e}");
                    }
                    if show.finished() {
                        eprintln!();
                        break;
                    }
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            });
            let stats = index::run(&root, jobs, &db_path, Some(progress))?;
            println!("{stats}");
            Ok(())
        }
        "stats" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            db::print_stats(&db_path)
        }
        "dupes" => dupes::cli(&args[1..]),
        "organize" => organize::cli(&args[1..]),
        "embed" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let jobs = flag_num(&args, "--jobs", 8);
            let batch = flag_num(&args, "--batch", 8);
            let progress = Arc::new(Progress::default());
            let show = progress.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(300));
                loop {
                    let (d, t, e) = (show.done(), show.total(), show.errors());
                    if t > 0 {
                        eprint!("\rCLIP: {d}/{t}, ошибок {e} — {}", show.msg());
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
            let stats = embed::embed_missing(&db_path, jobs, batch, Some(progress))?;
            println!("{stats}");
            Ok(())
        }
        "ocr" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let jobs = flag_num(&args, "--jobs", 4);
            let all = has_flag(&args, "--all");
            if has_flag(&args, "--dry-run") {
                let plan = ocr::plan(&db_path, all)?;
                println!(
                    "кандидатов: {} из {} снимков; tesseract: {}",
                    plan.candidates,
                    plan.images,
                    derive::tesseract_version().unwrap_or_else(|| "не найден".into())
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
                        eprint!("\rOCR: {d}/{t}, ошибок {e} — {}", show.msg());
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
            let stats = ocr::run(&db_path, jobs, all, Some(progress))?;
            println!("{stats}");
            Ok(())
        }
        "search" => {
            let query = match args.get(1) {
                Some(q) if !q.starts_with('-') => q.clone(),
                _ => {
                    eprintln!("нужен запрос: photocat search \"торт\"");
                    std::process::exit(2);
                }
            };
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let top = flag_num(&args, "--top", 30);
            if has_flag(&args, "--text") {
                // Поиск по распознанному тексту — без модели и без сети.
                let conn = db::open(&db_path)?;
                let t0 = std::time::Instant::now();
                let hits = db::search_ocr(&conn, &query, top)?;
                let ms = t0.elapsed().as_secs_f32() * 1000.0;
                println!("текст на фото, запрос «{query}»: файлов {} за {ms:.0} мс", hits.len());
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
            eprintln!("загрузка модели CLIP…");
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
                    println!("запрос «{query}»: {} результатов за {ms:.0} мс", hits.len());
                    first = hits;
                } else {
                    println!("повтор {n}: {ms:.0} мс");
                }
            }
            if repeat > 1 {
                println!("в среднем: {:.0} мс", total / repeat as f32);
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
                db::load_settings(&conn)
            };
            match args.get(1).map(String::as_str).unwrap_or("list") {
                "list" => {
                    let active = embed::active_entry(&settings);
                    for e in embed::catalog() {
                        let present = embed::model_present_in(&settings.models_dir, e);
                        println!(
                            "{} {:<10} {} · {} · {}",
                            if std::ptr::eq(e, active) { "*" } else { " " },
                            e.id,
                            db::human(e.bytes() as i64),
                            if present { "на месте" } else { "не скачана" },
                            e.note
                        );
                    }
                    println!("папка моделей: {}", settings.models_dir.display());
                    println!("лимит: {} МиБ", settings.max_model_mb);
                }
                "check" => {
                    embed::check_model(&settings)?;
                    println!("модель в порядке: {}", embed::active_entry(&settings).title);
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
                    let report = embed::download_model(&settings, &id, consent, Some(progress))?;
                    println!("{report}");
                }
                other => {
                    eprintln!("неизвестная подкоманда models: {other}\nmodels [list|check|download]");
                    std::process::exit(2);
                }
            }
            Ok(())
        }
        "settings" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let conn = db::open(&db_path)?;
            let mut settings = db::load_settings(&conn);
            let mut changed = false;
            if let Some(dir) = flag_str(&args, "--models-dir") {
                settings.models_dir = PathBuf::from(dir);
                changed = true;
            }
            if let Some(id) = flag_str(&args, "--model") {
                if embed::entry(&id).is_none() {
                    eprintln!("нет такой модели: {id} (см. models list)");
                    std::process::exit(2);
                }
                settings.model_id = id;
                changed = true;
            }
            if let Some(lang) = flag_str(&args, "--ocr-lang") {
                settings.ocr_lang = lang;
                changed = true;
            }
            if let Some(n) = flag_str(&args, "--max-model-mb") {
                settings.max_model_mb = n.parse().unwrap_or(settings.max_model_mb);
                changed = true;
            }
            if changed {
                db::save_settings(&conn, &settings)?;
                println!("настройки сохранены");
            }
            println!("модель: {}", embed::active_entry(&settings).id);
            println!("папка моделей: {}", settings.models_dir.display());
            println!("лимит: {} МиБ", settings.max_model_mb);
            println!("потоков: {}", settings.index_jobs);
            println!("порог дублей: {}", settings.max_dist);
            println!("тег: ≥ {:.2}, отрыв {:.2}", settings.tag_show, settings.tag_margin);
            println!("языки OCR: {}", settings.ocr_lang);
            println!(
                "tesseract: {}",
                derive::tesseract_version().unwrap_or_else(|| "не найден в PATH".into())
            );
            Ok(())
        }
        "clusters" => {            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let k = flag_num(&args, "--k", 30);
            let out = flag_str(&args, "--out")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("plan/M3-clusters.md"));
            let stats = embed::write_clusters(&db_path, &out, k)?;
            println!(
                "кластеров: {} на {} фото, за {:.1} с → {}",
                stats.clusters,
                stats.photos,
                stats.seconds,
                stats.out.display()
            );
            Ok(())
        }
        "doctor" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let conn = db::open(&db_path)?;
            let settings = db::load_settings(&conn);

            println!("внешние программы (ищутся в PATH, без них отключается только своя работа):");
            let tools: [(&str, &[&str], &str); 4] = [
                ("ffmpeg", &["-version"], "видео: кадр-обложка, dHash, превью"),
                ("ffprobe", &["-version"], "видео: длительность и разрешение"),
                ("tesseract", &["--version"], "OCR: текст с документов и скриншотов"),
                ("curl", &["--version"], "скачивание весов модели по согласию"),
            ];
            for (prog, vargs, purpose) in tools {
                let state = match derive::tool_version(prog, vargs) {
                    Some(v) => short_version(&v),
                    None => "не найден".to_string(),
                };
                println!("  {prog:<10} {state:<30} {purpose}");
            }

            match derive::tesseract_langs() {
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
                        println!("языки OCR: {} — на месте", settings.ocr_lang);
                    } else {
                        println!(
                            "языки OCR: {} — нет {}; установите языковые пакеты tesseract для них",
                            settings.ocr_lang,
                            missing.join(", ")
                        );
                    }
                    println!("  доступны: {}", langs.join(", "));
                }
                None => println!("языки OCR: не проверить — tesseract не найден"),
            }

            let entry = embed::active_entry(&settings);
            println!(
                "модель CLIP: {} · {} · {}",
                entry.title,
                db::human(entry.bytes() as i64),
                if embed::model_present(&settings) {
                    "на месте".to_string()
                } else {
                    "не скачана — смысловой поиск и теги недоступны".to_string()
                }
            );
            let (ocr_files, ocr_chars) = db::ocr_summary(&conn)?;
            let total: i64 = conn.query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))?;
            let size = std::fs::metadata(&db_path).map(|m| m.len() as i64).unwrap_or(0);
            println!(
                "БД: {} · {} · файлов {}, с текстом {} (знаков {})",
                db_path.display(),
                db::human(size),
                total,
                ocr_files,
                ocr_chars
            );
            Ok(())
        }
        "gui" | "" => app::run(flag_str(&args, "--db").map(PathBuf::from)),
        "-h" | "--help" | "help" => {
            usage();
            Ok(())
        }
        other => {
            eprintln!("неизвестная подкоманда: {other}\n");
            usage();
            std::process::exit(2);
        }
    }
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
