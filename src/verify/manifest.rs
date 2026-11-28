//! The `sha256sum` manifest: writing it, parsing it and checking a folder against it.

use super::compare::{scan_excluding, Entry};
use super::{positional, status_line, write_list};
use crate::db::{self, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use crate::{flag_num, flag_str};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

pub struct ManifestStats {
    pub files: usize,
    pub bytes: u64,
    pub skipped: usize,
    pub errors: usize,
    /// Not checked because of the stop.
    pub unchecked: usize,
    pub cancelled: bool,
    /// The manifest was written whole. False on a stop or a read error.
    pub written: bool,
    pub seconds: f64,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

pub struct CheckReport {
    /// In the manifest, missing from the folder.
    pub missing: Vec<String>,
    /// In the folder, missing from the manifest.
    pub extra: Vec<String>,
    /// In both, but with a different sha256.
    pub changed: Vec<String>,
    pub read_errors: Vec<String>,
    pub same: usize,
    pub unchecked: usize,
    pub scan_errors: usize,
    pub skipped: usize,
    pub cancelled: bool,
    pub seconds: f64,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl CheckReport {
    pub fn errors(&self) -> usize {
        self.scan_errors + self.read_errors.len()
    }

    pub fn identical(&self) -> bool {
        self.missing.is_empty()
            && self.extra.is_empty()
            && self.changed.is_empty()
            && self.errors() == 0
            && !self.cancelled
            && self.unchecked == 0
    }
}

/// A folder manifest in the `sha256sum` format: `<hash>  <relative path>`.
/// Without `--out` it goes to stdout; on an error or a stop it is not written at all.
pub fn manifest(
    root: &Path,
    out: Option<&Path>,
    jobs: usize,
    lang: Lang,
    progress: Option<Arc<Progress>>,
) -> Result<ManifestStats> {
    let t0 = std::time::Instant::now();
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("cannot open the folder {}", root.display()))?;
    let exclude = out.map(resolve_new_path);
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "walking the folder"));
    }
    let scan = scan_excluding(&root, exclude.as_deref())?;
    let entries: Vec<(String, Entry)> = scan.files.into_iter().collect();
    if let Some(p) = &progress {
        p.set_total(entries.len());
        p.set_msg(i18n::t(lang, "sha256 of every file"));
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()?;
    let outcomes: Vec<Option<Result<(String, String, u64)>>> = pool.install(|| {
        entries
            .par_iter()
            .map(|(rel, e)| {
                if progress.as_ref().is_some_and(|p| p.cancelled()) {
                    return None;
                }
                let res = derive::sha256_file(&e.path)
                    .map(|h| (rel.clone(), h, e.size))
                    .with_context(|| format!("cannot read {}", e.path.display()));
                if res.is_err()
                    && let Some(p) = &progress
                {
                    p.error();
                }
                if let Some(p) = &progress {
                    p.tick();
                }
                Some(res)
            })
            .collect()
    });
    let mut hashed: Vec<(String, String, u64)> = Vec::new();
    let mut read_errors = 0usize;
    let mut unchecked = 0usize;
    for out in outcomes {
        match out {
            Some(Ok(v)) => hashed.push(v),
            Some(Err(e)) => {
                read_errors += 1;
                eprintln!("{e:#}");
            }
            None => unchecked += 1,
        }
    }
    let cancelled = progress.as_ref().is_some_and(|p| p.cancelled());
    let errors = scan.errors + read_errors;
    let mut stats = ManifestStats {
        files: hashed.len(),
        bytes: hashed.iter().map(|(_, _, size)| *size).sum(),
        skipped: scan.skipped,
        errors,
        unchecked,
        cancelled,
        written: false,
        seconds: 0.0,
        lang: Lang::default(),
    };
    // A partial manifest is more dangerous than none: a check would call the missing files "extra".
    if errors == 0 && !cancelled {
        let mut text = String::new();
        for (rel, hash, _) in &hashed {
            text.push_str(&manifest_line(hash, rel));
        }
        match out {
            Some(p) => std::fs::write(p, text)
                .with_context(|| format!("cannot write the manifest {}", p.display()))?,
            None => print!("{text}"),
        }
        stats.written = true;
    }
    stats.seconds = t0.elapsed().as_secs_f64();
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "the manifest is ready"));
        p.finish();
    }
    Ok(stats)
}

