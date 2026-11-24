//! Arranging the archive by dates: plan → confirmation → apply (hardlink/copy/symlink).
//! The originals are neither deleted nor renamed: the showcase is new files or links next to
//! the archive, not over it. The names in the showcase come from a template (original by default).

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use crate::{flag_str, has_flag};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LinkMode {
    Hard,
    Copy,
    Symlink,
}

/// The depth of the showcase. `Event` is a day plus a place by GPS (a ~1 km cell): there is no
/// reverse geocoding (the network is forbidden), so the place is named by its coordinates and photos without GPS stay in the day folder.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layout {
    Day,
    Month,
    Event,
}

impl Layout {
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "day" | "день" => Some(Self::Day),
            "month" | "месяц" => Some(Self::Month),
            "event" | "событие" => Some(Self::Event),
            _ => None,
        }
    }

    pub fn title(self, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            match self {
                Self::Day => "by day",
                Self::Month => "by month",
                Self::Event => "by event (day + place)",
            },
        )
    }
}

/// The placeholders of the name template. `{ext}` is written without a dot; if the template has
/// no extension, the one of the source file is added.
pub const PLACEHOLDERS: [&str; 6] = ["{date}", "{time}", "{camera}", "{tag}", "{orig}", "{ext}"];

/// The default template for `--rename`: the same the rename flag used to give.
pub const RENAME_TEMPLATE: &str = "{date}_{time}_{orig}";

pub struct Options {
    pub out: PathBuf,
    pub layout: Layout,
    /// The name template without an extension; `None` means the original name.
    pub name: Option<String>,
    /// Leave out the files marked spare in the duplicates.
    pub exclude_dup: bool,
    /// The confident tag for `{tag}`: the visible tags and the thresholds from the settings.
    pub tags: HashMap<i64, Vec<(String, f32)>>,
    pub tag_show: f32,
    pub tag_margin: f32,
}

impl Options {
    pub fn new(out: impl Into<PathBuf>) -> Self {
        Self {
            out: out.into(),
            layout: Layout::Day,
            name: None,
            exclude_dup: false,
            tags: HashMap::new(),
            tag_show: 0.22,
            tag_margin: 0.01,
        }
    }
}

pub struct PlanItem {
    pub src: PathBuf,
    pub dst: PathBuf,
}

pub struct Plan {
    pub items: Vec<PlanItem>,
    /// Photos without a date do not reach the showcase.
    pub no_date: usize,
    /// Marked spare and left out by `--exclude-dup`.
    pub excluded: usize,
    /// How many showcase folders will come out (days, months or events).
    pub dirs: usize,
}

/// Checking the template: an unknown placeholder is an error at once, not in the middle of a pass.
pub fn validate_template(tpl: &str) -> Result<()> {
    validate_tokens(tpl, &PLACEHOLDERS)
}

fn validate_tokens(tpl: &str, allowed: &[&str]) -> Result<()> {
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        let after = &rest[start..];
        let Some(end) = after.find('}') else {
            anyhow::bail!("an unclosed brace in the name template: {after}");
        };
        let token = &after[..=end];
        if !allowed.contains(&token) {
            anyhow::bail!(
                "an unknown placeholder {token}; allowed: {}",
                allowed.join(" ")
            );
        }
        rest = &after[end + 1..];
    }
    Ok(())
}

