//! `photocat albums` — albums as data: the tree, the order and the rows in the DB.

use crate::db;
use crate::i18n::{self, Lang};
use crate::{flag_str, has_flag};
use anyhow::{Context, Result};
use std::path::PathBuf;

/// `photocat albums …` — albums as data: the tree, the order and the rows in the DB.
/// No command opens a file by path or writes to disk: PATH is a `files.path` key, not a
/// file operation. Exit codes: 0 — done, 1 — a data error, 2 — wrong arguments or a refusal
/// without a confirmation.
pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let lang = db::load_settings(&conn).lang;
    let cmd = args.first().map(String::as_str).unwrap_or("list");
    match cmd {
        "list" => list(&conn, args, lang),
        "new" => new(&conn, args, lang),
        "rename" => rename(&conn, args, lang),
        "move" => move_album(&conn, args, lang),
        "add" | "remove" => add_remove(&conn, args, lang, cmd),
        "delete" => delete(&conn, args, lang),
        "show" => show(&conn, args, lang),
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

/// The tree, one line per album; `--tree` indents the children.
fn list(conn: &rusqlite::Connection, args: &[String], lang: Lang) -> Result<()> {
    let tree = db::albums_tree(conn)?;
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
        let path = db::album_path(conn, node.id)?;
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

fn new(conn: &rusqlite::Connection, args: &[String], lang: Lang) -> Result<()> {
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
    let id = db::create_album(conn, parent, &name, &note)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "album created: {} (id {})",
            &[db::album_path(conn, id)?, id.to_string()]
        )
    );
    Ok(())
}

fn rename(conn: &rusqlite::Connection, args: &[String], lang: Lang) -> Result<()> {
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
    db::rename_album(conn, id, &name)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "album renamed: {}",
            &[db::album_path(conn, id)?]
        )
    );
    Ok(())
}

fn move_album(conn: &rusqlite::Connection, args: &[String], lang: Lang) -> Result<()> {
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
    db::move_album(conn, id, parent, position)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "album moved: {}",
            &[db::album_path(conn, id)?]
        )
    );
    Ok(())
}

/// Add the positional paths to the album or remove them; `--id` names the album.
fn add_remove(conn: &rusqlite::Connection, args: &[String], lang: Lang, cmd: &str) -> Result<()> {
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
    let file_ids = file_ids_for_paths(conn, &paths)?;
    let path = db::album_path(conn, id)?;
    if cmd == "add" {
        let (added, already) = db::add_to_album(conn, id, &file_ids)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "album “{}”: {} added, {} already there, {} files",
                &[
                    path,
                    added.to_string(),
                    already.to_string(),
                    db::album_items(conn, id)?.len().to_string(),
                ]
            )
        );
    } else {
        let removed = db::remove_from_album(conn, id, &file_ids)?;
        println!(
            "{}",
            i18n::fill(
                lang,
                "album “{}”: {} removed, {} left",
                &[
                    path,
                    removed.to_string(),
                    db::album_items(conn, id)?.len().to_string(),
                ]
            )
        );
    }
    Ok(())
}

/// Delete the album and its subtree; rows only, the files stay on disk.
fn delete(conn: &rusqlite::Connection, args: &[String], lang: Lang) -> Result<()> {
    let id = album_id_arg(args, lang);
    let tree = db::albums_tree(conn)?;
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
    let albums = db::delete_album(conn, id)?;
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

fn show(conn: &rusqlite::Connection, args: &[String], lang: Lang) -> Result<()> {
    let id = album_id_arg(args, lang);
    let path = db::album_path(conn, id)?;
    let tree = db::albums_tree(conn)?;
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
    for (position, row) in db::album_items(conn, id)?.iter().enumerate() {
        let mark = if row.trashed_at.is_some() {
            i18n::t(lang, "  [in the app trash]")
        } else {
            ""
        };
        println!("{:>4}  {}{mark}", position + 1, row.path);
    }
    Ok(())
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
