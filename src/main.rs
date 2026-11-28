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
mod cli;
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

pub use cli::{flag_num, flag_str, has_flag};

use anyhow::Result;
use std::path::PathBuf;

fn main() -> Result<()> {
    cli::quiet_broken_pipe();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let cmd = args.first().map(String::as_str).unwrap_or("gui");
    match cmd {
        "index" => cli::index::run(&args),
        "stats" => cli::stats::run(&args),
        "dupes" => dupes::cli(&args[1..]),
        "organize" => organize::cli(&args[1..]),
        "verify" => verify::cli(&args[1..], cli::language(&args)?),
        "manifest" => verify::manifest_cli(&args[1..], cli::language(&args)?),
        "play" => player::cli(&args[1..]),
        "embed" => cli::embed::run(&args),
        "faces" => cli::faces::run(&args),
        "ocr" => cli::ocr::run(&args),
        "search" => cli::search::run(&args),
        "models" => cli::models::run(&args),
        "settings" => cli::settings::run(&args),
        "albums" => cli::albums::run(&args[1..]),
        "export" => organize::export_cli(&args[1..]),
        "tags" => cli::tags::run(&args[1..]),
        "eval" => cli::eval::run(&args),
        "clusters" => cli::clusters::run(&args),
        "doctor" => cli::doctor::run(&args),
        "gui" | "" => app::run(flag_str(&args, "--db").map(PathBuf::from)),
        "-h" | "--help" | "help" => {
            cli::usage(cli::language(&args).unwrap_or_default());
            Ok(())
        }
        other => {
            let lang = cli::language(&args).unwrap_or_default();
            eprintln!(
                "{}",
                i18n::fill(lang, "unknown subcommand: {}", &[other.to_string()])
            );
            eprintln!();
            cli::usage(lang);
            std::process::exit(2);
        }
    }
}
