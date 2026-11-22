//! photocat — каталогизатор медиаархива: индексация, дубли, раскладка по датам, ML-теги.
//!
//! Подкоманды:
//!   photocat index <каталог> [--jobs N] [--db PATH]
//!   photocat stats
//!   photocat dupes [--max-dist 6] [--csv FILE]
//!   photocat organize --out <каталог> [--apply] [--copy|--symlink] [--rename]
//!   photocat            (без аргументов — GUI)

mod app;
mod db;
mod derive;
mod dupes;
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
  dupes [--max-dist N] [--csv FILE]        найти дубли и похожие снимки
  organize --out DIR [--apply] [--copy]    план (и применение) раскладки по датам
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
