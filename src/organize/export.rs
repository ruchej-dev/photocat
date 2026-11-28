//! The album export: the tree, the numbering, the idempotent repeat and its report.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use super::plan::{component, confident_tag, tidy, validate_tokens};
use crate::i18n::{self, Lang};
use crate::{flag_str, has_flag};
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------- M20: the album export

/// The file name template of an export: the position in the album and the original name.
pub const EXPORT_TEMPLATE: &str = "{n}_{orig}";

/// The export placeholders. `{n}` is the position in the album, `{hash8}` the first 8 sha256 signs.
pub const EXPORT_PLACEHOLDERS: [&str; 8] = [
    "{n}", "{date}", "{time}", "{camera}", "{tag}", "{orig}", "{ext}", "{hash8}",
];

/// The length limit of one path component and of the whole path on disk (a margin up to 260 on Windows).
pub const MAX_NAME: usize = 120;
pub const MAX_PATH: usize = 240;

/// An album ready for export: a tree node and its files in album order.
#[derive(Clone, Debug)]
pub struct ExportAlbum {
    pub id: i64,
    pub parent_id: i64,
    pub name: String,
    pub files: Vec<FileRow>,
}

/// Data from the DB (`db::album_export_data`) turned into what `plan_album` understands.
pub fn export_albums(data: Vec<(db::AlbumNode, Vec<FileRow>)>) -> Vec<ExportAlbum> {
    data.into_iter()
        .map(|(node, files)| ExportAlbum {
            id: node.id,
            parent_id: node.parent_id,
            name: node.name,
            files,
        })
        .collect()
}

#[derive(Clone, Debug)]
pub struct ExportOptions {
    /// The folder inside which the root folder of the album is created.
    pub out: PathBuf,
    /// The file name template; `{n}_{orig}` by default.
    pub name: String,
    pub tags: HashMap<i64, Vec<(String, f32)>>,
    pub tag_show: f32,
    pub tag_margin: f32,
}