/// Checking a folder against a manifest. The manifest file itself is excluded from the walk, even inside the archive.
pub fn check_manifest(
    root: &Path,
    file: &Path,
    jobs: usize,
    lang: Lang,
    progress: Option<Arc<Progress>>,
) -> Result<CheckReport> {
    let t0 = std::time::Instant::now();
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("cannot read the manifest {}", file.display()))?;
    let entries = parse_manifest(&text)?;
    let expected: BTreeMap<String, String> = entries.into_iter().map(|e| (e.rel, e.hash)).collect();
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("cannot open the folder {}", root.display()))?;
    let exclude = resolve_new_path(file);
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "walking the folder"));
    }
    let scan = scan_excluding(&root, Some(&exclude))?;

    let missing: Vec<String> = expected
        .keys()
        .filter(|rel| !scan.files.contains_key(*rel))
        .cloned()
        .collect();
    let extra: Vec<String> = scan
        .files
        .keys()
        .filter(|rel| !expected.contains_key(*rel))
        .cloned()
        .collect();
    let common: Vec<(String, Entry, String)> = expected
        .iter()
        .filter_map(|(rel, want)| {
            scan.files
                .get(rel)
                .map(|e| (rel.clone(), e.clone(), want.clone()))
        })
        .collect();

    if let Some(p) = &progress {
        p.set_total(common.len());
        p.set_msg(i18n::t(lang, "sha256 against the manifest"));
    }
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(jobs.max(1))
        .build()?;
    let outcomes: Vec<CheckOutcome> = pool.install(|| {
        common
            .par_iter()
            .map(|(rel, e, want)| {
                if progress.as_ref().is_some_and(|p| p.cancelled()) {
                    return CheckOutcome::Skipped;
                }
                let out = match derive::sha256_file(&e.path) {
                    Ok(hash) if &hash == want => CheckOutcome::Same,
                    Ok(_) => CheckOutcome::Changed(rel.clone()),
                    Err(err) => CheckOutcome::ReadError(format!("{}: {err:#}", e.path.display())),
                };
                if matches!(out, CheckOutcome::ReadError(_))
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
    });

    let mut report = CheckReport {
        missing,
        extra,
        changed: Vec::new(),
        read_errors: Vec::new(),
        same: 0,
        unchecked: 0,
        scan_errors: scan.errors,
        skipped: scan.skipped,
        cancelled: false,
        seconds: 0.0,
        lang: Lang::default(),
    };
    for out in outcomes {
        match out {
            CheckOutcome::Same => report.same += 1,
            CheckOutcome::Changed(rel) => report.changed.push(rel),
            CheckOutcome::ReadError(msg) => report.read_errors.push(msg),
            CheckOutcome::Skipped => {
                report.unchecked += 1;
                report.cancelled = true;
            }
        }
    }
    report.seconds = t0.elapsed().as_secs_f64();
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "the manifest check is done"));
        p.finish();
    }
    Ok(report)
}

enum CheckOutcome {
    Same,
    Changed(String),
    ReadError(String),
    Skipped,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManifestEntry {
    pub hash: String,
    pub rel: String,
}

/// Parsing a `sha256sum` manifest. Handles escaped names (backslash, newline, carriage return)
/// and the binary mode of `sha256sum -b` (`hash *name`).
pub fn parse_manifest(text: &str) -> Result<Vec<ManifestEntry>> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for (n, raw) in text.lines().enumerate() {
        let line_no = n + 1;
        if raw.is_empty() {
            continue;
        }
        let (escaped, line) = match raw.strip_prefix('\\') {
            Some(rest) => (true, rest),
            None => (false, raw),
        };
        if line.len() < 66 {
            anyhow::bail!("manifest, line {line_no}: too short");
        }
        let (hash, rest) = line.split_at(64);
        if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            anyhow::bail!("manifest, line {line_no}: the sha256 is not hexadecimal");
        }
        let name = if let Some(rest) = rest.strip_prefix("  ") {
            rest
        } else if let Some(rest) = rest.strip_prefix(" *") {
            rest
        } else {
            anyhow::bail!("manifest, line {line_no}: two spaces or “ *” are expected after the sha256");
        };
        if name.is_empty() {
            anyhow::bail!("manifest, line {line_no}: an empty file name");
        }
        let rel = if escaped {
            unescape_name(name).with_context(|| format!("manifest, line {line_no}"))?
        } else {
            name.to_string()
        };
        if !safe_rel(&rel) {
            anyhow::bail!(
                "manifest, line {line_no}: the path is outside the folder or not relative: “{rel}”"
            );
        }
        if !seen.insert(rel.clone()) {
            anyhow::bail!("manifest, line {line_no}: the file “{rel}” is listed twice");
        }
        out.push(ManifestEntry {
            hash: hash.to_ascii_lowercase(),
            rel,
        });
    }
    Ok(out)
}

/// One manifest line. Names with a backslash, a newline or a carriage return are escaped the
/// same way GNU `sha256sum` does it; ordinary names stay readable.
pub fn manifest_line(hash: &str, rel: &str) -> String {
    let need_escape = rel.contains(['\\', '\n', '\r']);
    if need_escape {
        format!("\\{hash}  {}\n", escape_name(rel))
    } else {
        format!("{hash}  {rel}\n")
    }
}