pub fn build_plan(rows: &[FileRow], opts: &Options) -> Plan {
    let mut used: BTreeMap<String, u32> = BTreeMap::new();
    let mut dirs: BTreeMap<String, ()> = BTreeMap::new();
    let mut items = Vec::new();
    let mut no_date = 0;
    let mut excluded = 0;
    for r in rows {
        if opts.exclude_dup && r.decision.as_deref() == Some("drop") {
            excluded += 1;
            continue;
        }
        let Some(taken) = r.taken_at.as_deref() else {
            no_date += 1;
            continue;
        };
        let (date, time) = taken.split_once('T').unwrap_or((taken, ""));
        let mut it = date.split('-');
        let (Some(y), Some(m), Some(d)) = (it.next(), it.next(), it.next()) else {
            no_date += 1;
            continue;
        };
        let src = PathBuf::from(&r.path);
        let ext = src
            .extension()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let orig_name = src
            .file_name()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".into());
        let stem = src
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let name = match opts.name.as_deref() {
            Some(tpl) => template_name(tpl, r, opts, (y, m, d), time, &stem, &ext),
            None => orig_name,
        };
        let dir = layout_dir(opts, r, y, m, d);
        dirs.insert(dir.to_string_lossy().to_string(), ());
        items.push(PlanItem {
            src,
            dst: dir.join(&name),
        });
    }
    // The order is by future paths first (ties by source), then the collisions are numbered:
    // so `-2` reliably follows the base name.
    items.sort_by(|a, b| a.dst.cmp(&b.dst).then_with(|| a.src.cmp(&b.src)));
    for it in &mut items {
        let key = it.dst.to_string_lossy().to_string();
        let n = used.entry(key).or_insert(0);
        *n += 1;
        if *n > 1 {
            it.dst = numbered(&it.dst, *n);
        }
    }
    Plan {
        items,
        no_date,
        excluded,
        dirs: dirs.len(),
    }
}

fn layout_dir(opts: &Options, r: &FileRow, y: &str, m: &str, d: &str) -> PathBuf {
    let out = &opts.out;
    match opts.layout {
        Layout::Month => out.join(y).join(format!("{y}-{m}")),
        Layout::Day => out.join(y).join(format!("{y}-{m}-{d}")),
        Layout::Event => {
            let day = out.join(y).join(format!("{y}-{m}-{d}"));
            match (r.lat, r.lon) {
                (Some(lat), Some(lon)) => day.join(gps_cell(lat, lon)),
                _ => day,
            }
        }
    }
}

/// A GPS cell of ~1 km (two decimals): neighbouring frames of one walk land together, frames
/// from different places land apart. Without reverse geocoding the name is the coordinates.
pub fn gps_cell(lat: f64, lon: f64) -> String {
    let lat = lat.clamp(-90.0, 90.0);
    let lon = lon.clamp(-180.0, 180.0);
    let (lat, ns) = if lat < 0.0 { (-lat, 'S') } else { (lat, 'N') };
    let (lon, ew) = if lon < 0.0 { (-lon, 'W') } else { (lon, 'E') };
    let (lat, ns) = if lat < 0.005 { (0.0, 'N') } else { (lat, ns) };
    let (lon, ew) = if lon < 0.005 { (0.0, 'E') } else { (lon, ew) };
    format!("{lat:.2}{ns}_{lon:.2}{ew}")
}

fn template_name(
    tpl: &str,
    r: &FileRow,
    opts: &Options,
    (y, m, d): (&str, &str, &str),
    time: &str,
    stem: &str,
    ext: &str,
) -> String {
    let camera = r.camera.as_deref().map(component).unwrap_or_default();
    let tag = opts
        .tags
        .get(&r.id)
        .and_then(|list| confident_tag(list, opts.tag_show, opts.tag_margin))
        .map(|(tag, _)| component(tag))
        .unwrap_or_default();
    let date = format!("{y}{m}{d}");
    let digits: String = time.chars().filter(|c| c.is_ascii_digit()).take(6).collect();
    let mut name = tpl
        .replace("{date}", &date)
        .replace("{time}", &digits)
        .replace("{camera}", &camera)
        .replace("{tag}", &tag)
        .replace("{orig}", &component(stem))
        .replace("{ext}", ext);
    name = tidy(&name);
    let name = if name.is_empty() {
        let fallback = tidy(stem);
        if fallback.is_empty() { "file".to_string() } else { fallback }
    } else {
        name
    };
    let name: String = name.chars().take(120).collect();
    let name = name.trim_end_matches(['_', '-', '.']).to_string();
    if tpl.contains("{ext}") || ext.is_empty() {
        name
    } else {
        format!("{name}.{ext}")
    }
}

/// The confident top tag — the same formula as `embed::confident_tag_with`.
/// Duplicated on purpose: `organize` does not depend on the model or its module (the layer rule).
fn confident_tag(scores: &[(String, f32)], show: f32, margin: f32) -> Option<(&str, f32)> {
    let (tag, top) = scores.first()?;
    if *top < show {
        return None;
    }
    if let Some((_, second)) = scores.get(1)
        && top - second < margin
    {
        return None;
    }
    Some((tag.as_str(), *top))
}