impl ExportOptions {
    pub fn new(out: impl Into<PathBuf>) -> Self {
        Self {
            out: out.into(),
            name: EXPORT_TEMPLATE.to_string(),
            tags: HashMap::new(),
            tag_show: 0.22,
            tag_margin: 0.01,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ExportDir {
    pub album_id: i64,
    /// The path relative to `out`; includes the name of the album root folder.
    pub rel: PathBuf,
    pub depth: usize,
    pub files: usize,
}

#[derive(Clone, Debug)]
pub struct ExportItem {
    pub album_id: i64,
    pub src: PathBuf,
    pub rel: PathBuf,
    pub sha256: String,
    pub size: i64,
}

/// The export plan: a folder tree and file copies. Nothing is written to disk.
#[derive(Clone, Debug, Default)]
pub struct ExportPlan {
    pub root_id: i64,
    pub out: PathBuf,
    /// The name of the album root folder (relative to `out`).
    pub root: PathBuf,
    pub dirs: Vec<ExportDir>,
    pub items: Vec<ExportItem>,
    /// Files in the app trash: they stay in the album but are not exported to disk.
    pub trashed: usize,
    /// Files whose full path is longer than `MAX_PATH`: skipped with a reason.
    pub too_long: usize,
    /// Names that got a `-2` collision already at the plan stage.
    pub renamed: usize,
    pub bytes: i64,
}

impl ExportPlan {
    pub fn files(&self) -> usize {
        self.items.len()
    }

    /// The summary line for the GUI and the CLI; numbers with units go through `db::human_in`.
    pub fn summary(&self, lang: Lang) -> String {
        let mut s = i18n::fill(
            lang,
            "folders {} · files {} · {}; in the trash and not exported: {}; names with a collision: {}; paths too long: {}",
            &[
                self.dirs.len().to_string(),
                self.items.len().to_string(),
                db::human_in(lang, self.bytes),
                self.trashed.to_string(),
                self.renamed.to_string(),
                self.too_long.to_string(),
            ],
        );
        let empty = self.dirs.iter().filter(|d| d.files == 0).count();
        if empty > 0 {
            s.push_str(&i18n::fill(
                lang,
                "; empty folders: {}",
                &[empty.to_string()],
            ));
        }
        if self.too_long > 0 {
            s.push_str(i18n::t(lang, "; those files are skipped"));
        }
        s
    }

    /// The plan tree: folders with indents and file names. `limit` lines for the window;
    /// the CLI prints everything.
    pub fn outline(&self, limit: usize) -> String {
        let mut by_album: HashMap<i64, Vec<&ExportItem>> = HashMap::new();
        for item in &self.items {
            by_album.entry(item.album_id).or_default().push(item);
        }
        let mut s = String::new();
        let mut used = 0usize;
        for dir in &self.dirs {
            if used >= limit {
                s.push_str("  …\n");
                break;
            }
            let name = dir
                .rel
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| dir.rel.to_string_lossy().to_string());
            let indent = "  ".repeat(dir.depth + 1);
            s.push_str(&format!("{indent}{name}/\n"));
            used += 1;
            if let Some(items) = by_album.get(&dir.album_id) {
                for item in items {
                    if used >= limit {
                        s.push_str(&format!("{indent}  …\n"));
                        break;
                    }
                    let file = item
                        .rel
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    s.push_str(&format!("{indent}  {file}\n"));
                    used += 1;
                }
            }
        }
        s
    }
}

/// The export report. Not a single file in the target was overwritten or deleted.
#[derive(Clone, Debug, Default)]
pub struct ExportReport {
    pub copied: usize,
    /// Files exported already, including under an old number (`stale`).
    pub skipped: usize,
    /// How many skipped files lie in the target under another name: the order is stale.
    pub stale: usize,
    /// Copied under a `-2` name because the name was taken.
    pub renamed: usize,
    pub failed: usize,
    pub bytes: u64,
    pub dirs_created: usize,
    pub cancelled: bool,
    /// What exactly was created — for the report and the "cancel in the middle" test.
    pub created: Vec<PathBuf>,
    pub errors: Vec<String>,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl fmt::Display for ExportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = i18n::fill(
            self.lang,
            "exported {} files · {}; {} already exported skipped ({} stale order in the target); {} renamed because of a collision; {} folders created; {} errors",
            &[
                self.copied.to_string(),
                db::human_in(self.lang, self.bytes as i64),
                self.skipped.to_string(),
                self.stale.to_string(),
                self.renamed.to_string(),
                self.dirs_created.to_string(),
                self.failed.to_string(),
            ],
        );
        let tail = if self.cancelled {
            i18n::t(self.lang, "; stopped")
        } else {
            ""
        };
        write!(f, "{text}{tail}")
    }
}

/// Checking the export template: `{n}` and `{hash8}` exist here, but not in the showcase.
pub fn validate_export_template(tpl: &str) -> Result<()> {
    validate_tokens(tpl, &EXPORT_PLACEHOLDERS)
}

/// `--out` must exist, be a folder and not lie inside the archive folder.
pub fn check_out_dir(out: &Path, roots: &[String]) -> Result<()> {
    if !out.exists() {
        anyhow::bail!(
            "the target folder does not exist: {} — create it yourself",
            out.display()
        );
    }
    if !out.is_dir() {
        anyhow::bail!("the target is not a folder: {}", out.display());
    }
    let canon = out.canonicalize().unwrap_or_else(|_| out.to_path_buf());
    for root in roots {
        if let Ok(root_canon) = Path::new(root).canonicalize()
            && (canon == root_canon || canon.starts_with(&root_canon))
        {
            anyhow::bail!(
                "an export inside the archive folder is forbidden: {} — choose a folder outside {}",
                out.display(),
                root
            );
        }
    }
    Ok(())
}

/// The export plan: the folder tree of the album and file copies. Creates nothing.
pub fn plan_album(albums: &[ExportAlbum], opts: &ExportOptions) -> Result<ExportPlan> {
    let Some(root) = albums.first() else {
        anyhow::bail!("the album is empty — there is nothing to export");
    };
    let root_name = dir_component(&root.name, root.id);
    let root_rel = PathBuf::from(&root_name);
    let root_path = opts.out.join(&root_rel);
    if root_path.exists() && !root_path.is_dir() {
        anyhow::bail!(
            "{} already holds a file “{root_name}” where a folder is needed",
            opts.out.display()
        );
    }
    let mut plan = ExportPlan {
        root_id: root.id,
        out: opts.out.clone(),
        root: root_rel.clone(),
        ..Default::default()
    };
    let mut dir_rel: HashMap<i64, PathBuf> = HashMap::new();
    dir_rel.insert(root.id, root_rel.clone());
    // Names among siblings: compared case-insensitively everywhere, not only on Windows
    // and macOS. Albums in the DB already ignore case, so being stricter means being
    // more predictable.
    let mut used: HashMap<i64, HashSet<String>> = HashMap::new();
    used.entry(root.id)
        .or_default()
        .insert(name_key(&root_name));

    for album in albums {
        let rel = if album.id == root.id {
            root_rel.clone()
        } else {
            let Some(parent) = dir_rel.get(&album.parent_id).cloned() else {
                continue;
            };
            let base = dir_component(&album.name, album.id);
            let taken = used.entry(album.parent_id).or_default();
            let mut name = base.clone();
            let mut k = 2;
            while taken.contains(&name_key(&name)) {
                name = format!("{base}-{k}");
                k += 1;
                plan.renamed += 1;
            }
            taken.insert(name_key(&name));
            parent.join(name)
        };
        dir_rel.insert(album.id, rel.clone());
        let live: Vec<&FileRow> = album
            .files
            .iter()
            .filter(|r| r.trashed_at.is_none())
            .collect();
        plan.trashed += album.files.len() - live.len();
        let depth = rel.components().count().saturating_sub(1);
        plan.dirs.push(ExportDir {
            album_id: album.id,
            rel: rel.clone(),
            depth,
            files: live.len(),
        });
        let width = number_width(live.len());
        let mut used_names: HashSet<String> = HashSet::new();
        for (position, row) in live.iter().enumerate() {
            let mut name = export_name(&opts.name, row, opts, position + 1, width);
            let (stem, ext) = split_name(&name);
            let mut k = 2;
            while used_names.contains(&name_key(&name)) {
                name = if ext.is_empty() {
                    format!("{stem}-{k}")
                } else {
                    format!("{stem}-{k}.{ext}")
                };
                k += 1;
                plan.renamed += 1;
            }
            used_names.insert(name_key(&name));
            let file_rel = rel.join(&name);
            let full = opts.out.join(&file_rel);
            if full.as_os_str().len() > MAX_PATH {
                plan.too_long += 1;
                continue;
            }
            plan.items.push(ExportItem {
                album_id: album.id,
                src: PathBuf::from(&row.path),
                rel: file_rel,
                sha256: row.sha256.clone(),
                size: row.size,
            });
            plan.bytes += row.size;
        }
    }
    Ok(plan)
}

/// Writing the plan to disk: copies only, nothing is overwritten.
/// An existing file with the same sha256 is a skip (the pass is idempotent); with other
/// content it gets a new `-2` name; a file with the same content under an old number
/// means "the order in the target is stale".
pub fn apply_export(plan: &ExportPlan, lang: Lang, progress: Option<&Progress>) -> ExportReport {
    apply_export_in(plan, lang, progress, &db::cache_dir().join("exports"))
}

/// The same with an explicit cache folder: the tests do not write into the owner's cache.
pub(super) fn apply_export_in(
    plan: &ExportPlan,
    lang: Lang,
    progress: Option<&Progress>,
    cache_dir: &Path,
) -> ExportReport {
    let mut report = ExportReport::default();
    if let Some(p) = progress {
        p.set_total(plan.items.len());
        p.set_msg(i18n::t(lang, "exporting the album"));
    }
    let cache_key = format!("{}-{:08x}", plan.root_id, path_hash(&plan.out));
    let mut cache = load_export_cache(cache_dir, &cache_key);
    let mut hashes: HashMap<PathBuf, String> = HashMap::new();
    let mut created: HashSet<PathBuf> = HashSet::new();
    let mut entries: HashMap<PathBuf, Vec<(String, u64)>> = HashMap::new();

    // The structure matters more than the content: an empty album gives an empty folder too.
    for dir in &plan.dirs {
        let path = plan.out.join(&dir.rel);
        if path.is_dir() {
            continue;
        }
        match std::fs::create_dir_all(&path) {
            Ok(()) => report.dirs_created += 1,
            Err(e) => note_export_error(
                &mut report,
                format!("folder {}: {e}", path.display()),
            ),
        }
    }

    for item in &plan.items {
        if progress.is_some_and(|p| p.cancelled()) {
            report.cancelled = true;
            break;
        }
        let failed_before = report.failed;
        'item: {
            let Ok(meta) = std::fs::metadata(&item.src) else {
                note_export_error(
                    &mut report,
                    format!("no such file: {}", item.src.display()),
                );
                break 'item;
            };
            if !meta.is_file() {
                note_export_error(
                    &mut report,
                    format!("not a regular file: {}", item.src.display()),
                );
                break 'item;
            }
            let parent_rel = item
                .rel
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default();
            let parent = plan.out.join(&parent_rel);
            let planned = plan.out.join(&item.rel);
            let outcome = {
                let list = entries
                    .entry(parent.clone())
                    .or_insert_with(|| read_entries(&parent));
                decide_export(item, &planned, list, &plan.out, &mut cache, &mut hashes, &created)
            };
            match outcome {
                ExportOutcome::Skip => report.skipped += 1,
                ExportOutcome::Stale => {
                    report.skipped += 1;
                    report.stale += 1;
                }
                ExportOutcome::Copy { dst, renamed } => match std::fs::copy(&item.src, &dst) {
                    Ok(bytes) => {
                        report.copied += 1;
                        report.bytes += bytes;
                        if renamed {
                            report.renamed += 1;
                        }
                        report.created.push(dst.clone());
                        created.insert(dst.clone());
                        let name = dst
                            .file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        if let Some(list) = entries.get_mut(&parent) {
                            list.push((name, bytes));
                        }
                        if let Ok(rel) = dst.strip_prefix(&plan.out) {
                            cache.insert(
                                rel.to_string_lossy().to_string(),
                                (bytes, mtime_ms(&dst), item.sha256.clone()),
                            );
                        }
                    }
                    Err(e) => note_export_error(
                        &mut report,
                        format!("copy {}: {e}", dst.display()),
                    ),
                },
                ExportOutcome::Fail(msg) => note_export_error(&mut report, msg),
            }
        }
        if let Some(p) = progress {
            p.tick();
            if report.failed > failed_before {
                p.error();
            }
        }
    }