fn escape_name(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            _ => out.push(c),
        }
    }
    out
}

/// The manifest allows a relative path only, with no `..` and no roots: a check must not
/// leave the folder even if the manifest file was planted.
fn safe_rel(rel: &str) -> bool {
    !rel.is_empty()
        && Path::new(rel)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

fn unescape_name(s: &str) -> Result<String> {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars();
    while let Some(c) = it.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match it.next() {
            Some('\\') => out.push('\\'),
            Some('n') => out.push('\n'),
            Some('r') => out.push('\r'),
            Some(other) => anyhow::bail!("an unknown escape sequence “\\{other}”"),
            None => anyhow::bail!("an escape sequence at the end of the line"),
        }
    }
    Ok(out)
}

/// A relative path with `/` on any OS: the manifest can move between Linux and Windows.
pub(super) fn rel_display(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// A path that does not exist yet (the manifest file) — its parent is canonicalized.
fn resolve_new_path(p: &Path) -> PathBuf {
    if let Ok(c) = p.canonicalize() {
        return c;
    }
    match (p.parent(), p.file_name()) {
        (Some(dir), Some(name)) if !dir.as_os_str().is_empty() => dir
            .canonicalize()
            .unwrap_or_else(|_| dir.to_path_buf())
            .join(name),
        _ => p.to_path_buf(),
    }
}

pub fn manifest_cli(args: &[String], lang: Lang) -> Result<()> {
    let jobs = flag_num(args, "--jobs", 4).max(1);
    let out = flag_str(args, "--out").map(PathBuf::from);
    let root = positional(args)
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    let progress = Arc::new(Progress::default());
    let show = progress.clone();
    std::thread::spawn(move || status_line(i18n::t(lang, "manifest"), lang, show));
    let mut stats = manifest(&root, out.as_deref(), jobs, lang, Some(progress))?;
    stats.lang = lang;
    let summary = manifest_report(&root, out.as_deref(), &stats);
    if out.is_some() && stats.written {
        print!("{summary}");
    } else {
        eprint!("{summary}");
    }
    if !stats.written {
        std::process::exit(1);
    }
    Ok(())
}

/// The text of the manifest check: the same source for the CLI and the "Integrity" screen.
pub fn check_report(file: &Path, root: &Path, r: &CheckReport, all: bool) -> String {
    use std::fmt::Write;
    let lang = r.lang;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "manifest check: {}", &[file.display().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "            folder: {}", &[root.display().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(
            lang,
            "matched: {}, changed: {}, missing from the folder: {}, missing from the manifest: {}",
            &[
                r.same.to_string(),
                r.changed.len().to_string(),
                r.missing.len().to_string(),
                r.extra.len().to_string(),
            ]
        )
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
    if r.skipped > 0 {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "skipped (symlinks/special): {}",
                &[r.skipped.to_string()]
            )
        );
    }
    write_list(&mut s, lang, "missing from the folder", &r.missing, all);
    write_list(&mut s, lang, "missing from the manifest", &r.extra, all);
    write_list(&mut s, lang, "changed", &r.changed, all);
    write_list(&mut s, lang, "read errors", &r.read_errors, all);
    let verdict = i18n::fill(
        lang,
        "verdict: {} ({} s)",
        &[
            i18n::t(
                lang,
                if r.identical() {
                    "the folder matches the manifest"
                } else {
                    "the folder does not match the manifest"
                },
            )
            .to_string(),
            format!("{:.1}", r.seconds),
        ],
    );
    let _ = writeln!(s, "\n{verdict}");
    s
}

/// The result of writing a manifest: the path is added only when the file was really written.
pub fn manifest_report(root: &Path, out: Option<&Path>, stats: &ManifestStats) -> String {
    let lang = stats.lang;
    let prefix = if stats.written {
        ""
    } else if stats.cancelled {
        i18n::t(lang, "cancelled: ")
    } else {
        i18n::t(lang, "not written: ")
    };
    let arrow = match out {
        Some(p) if stats.written => format!(" → {}", p.display()),
        _ => String::new(),
    };
    format!(
        "{prefix}{}{arrow}\n",
        i18n::fill(
            lang,
            "manifest {}: {} files ({}), skipped {}, errors {}, not checked {}, {} s",
            &[
                root.display().to_string(),
                stats.files.to_string(),
                db::human_in(lang, stats.bytes as i64),
                stats.skipped.to_string(),
                stats.errors.to_string(),
                stats.unchecked.to_string(),
                format!("{:.1}", stats.seconds),
            ]
        )
    )
}
