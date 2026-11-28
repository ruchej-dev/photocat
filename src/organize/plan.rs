//! The dry-run plan: the layout, the name template and one destination per file.

use crate::db::FileRow;
use crate::i18n::{self, Lang};
use anyhow::Result;
use std::collections::{BTreeMap, HashMap};
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

pub(super) fn validate_tokens(tpl: &str, allowed: &[&str]) -> Result<()> {
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
pub(super) fn confident_tag(scores: &[(String, f32)], show: f32, margin: f32) -> Option<(&str, f32)> {
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
pub(super) fn component(s: &str) -> String {
    tidy(s).chars().take(40).collect::<String>()
        .trim_end_matches(['_', '-', '.'])
        .to_string()
}

/// Spaces → `_`, forbidden characters dropped, repeated and trailing separators removed.
pub(super) fn tidy(s: &str) -> String {
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
pub(super) fn numbered(dst: &Path, n: u32) -> PathBuf {
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