    // The "already exported" cache lives in the app cache, not in `--out`: the target stays clean.
    cache.retain(|rel, _| plan.out.join(rel).is_file());
    save_export_cache(cache_dir, &cache_key, &cache);
    if let Some(p) = progress {
        p.finish();
    }
    report
}

/// The decision on one file: what to do with a name that already exists in the target.
enum ExportOutcome {
    Skip,
    Stale,
    Copy { dst: PathBuf, renamed: bool },
    Fail(String),
}

fn decide_export(
    item: &ExportItem,
    planned: &Path,
    list: &[(String, u64)],
    out: &Path,
    cache: &mut ExportCache,
    hashes: &mut HashMap<PathBuf, String>,
    created: &HashSet<PathBuf>,
) -> ExportOutcome {
    let name = item
        .rel
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let exact = list.iter().find(|(n, _)| same_name(n, &name));
    if let Some((_, size)) = exact
        && *size == item.size as u64
    {
        match target_sha(planned, *size, out, cache, hashes) {
            Some(sha) if sha == item.sha256 => return ExportOutcome::Skip,
            Some(_) => {}
            None => {
                return ExportOutcome::Fail(format!(
                    "cannot read the content of the target: {}",
                    planned.display()
                ));
            }
        }
    }
    // The same photo lies in the folder under another number: the order in the target is stale.
    if find_same_content(item, &name, planned, list, out, cache, hashes, created) {
        return ExportOutcome::Stale;
    }
    if exact.is_some() {
        return ExportOutcome::Copy {
            dst: free_numbered(planned, list),
            renamed: true,
        };
    }
    ExportOutcome::Copy {
        dst: planned.to_path_buf(),
        renamed: false,
    }
}

