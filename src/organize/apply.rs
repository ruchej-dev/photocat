//! Creating the showcase: a hardlink, a copy or a symlink per item, never a move.

use super::plan::{LinkMode, PlanItem};
use anyhow::{Context, Result};
use std::path::Path;

/// A symbolic link to a file. Windows needs developer mode or administrator rights; without
/// them the error lands in the `fail` counter and does not take the whole plan down.
#[cfg(unix)]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(src, dst)
}

#[cfg(windows)]
fn symlink(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(src, dst)
}

pub fn apply(plan: &[PlanItem], mode: LinkMode) -> Result<(usize, usize)> {
    let mut ok = 0;
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