/// One substitution: only characters safe for any file system, 40 characters at most.
fn component(s: &str) -> String {
    tidy(s).chars().take(40).collect::<String>()
        .trim_end_matches(['_', '-', '.'])
        .to_string()
}

/// Spaces → `_`, forbidden characters dropped, repeated and trailing separators removed.
fn tidy(s: &str) -> String {
    let mut out = String::new();
    let mut sep = false;
    for c in s.chars() {
        let c = if c.is_whitespace() { '_' } else { c };
        if !(c.is_alphanumeric() || "-_.()[]".contains(c)) {
            continue;
        }
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

/// `file.jpg` + 2 → `file-2.jpg`; a file without an extension gets no trailing dot.
fn numbered(dst: &Path, n: u32) -> PathBuf {
    let ext = dst
        .extension()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = dst
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "file".into());
    let name = if ext.is_empty() {
        format!("{stem}-{n}")
    } else {
        format!("{stem}-{n}.{ext}")
    };
    dst.with_file_name(name)
}

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
fn apply_export_in(
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
fn number_width(count: usize) -> usize {
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
fn export_name(tpl: &str, r: &FileRow, opts: &ExportOptions, n: usize, width: usize) -> String {
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

pub fn cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let settings = db::load_settings(&conn);
    let lang = settings.lang;
    let out = match flag_str(args, "--out") {
        Some(o) => PathBuf::from(o),
        None => {
            eprintln!(
                "{}",
                i18n::t(
                    lang,
                    "a destination folder is needed: organize --out DIR [--apply] [--by day|month|event]"
                )
            );
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
    let layout = match flag_str(args, "--by") {
        None => Layout::Day,
        Some(v) => match Layout::parse(&v) {
            Some(l) => l,
            None => {
                eprintln!(
                    "{}",
                    i18n::fill(
                        lang,
                        "--by: expected day, month or event, got “{}”",
                        &[v]
                    )
                );
                std::process::exit(2);
            }
        },
    };
    let name = match flag_str(args, "--name") {
        Some(tpl) => Some(tpl),
        None if has_flag(args, "--rename") => Some(RENAME_TEMPLATE.to_string()),
        None => None,
    };
    if let Some(tpl) = name.as_deref()
        && let Err(e) = validate_template(tpl)
    {
        eprintln!("{e}");
        std::process::exit(2);
    }
    let apply_it = has_flag(args, "--apply");

    let rows = db::load_all(&conn)?;
    let tags = if name.as_deref().is_some_and(|t| t.contains("{tag}")) {
        db::load_visible_tags(&conn)?
    } else {
        HashMap::new()
    };
    let mut opts = Options::new(&out);
    opts.layout = layout;
    opts.name = name;
    opts.exclude_dup = has_flag(args, "--exclude-dup");
    opts.tags = tags;
    opts.tag_show = settings.tag_show;
    opts.tag_margin = settings.tag_margin;
    let plan = build_plan(&rows, &opts);

    // Counted by the top folder of the showcase (a year or "no date"), not by the path depth:
    // events inside a day have one more folder for the place.
    let mut by_top: BTreeMap<String, usize> = BTreeMap::new();
    for it in &plan.items {
        let top = it
            .dst
            .strip_prefix(&out)
            .ok()
            .and_then(|p| p.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();
        *by_top.entry(top).or_insert(0) += 1;
    }
    println!(
        "{}",
        i18n::fill(
            lang,
            "showcase: {} → {}",
            &[layout.title(lang).to_string(), out.display().to_string()]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "files with a date: {}, without a date (not in the plan): {}, marked spare and left out: {}",
            &[
                plan.items.len().to_string(),
                plan.no_date.to_string(),
                plan.excluded.to_string(),
            ]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "showcase folders: {}",
            &[plan.dirs.to_string()]
        )
    );
    for (top, n) in &by_top {
        println!("  {top}: {n}");
    }
    println!("\n{}", i18n::t(lang, "examples:"));
    for it in plan.items.iter().take(5) {
        println!("  {}  →  {}", it.src.display(), it.dst.display());
    }
    if !apply_it {
        println!(
            "\n{}",
            i18n::fill(
                lang,
                "this is a plan (dry-run). Add --apply to create links/copies in {}",
                &[out.display().to_string()]
            )
        );
        return Ok(());
    }
    let (ok, fail) = apply(&plan.items, mode)?;
    println!(
        "{}",
        i18n::fill(
            lang,
            "created: {}, errors: {}",
            &[ok.to_string(), fail.to_string()]
        )
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: i64, path: &str, taken: Option<&str>) -> FileRow {
        FileRow {
            id,
            path: path.to_string(),
            size: 1000,
            mtime: 0,
            sha256: format!("{id:064}"),
            kind: "image".into(),
            width: 100,
            height: 100,
            taken_at: taken.map(|s| s.to_string()),
            taken_src: "exif".into(),
            lat: None,
            lon: None,
            camera: None,
            dhash: None,
            sharpness: None,
            duration_s: None,
            thumb: None,
            decision: None,
            trashed_at: None,
        }
    }

    fn plan_of(rows: &[FileRow], opts: &Options) -> Plan {
        build_plan(rows, opts)
    }

    fn dst(plan: &Plan, i: usize) -> String {
        plan.items[i].dst.to_string_lossy().to_string()
    }

    #[test]
    fn days_and_months_give_a_different_depth() {
        let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
        let mut opts = Options::new("/out");
        let day = plan_of(&rows, &opts);
        assert_eq!(dst(&day, 0), "/out/2024/2024-05-17/IMG_1.jpg");
        opts.layout = Layout::Month;
        let month = plan_of(&rows, &opts);
        assert_eq!(dst(&month, 0), "/out/2024/2024-05/IMG_1.jpg");
    }

    #[test]
    fn an_event_splits_a_day_by_place_and_no_gps_keeps_it_in_the_day() {
        let mut a = row(1, "/a/a.jpg", Some("2024-05-17T10:00:00"));
        a.lat = Some(55.751_244);
        a.lon = Some(37.617_494);
        let mut b = row(2, "/a/b.jpg", Some("2024-05-17T11:00:00"));
        b.lat = Some(55.751_9);
        b.lon = Some(37.617_1);
        let c = row(3, "/a/c.jpg", Some("2024-05-17T12:00:00"));
        let mut d = row(4, "/a/d.jpg", Some("2024-05-17T09:00:00"));
        d.lat = Some(59.93);
        d.lon = Some(30.31);
        let mut opts = Options::new("/out");
        opts.layout = Layout::Event;
        let plan = plan_of(&[a, b, c, d], &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/55.75N_37.62E/a.jpg");
        assert_eq!(dst(&plan, 1), "/out/2024/2024-05-17/55.75N_37.62E/b.jpg");
        assert_eq!(dst(&plan, 2), "/out/2024/2024-05-17/59.93N_30.31E/d.jpg");
        assert_eq!(dst(&plan, 3), "/out/2024/2024-05-17/c.jpg");
        assert_eq!(plan.dirs, 3);
    }

    #[test]
    fn the_gps_cell_knows_south_and_west() {
        assert_eq!(gps_cell(-33.868_8, -151.209_3), "33.87S_151.21W");
        assert_eq!(gps_cell(-0.001, 0.000_1), "0.00N_0.00E");
    }

    #[test]
    fn the_name_template_fills_date_time_camera_and_extension() {
        let mut r = row(1, "/a/DSC_0001.JPG", Some("2024-05-17T14:42:43"));
        r.camera = Some("Xiaomi Redmi Note 8 Pro".into());
        let mut opts = Options::new("/out");
        opts.name = Some("{date}_{time}_{camera}.{ext}".into());
        let plan = plan_of(&[r], &opts);
        assert_eq!(
            dst(&plan, 0),
            "/out/2024/2024-05-17/20240517_144243_Xiaomi_Redmi_Note_8_Pro.JPG"
        );
    }

    #[test]
    fn a_tag_goes_into_the_name_only_when_confident() {
        let r = row(1, "/a/x.jpg", Some("2024-05-17T14:42:43"));
        let mut opts = Options::new("/out");
        opts.name = Some("{date}_{tag}_{orig}".into());
        opts.tags
            .insert(1, vec![("дети".into(), 0.24), ("документ".into(), 0.05)]);
        let plan = plan_of(std::slice::from_ref(&r), &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_дети_x.jpg");

        opts.tags.insert(1, vec![("дети".into(), 0.24), ("еда".into(), 0.235)]);
        let plan = plan_of(&[r], &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_x.jpg");
    }

    #[test]
    fn empty_substitutions_leave_no_separators() {
        let r = row(1, "/a/x.jpg", Some("2024-05-17T14:42:43"));
        let mut opts = Options::new("/out");
        opts.name = Some("{tag}_{date}_{camera}_{orig}".into());
        let plan = plan_of(&[r], &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_x.jpg");
    }

    #[test]
    fn forbidden_characters_are_replaced_and_collisions_numbered() {
        let a = row(1, "/a/photo 1.jpg", Some("2024-05-17T14:42:43"));
        let b = row(2, "/b/photo 1.jpg", Some("2024-05-17T15:00:00"));
        let mut opts = Options::new("/out");
        opts.name = Some("{date}_{orig}".into());
        let plan = plan_of(&[a, b], &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_photo_1.jpg");
        assert_eq!(dst(&plan, 1), "/out/2024/2024-05-17/20240517_photo_1-2.jpg");
    }

    #[test]
    fn without_a_template_the_name_stays_original() {
        let rows = [row(1, "/a/IMG_20240517_144243.jpg", Some("2024-05-17T14:42:43"))];
        let plan = plan_of(&rows, &Options::new("/out"));
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/IMG_20240517_144243.jpg");
    }

    #[test]
    fn the_renaming_stays_compatible_with_the_old_format() {
        let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
        let mut opts = Options::new("/out");
        opts.name = Some(RENAME_TEMPLATE.into());
        let plan = plan_of(&rows, &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_144243_IMG_1.jpg");
    }

    #[test]
    fn the_spare_are_excluded_while_undated_are_counted() {
        let mut a = row(1, "/a/a.jpg", Some("2024-05-17T14:42:43"));
        a.decision = Some("drop".into());
        let mut b = row(2, "/a/b.jpg", Some("2024-05-17T14:42:44"));
        b.decision = Some("keep".into());
        let c = row(3, "/a/c.jpg", None);
        let mut opts = Options::new("/out");
        opts.exclude_dup = true;
        let plan = plan_of(&[a, b, c], &opts);
        assert_eq!(plan.items.len(), 1);
        assert_eq!(plan.excluded, 1);
        assert_eq!(plan.no_date, 1);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/b.jpg");

        opts.exclude_dup = false;
        let plan = plan_of(&[row(4, "/a/d.jpg", Some("2024-05-17T14:42:45"))], &opts);
        assert_eq!(plan.excluded, 0);
        assert_eq!(plan.items.len(), 1);
    }

    #[test]
    fn a_template_with_an_unknown_placeholder_is_refused() {
        assert!(validate_template("{date}_{foo}").is_err());
        assert!(validate_template("{date}_{").is_err());
        assert!(validate_template(RENAME_TEMPLATE).is_ok());
        assert!(validate_template(&PLACEHOLDERS.join("")).is_ok());
    }

    #[test]
    fn the_plan_creates_nothing_without_apply() {
        let dir = std::env::temp_dir().join(format!("photocat-organize-{}", std::process::id()));
        let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
        let _ = build_plan(&rows, &Options::new(&dir));
        assert!(!dir.exists());
    }

    #[test]
    fn a_collision_number_does_not_break_a_file_without_an_extension() {
        let dst = PathBuf::from("/out/2024/2024-05-17/file");
        assert_eq!(numbered(&dst, 2), PathBuf::from("/out/2024/2024-05-17/file-2"));
        let dst = PathBuf::from("/out/file.jpg");
        assert_eq!(numbered(&dst, 3), PathBuf::from("/out/file-3.jpg"));
    }

    // ------------------------------------------------------------ M20: the export

    fn tmp_dir(tag: &str) -> PathBuf {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "photocat-export-{tag}-{}-{n}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn export_row(id: i64, path: &str, taken: Option<&str>) -> FileRow {
        let mut r = row(id, path, taken);
        r.size = 100 + id;
        r
    }

    fn album(id: i64, parent: i64, name: &str, files: Vec<FileRow>) -> ExportAlbum {
        ExportAlbum {
            id,
            parent_id: parent,
            name: name.into(),
            files,
        }
    }

    #[test]
    fn the_export_repeats_the_tree_and_numbers_the_files() {
        let albums = [
            album(
                1,
                0,
                "Дача 2024",
                vec![export_row(1, "/a/IMG_1.jpg", Some("2024-05-17T10:00:00"))],
            ),
            album(
                2,
                1,
                "Май",
                vec![
                    export_row(2, "/a/IMG_2.jpg", Some("2024-05-01T10:00:00")),
                    export_row(3, "/a/IMG_3.jpg", Some("2024-05-02T10:00:00")),
                ],
            ),
            album(3, 1, "Июнь", Vec::new()),
        ];
        let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
        assert_eq!(plan.root, PathBuf::from("Дача_2024"));
        let rels: Vec<String> = plan
            .dirs
            .iter()
            .map(|d| d.rel.to_string_lossy().to_string())
            .collect();
        assert_eq!(
            rels,
            ["Дача_2024", "Дача_2024/Май", "Дача_2024/Июнь"]
        );
        assert_eq!(plan.dirs[1].depth, 1);
        assert_eq!(plan.dirs[2].files, 0, "an empty album stays in the plan");
        assert_eq!(plan.files(), 3);
        assert!(plan.items.iter().any(|i| i.rel.ends_with("01_IMG_2.jpg")));
        assert!(plan.items.iter().any(|i| i.rel.ends_with("02_IMG_3.jpg")));
        assert!(plan.items.iter().any(|i| i.rel.ends_with("Дача_2024/01_IMG_1.jpg")));
        assert!(plan.summary(Lang::Ru).contains("файлов 3"));
    }

    #[test]
    fn the_number_width_grows_with_the_file_count() {
        assert_eq!(number_width(1), 2);
        assert_eq!(number_width(99), 2);
        assert_eq!(number_width(100), 3);
        assert_eq!(number_width(1000), 4);
    }

    #[test]
    fn folder_names_are_cleaned_and_stay_unique_among_siblings() {
        let albums = [
            album(1, 0, "Дача/2024", Vec::new()),
            album(2, 1, "Дача:2024", Vec::new()),
            album(3, 1, "CON", Vec::new()),
            album(4, 1, "", Vec::new()),
        ];
        let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
        let names: Vec<String> = plan
            .dirs
            .iter()
            .map(|d| d.rel.file_name().unwrap().to_string_lossy().to_string())
            .collect();
        assert_eq!(names, ["Дача_2024", "Дача_2024-2", "_CON", "album-4"]);
        assert_eq!(plan.renamed, 1);
    }

    #[test]
    fn a_file_name_collision_is_split_by_a_number() {
        let albums = [album(
            1,
            0,
            "На печать",
            vec![
                export_row(1, "/a/x.jpg", None),
                export_row(2, "/b/x.jpg", None),
            ],
        )];
        let mut opts = ExportOptions::new("/out");
        opts.name = "{orig}".into();
        let plan = plan_album(&albums, &opts).unwrap();
        assert_eq!(plan.items[0].rel, PathBuf::from("На_печать/x.jpg"));
        assert_eq!(plan.items[1].rel, PathBuf::from("На_печать/x-2.jpg"));
        assert_eq!(plan.renamed, 1);
    }

    #[test]
    fn the_export_template_knows_n_and_hash8() {
        assert!(validate_export_template(EXPORT_TEMPLATE).is_ok());
        assert!(validate_export_template("{n}_{date}_{time}_{camera}_{tag}_{orig}_{ext}").is_ok());
        assert!(validate_export_template("{hash8}").is_ok());
        assert!(validate_export_template("{foo}").is_err());
        // The showcase template does not know `{n}`, and the other way round.
        assert!(validate_template("{n}_{orig}").is_err());
        assert!(validate_export_template("{date}_{orig}").is_ok());
    }

    #[test]
    fn a_name_from_the_template_carries_the_position_date_and_hash() {
        let r = export_row(1, "/a/IMG 1.jpg", Some("2024-05-17T14:42:43"));
        let opts = ExportOptions::new("/out");
        let name = export_name("{n}_{date}_{time}_{orig}_{hash8}", &r, &opts, 7, 2);
        assert_eq!(name, "07_20240517_144243_IMG_1_00000000.jpg");
    }

    #[test]
    fn the_trash_and_a_long_path_are_counted_and_not_exported() {
        let mut trashed = export_row(2, "/a/b.jpg", Some("2024-05-17T10:00:00"));
        trashed.trashed_at = Some(1_716_000_000);
        let albums = [album(
            1,
            0,
            "Архив",
            vec![export_row(1, "/a/a.jpg", Some("2024-05-17T09:00:00")), trashed],
        )];
        let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
        assert_eq!(plan.files(), 1);
        assert_eq!(plan.trashed, 1);

        // 240 characters of path: 120 for the root and 120 for the file — beyond the limit.
        let long_out = PathBuf::from(format!("/{}", "o".repeat(150)));
        let albums = [album(
            1,
            0,
            &"к".repeat(80),
            vec![export_row(1, &format!("/a/{}.jpg", "x".repeat(80)), None)],
        )];
        let plan = plan_album(&albums, &ExportOptions::new(&long_out)).unwrap();
        assert_eq!(plan.too_long, 1);
        assert_eq!(plan.files(), 0);
    }

    #[test]
    fn the_export_copies_rather_than_links_and_a_repeat_is_idempotent() {
        let dir = tmp_dir("copy");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let src = dir.join("src.jpg");
        std::fs::write(&src, b"photocat export").unwrap();
        let sha = derive::sha256_file(&src).unwrap();
        let mut r = export_row(1, src.to_string_lossy().as_ref(), None);
        r.size = 15;
        r.sha256 = sha;
        let albums = [album(1, 0, "На печать", vec![r])];
        let plan = plan_album(&albums, &ExportOptions::new(&out)).unwrap();
        let first = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
        assert_eq!(first.copied, 1);
        assert_eq!(first.failed, 0, "{:?}", first.errors);
        let dst = out.join("На_печать/01_src.jpg");
        assert!(dst.is_file());
        assert_eq!(std::fs::read(&dst).unwrap(), b"photocat export");
        assert!(!std::fs::symlink_metadata(&dst).unwrap().file_type().is_symlink());
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            assert_ne!(
                std::fs::metadata(&src).unwrap().ino(),
                std::fs::metadata(&dst).unwrap().ino(),
                "the export is a copy, not a hardlink"
            );
        }
        let second = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
        assert_eq!(second.copied, 0);
        assert_eq!(second.skipped, 1);
        assert_eq!(second.failed, 0);
    }

    #[test]
    fn a_name_taken_by_other_content_gets_two_and_is_not_overwritten() {
        let dir = tmp_dir("collision");
        let out = dir.join("out");
        std::fs::create_dir_all(out.join("На_печать")).unwrap();
        let taken = out.join("На_печать/01_src.jpg");
        std::fs::write(&taken, b"someone else").unwrap();
        let src = dir.join("src.jpg");
        std::fs::write(&src, b"photocat export").unwrap();
        let mut r = export_row(1, src.to_string_lossy().as_ref(), None);
        r.size = 15;
        r.sha256 = derive::sha256_file(&src).unwrap();
        let albums = [album(1, 0, "На печать", vec![r])];
        let plan = plan_album(&albums, &ExportOptions::new(&out)).unwrap();
        let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
        assert_eq!(report.copied, 1);
        assert_eq!(report.renamed, 1);
        assert_eq!(std::fs::read(&taken).unwrap(), b"someone else");
        assert_eq!(
            std::fs::read(out.join("На_печать/01_src-2.jpg")).unwrap(),
            b"photocat export"
        );
    }

    #[test]
    fn reordering_gives_a_stale_order_and_no_duplicates() {
        let dir = tmp_dir("stale");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let a = dir.join("a.jpg");
        let b = dir.join("b.jpg");
        std::fs::write(&a, b"aaaa").unwrap();
        std::fs::write(&b, b"bbbb").unwrap();
        let mut ra = export_row(1, a.to_string_lossy().as_ref(), None);
        ra.size = 4;
        ra.sha256 = derive::sha256_file(&a).unwrap();
        let mut rb = export_row(2, b.to_string_lossy().as_ref(), None);
        rb.size = 4;
        rb.sha256 = derive::sha256_file(&b).unwrap();
        let first = plan_album(
            &[album(1, 0, "Порядок", vec![ra.clone(), rb.clone()])],
            &ExportOptions::new(&out),
        )
        .unwrap();
        assert_eq!(apply_export_in(&first, Lang::default(), None, &dir.join("cache")).copied, 2);
        // The order changes: the numbered names differ, but the content is in the target already.
        let second = plan_album(
            &[album(1, 0, "Порядок", vec![rb, ra])],
            &ExportOptions::new(&out),
        )
        .unwrap();
        let report = apply_export_in(&second, Lang::default(), None, &dir.join("cache"));
        assert_eq!(report.copied, 0);
        assert_eq!(report.stale, 2);
        assert_eq!(report.renamed, 0);
        let files = std::fs::read_dir(out.join("Порядок")).unwrap().count();
        assert_eq!(files, 2, "no duplicates appeared at the destination");
    }

    #[test]
    fn an_empty_album_creates_an_empty_folder() {
        let dir = tmp_dir("empty");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let planets = [album(1, 0, "Пустой", Vec::new())];
        let plan = plan_album(&planets, &ExportOptions::new(&out)).unwrap();
        let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
        assert_eq!(report.copied, 0);
        assert!(out.join("Пустой").is_dir());
        assert_eq!(std::fs::read_dir(out.join("Пустой")).unwrap().count(), 0);
    }

    #[test]
    fn a_missing_file_does_not_fail_the_pass() {
        let dir = tmp_dir("missing");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let src = dir.join("ok.jpg");
        std::fs::write(&src, b"ok").unwrap();
        let mut good = export_row(1, src.to_string_lossy().as_ref(), None);
        good.size = 2;
        good.sha256 = derive::sha256_file(&src).unwrap();
        let gone = export_row(2, dir.join("gone.jpg").to_string_lossy().as_ref(), None);
        let albums = [album(1, 0, "Смесь", vec![good, gone])];
        let plan = plan_album(&albums, &ExportOptions::new(&out)).unwrap();
        let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
        assert_eq!(report.copied, 1);
        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.contains("no such file")));
    }

    #[test]
    fn a_cancel_before_the_start_creates_nothing_and_remembers_what_was_created() {
        let dir = tmp_dir("cancel");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let src = dir.join("a.jpg");
        std::fs::write(&src, b"a").unwrap();
        let mut r = export_row(1, src.to_string_lossy().as_ref(), None);
        r.size = 1;
        r.sha256 = derive::sha256_file(&src).unwrap();
        let plan = plan_album(&[album(1, 0, "Стоп", vec![r])], &ExportOptions::new(&out)).unwrap();
        let progress = Progress::default();
        progress.cancel();
        let report = apply_export_in(&plan, Lang::default(), Some(&progress), &dir.join("cache"));
        assert!(report.cancelled);
        assert_eq!(report.created.len(), 0);
        assert!(!out.join("Стоп/01_a.jpg").exists());

        // A normal pass lists the created files (for the report and the test).
        let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
        assert_eq!(report.created, vec![out.join("Стоп/01_a.jpg")]);
    }

    #[test]
    fn an_out_inside_the_archive_is_refused() {
        let root = tmp_dir("roots");
        let inside = root.join("photos");
        std::fs::create_dir_all(&inside).unwrap();
        let roots = vec![root.to_string_lossy().to_string()];
        assert!(check_out_dir(&inside, &roots).is_err());
        assert!(check_out_dir(&root, &roots).is_err());
        let outside = tmp_dir("outside");
        assert!(check_out_dir(&outside, &roots).is_ok());
        assert!(check_out_dir(&root.join("nope"), &roots).is_err());
    }

    #[test]
    fn depth_three_and_the_sibling_order_are_kept() {
        let albums = [
            album(1, 0, "Дача 2024", Vec::new()),
            album(2, 1, "Май", Vec::new()),
            album(3, 2, "Неделя 1", Vec::new()),
            album(4, 1, "Июнь", Vec::new()),
        ];
        let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
        assert_eq!(plan.dirs[2].rel, PathBuf::from("Дача_2024/Май/Неделя_1"));
        assert_eq!(plan.dirs[2].depth, 2);
        assert_eq!(plan.dirs[3].rel, PathBuf::from("Дача_2024/Июнь"));
        assert_eq!(plan.dirs.len(), 4);
    }
}
