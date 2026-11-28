//! Comparing two copies: the walk, the size-first pass and the sha256 of equal sizes.

use super::manifest::rel_display;
use super::{write_list, write_size_diff};
use crate::db::{self, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Default)]
pub struct Scan {
    /// Relative path → file. A BTreeMap gives the report a stable order.
    pub files: BTreeMap<String, Entry>,
    /// Symlinks and other non-files: not compared.
    pub skipped: usize,
    /// Folders or files that could not be read during the walk.
    pub errors: usize,
}

pub struct Report {
    pub files_a: usize,
    pub files_b: usize,
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
    /// The path and the sizes in A and B; no sha256 is computed for them.
    pub size_diff: Vec<(String, u64, u64)>,
    pub content_diff: Vec<String>,
    pub read_errors: Vec<String>,
    pub same: usize,
    /// How many files of equal size actually went through sha256.
    pub hashed: usize,
    /// The size of one copy of those files (read twice — in A and in B).
    pub hashed_bytes: u64,
    /// Files of equal size not checked because of the stop.
    pub unchecked: usize,
    pub scan_errors: usize,
    pub skipped_a: usize,
    pub skipped_b: usize,
    pub cancelled: bool,
    pub seconds: f64,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl Report {
    pub fn errors(&self) -> usize {
        self.scan_errors + self.read_errors.len()
    }

    pub fn identical(&self) -> bool {
        self.only_a.is_empty()
            && self.only_b.is_empty()
            && self.size_diff.is_empty()
            && self.content_diff.is_empty()
            && self.errors() == 0
            && !self.cancelled
            && self.unchecked == 0
    }
}

/// A tree walk: regular files only, the path relative to the root. Symlinks are not followed.
pub fn scan(root: &Path) -> Result<Scan> {
    scan_excluding(root, None)
}

pub(super) fn scan_excluding(root: &Path, exclude: Option<&Path>) -> Result<Scan> {
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("cannot open the folder {}", root.display()))?;
    let mut scan = Scan::default();
    for e in walkdir::WalkDir::new(&root).follow_links(false) {
        let e = match e {
            Ok(e) => e,
            Err(err) => {
                scan.errors += 1;
                eprintln!("skipped during the walk: {err}");
                continue;
            }
        };
        if e.file_type().is_symlink() {
            scan.skipped += 1;
            continue;
        }
        if !e.file_type().is_file() {
            continue;
        }
        if exclude.is_some_and(|x| x == e.path()) {
            continue;
        }
        let Ok(rel) = e.path().strip_prefix(&root) else {
            continue;
        };
        let size = match e.metadata() {
            Ok(m) => m.len(),
            Err(err) => {
                scan.errors += 1;
                eprintln!("cannot read {}: {err}", e.path().display());
                continue;
            }
        };
        let rel = rel_display(rel);
        if scan.files.contains_key(&rel) {
            // Non-UTF-8 names can collide after to_string_lossy: a file must not be overwritten silently.
            scan.errors += 1;
            eprintln!(
                "different files give one name after the bytes are lost: {}",
                e.path().display()
            );
            continue;
        }
        scan.files.insert(
            rel,
            Entry {
                path: e.path().to_path_buf(),
                size,
            },
        );
    }
    Ok(scan)
}

/// Compare two folders. Progress is by the files that need a sha256.
pub fn verify(
    a: &Path,
    b: &Path,
    jobs: usize,
    lang: Lang,
    progress: Option<Arc<Progress>>,
) -> Result<Report> {
    let t0 = std::time::Instant::now();
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "walking both folders"));
    }
    let sa = scan(a)?;
    let sb = scan(b)?;
    let mut report = compare(&sa, &sb, jobs, lang, progress.clone(), derive::sha256_file);
    report.seconds = t0.elapsed().as_secs_f64();
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "the comparison is done"));
        p.finish();
    }
    Ok(report)
}