/// Whether this folder holds the same sha256 under another name (and it is not a file
/// created in this very pass: two identical copies in an album are two rows).
#[allow(clippy::too_many_arguments)]
fn find_same_content(
    item: &ExportItem,
    own_name: &str,
    planned: &Path,
    list: &[(String, u64)],
    out: &Path,
    cache: &mut ExportCache,
    hashes: &mut HashMap<PathBuf, String>,
    created: &HashSet<PathBuf>,
) -> bool {
    let parent = planned.parent().unwrap_or(Path::new(""));
    for (name, size) in list {
        if same_name(name, own_name) || *size != item.size as u64 {
            continue;
        }
        let path = parent.join(name);
        if created.contains(&path) {
            continue;
        }
        if target_sha(&path, *size, out, cache, hashes).as_deref() == Some(item.sha256.as_str()) {
            return true;
        }
    }
    false
}

/// The export cache: relative path → (size, mtime ms, sha256).
type ExportCache = HashMap<String, (u64, u64, String)>;

/// The sha256 of a file in the target: the export cache first (if size and mtime match),
/// otherwise an honest read. A content swap that keeps the mtime costs the same as in
/// indexing (AGENTS §8).
fn target_sha(
    path: &Path,
    size: u64,
    out: &Path,
    cache: &mut ExportCache,
    hashes: &mut HashMap<PathBuf, String>,
) -> Option<String> {
    if let Some(sha) = hashes.get(path) {
        return Some(sha.clone());
    }
    let mtime = mtime_ms(path);
    let rel = path.strip_prefix(out).ok()?.to_string_lossy().to_string();
    if let Some((cached_size, cached_mtime, sha)) = cache.get(&rel)
        && *cached_size == size
        && (*cached_mtime == mtime || mtime == 0)
    {
        hashes.insert(path.to_path_buf(), sha.clone());
        return Some(sha.clone());
    }
    let sha = derive::sha256_file(path).ok()?;
    hashes.insert(path.to_path_buf(), sha.clone());
    cache.insert(rel, (size, mtime, sha.clone()));
    Some(sha)
}

