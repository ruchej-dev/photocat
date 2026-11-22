//! Раскладка архива по датам: план → подтверждение → применение (hardlink/copy/symlink).
//! Оригиналы не удаляются и не переименовываются, пока не передан --rename.

use crate::db::{self, FileRow};
use crate::{flag_str, has_flag};
use anyhow::{Context, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq)]
pub enum LinkMode {
    Hard,
    Copy,
    Symlink,
}

pub struct PlanItem {
    pub src: PathBuf,
    pub dst: PathBuf,
}

pub fn build_plan(rows: &[FileRow], out: &Path, rename: bool) -> Vec<PlanItem> {
    let mut used: BTreeMap<String, u32> = BTreeMap::new();
    let mut plan = Vec::new();
    for r in rows {
        let Some(taken) = r.taken_at.as_deref() else {
            continue;
        };
        let (date, time) = taken.split_once('T').unwrap_or((taken, "000000"));
        let mut it = date.split('-');
        let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else {
            continue;
        };
        let src = PathBuf::from(&r.path);
        let file_name = src
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let name = if rename {
            let stem = src.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let ext = src.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let safe: String = stem
                .chars()
                .map(|c| if c.is_whitespace() { '_' } else { c })
                .filter(|c| c.is_alphanumeric() || "-_.".contains(*c))
                .take(40)
                .collect();
            let t: String = time.chars().filter(|c| c.is_ascii_digit()).collect();
            let t = &t[..t.len().min(6)];
            if safe.is_empty() {
                format!("{y}{m}{d}_{t}.{ext}")
            } else {
                format!("{y}{m}{d}_{t}_{safe}.{ext}")
            }
        } else {
            file_name
        };
        let dir = out.join(y).join(format!("{y}-{m}-{d}"));
        let mut dst = dir.join(&name);
        // коллизии имён в одном каталоге
        let key = dst.to_string_lossy().to_string();
        let n = used.entry(key.clone()).or_insert(0);
        *n += 1;
        if *n > 1 {
            let stem = dst.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            let ext = dst.extension().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
            dst.set_file_name(format!("{stem}-{n}.{ext}"));
        }
        plan.push(PlanItem { src, dst });
    }
    plan.sort_by(|a, b| a.dst.cmp(&b.dst));
    plan
}

/// Символическая ссылка на файл. На Windows нужен режим разработчика или права администратора;
/// без них ошибка попадает в счётчик `fail`, а не роняет применение плана.
#[cfg(unix)]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

pub fn apply(plan: &[PlanItem], mode: LinkMode) -> Result<(usize, usize)> {    let mut ok = 0;
    let mut fail = 0;
    for it in plan {
        let res = (|| -> Result<()> {
            if let Some(dir) = it.dst.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if it.dst.exists() {
                return Ok(());
            }
            match mode {
                LinkMode::Hard => std::fs::hard_link(&it.src, &it.dst)
                    .with_context(|| format!("hardlink {}", it.dst.display()))?,
                LinkMode::Copy => {
                    std::fs::copy(&it.src, &it.dst)
                        .with_context(|| format!("copy {}", it.dst.display()))?;
                }
                LinkMode::Symlink => {
                    symlink(&it.src, &it.dst)
                        .with_context(|| format!("symlink {}", it.dst.display()))?;
                }
            }
            Ok(())
        })();
        match res {
            Ok(()) => ok += 1,
            Err(e) => {
                fail += 1;
                eprintln!("{e}");
            }
        }
    }
    Ok((ok, fail))
}

pub fn cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let out = match flag_str(args, "--out") {
        Some(o) => PathBuf::from(o),
        None => {
            eprintln!("нужен каталог назначения: organize --out DIR [--apply]");
            std::process::exit(2);
        }
    };
    let mode = if has_flag(args, "--copy") {
        LinkMode::Copy
    } else if has_flag(args, "--symlink") {
        LinkMode::Symlink
    } else {
        LinkMode::Hard
    };
    let rename = has_flag(args, "--rename");
    let apply_it = has_flag(args, "--apply");

    let conn = db::open(&db_path)?;
    let rows = db::load_all(&conn)?;
    let plan = build_plan(&rows, &out, rename);

    let mut by_year: BTreeMap<String, usize> = BTreeMap::new();
    for it in &plan {
        if let Some(y) = it.dst.parent().and_then(|p| p.parent()) {
            *by_year.entry(y.file_name().unwrap_or_default().to_string_lossy().to_string()).or_insert(0) += 1;
        }
    }
    println!("файлов с датой: {}, без даты (не попадут в план): {}",
        plan.len(),
        rows.len() - plan.len());
    for (y, n) in &by_year {
        println!("  {y}: {n}");
    }
    println!("\nпримеры:");
    for it in plan.iter().take(5) {
        println!("  {}  →  {}", it.src.display(), it.dst.display());
    }
    if !apply_it {
        println!("\nэто план (dry-run). Добавьте --apply, чтобы создать ссылки/копии в {}", out.display());
        return Ok(());
    }
    let (ok, fail) = apply(&plan, mode)?;
    println!("создано: {ok}, ошибок: {fail}");
    Ok(())
}
