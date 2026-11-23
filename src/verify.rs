//! Сверка двух копий архива и слепок `sha256sum`.
//!
//! Обе команды только читают архив. Единственный созданный файл — слепок по явному `--out`.
//! Сначала сравниваются размеры: если они различаются, читать файл незачем. sha256 считается
//! только для файлов одинакового размера, поэтому `verify` двух копий читает оба дерева целиком,
//! а не «на всякий случай» ещё и заведомо разные файлы.

use crate::db::{self, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use crate::{flag_num, flag_str, has_flag};
use anyhow::{Context, Result};
use rayon::prelude::*;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Entry {
    pub path: PathBuf,
    pub size: u64,
}

#[derive(Default)]
pub struct Scan {
    /// Относительный путь → файл. BTreeMap даёт стабильный порядок отчёта.
    pub files: BTreeMap<String, Entry>,
    /// Симлинки и другие не-файлы: не сравниваются.
    pub skipped: usize,
    /// Каталоги/файлы, которые не удалось прочитать при обходе.
    pub errors: usize,
}

pub struct Report {
    pub files_a: usize,
    pub files_b: usize,
    pub only_a: Vec<String>,
    pub only_b: Vec<String>,
    /// Путь и размеры в A и B; sha256 для них не считается.
    pub size_diff: Vec<(String, u64, u64)>,
    pub content_diff: Vec<String>,
    pub read_errors: Vec<String>,
    pub same: usize,
    /// Сколько файлов одинакового размера реально прошло через sha256.
    pub hashed: usize,
    /// Объём одной копии этих файлов (читается дважды — в A и B).
    pub hashed_bytes: u64,
    /// Файлы одинакового размера, не проверенные из-за отмены.
    pub unchecked: usize,
    pub scan_errors: usize,
    pub skipped_a: usize,
    pub skipped_b: usize,
    pub cancelled: bool,
    pub seconds: f64,
    /// Язык отчёта (M21.6): ставит вызывающий до печати; английский по умолчанию.
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

pub struct ManifestStats {
    pub files: usize,
    pub bytes: u64,
    pub skipped: usize,
    pub errors: usize,
    /// Не проверено из-за отмены.
    pub unchecked: usize,
    pub cancelled: bool,
    /// Слепок записан целиком. При отмене или ошибке чтения — false.
    pub written: bool,
    pub seconds: f64,
    /// Язык отчёта (M21.6): ставит вызывающий до печати; английский по умолчанию.
    pub lang: Lang,
}

pub struct CheckReport {
    /// Есть в слепке, нет в каталоге.
    pub missing: Vec<String>,
    /// Есть в каталоге, нет в слепке.
    pub extra: Vec<String>,
    /// Есть в обоих, но sha256 другой.
    pub changed: Vec<String>,
    pub read_errors: Vec<String>,
    pub same: usize,
    pub unchecked: usize,
    pub scan_errors: usize,
    pub skipped: usize,
    pub cancelled: bool,
    pub seconds: f64,
    /// Язык отчёта (M21.6): ставит вызывающий до печати; английский по умолчанию.
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

/// Обход дерева: только обычные файлы, путь относительно корня. Симлинки не разыменовываются.
pub fn scan(root: &Path) -> Result<Scan> {
    scan_excluding(root, None)
}

fn scan_excluding(root: &Path, exclude: Option<&Path>) -> Result<Scan> {
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("не открыть каталог {}", root.display()))?;
    let mut scan = Scan::default();
    for e in walkdir::WalkDir::new(&root).follow_links(false) {
        let e = match e {
            Ok(e) => e,
            Err(err) => {
                scan.errors += 1;
                eprintln!("пропущено при обходе: {err}");
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
                eprintln!("не прочитать {}: {err}", e.path().display());
                continue;
            }
        };
        let rel = rel_display(rel);
        if scan.files.contains_key(&rel) {
            // Не-UTF-8 имена после to_string_lossy могут совпасть: молча затирать файл нельзя.
            scan.errors += 1;
            eprintln!(
                "разные файлы дают одно имя после потери байтов: {}",
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

/// Сверка двух каталогов. Прогресс — по файлам, которым нужен sha256.
pub fn verify(a: &Path, b: &Path, jobs: usize, progress: Option<Arc<Progress>>) -> Result<Report> {
    let t0 = std::time::Instant::now();
    if let Some(p) = &progress {
        p.set_msg("обход двух каталогов");
    }
    let sa = scan(a)?;
    let sb = scan(b)?;
    let mut report = compare(&sa, &sb, jobs, progress.clone(), derive::sha256_file);
    report.seconds = t0.elapsed().as_secs_f64();
    if let Some(p) = &progress {
        p.set_msg("сверка завершена");
        p.finish();
    }
    Ok(report)
}

/// Ядро сверки отдельно от файловой системы: в тестах `hash` подменяется, чтобы проверить,
/// что при разных размерах содержимое вообще не читается.
fn compare<F>(a: &Scan, b: &Scan, jobs: usize, progress: Option<Arc<Progress>>, hash: F) -> Report
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
        p.set_msg("sha256 по совпавшим размерам");
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
            // Пул не собрался — честно сообщаем об этом, а не делаем вид, что всё сверено.
            return Report {
                files_a: a.files.len(),
                files_b: b.files.len(),
                only_a,
                only_b,
                size_diff,
                content_diff: Vec::new(),
                read_errors: vec![format!("не создать пул потоков: {e}")],
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

/// Слепок каталога в формате `sha256sum`: `<hash>  <относительный путь>`.
/// Без `--out` печатается в stdout; при ошибке или отмене не пишется вовсе.
pub fn manifest(
    root: &Path,
    out: Option<&Path>,
    jobs: usize,
    progress: Option<Arc<Progress>>,
) -> Result<ManifestStats> {
    let t0 = std::time::Instant::now();
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("не открыть каталог {}", root.display()))?;
    let exclude = out.map(resolve_new_path);
    if let Some(p) = &progress {
        p.set_msg("обход каталога");
    }
    let scan = scan_excluding(&root, exclude.as_deref())?;
    let entries: Vec<(String, Entry)> = scan.files.into_iter().collect();
    if let Some(p) = &progress {
        p.set_total(entries.len());
        p.set_msg("sha256 всех файлов");
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
                    .with_context(|| format!("не прочитать {}", e.path.display()));
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
    // Неполный слепок опаснее отсутствующего: проверка объявит пропущенные файлы «лишними».
    if errors == 0 && !cancelled {
        let mut text = String::new();
        for (rel, hash, _) in &hashed {
            text.push_str(&manifest_line(hash, rel));
        }
        match out {
            Some(p) => std::fs::write(p, text)
                .with_context(|| format!("не записать слепок {}", p.display()))?,
            None => print!("{text}"),
        }
        stats.written = true;
    }
    stats.seconds = t0.elapsed().as_secs_f64();
    if let Some(p) = &progress {
        p.set_msg("слепок готов");
        p.finish();
    }
    Ok(stats)
}

/// Проверка каталога по слепку. Сам файл слепка из обхода исключается, даже если лежит в архиве.
pub fn check_manifest(
    root: &Path,
    file: &Path,
    jobs: usize,
    progress: Option<Arc<Progress>>,
) -> Result<CheckReport> {
    let t0 = std::time::Instant::now();
    let text = std::fs::read_to_string(file)
        .with_context(|| format!("не прочитать слепок {}", file.display()))?;
    let entries = parse_manifest(&text)?;
    let expected: BTreeMap<String, String> = entries.into_iter().map(|e| (e.rel, e.hash)).collect();
    let root = std::fs::canonicalize(root)
        .with_context(|| format!("не открыть каталог {}", root.display()))?;
    let exclude = resolve_new_path(file);
    if let Some(p) = &progress {
        p.set_msg("обход каталога");
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
        p.set_msg("sha256 по слепку");
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
        p.set_msg("проверка слепка завершена");
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

/// Разбор слепка `sha256sum`. Поддерживает экранированные имена (обратный слэш, перевод строки,
/// возврат каретки) и бинарный режим `sha256sum -b` (`hash *имя`).
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
            anyhow::bail!("слепок, строка {line_no}: слишком короткая");
        }
        let (hash, rest) = line.split_at(64);
        if !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
            anyhow::bail!("слепок, строка {line_no}: sha256 не из шестнадцатеричных цифр");
        }
        let name = if let Some(rest) = rest.strip_prefix("  ") {
            rest
        } else if let Some(rest) = rest.strip_prefix(" *") {
            rest
        } else {
            anyhow::bail!("слепок, строка {line_no}: после sha256 ожидается два пробела или « *»");
        };
        if name.is_empty() {
            anyhow::bail!("слепок, строка {line_no}: пустое имя файла");
        }
        let rel = if escaped {
            unescape_name(name).with_context(|| format!("слепок, строка {line_no}"))?
        } else {
            name.to_string()
        };
        if !safe_rel(&rel) {
            anyhow::bail!(
                "слепок, строка {line_no}: путь вне каталога или не относительный: «{rel}»"
            );
        }
        if !seen.insert(rel.clone()) {
            anyhow::bail!("слепок, строка {line_no}: файл «{rel}» указан дважды");
        }
        out.push(ManifestEntry {
            hash: hash.to_ascii_lowercase(),
            rel,
        });
    }
    Ok(out)
}

/// Одна строка слепка. Имена с обратным слэшем, переводом строки или возвратом каретки
/// экранируются так же, как у GNU `sha256sum`; обычные имена остаются читаемыми.
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

/// В манифесте допустим только относительный путь без `..` и корней: проверка не должна
/// выйти за пределы каталога, даже если файл слепка подсунули.
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
            Some(other) => anyhow::bail!("неизвестная escape-последовательность «\\{other}»"),
            None => anyhow::bail!("escape-последовательность в конце строки"),
        }
    }
    Ok(out)
}

/// Относительный путь с `/` на любой ОС: слепок можно переносить между Linux и Windows.
fn rel_display(p: &Path) -> String {
    p.components()
        .filter_map(|c| match c {
            Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
            Component::ParentDir => Some("..".to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Путь, который ещё не существует (файл слепка), — канонизируем родителя.
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

pub fn cli(args: &[String], lang: Lang) -> Result<()> {
    let jobs = flag_num(args, "--jobs", 4).max(1);
    let all = has_flag(args, "--all");
    if let Some(file) = flag_str(args, "--manifest") {
        let root = match positional(args).first() {
            Some(p) => PathBuf::from(p),
            None => {
                eprintln!(
                    "{}",
                    i18n::t(lang, "нужен каталог: photocat verify --manifest FILE DIR")
                );
                std::process::exit(2);
            }
        };
        let progress = Arc::new(Progress::default());
        let show = progress.clone();
        std::thread::spawn(move || status_line(i18n::t(lang, "проверка"), lang, show));
        let mut report = check_manifest(&root, Path::new(&file), jobs, Some(progress))?;
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
            i18n::t(lang, "нужны два каталога: photocat verify A B [--jobs N] [--all]")
        );
        std::process::exit(2);
    }
    let (a, b) = (PathBuf::from(&pos[0]), PathBuf::from(&pos[1]));
    let progress = Arc::new(Progress::default());
    let show = progress.clone();
    std::thread::spawn(move || status_line(i18n::t(lang, "сверка"), lang, show));
    let mut report = verify(&a, &b, jobs, Some(progress))?;
    report.lang = lang;
    print!("{}", verify_report(&a, &b, &report, all));
    if !report.identical() {
        std::process::exit(1);
    }
    Ok(())
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
    std::thread::spawn(move || status_line(i18n::t(lang, "слепок"), lang, show));
    let mut stats = manifest(&root, out.as_deref(), jobs, Some(progress))?;
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

fn status_line(prefix: &str, lang: Lang, p: Arc<Progress>) {
    std::thread::sleep(std::time::Duration::from_millis(300));
    loop {
        let (d, t, e) = (p.done(), p.total(), p.errors());
        if t > 0 {
            let line = i18n::fill(
                lang,
                "{}/{} файлов, ошибок {} — {}",
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

fn positional(args: &[String]) -> Vec<String> {
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

/// Текст отчёта сверки. Его печатает CLI и копирует GUI: слова и числа живут
/// в одном месте, `app.rs` отчёт сам не собирает.
pub fn verify_report(a: &Path, b: &Path, r: &Report, all: bool) -> String {
    use std::fmt::Write;
    let lang = r.lang;
    let mut s = String::new();
    let _ = writeln!(s, "{}", i18n::fill(lang, "сверка: {}", &[a.display().to_string()]));
    let _ = writeln!(s, "{}", i18n::fill(lang, "     с: {}", &[b.display().to_string()]));
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(
            lang,
            "файлов: A {}, B {}; совпало по содержимому {}; sha256 проверено {} файлов ({})",
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
        i18n::fill(lang, "размер отличается: {}", &[r.size_diff.len().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(
            lang,
            "содержимое отличается: {}",
            &[r.content_diff.len().to_string()]
        )
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "только в A: {}", &[r.only_a.len().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "только в B: {}", &[r.only_b.len().to_string()])
    );
    if r.scan_errors > 0 {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(lang, "ошибок обхода: {}", &[r.scan_errors.to_string()])
        );
    }
    if !r.read_errors.is_empty() {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "ошибок чтения: {}",
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
                "не проверено из-за отмены: {}",
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
                "пропущено (симлинки/особые): A {}, B {}",
                &[r.skipped_a.to_string(), r.skipped_b.to_string()]
            )
        );
    }
    write_list(&mut s, lang, "только в A", &r.only_a, all);
    write_list(&mut s, lang, "только в B", &r.only_b, all);
    write_size_diff(&mut s, lang, &r.size_diff, all);
    write_list(&mut s, lang, "содержимое отличается", &r.content_diff, all);
    write_list(&mut s, lang, "ошибки чтения", &r.read_errors, all);
    let verdict = i18n::fill(
        lang,
        "вердикт: {} (за {} с)",
        &[
            i18n::t(
                lang,
                if r.identical() {
                    "копии совпадают"
                } else {
                    "найдены расхождения"
                },
            )
            .to_string(),
            format!("{:.1}", r.seconds),
        ],
    );
    let _ = writeln!(s, "\n{verdict}");
    s
}

/// Текст проверки по слепку: тот же источник для CLI и экрана «Целостность».
pub fn check_report(file: &Path, root: &Path, r: &CheckReport, all: bool) -> String {
    use std::fmt::Write;
    let lang = r.lang;
    let mut s = String::new();
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "проверка слепка: {}", &[file.display().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(lang, "           каталог: {}", &[root.display().to_string()])
    );
    let _ = writeln!(
        s,
        "{}",
        i18n::fill(
            lang,
            "совпало: {}, изменено: {}, нет в каталоге: {}, нет в слепке: {}",
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
            i18n::fill(lang, "ошибок обхода: {}", &[r.scan_errors.to_string()])
        );
    }
    if !r.read_errors.is_empty() {
        let _ = writeln!(
            s,
            "{}",
            i18n::fill(
                lang,
                "ошибок чтения: {}",
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
                "не проверено из-за отмены: {}",
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
                "пропущено (симлинки/особые): {}",
                &[r.skipped.to_string()]
            )
        );
    }
    write_list(&mut s, lang, "нет в каталоге", &r.missing, all);
    write_list(&mut s, lang, "нет в слепке", &r.extra, all);
    write_list(&mut s, lang, "изменено", &r.changed, all);
    write_list(&mut s, lang, "ошибки чтения", &r.read_errors, all);
    let verdict = i18n::fill(
        lang,
        "вердикт: {} (за {} с)",
        &[
            i18n::t(
                lang,
                if r.identical() {
                    "каталог совпадает со слепком"
                } else {
                    "каталог расходится со слепком"
                },
            )
            .to_string(),
            format!("{:.1}", r.seconds),
        ],
    );
    let _ = writeln!(s, "\n{verdict}");
    s
}

/// Итог записи слепка: путь дописывается только тогда, когда файл действительно записан.
pub fn manifest_report(root: &Path, out: Option<&Path>, stats: &ManifestStats) -> String {
    let lang = stats.lang;
    let prefix = if stats.written {
        ""
    } else if stats.cancelled {
        i18n::t(lang, "отменено: ")
    } else {
        i18n::t(lang, "не записан: ")
    };
    let arrow = match out {
        Some(p) if stats.written => format!(" → {}", p.display()),
        _ => String::new(),
    };
    format!(
        "{prefix}{}{arrow}\n",
        i18n::fill(
            lang,
            "слепок {}: {} файлов ({}), пропущено {}, ошибок {}, не проверено {}, за {} с",
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

fn write_list(s: &mut String, lang: Lang, title: &'static str, list: &[String], all: bool) {
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
                "  … ещё {} (показать все)",
                &[(list.len() - limit).to_string()]
            )
        );
    }
}

fn write_size_diff(s: &mut String, lang: Lang, list: &[(String, u64, u64)], all: bool) {
    use std::fmt::Write;
    if list.is_empty() {
        return;
    }
    let _ = writeln!(s, "\n{}:", i18n::t(lang, "размер отличается"));
    let limit = if all { list.len() } else { 20 };
    for (path, a, b) in list.iter().take(limit) {
        let delta = *b as i64 - *a as i64;
        let bytes = format!(
            "{}{} {}",
            if delta >= 0 { "+" } else { "" },
            delta,
            match lang {
                Lang::En => "B",
                Lang::Ru => "Б",
            }
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
                "  … ещё {} (показать все)",
                &[(list.len() - limit).to_string()]
            )
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn temp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "photocat-verify-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&p).unwrap();
        p
    }

    fn scan_of(items: &[(&str, &str, u64, &str)]) -> Scan {
        let mut s = Scan::default();
        for (rel, path, size, _) in items {
            s.files.insert(
                (*rel).to_string(),
                Entry {
                    path: PathBuf::from(path),
                    size: *size,
                },
            );
        }
        s
    }

    fn hash_map(items: &[(&str, &str)]) -> HashMap<PathBuf, String> {
        items
            .iter()
            .map(|(p, h)| (PathBuf::from(p), (*h).to_string()))
            .collect()
    }

    #[test]
    fn разные_размеры_не_читаются() {
        let a = scan_of(&[("x.jpg", "/a/x.jpg", 10, "")]);
        let b = scan_of(&[("x.jpg", "/b/x.jpg", 20, "")]);
        let calls = AtomicUsize::new(0);
        let r = compare(&a, &b, 2, None, |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok("h".into())
        });
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(r.size_diff.len(), 1);
        assert_eq!(r.hashed, 0);
    }

    #[test]
    fn одинаковый_размер_сверяется_по_sha256() {
        let a = scan_of(&[
            ("same.jpg", "/a/same.jpg", 10, ""),
            ("diff.jpg", "/a/diff.jpg", 20, ""),
        ]);
        let b = scan_of(&[
            ("same.jpg", "/b/same.jpg", 10, ""),
            ("diff.jpg", "/b/diff.jpg", 20, ""),
        ]);
        let hashes = hash_map(&[
            ("/a/same.jpg", "h1"),
            ("/b/same.jpg", "h1"),
            ("/a/diff.jpg", "h2"),
            ("/b/diff.jpg", "h3"),
        ]);
        let r = compare(&a, &b, 2, None, |p| Ok(hashes[p].clone()));
        assert_eq!(r.same, 1);
        assert_eq!(r.content_diff, vec!["diff.jpg".to_string()]);
        assert_eq!(r.hashed, 2);
        assert_eq!(r.hashed_bytes, 30);
    }

    #[test]
    fn файлы_только_в_одной_копии_видны_по_именам() {
        let a = scan_of(&[
            ("a.jpg", "/a/a.jpg", 1, ""),
            ("both.jpg", "/a/both.jpg", 2, ""),
        ]);
        let b = scan_of(&[
            ("b.jpg", "/b/b.jpg", 1, ""),
            ("both.jpg", "/b/both.jpg", 2, ""),
        ]);
        let r = compare(&a, &b, 1, None, |_| Ok("same".into()));
        assert_eq!(r.only_a, vec!["a.jpg".to_string()]);
        assert_eq!(r.only_b, vec!["b.jpg".to_string()]);
        assert_eq!(r.same, 1);
        assert!(!r.identical());
    }

    #[test]
    fn ошибка_чтения_попадает_в_отчёт_а_не_валит_сверку() {
        let a = scan_of(&[("x.jpg", "/a/x.jpg", 10, "")]);
        let b = scan_of(&[("x.jpg", "/b/x.jpg", 10, "")]);
        let r = compare(&a, &b, 1, None, |p| {
            anyhow::bail!("нет доступа: {}", p.display())
        });
        assert_eq!(r.read_errors.len(), 1);
        assert!(!r.identical());
    }

    #[test]
    fn отмена_не_считает_файлы_проверенными() {
        let a = scan_of(&[("x.jpg", "/a/x.jpg", 10, "")]);
        let b = scan_of(&[("x.jpg", "/b/x.jpg", 10, "")]);
        let p = Arc::new(Progress::default());
        p.cancel();
        let calls = AtomicUsize::new(0);
        let r = compare(&a, &b, 1, Some(p), |_| {
            calls.fetch_add(1, Ordering::Relaxed);
            Ok("h".into())
        });
        assert!(r.cancelled);
        assert_eq!(r.unchecked, 1);
        assert_eq!(r.hashed, 0);
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert!(!r.identical());
    }

    #[test]
    fn слепок_совместим_с_sha256sum_и_экранирует_имена() {
        let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
        assert_eq!(
            manifest_line(hash, "2024/2024-05-17/a.jpg"),
            format!("{hash}  2024/2024-05-17/a.jpg\n")
        );
        let line = manifest_line(hash, "a\\b\nc\rd");
        assert_eq!(line, format!("\\{hash}  a\\\\b\\nc\\rd\n"));
        let parsed = parse_manifest(&line).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].rel, "a\\b\nc\rd");
        assert_eq!(parsed[0].hash, hash);
    }

    #[test]
    fn разбор_слепка_поддерживает_бинарный_режим_и_отвергает_мусор() {
        let hash = "a".repeat(64);
        let ok = parse_manifest(&format!("{hash} *file.jpg\n")).unwrap();
        assert_eq!(ok[0].rel, "file.jpg");
        assert!(parse_manifest("не слепок\n").is_err());
        assert!(parse_manifest(&format!("{hash}  one\n{hash}  one\n")).is_err());
        assert!(parse_manifest(&format!("\\{hash}  bad\\q\n")).is_err());
        assert!(parse_manifest(&format!("{hash}  \n")).is_err());
        assert!(parse_manifest(&format!("{hash}  ../secret\n")).is_err());
        assert!(parse_manifest(&format!("{hash}  /etc/passwd\n")).is_err());
        assert!(parse_manifest(&format!("{hash}  ./x\n")).is_err());
    }

    #[test]
    fn слепок_и_проверка_каталога_работают_на_диске() {
        let root = temp_dir("roundtrip");
        std::fs::create_dir_all(root.join("2024")).unwrap();
        std::fs::write(root.join("a.txt"), b"hello").unwrap();
        std::fs::write(root.join("2024/b.txt"), b"world").unwrap();
        let out = root.join("archive.sha256");
        let stats = manifest(&root, Some(&out), 2, None).unwrap();
        assert!(stats.written);
        assert_eq!(stats.files, 2);
        assert_eq!(stats.bytes, 10);
        assert_eq!(stats.errors, 0);

        let text = std::fs::read_to_string(&out).unwrap();
        let parsed = parse_manifest(&text).unwrap();
        let names: Vec<&str> = parsed.iter().map(|e| e.rel.as_str()).collect();
        assert_eq!(names, vec!["2024/b.txt", "a.txt"]);

        let check = check_manifest(&root, &out, 2, None).unwrap();
        assert!(check.identical(), "{:?}", check.changed);
        assert_eq!(check.same, 2);

        std::fs::write(root.join("a.txt"), b"changed").unwrap();
        let check = check_manifest(&root, &out, 2, None).unwrap();
        assert_eq!(check.changed, vec!["a.txt".to_string()]);
        assert!(!check.identical());

        std::fs::write(root.join("extra.txt"), b"x").unwrap();
        let check = check_manifest(&root, &out, 2, None).unwrap();
        assert_eq!(check.extra, vec!["extra.txt".to_string()]);
        assert!(!check.identical());
    }

    #[test]
    fn verify_двух_каталогов_находит_размер_и_содержимое() {
        let a = temp_dir("verify-a");
        let b = temp_dir("verify-b");
        for dir in [&a, &b] {
            std::fs::create_dir_all(dir.join("sub")).unwrap();
        }
        std::fs::write(a.join("same.jpg"), b"hello").unwrap();
        std::fs::write(b.join("same.jpg"), b"hello").unwrap();
        std::fs::write(a.join("size.jpg"), b"aa").unwrap();
        std::fs::write(b.join("size.jpg"), b"bbb").unwrap();
        std::fs::write(a.join("sub/content.jpg"), b"hello").unwrap();
        std::fs::write(b.join("sub/content.jpg"), b"world").unwrap();
        std::fs::write(a.join("only-a.jpg"), b"a").unwrap();
        std::fs::write(b.join("only-b.jpg"), b"b").unwrap();

        let r = verify(&a, &b, 2, None).unwrap();
        assert_eq!(r.same, 1);
        assert_eq!(r.size_diff.len(), 1);
        assert_eq!(r.content_diff, vec!["sub/content.jpg".to_string()]);
        assert_eq!(r.only_a, vec!["only-a.jpg".to_string()]);
        assert_eq!(r.only_b, vec!["only-b.jpg".to_string()]);
        assert!(!r.identical());
    }

    #[test]
    fn отмена_не_пишет_неполный_слепок() {
        let root = temp_dir("cancelled");
        std::fs::write(root.join("a.txt"), b"hello").unwrap();
        let out = root.join("list.sha256");
        // Неполный слепок не должен появиться на диске: иначе проверка объявит
        // непрочитанные файлы «лишними».
        let p = Arc::new(Progress::default());
        p.cancel();
        let stats = manifest(&root, Some(&out), 1, Some(p)).unwrap();
        assert!(!stats.written);
        assert_eq!(stats.unchecked, 1);
        assert_eq!(stats.errors, 0);
        assert!(!out.exists());
    }

    /// Один и тот же текст видят CLI и экран «Целостность»: счётчики, пути и вердикт.
    #[test]
    fn отчёт_сверки_пишет_счётчики_пути_и_вердикт() {
        let r = Report {
            files_a: 2,
            files_b: 2,
            only_a: vec!["only.jpg".into()],
            only_b: Vec::new(),
            size_diff: vec![("size.jpg".into(), 10, 20)],
            content_diff: Vec::new(),
            read_errors: Vec::new(),
            same: 1,
            hashed: 1,
            hashed_bytes: 10,
            unchecked: 0,
            scan_errors: 0,
            skipped_a: 0,
            skipped_b: 0,
            cancelled: false,
            seconds: 1.5,
            lang: Lang::Ru,
        };
        let text = verify_report(Path::new("/a"), Path::new("/b"), &r, true);
        assert!(text.contains("только в A: 1"), "{text}");
        assert!(text.contains("только в A:\n  only.jpg"), "{text}");
        assert!(text.contains("размер отличается:\n  size.jpg"), "{text}");
        assert!(text.contains("вердикт: найдены расхождения"), "{text}");

        // Без «показать все» список обрезается и говорит об этом.
        let mut many = r;
        many.only_a = (0..25).map(|n| format!("p{n}.jpg")).collect();
        let text = verify_report(Path::new("/a"), Path::new("/b"), &many, false);
        assert!(text.contains("… ещё 5 (показать все)"), "{text}");
        assert!(!text.contains("p24.jpg"), "{text}");
    }

    /// Путь слепка в отчёте появляется только тогда, когда файл действительно записан:
    /// отменённый или неудачный проход не должен выглядеть как успех.
    #[test]
    fn отчёт_слепка_не_выдаёт_отмену_за_запись() {
        let stats = ManifestStats {
            files: 3,
            bytes: 30,
            skipped: 0,
            errors: 0,
            unchecked: 0,
            cancelled: false,
            written: true,
            seconds: 0.5,
            lang: Lang::Ru,
        };
        let text = manifest_report(
            Path::new("/archive"),
            Some(Path::new("/tmp/list.sha256")),
            &stats,
        );
        assert!(text.contains("слепок /archive: 3 файлов"), "{text}");
        assert!(text.contains("→ /tmp/list.sha256"), "{text}");

        let cancelled = ManifestStats {
            cancelled: true,
            written: false,
            ..stats
        };
        let text = manifest_report(
            Path::new("/archive"),
            Some(Path::new("/tmp/list.sha256")),
            &cancelled,
        );
        assert!(text.starts_with("отменено: "), "{text}");
        assert!(!text.contains("→"), "{text}");
    }

    #[test]
    fn отчёт_проверки_по_слепку_называет_расхождения() {
        let r = CheckReport {
            missing: vec!["gone.jpg".into()],
            extra: Vec::new(),
            changed: vec!["edit.jpg".into()],
            read_errors: Vec::new(),
            same: 4,
            unchecked: 0,
            scan_errors: 0,
            skipped: 1,
            cancelled: false,
            seconds: 0.2,
            lang: Lang::Ru,
        };
        let text = check_report(
            Path::new("/tmp/list.sha256"),
            Path::new("/archive"),
            &r,
            true,
        );
        assert!(text.contains("совпало: 4, изменено: 1"), "{text}");
        assert!(text.contains("нет в каталоге:\n  gone.jpg"), "{text}");
        assert!(text.contains("каталог расходится со слепком"), "{text}");
    }
}