fn mtime_ms(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn read_entries(dir: &Path) -> Vec<(String, u64)> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    rd.filter_map(|e| e.ok())
        .filter_map(|e| {
            let meta = e.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((e.file_name().to_string_lossy().to_string(), meta.len()))
        })
        .collect()
}

/// The first free `name-2.ext`, `name-3.ext`, … in this folder.
fn free_numbered(planned: &Path, list: &[(String, u64)]) -> PathBuf {
    let name = planned
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let (stem, ext) = split_name(&name);
    for k in 2..10_000 {
        let candidate = if ext.is_empty() {
            format!("{stem}-{k}")
        } else {
            format!("{stem}-{k}.{ext}")
        };
        if !list.iter().any(|(n, _)| same_name(n, &candidate)) {
            return planned.with_file_name(candidate);
        }
    }
    planned.to_path_buf()
}

fn note_export_error(report: &mut ExportReport, msg: String) {
    report.failed += 1;
    if report.errors.len() < 20 {
        report.errors.push(msg);
    }
}

/// `stem` and `ext` without a dot; the extension is the last dot, as in `Path`.
fn split_name(name: &str) -> (String, String) {
    let path = Path::new(name);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| name.to_string());
    let ext = path
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    (stem, ext)
}

/// The width of the position number: 2 digits up to 99, 3 up to 999, then by the digit count.
pub(super) fn number_width(count: usize) -> usize {
    if count <= 99 {
        2
    } else if count <= 999 {
        3
    } else {
        count.to_string().len()
    }
}