/// The core of the comparison split from the file system: the tests replace `hash` to check
/// that with different sizes the content is not read at all.
pub(super) fn compare<F>(
    a: &Scan,
    b: &Scan,
    jobs: usize,
    lang: Lang,
    progress: Option<Arc<Progress>>,
    hash: F,
) -> Report
where
    F: Fn(&Path) -> Result<String> + Sync,
{
    let mut only_a = Vec::new();
    let mut only_b = Vec::new();
    let mut size_diff = Vec::new();
    let mut jobs_vec: Vec<(String, Entry, Entry)> = Vec::new();
    for (rel, ea) in &a.files {
        match b.files.get(rel) {
            None => only_a.push(rel.clone()),
            Some(eb) => {
                if ea.size != eb.size {
                    size_diff.push((rel.clone(), ea.size, eb.size));
                } else {
                    jobs_vec.push((rel.clone(), ea.clone(), eb.clone()));
                }
            }
        }
    }
    for rel in b.files.keys() {
        if !a.files.contains_key(rel) {
            only_b.push(rel.clone());
        }
    }

    if let Some(p) = &progress {
        p.set_total(jobs_vec.len());
        p.set_msg(i18n::t(lang, "sha256 over the matching sizes"));
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build();
    let outcomes: Vec<Outcome> = match pool {
        Ok(pool) => pool.install(|| {
            jobs_vec
                .par_iter()
                .map(|(rel, ea, eb)| {
                    if progress.as_ref().is_some_and(|p| p.cancelled()) {
                        return Outcome::Skipped;
                    }
                    let out = match hash(&ea.path) {
                        Ok(ha) => match hash(&eb.path) {
                            Ok(hb) if ha == hb => Outcome::Same(ea.size),
                            Ok(_) => Outcome::Content(rel.clone(), ea.size),
                            Err(e) => Outcome::ReadError(format!("{}: {e:#}", eb.path.display())),
                        },
                        Err(e) => Outcome::ReadError(format!("{}: {e:#}", ea.path.display())),
                    };
                    if matches!(out, Outcome::ReadError(_))
                        && let Some(p) = &progress
                    {
                        p.error();
                    }
                    if let Some(p) = &progress {
                        p.tick();
                    }
                    out
                })
                .collect()
        }),
        Err(e) => {
            // The pool could not be built — say so honestly instead of pretending everything was compared.
            return Report {
                files_a: a.files.len(),
                files_b: b.files.len(),
                only_a,
                only_b,
                size_diff,
                content_diff: Vec::new(),
                read_errors: vec![format!("cannot build the thread pool: {e}")],
                same: 0,
                hashed: 0,
                hashed_bytes: 0,
                unchecked: jobs_vec.len(),
                scan_errors: a.errors + b.errors,
                skipped_a: a.skipped,
                skipped_b: b.skipped,
                cancelled: progress.as_ref().is_some_and(|p| p.cancelled()),
                seconds: 0.0,
                lang: Lang::default(),
            };
        }
    };

    let mut report = Report {
        files_a: a.files.len(),
        files_b: b.files.len(),
        only_a,
        only_b,
        size_diff,
        content_diff: Vec::new(),
        read_errors: Vec::new(),
        same: 0,
        hashed: 0,
        hashed_bytes: 0,
        unchecked: 0,
        scan_errors: a.errors + b.errors,
        skipped_a: a.skipped,
        skipped_b: b.skipped,
        cancelled: false,
        seconds: 0.0,
        lang: Lang::default(),
    };
    for out in outcomes {
        match out {
            Outcome::Same(size) => {
                report.same += 1;
                report.hashed += 1;
                report.hashed_bytes += size;
            }
            Outcome::Content(rel, size) => {
                report.content_diff.push(rel);
                report.hashed += 1;
                report.hashed_bytes += size;
            }
            Outcome::ReadError(msg) => report.read_errors.push(msg),
            Outcome::Skipped => {
                report.unchecked += 1;
                report.cancelled = true;
            }
        }
    }
    report
}

enum Outcome {
    Same(u64),
    Content(String, u64),
    ReadError(String),
    Skipped,
}


/// The text of the comparison report. The CLI prints it and the GUI copies it: the words and
/// the numbers live in one place, `app.rs` does not build the report itself.
pub fn verify_report(a: &Path, b: &Path, r: &Report, all: bool) -> String {
    use std::fmt::Write;
    let lang = r.lang;
    let mut s = String::new();
    let _ = writeln!(s, "{}", i18n::fill(lang, "comparison: {}", &[a.display().to_string()]));
    let _ = writeln!(s, "{}", i18n::fill(lang, "     with: {}", &[b.display().to_string()]));
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(
            lang,
            "files: A {}, B {}; identical content {}; sha256 checked {} files ({})",
            &[
                r.files_a.to_string(),
                r.files_b.to_string(),
                r.same.to_string(),
                r.hashed.to_string(),
                db::human_in(lang, r.hashed_bytes as i64),
            ]
        )
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "size differs: {}", &[r.size_diff.len().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(
            lang,
            "content differs: {}",
            &[r.content_diff.len().to_string()]
        )
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "only in A: {}", &[r.only_a.len().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "only in B: {}", &[r.only_b.len().to_string()])
    );
    if r.scan_errors > 0 {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(lang, "scan errors: {}", &[r.scan_errors.to_string()])
        );
    }
    if !r.read_errors.is_empty() {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "read errors: {}",
                &[r.read_errors.len().to_string()]
            )
        );
    }
    if r.unchecked > 0 {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "not checked because of the stop: {}",
                &[r.unchecked.to_string()]
            )
        );
    }
    if r.skipped_a > 0 || r.skipped_b > 0 {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "skipped (symlinks/special): A {}, B {}",
                &[r.skipped_a.to_string(), r.skipped_b.to_string()]
            )
        );
    }
    write_list(&mut s, lang, "only in A", &r.only_a, all);
    write_list(&mut s, lang, "only in B", &r.only_b, all);
    write_size_diff(&mut s, lang, &r.size_diff, all);
    write_list(&mut s, lang, "content differs", &r.content_diff, all);
    write_list(&mut s, lang, "read errors", &r.read_errors, all);
    let verdict = i18n::fill(
        lang,
        "verdict: {} ({} s)",
        &[
            i18n::t(
                lang,
                if r.identical() {
                    "the copies match"
                } else {
                    "differences found"
                },
            )
            .to_string(),
            format!("{:.1}", r.seconds),
        ],
    );
    let _ = writeln!(s, "\n{verdict}");
    s
}
