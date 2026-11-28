//! Comparing two copies of an archive and the `sha256sum` manifest.
//!
//! Both commands only read the archive. The only file created is a manifest on an explicit `--out`.
//! Sizes are compared first: if they differ there is no point in reading the file. sha256 is
//! computed only for files of equal size, so `verify` of two copies reads both trees whole
//! instead of reading obviously different files "just in case".
//!
//! Split by command (M27.5): `compare.rs` holds `verify`, `manifest.rs` the manifest, and this
//! module the report helpers both share.

mod compare;
mod manifest;

#[cfg(test)]
mod tests;

pub use compare::*;
pub use manifest::*;

use crate::db::{self, Progress};
use crate::i18n::{self, Lang};
use crate::{flag_num, flag_str, has_flag};
use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub(super) fn status_line(prefix: &str, lang: Lang, p: Arc<Progress>) {
    std::thread::sleep(std::time::Duration::from_millis(300));
    loop {
        let (d, t, e) = (p.done(), p.total(), p.errors());
        if t > 0 {
            let line = i18n::fill(
                lang,
                "{}/{} files, {} errors — {}",
                &[d.to_string(), t.to_string(), e.to_string(), p.msg()],
            );
            eprint!("\r{prefix}: {line}");
        } else {
            eprint!("\r{prefix}: {}", p.msg());
        }
        if p.finished() {
            eprintln!();
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(200));
    }
}

pub(super) fn positional(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip = false;
    for a in args {
        if skip {
            skip = false;
            continue;
        }
        if a == "--jobs" || a == "--manifest" || a == "--out" {
            skip = true;
            continue;
        }
        if a.starts_with('-') {
            continue;
        }
        out.push(a.clone());
    }
    out
}

pub(super) fn write_list(s: &mut String, lang: Lang, title: &'static str, list: &[String], all: bool) {
    use std::fmt::Write;
    if list.is_empty() {
        return;
    }
    let _ = writeln!(s, "\n{}:", i18n::t(lang, title));
    let limit = if all { list.len() } else { 20 };
    for path in list.iter().take(limit) {
        let _ = writeln!(s, "  {path}");
    }
    if list.len() > limit {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "  … {} more (show all)",
                &[(list.len() - limit).to_string()]
            )
        );
    }
}

pub(super) fn write_size_diff(s: &mut String, lang: Lang, list: &[(String, u64, u64)], all: bool) {
    use std::fmt::Write;
    if list.is_empty() {
        return;
    }
    let _ = writeln!(s, "\n{}:", i18n::t(lang, "size differs"));
    let limit = if all { list.len() } else { 20 };
    for (path, a, b) in list.iter().take(limit) {
        let delta = *b as i64 - *a as i64;
        let bytes = format!(
            "{}{} {}",
            if delta >= 0 { "+" } else { "" },
            delta,
            i18n::units(lang)[0]
        );
        let _ = writeln!(
            s,
            "  {}",
            i18n::fill(
                lang,
                "{}  A {}  B {}  ({})",
                &[
                    path.clone(),
                    db::human_in(lang, *a as i64),
                    db::human_in(lang, *b as i64),
                    bytes,
                ]
            )
        );
    }
    if list.len() > limit {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "  … {} more (show all)",
                &[(list.len() - limit).to_string()]
            )
        );
    }
}

pub fn cli(args: &[String], lang: Lang) -> Result<()> {
    let jobs = flag_num(args, "--jobs", 4).max(1);
    let all = has_flag(args, "--all");
    if let Some(file) = flag_str(args, "--manifest") {
        let root = match positional(args).first() {
            Some(p) => PathBuf::from(p),
            None => {
                eprintln!(
                    "{}",
                    i18n::t(lang, "a folder is needed: photocat verify --manifest FILE DIR")
                );
                std::process::exit(2);
            }
        };
        let progress = Arc::new(Progress::default());
        let show = progress.clone();
        std::thread::spawn(move || status_line(i18n::t(lang, "checking"), lang, show));
        let mut report = check_manifest(&root, Path::new(&file), jobs, lang, Some(progress))?;
        report.lang = lang;
        print!("{}", check_report(Path::new(&file), &root, &report, all));
        if !report.identical() {
            std::process::exit(1);
        }
        return Ok(());
    }
    let pos = positional(args);
    if pos.len() != 2 {
        eprintln!(
            "{}",
            i18n::t(lang, "two folders are needed: photocat verify A B [--jobs N] [--all]")
        );
        std::process::exit(2);
    }
    let (a, b) = (PathBuf::from(&pos[0]), PathBuf::from(&pos[1]));
    let progress = Arc::new(Progress::default());
    let show = progress.clone();
    std::thread::spawn(move || status_line(i18n::t(lang, "comparing"), lang, show));
    let mut report = verify(&a, &b, jobs, lang, Some(progress))?;
    report.lang = lang;
    print!("{}", verify_report(&a, &b, &report, all));
    if !report.identical() {
        std::process::exit(1);
    }
    Ok(())
}