/// A file name by the export template. An empty name is replaced with `{hash8}` (and without
/// a hash with `file`); the component is no longer than `MAX_NAME`.
pub(super) fn export_name(tpl: &str, r: &FileRow, opts: &ExportOptions, n: usize, width: usize) -> String {
    let src = Path::new(&r.path);
    let ext = src
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let camera = r.camera.as_deref().map(component).unwrap_or_default();
    let tag = opts
        .tags
        .get(&r.id)
        .and_then(|list| confident_tag(list, opts.tag_show, opts.tag_margin))
        .map(|(tag, _)| component(tag))
        .unwrap_or_default();
    let (date, time) = taken_parts(r.taken_at.as_deref());
    let hash8: String = r.sha256.chars().take(8).collect();
    let mut name = tpl
        .replace("{n}", &format!("{n:0width$}"))
        .replace("{date}", &date)
        .replace("{time}", &time)
        .replace("{camera}", &camera)
        .replace("{tag}", &tag)
        .replace("{orig}", &component(&stem))
        .replace("{ext}", &ext)
        .replace("{hash8}", &hash8);
    name = tidy(&name);
    if name.is_empty() {
        name = if hash8.is_empty() {
            "file".to_string()
        } else {
            hash8
        };
    }
    let explicit_ext = tpl.contains("{ext}") || ext.is_empty();
    if explicit_ext {
        return name.chars().take(MAX_NAME).collect();
    }
    let suffix = format!(".{ext}");
    let keep = MAX_NAME.saturating_sub(suffix.chars().count());
    let base: String = name.chars().take(keep).collect();
    format!("{}{suffix}", base.trim_end_matches(['_', '-', '.']))
}

/// The date and time from `taken_at` for the substitutions: `20240517` and `144243`.
fn taken_parts(taken: Option<&str>) -> (String, String) {
    let Some(taken) = taken else {
        return (String::new(), String::new());
    };
    let (date, time) = taken.split_once('T').unwrap_or((taken, ""));
    let date: String = date.chars().filter(|c| c.is_ascii_digit()).collect();
    let time: String = time
        .chars()
        .filter(|c| c.is_ascii_digit())
        .take(6)
        .collect();
    (date, time)
}

/// The folder name of an album: safe characters, ≤ `MAX_NAME`, empty means `album-<id>`.
fn dir_component(name: &str, id: i64) -> String {
    let clean = safe_component(name, MAX_NAME);
    if clean.is_empty() {
        format!("album-{id}")
    } else {
        clean
    }
}

fn safe_component(s: &str, max: usize) -> String {
    let name: String = tidy_sep(s).chars().take(max).collect();
    let name = name.trim_end_matches(['_', '-', '.']).to_string();
    if is_windows_reserved(&name) {
        format!("_{name}")
    } else {
        name
    }
}

/// Like `tidy`, but a forbidden character becomes a separator instead of disappearing:
/// "Dacha/2024" and "Dacha:2024" give one name `Dacha_2024`, and the collision is visible
/// instead of silently turning into another name.
fn tidy_sep(s: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in s.chars() {
        let c = if c.is_alphanumeric() || "-_.()[]".contains(c) {
            c
        } else {
            '_'
        };
        if c == '_' || c == '-' {
            if sep || out.is_empty() {
                continue;
            }
            sep = true;
        } else {
            sep = false;
        }
        out.push(c);
    }
    out.trim_end_matches(['_', '-', '.']).to_string()
}

/// Windows device names cannot be used as a file or folder name.
fn is_windows_reserved(name: &str) -> bool {
    let upper = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_uppercase();
    matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || (upper.len() == 4
            && (upper.starts_with("COM") || upper.starts_with("LPT"))
            && upper.as_bytes()[3].is_ascii_digit())
}

/// The comparison key of names: case-insensitive on every OS.
fn name_key(s: &str) -> String {
    s.to_lowercase()
}

/// Comparing a name with an entry in the target: on Windows and macOS the file system
/// ignores case, on Linux it does not.
fn same_name(a: &str, b: &str) -> bool {
    #[cfg(any(windows, target_os = "macos"))]
    {
        name_key(a) == name_key(b)
    }
    #[cfg(not(any(windows, target_os = "macos")))]
    {
        a == b
    }
}

/// The cache is bound to the album and the target folder: another `--out` means another cache.
fn export_cache_path(cache_dir: &Path, key: &str) -> PathBuf {
    cache_dir.join(format!("{key}.txt"))
}

fn path_hash(path: &Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    path.hash(&mut h);
    h.finish()
}

