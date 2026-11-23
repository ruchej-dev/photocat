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
//!   photocat albums list|new|rename|move|add|remove|delete|show
//!   photocat export --album N --out DIR [--name ШАБЛОН] [--apply]
//!   photocat            (без аргументов — GUI)

mod app;
mod db;
mod derive;
mod dupes;
mod embed;
mod faces;
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
  organize --out DIR [--by day|month|event] [--name ШАБЛОН] [--exclude-dup] [--apply]
                                            план (и применение) витрины: по дням, месяцам или
                                            событиям; имена из {{date}} {{time}} {{camera}} {{tag}}
  verify <a> <b> [--jobs N] [--all]        сверка двух копий: размер → sha256, отчёт
  verify --manifest FILE <каталог>         проверить каталог по слепку sha256
  manifest [DIR] [--out FILE] [--jobs N]  слепок каталога в формате sha256sum
  embed [--jobs N] [--batch N] [--refresh] [--retag]  CLIP-эмбеддинги и теги (модель локальная);
                                            --refresh пересчитывает векторы, --retag — только теги
  tags [--list] [--add МЕТКА --prompt ТЕКСТ] [--enable ID] [--disable ID] [--rm ID]
       [--export FILE] [--import FILE] [--reset]
                                            словарь тегов модели: метки, промпты, порядок
  faces [--jobs N] [--threshold X] [--dry-run] [--recluster] [--list]
                                            лица: детекция, эмбеддинги, группы
  faces download --yes | label --id N --name ИМЯ | split --id N
                                            веса лиц, подпись группы, разъединить лицо
  ocr [--jobs N] [--all] [--dry-run] [--file PATH]
                                            текст с документов и скриншотов (tesseract)
  search <запрос> [--top N] [--text]        смысловой поиск или поиск по тексту на фото
  clusters [--k N] [--out FILE]             кластеры CLIP → отчёт с примерами
  albums list [--tree] | new --name ИМЯ [--parent ID] [--note ТЕКСТ]
         | rename --id N --name ИМЯ | move --id N --parent ID [--position K]
         | add|remove --id N PATH... | delete --id N [--yes] | show --id N [--tree]
                                            альбомы: дерево и порядок файлов; только строки БД
  export --album N --out DIR [--name ШАБЛОН] [--apply]
                                            выгрузить альбом копиями файлов;
                                            структура и порядок как в альбоме, {{n}}_{{orig}}
  models [list|check|download --id ID --yes]
                                            каталог моделей, проверка и загрузка весов
  settings [--models-dir DIR] [--model ID] [--max-model-mb N] [--ocr-lang L]
           [--face-threshold X] [--ffmpeg PATH] [--ffprobe PATH] [--tesseract PATH] [--curl PATH]
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
            let settings = {
                let conn = db::open(&db_path)?;
                db::load_settings(&conn)
            };
            let jobs = flag_num(&args, "--jobs", settings.index_jobs as u64);
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
            let stats = index::run(&root, jobs, &db_path, &settings.tools, Some(progress))?;
            println!("{stats}");
            Ok(())
        }
        "stats" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            db::print_stats(&db_path)
        }
        "dupes" => dupes::cli(&args[1..]),
        "organize" => organize::cli(&args[1..]),
        "verify" => verify::cli(&args[1..]),
        "manifest" => verify::manifest_cli(&args[1..]),
        "play" => player::cli(&args[1..]),
        "embed" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
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
            let refresh = args.iter().any(|a| a == "--refresh");
            let stats = embed::embed_missing(&db_path, jobs, batch, Some(progress), refresh)?;
            println!("{stats}");
            Ok(())
        }
        "faces" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
            let settings = {
                let conn = db::open(&db_path)?;
                db::load_settings(&conn)
            };
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
                        eprintln!("нужны номер группы и имя: faces label --id N --name ИМЯ");
                        std::process::exit(2);
                    }
                    faces::label(&db_path, id, &label)?;
                    println!("группа {id}: «{}»", label.trim());
                    return Ok(());
                }
                "split" => {
                    let id = flag_str(&args, "--id")
                        .and_then(|v| v.parse::<i64>().ok())
                        .unwrap_or(0);
                    if id == 0 {
                        eprintln!("нужен номер лица: faces split --id N");
                        std::process::exit(2);
                    }
                    let cid = faces::split(&db_path, id)?;
                    println!("лицо {id} отделено в группу {cid}");
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
                    "на месте".to_string()
                } else {
                    "не скачана — «Настройки» или faces download --yes".to_string()
                };
                println!(
                    "снимков: {}, ещё без лиц: {}; модель лиц: {model}",
                    plan.images, plan.todo
                );
                return Ok(());
            }
            if has_flag(&args, "--list") {
                let conn = db::open(&db_path)?;
                let clusters = db::load_face_clusters(&conn)?;
                if clusters.is_empty() {
                    println!("групп лиц нет: запустите photocat faces");
                }
                for c in clusters {
                    println!(
                        "{} {:<24} лиц {}, фото {}",
                        c.id,
                        if c.label.is_empty() { "(без имени)" } else { c.label.as_str() },
                        c.faces,
                        c.photos
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
                        eprint!("\rлица: {d}/{t}, ошибок {e} — {}", show.msg());
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
            let jobs = flag_num(&args, "--jobs", 4);
            let all = has_flag(&args, "--all");
            if has_flag(&args, "--dry-run") {
                let plan = ocr::plan(&db_path, all)?;
                let settings = {
                    let conn = db::open(&db_path)?;
                    db::load_settings(&conn)
                };
                println!(
                    "кандидатов: {} из {} снимков; tesseract ({}): {}",
                    plan.candidates,
                    plan.images,
                    settings.tools.source(tools::Tool::Tesseract),
                    settings
                        .tools
                        .probe(tools::Tool::Tesseract)
                        .map(|v| short_version(&v))
                        .unwrap_or_else(|| "не найден".into())
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
                        println!("{}: {}", tool.name(), short_version(&v));
                        settings.tools = probe;
                        changed = true;
                    }
                    Err(e) => {
                        eprintln!("не сохранён: {e:#}");
                        std::process::exit(2);
                    }
                }
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
            println!("тег: уверенный ≥ {:.2}, отрыв {:.2}", settings.tag_show, settings.tag_margin);
            println!("фильтр по тегу: порог {}% (процентиль)", settings.tag_show_pct);
            println!("языки OCR: {}", settings.ocr_lang);
            println!("порог лиц: {:.2}", settings.face_threshold);
            for tool in tools::ALL {
                println!(
                    "{:<10} {} ({})",
                    tool.name(),
                    settings
                        .tools
                        .probe(tool)
                        .map(|v| short_version(&v))
                        .unwrap_or_else(|| "не найден".into()),
                    settings.tools.source(tool)
                );
            }
            Ok(())
        }
        "albums" => albums_cli(&args[1..]),
        "export" => organize::export_cli(&args[1..]),
        "tags" => tags_cli(&args[1..]),
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

            println!(
                "внешние программы (пусто в настройках — ищутся в PATH; без программы \
                 отключается только её работа):"
            );
            for tool in tools::ALL {
                let state = match settings.tools.version(tool) {
                    Ok(v) => short_version(&v),
                    Err(e) => format!("не работает: {e:#}"),
                };
                let found = match settings.tools.locate(tool) {
                    Some(p) => format!("{} ({})", p.display(), settings.tools.source(tool)),
                    None => format!(
                        "не найдена: {} ({})",
                        settings.tools.program(tool),
                        settings.tools.source(tool)
                    ),
                };
                println!("  {:<10} {}", tool.name(), state);
                println!("  {:<10} {}", "", found);
                println!(
                    "  {:<10} зачем: {}; без неё: {}",
                    "",
                    tool.purpose(),
                    tool.without()
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
            println!(
                "модель лиц: {} · {} · {}",
                faces::MODEL_ID,
                db::human(faces::model_bytes(&settings)),
                if faces::model_present(&settings) {
                    "на месте".to_string()
                } else {
                    "не скачана — «Настройки» или faces download --yes".to_string()
                }
            );
            let (face_faces, face_photos, face_clusters) = db::face_summary(&conn)?;
            let (face_images, face_todo) = db::face_scan_plan(&conn)?;
            println!(
                "лица: {} на {} снимках, групп {}, ещё без лиц {} из {}",
                face_faces, face_photos, face_clusters, face_todo, face_images
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

/// `photocat albums …` — альбомы как данные: дерево, порядок и строки в БД.
/// Ни одна команда не открывает файл по пути и не пишет на диск: PATH — ключ
/// `files.path`, а не файловая операция. Коды: 0 — сделано, 1 — ошибка данных,
/// 2 — неверные аргументы или отказ без подтверждения.
fn albums_cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    let cmd = args.first().map(String::as_str).unwrap_or("list");
    match cmd {
        "list" => {
            let tree = db::albums_tree(&conn)?;
            if tree.is_empty() {
                println!("альбомов нет — создайте: photocat albums new --name \"Дача 2024\"");
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
                    format!(" · в корзине {}", node.trashed)
                } else {
                    String::new()
                };
                println!(
                    "{:>4}  {indent}{path}  · файлов {} · подальбомов {} · {}{}",
                    node.id,
                    node.files,
                    node.subalbums,
                    db::human(node.bytes),
                    trashed
                );
            }
            Ok(())
        }
        "new" => {
            let name = match flag_str(args, "--name") {
                Some(n) => n,
                None => {
                    eprintln!("нужно имя: photocat albums new --name \"Дача 2024\" [--parent ID]");
                    std::process::exit(2);
                }
            };
            let parent = flag_str(args, "--parent")
                .and_then(|v| v.parse::<i64>().ok())
                .unwrap_or(0);
            let note = flag_str(args, "--note").unwrap_or_default();
            let id = db::create_album(&conn, parent, &name, &note)?;
            println!(
                "альбом создан: {} (id {id})",
                db::album_path(&conn, id)?
            );
            Ok(())
        }
        "rename" => {
            let id = album_id_arg(args);
            let name = match flag_str(args, "--name") {
                Some(n) => n,
                None => {
                    eprintln!("нужно имя: photocat albums rename --id N --name ИМЯ");
                    std::process::exit(2);
                }
            };
            db::rename_album(&conn, id, &name)?;
            println!("альбом переименован: {}", db::album_path(&conn, id)?);
            Ok(())
        }
        "move" => {
            let id = album_id_arg(args);
            let Some(parent) = flag_str(args, "--parent").and_then(|v| v.parse::<i64>().ok()) else {
                eprintln!("нужен родитель: photocat albums move --id N --parent ID [--position K]");
                std::process::exit(2);
            };
            let position = flag_str(args, "--position").and_then(|v| v.parse::<i64>().ok());
            db::move_album(&conn, id, parent, position)?;
            println!("альбом перемещён: {}", db::album_path(&conn, id)?);
            Ok(())
        }
        "add" | "remove" => {
            let id = album_id_arg(args);
            let paths = positional_paths(args, &["--id", "--db"]);
            if paths.is_empty() {
                eprintln!("нужны пути: photocat albums {cmd} --id N PATH...");
                std::process::exit(2);
            }
            let file_ids = file_ids_for_paths(&conn, &paths)?;
            let path = db::album_path(&conn, id)?;
            if cmd == "add" {
                let (added, already) = db::add_to_album(&conn, id, &file_ids)?;
                println!(
                    "альбом «{path}»: добавлено {added}, уже было {already}, файлов {}",
                    db::album_items(&conn, id)?.len()
                );
            } else {
                let removed = db::remove_from_album(&conn, id, &file_ids)?;
                println!(
                    "альбом «{path}»: убрано {removed}, осталось {}",
                    db::album_items(&conn, id)?.len()
                );
            }
            Ok(())
        }
        "delete" => {
            let id = album_id_arg(args);
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
                    "будет удалено: {} — альбомов {}, строк файлов {}; файлы на диске останутся.                      Повторите с --yes.",
                    names.join(", "),
                    subtree.len(),
                    rows
                );
                std::process::exit(2);
            }
            let albums = db::delete_album(&conn, id)?;
            println!(
                "удалено альбомов {albums}, строк файлов {rows}; файлы на диске не тронуты"
            );
            Ok(())
        }
        "show" => {
            let id = album_id_arg(args);
            let path = db::album_path(&conn, id)?;
            let tree = db::albums_tree(&conn)?;
            let node = tree
                .iter()
                .find(|n| n.id == id)
                .with_context(|| format!("альбома {id} нет"))?;
            println!(
                "{} (id {id}) · файлов {} · подальбомов {} · {}{}",
                path,
                node.files,
                node.subalbums,
                db::human(node.bytes),
                if node.note.trim().is_empty() {
                    String::new()
                } else {
                    format!(" · {}", node.note.trim())
                }
            );
            if has_flag(args, "--tree") {
                for child in tree.iter().filter(|n| n.parent_id == id) {
                    println!(
                        "  подальбом {} · файлов {} · {}",
                        child.name,
                        child.files,
                        db::human(child.bytes)
                    );
                }
            }
            for (position, row) in db::album_items(&conn, id)?.iter().enumerate() {
                let mark = if row.trashed_at.is_some() {
                    "  [в корзине приложения]"
                } else {
                    ""
                };
                println!("{:>4}  {}{mark}", position + 1, row.path);
            }
            Ok(())
        }
        other => {
            eprintln!(
                "неизвестная команда albums: {other}\n\
                 list [--tree] | new | rename | move | add | remove | delete | show"
            );
            std::process::exit(2);
        }
    }
}

fn album_id_arg(args: &[String]) -> i64 {
    match flag_str(args, "--id").and_then(|v| v.parse::<i64>().ok()) {
        Some(id) => id,
        None => {
            eprintln!("нужен номер альбома: --id N");
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
        db::load_settings(&conn)
    };
    let entry = embed::active_entry(&settings);
    let mut defs = embed::ensure_tag_defs(&db_path, entry)?;
    let source = embed::tag_source(entry);
    let list = || -> Result<()> {
        let conn = db::open(&db_path)?;
        let counts = db::tag_file_counts(&conn, &source)?;
        println!("словарь модели {} — {} тегов", entry.id, defs.len());
        for d in &defs {
            println!(
                "  {:<10} {:<16} {}{} · файлов {} · промптов {}",
                d.id,
                d.label,
                if d.enabled { "ок" } else { "выкл" },
                if d.min_percentile > 0 {
                    format!(", порог {}%", d.min_percentile)
                } else {
                    String::new()
                },
                counts.get(&d.label).copied().unwrap_or(0),
                d.prompts.len()
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
            anyhow::bail!("--add: нужна непустая метка");
        }
        if defs.iter().any(|d| d.label == label) {
            anyhow::bail!("тег «{label}» уже есть в словаре");
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
            anyhow::bail!("--add: нужен хотя бы один --prompt «фотография …»");
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
            "тег «{}» добавлен (id {}) — пересчитайте веса: photocat embed --retag",
            d.label, d.id
        );
        return Ok(());
    }

    if let Some(which) = flag_str(args, "--enable").or_else(|| flag_str(args, "--disable")) {
        let enable = has_flag(args, "--enable");
        let want = which.trim();
        let conn = db::open(&db_path)?;
        let d = defs
            .iter_mut()
            .find(|d| d.id == want || d.label == want)
            .with_context(|| format!("нет тега «{want}»"))?;
        d.enabled = enable;
        db::save_tag_def(&conn, d)?;
        println!(
            "тег «{}» {}",
            d.label,
            if enable { "включён" } else { "выключен" }
        );
        return Ok(());
    }

    if let Some(which) = flag_str(args, "--rm") {
        let want = which.trim();
        let conn = db::open(&db_path)?;
        let d = defs
            .iter()
            .find(|d| d.id == want || d.label == want)
            .with_context(|| format!("нет тега «{want}»"))?;
        db::delete_tag_def(&conn, entry.id, &d.id)?;
        println!(
            "тег «{}» убран из словаря — веса исчезнут после photocat embed --retag",
            d.label
        );
        return Ok(());
    }

    if let Some(path) = flag_str(args, "--export") {
        std::fs::write(&path, embed::tag_defs_to_json(&defs))
            .with_context(|| format!("не записать {path}") )?;
        println!("словарь выгружен: {path} ({} тегов)", defs.len());
        return Ok(());
    }

    if let Some(path) = flag_str(args, "--import") {
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("не прочитать {path}"))?;
        let defs = embed::tag_defs_from_json(entry.id, &text)?;
        embed::import_defs(&db_path, entry.id, &defs)?;
        println!(
            "словарь загружен: {} тегов — пересчитайте веса: photocat embed --retag",
            defs.len()
        );
        return Ok(());
    }

    if has_flag(args, "--reset") {
        let defs = embed::reset_to_defaults(&db_path, entry)?;
        println!(
            "словарь возвращён к стандартному ({} тегов) — photocat embed --retag",
            defs.len()
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
