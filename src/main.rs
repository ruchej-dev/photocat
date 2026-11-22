//! photocat — каталогизатор медиаархива: индексация, дубли, раскладка по датам, ML-теги.
//!
//! Подкоманды:
//!   photocat index <каталог> [--jobs N] [--db PATH]
//!   photocat stats
//!   photocat dupes [--max-dist 6] [--csv FILE] [--plan FILE]
//!   photocat organize --out <каталог> [--apply] [--copy|--symlink] [--rename]
//!   photocat embed [--jobs N] [--batch N]
//!   photocat search <запрос> [--top N]
//!   photocat clusters [--k N] [--out FILE]
//!   photocat            (без аргументов — GUI)

mod app;
mod db;
mod derive;
mod dupes;
mod embed;
mod index;
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
  search <запрос> [--top N]                 смысловой поиск по эмбеддингам
  clusters [--k N] [--out FILE]             кластеры CLIP → отчёт с примерами
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
            let jobs = flag_num(&args, "--jobs", 8);
            let db_path = flag_str(&args, "--db").map(PathBuf::from);
            let progress = Arc::new(Progress::default());
            let db_path = db::resolve_db(db_path.as_deref())?;
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
            let repeat = flag_num(&args, "--repeat", 1).max(1);
            eprintln!("загрузка модели CLIP…");
            let mut model = embed::Model::load(flag_num(&args, "--jobs", 8))?;
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
        "clusters" => {
            let db_path = db::resolve_db(flag_str(&args, "--db").map(PathBuf::from).as_deref())?;
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