fn load_export_cache(cache_dir: &Path, key: &str) -> ExportCache {
    let mut out = HashMap::new();
    let Ok(text) = std::fs::read_to_string(export_cache_path(cache_dir, key)) else {
        return out;
    };
    for line in text.lines() {
        let mut it = line.splitn(4, '\t');
        let (Some(sha), Some(size), Some(mtime), Some(rel)) =
            (it.next(), it.next(), it.next(), it.next())
        else {
            // The old format (sha, size, rel) is not trusted: the file is read once more.
            continue;
        };
        if sha.len() != 64 {
            continue;
        }
        if let (Ok(size), Ok(mtime)) = (size.parse::<u64>(), mtime.parse::<u64>()) {
            out.insert(rel.to_string(), (size, mtime, sha.to_string()));
        }
    }
    out
}

/// The cache speeds a repeat up: a known sha256 is not read from disk if the size matches.
/// A cache write error does not fail the export — the target is created already.
fn save_export_cache(cache_dir: &Path, key: &str, cache: &ExportCache) {
    let path = export_cache_path(cache_dir, key);
    if let Some(dir) = path.parent()
        && std::fs::create_dir_all(dir).is_err()
    {
        return;
    }
    let mut lines: Vec<String> = cache
        .iter()
        .map(|(rel, (size, mtime, sha))| format!("{sha}\t{size}\t{mtime}\t{rel}"))
        .collect();
    lines.sort();
    let _ = std::fs::write(&path, lines.join("\n"));
}

/// CLI: `photocat export --album N --out DIR [--name TEMPLATE] [--apply]`.
/// Without `--apply` — the plan tree only; nothing is created on disk.
pub fn export_cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;
    let album = match flag_str(args, "--album").and_then(|v| v.parse::<i64>().ok()) {
        Some(id) if id > 0 => id,
        _ => {
            eprintln!(
                "{}",
                i18n::t(
                    lang,
                    "an album number is needed: export --album N --out DIR [--name TEMPLATE] [--apply]"
                )
            );
            std::process::exit(2);
        }
    };
    let out = match flag_str(args, "--out") {
        Some(o) => PathBuf::from(o),
        None => {
            eprintln!(
                "{}",
                i18n::t(
                    lang,
                    "a destination folder is needed: export --album N --out DIR [--apply]"
                )
            );
            std::process::exit(2);
        }
    };
    let name = flag_str(args, "--name").unwrap_or_else(|| EXPORT_TEMPLATE.to_string());
    if let Err(e) = validate_export_template(&name) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    if let Err(e) = check_out_dir(&out, &settings.roots) {
        eprintln!("{e}");
        std::process::exit(2);
    }
    let data = match db::album_export_data(&conn, album) {
        Ok(data) => data,
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(2);
        }
    };
    let tags = if name.contains("{tag}") {
        db::load_visible_tags(&conn)?
    } else {
        HashMap::new()
    };
    let mut opts = ExportOptions::new(&out);
    opts.name = name;
    opts.tags = tags;
    opts.tag_show = settings.tag_show;
    opts.tag_margin = settings.tag_margin;
    let plan = match plan_album(&export_albums(data), &opts) {
        Ok(plan) => plan,
        Err(e) => {
            eprintln!("{e:#}");
            std::process::exit(2);
        }
    };
    let root_name = plan.root.display().to_string();
    println!(
        "{}",
        i18n::fill(
            lang,
            "album export: {} → {}",
            &[root_name, plan.out.join(&plan.root).display().to_string()]
        )
    );
    println!("{}", plan.summary(lang));
    print!("{}", plan.outline(usize::MAX));
    if !has_flag(args, "--apply") {
        println!(
            "\n{}",
            i18n::t(
                lang,
                "this is a plan (dry-run). Add --apply to copy the files (not links)."
            )
        );
        return Ok(());
    }
    let progress = std::sync::Arc::new(Progress::default());
    let show = progress.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        loop {
            let (d, t, e) = (show.done(), show.total(), show.errors());
            if t > 0 {
                let line = i18n::fill(
                    lang,
                    "export: {}/{} files, {} errors",
                    &[d.to_string(), t.to_string(), e.to_string()],
                );
                eprint!("\r{line}");
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
    let mut report = apply_export(&plan, Lang::default(), Some(&progress));
    report.lang = lang;
    println!("{report}");
    for line in &report.errors {
        println!("  {line}");
    }
    if report.failed > 0 {
        std::process::exit(1);
    }
    Ok(())
}
