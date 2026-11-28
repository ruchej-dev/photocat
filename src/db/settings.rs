//! The application settings: the small enums they hold and the `settings` table.

use anyhow::Result;
use rusqlite::{Connection, params};
use std::path::PathBuf;

use super::schema::cache_dir;

/// How the grid shows files: grouped by months or one continuous ribbon (M17).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridLayout {
    #[default]
    Months,
    Flat,
}

impl GridLayout {
    pub fn as_str(self) -> &'static str {
        match self {
            GridLayout::Months => "months",
            GridLayout::Flat => "flat",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "flat" => GridLayout::Flat,
            _ => GridLayout::Months,
        }
    }
}

/// The interface theme (M22, ADR 0044): dark, light or follow the system.
/// The two palettes of the mock-up are `dark` and `light`; `system` takes the OS choice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeMode {
    #[default]
    Dark,
    Light,
    System,
}

impl ThemeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ThemeMode::Dark => "dark",
            ThemeMode::Light => "light",
            ThemeMode::System => "system",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "light" => ThemeMode::Light,
            "system" => ThemeMode::System,
            _ => ThemeMode::Dark,
        }
    }
}

/// How files are shown in the grid (M22): the mock-up tile, proportion rows or a list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CellStyle {
    /// The mock-up tile: a 4:3 frame, the snapshot filling it (the sides are cut), the caption
    /// below.
    #[default]
    Tiles,
    /// Proportion rows: the row height comes from the slider, the tile width from the frame.
    Rows,
    /// A list: thumbnail, name, date, size.
    List,
}

impl CellStyle {
    pub fn as_str(self) -> &'static str {
        match self {
            CellStyle::Tiles => "tiles",
            CellStyle::Rows => "rows",
            CellStyle::List => "list",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "rows" => CellStyle::Rows,
            "list" => CellStyle::List,
            _ => CellStyle::Tiles,
        }
    }
}

/// The order of the album cards on the "Albums" screen (M22.20). It is a look, not data: the
/// album order (`position`) stays the order of the export whatever the cards show.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AlbumSort {
    /// The owner's own order — the order a sub-album takes on disk.
    #[default]
    Position,
    /// The album changed most recently first (a photo was added, the name changed).
    Updated,
    Name,
    Count,
    Size,
}

impl AlbumSort {
    pub const ALL: [AlbumSort; 5] = [
        AlbumSort::Position,
        AlbumSort::Updated,
        AlbumSort::Name,
        AlbumSort::Count,
        AlbumSort::Size,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            AlbumSort::Position => "position",
            AlbumSort::Updated => "updated",
            AlbumSort::Name => "name",
            AlbumSort::Count => "count",
            AlbumSort::Size => "size",
        }
    }

    /// An unknown value from an old DB means the default, never a panic.
    pub fn parse(value: &str) -> Self {
        match value {
            "updated" => AlbumSort::Updated,
            "name" => AlbumSort::Name,
            "count" => AlbumSort::Count,
            "size" => AlbumSort::Size,
            _ => AlbumSort::Position,
        }
    }
}

/// The order of the tiles (M17). "Album order" (M19) works only inside the chosen
/// album; outside it reads as "date: newest first".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GridSort {
    #[default]
    DateDesc,
    DateAsc,
    Name,
    SizeDesc,
    Album,
}

impl GridSort {
    pub fn as_str(self) -> &'static str {
        match self {
            GridSort::DateDesc => "date_desc",
            GridSort::DateAsc => "date_asc",
            GridSort::Name => "name",
            GridSort::SizeDesc => "size_desc",
            GridSort::Album => "album",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "date_asc" => GridSort::DateAsc,
            "name" => GridSort::Name,
            "size_desc" => GridSort::SizeDesc,
            "album" => GridSort::Album,
            _ => GridSort::DateDesc,
        }
    }
}

/// A pass an owner can switch off for one catalog (M26.9): the expensive analysis that says
/// something *about* a photo. Indexing itself is not a pass here — the grid, the duplicates and
/// the calendar need it for every catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Pass {
    Faces,
    Tags,
    Text,
}

impl Pass {
    pub const ALL: [Pass; 3] = [Pass::Faces, Pass::Tags, Pass::Text];

    /// The key in the `root_passes` JSON: data, not a label.
    pub fn key(self) -> &'static str {
        match self {
            Pass::Faces => "faces",
            Pass::Tags => "tags",
            Pass::Text => "text",
        }
    }

    pub fn parse(s: &str) -> Option<Pass> {
        match s {
            "faces" => Some(Pass::Faces),
            "tags" => Some(Pass::Tags),
            "text" => Some(Pass::Text),
            _ => None,
        }
    }
}

/// The application settings. Kept in the DB (next to the index), not in a file by the archive.
#[derive(Debug, Clone)]
pub struct Settings {
    /// Where to download models to and read them from.
    pub models_dir: PathBuf,
    /// The catalogue model id; empty means the first available one.
    pub model_id: String,
    /// The size ceiling of a downloaded model, MiB.
    pub max_model_mb: u64,
    /// Indexing threads by default.
    pub index_jobs: usize,
    /// The default dHash similarity threshold.
    pub max_dist: u32,
    /// The confident-tag threshold (raw cosine) and the margin over the second tag.
    /// This is about the card, OCR and `{tag}` in the showcase, not about the filter.
    pub tag_show: f32,
    pub tag_margin: f32,
    /// The threshold of the tag filter — a percentile 0–100 (ADR 0032).
    /// 100% — the strongest photo of this tag, 0% — all; 70% — the top 30%.
    pub tag_show_pct: i32,
    /// The tesseract languages for text recognition, for instance `rus+eng`.
    pub ocr_lang: String,
    /// Paths to external programs; empty means a lookup in `PATH` (ADR 0020).
    pub tools: crate::tools::Tools,
    /// The cosine threshold for merging faces into one person (ADR 0021).
    pub face_threshold: f32,
    /// The cosine an automatically matched face needs to reach a named person (M26.11).
    pub face_match: f32,
    /// How far the best named person must lead the second one (M26.11): a relative's face is
    /// left alone rather than guessed.
    pub face_match_margin: f32,
    /// The grid tile size, px (docs/UI.md).
    pub tile_size: f32,
    /// The grid layout: months or a flat ribbon (M17).
    pub grid_layout: GridLayout,
    /// The tile order: date, name or size (M17).
    pub grid_sort: GridSort,
    /// How files are shown: the mock-up tile, proportion rows or a list (M22).
    pub cell_style: CellStyle,
    /// The order of the album cards and the album look: cards or a list (M22.20).
    pub album_sort: AlbumSort,
    pub album_list: bool,
    /// Folders indexed with one button; empty means the path from the top bar.
    pub roots: Vec<String>,
    /// Folders switched off (M26.1): their files are hidden from every view, the rows, the
    /// previews and the files on disk stay. The same newline-separated form as `roots`.
    pub roots_off: Vec<String>,
    /// Passes switched off per catalog (M26.9): only the exceptions, keyed by the root path. A
    /// root that is not listed runs every pass; a root that is gone keeps no key.
    pub root_passes: std::collections::BTreeMap<String, Vec<Pass>>,
    /// The integrity reminder (M10): the folder and the manifest the term belongs to,
    /// the interval in days (0 — the reminder is off) and the result of the last pass.
    pub verify_root: String,
    pub verify_manifest: String,
    pub verify_interval_days: u32,
    /// RFC3339 UTC of the last complete pass over the archive (a manifest or a check).
    pub verify_last_at: String,
    pub verify_last_ok: bool,
    /// The same report text that goes to `<cache>/photocat/verify-last.txt`.
    pub verify_last_report: String,
    /// The interface language (M21, ADR 0040): `en` is the main one, `ru` the second.
    pub lang: crate::i18n::Lang,
    /// The interface theme (M22, ADR 0044): dark by default, light at the owner's choice.
    pub theme: ThemeMode,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            models_dir: cache_dir().join("models"),
            model_id: String::new(),
            max_model_mb: 250,
            index_jobs: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(8)
                .clamp(2, 16),
            max_dist: 6,
            tag_show: 0.22,
            tag_margin: 0.01,
            tag_show_pct: 70,
            ocr_lang: "rus+eng".into(),
            tools: crate::tools::Tools::default(),
            face_threshold: 0.45,
            face_match: 0.50,
            face_match_margin: 0.03,
            tile_size: 150.0,
            grid_layout: GridLayout::Months,
            grid_sort: GridSort::DateDesc,
            cell_style: CellStyle::Tiles,
            album_sort: AlbumSort::Position,
            album_list: false,
            roots: Vec::new(),
            roots_off: Vec::new(),
            root_passes: std::collections::BTreeMap::new(),
            verify_root: String::new(),
            verify_manifest: String::new(),
            // 30 days is a calm term for a family archive; 0 switches the reminder off.
            verify_interval_days: 30,
            verify_last_at: String::new(),
            verify_last_ok: false,
            verify_last_report: String::new(),
            // The project continues in English; Russian stays a choice in the settings.
            lang: crate::i18n::Lang::En,
            // The mock-up paints the dark palette as the main one: a family archive is looked at in the evening.
            theme: ThemeMode::Dark,
        }
    }
}

/// A root without its trailing separator; a lone `/` or `C:\` stays as it is, so the whole
/// drive or the file system can still be named as a folder.
fn trim_root(root: &str) -> &str {
    let trimmed = root.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() { root } else { trimmed }
}

/// Whether `path` lies inside `root` (M26.1). A component-wise prefix, not a string one:
/// `/archive2` is not inside `/archive`. A trailing separator on either side is ignored.
pub fn under_root(path: &str, root: &str) -> bool {
    if root.is_empty() {
        return false;
    }
    std::path::Path::new(trim_root(path)).starts_with(std::path::Path::new(trim_root(root)))
}

impl Settings {
    /// Whether a file path lies in a folder that is switched off (M26.1). The switch is about
    /// showing, not about deleting: the rows stay in the DB.
    pub fn path_hidden(&self, path: &str) -> bool {
        self.roots_off.iter().any(|root| under_root(path, root))
    }

    /// Whether this very folder is switched off — the folder list and the catalog block.
    pub fn root_off(&self, root: &str) -> bool {
        let root = trim_root(root);
        self.roots_off.iter().any(|r| trim_root(r) == root)
    }

    /// Switch a folder on or off (M26.1): only the setting changes, no row is touched.
    pub fn set_root_off(&mut self, root: &str, off: bool) {
        let root = trim_root(root).to_string();
        if off {
            if !self.root_off(&root) {
                self.roots_off.push(root);
            }
        } else {
            self.roots_off.retain(|r| trim_root(r) != root);
        }
    }

    /// Whether the pass runs over this file (M26.9). One helper for the GUI and the CLI: the most
    /// specific root that holds the path decides; a file outside every root and a root without an
    /// exception allow the pass. A catalog switched off for display (M26.1) allows no pass at all —
    /// switching it back on lets the passes catch up.
    pub fn pass_allows(&self, path: &str, pass: Pass) -> bool {
        if self.path_hidden(path) {
            return false;
        }
        self.root_passes
            .iter()
            .filter(|(root, _)| under_root(path, root))
            .max_by_key(|(root, _)| trim_root(root).len())
            .is_none_or(|(_, off)| !off.contains(&pass))
    }

    /// Whether this catalog runs a pass — the three boxes of the folder list. A path that is not
    /// a root itself is looked up by the same rule as `pass_allows`.
    pub fn root_pass(&self, root: &str, pass: Pass) -> bool {
        self.root_passes
            .iter()
            .filter(|(key, _)| trim_root(key) == trim_root(root))
            .max_by_key(|(key, _)| trim_root(key).len())
            .is_none_or(|(_, off)| !off.contains(&pass))
    }

    /// Switch one pass off or on for one catalog (M26.9): only the exception list changes, nothing
    /// derived is deleted — the vectors, tags, text and faces stay until the owner forgets them.
    pub fn set_root_pass(&mut self, root: &str, pass: Pass, on: bool) {
        let key = trim_root(root).to_string();
        let list = self.root_passes.entry(key.clone()).or_default();
        if on {
            list.retain(|p| *p != pass);
        } else if !list.contains(&pass) {
            list.push(pass);
        }
        if list.is_empty() {
            self.root_passes.remove(&key);
        }
    }

    /// Drop the exceptions of roots that are no longer in `roots` (M26.9): a catalog that is gone
    /// keeps no key. Returns whether anything left.
    pub fn prune_root_passes(&mut self) -> bool {
        let before = self.root_passes.len();
        self.root_passes.retain(|key, _| {
            self.roots
                .iter()
                .any(|root| trim_root(root) == trim_root(key))
        });
        before != self.root_passes.len()
    }
}

pub fn load_settings(conn: &Connection) -> Settings {
    let mut s = Settings::default();
    let mut has_tag_pct = false;
    let Ok(mut st) = conn.prepare("SELECT key, value FROM settings") else {
        return s;
    };
    let Ok(rows) = st.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
    else {
        return s;
    };
    for row in rows.flatten() {
        let (k, v) = row;
        match k.as_str() {
            "models_dir" => s.models_dir = PathBuf::from(v),
            "model_id" => s.model_id = v,
            "max_model_mb" => s.max_model_mb = v.parse().unwrap_or(s.max_model_mb),
            "index_jobs" => s.index_jobs = v.parse().unwrap_or(s.index_jobs),
            "max_dist" => s.max_dist = v.parse().unwrap_or(s.max_dist),
            "tag_show" => s.tag_show = v.parse().unwrap_or(s.tag_show),
            "tag_margin" => s.tag_margin = v.parse().unwrap_or(s.tag_margin),
            "tag_show_pct" => {
                s.tag_show_pct = v.parse().unwrap_or(s.tag_show_pct).clamp(0, 100);
                has_tag_pct = true;
            }
            "ocr_lang" if !v.trim().is_empty() => s.ocr_lang = v,
            "tool_ffmpeg" => s.tools.ffmpeg = v,
            "tool_ffprobe" => s.tools.ffprobe = v,
            "tool_tesseract" => s.tools.tesseract = v,
            "face_threshold" => s.face_threshold = v.parse().unwrap_or(s.face_threshold),
            "face_match" => s.face_match = v.parse().unwrap_or(s.face_match),
            "face_match_margin" => {
                s.face_match_margin = v.parse().unwrap_or(s.face_match_margin)
            }
            "tile_size" => s.tile_size = v.parse().unwrap_or(s.tile_size),
            "grid_layout" => s.grid_layout = GridLayout::parse(&v),
            "grid_sort" => s.grid_sort = GridSort::parse(&v),
            "cell_style" => s.cell_style = CellStyle::parse(&v),
            "album_sort" => s.album_sort = AlbumSort::parse(&v),
            "album_list" => s.album_list = v == "1" || v == "true",
            "roots_off" => {
                s.roots_off = v
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            // Passes switched off per catalog (M26.9): a JSON object root → disabled pass keys.
            "root_passes" => {
                let parsed = serde_json::from_str::<serde_json::Value>(&v)
                    .ok()
                    .and_then(|value| value.as_object().cloned());
                for (root, list) in parsed.unwrap_or_default() {
                    let off: Vec<Pass> = list
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().and_then(Pass::parse))
                                .collect()
                        })
                        .unwrap_or_default();
                    if !off.is_empty() {
                        s.root_passes.insert(trim_root(&root).to_string(), off);
                    }
                }
            }
            "roots" => {
                s.roots = v
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            }
            "verify_root" => s.verify_root = v,
            "verify_manifest" => s.verify_manifest = v,
            "verify_interval_days" => {
                s.verify_interval_days = v.parse().unwrap_or(s.verify_interval_days).min(3650)
            }
            "verify_last_at" => s.verify_last_at = v,
            "verify_last_ok" => s.verify_last_ok = v == "1" || v == "true",
            "verify_last_report" => s.verify_last_report = v,
            "lang" => s.lang = crate::i18n::Lang::parse(&v),
            "theme" => s.theme = ThemeMode::parse(&v),
            _ => {}
        }
    }
    if !has_tag_pct {
        // Migration M15.2: `tag_show` used to be both a filter and a confidence threshold,
        // so the slider drove it down to 0.50. The filter is in percentiles now (70% by
        // default) and confidence is back to ADR 0013 — 0.22 with a margin of 0.01.
        s.tag_show = 0.22;
        s.tag_show_pct = 70;
    }
    // A catalog that was removed from the index keeps no exception (M26.9).
    s.prune_root_passes();
    s
}

pub fn save_settings(conn: &Connection, s: &Settings) -> Result<()> {
    let mut pairs = vec![
        ("models_dir", s.models_dir.to_string_lossy().to_string()),
        ("model_id", s.model_id.clone()),
        ("max_model_mb", s.max_model_mb.to_string()),
        ("index_jobs", s.index_jobs.to_string()),
        ("max_dist", s.max_dist.to_string()),
        ("tag_show", s.tag_show.to_string()),
        ("tag_margin", s.tag_margin.to_string()),
        ("tag_show_pct", s.tag_show_pct.to_string()),
        ("ocr_lang", s.ocr_lang.clone()),
        ("face_threshold", s.face_threshold.to_string()),
        ("face_match", s.face_match.to_string()),
        ("face_match_margin", s.face_match_margin.to_string()),
        ("tile_size", s.tile_size.to_string()),
        ("grid_layout", s.grid_layout.as_str().to_string()),
        ("grid_sort", s.grid_sort.as_str().to_string()),
        ("cell_style", s.cell_style.as_str().to_string()),
        ("album_sort", s.album_sort.as_str().to_string()),
        (
            "album_list",
            if s.album_list { "1" } else { "0" }.to_string(),
        ),
        ("roots", s.roots.join("\n")),
        ("roots_off", s.roots_off.join("\n")),
        (
            "root_passes",
            {
                // Only the exceptions, and only for roots that are still in the list.
                let mut obj = serde_json::Map::new();
                for (root, off) in &s.root_passes {
                    if off.is_empty()
                        || !s
                            .roots
                            .iter()
                            .any(|r| trim_root(r) == trim_root(root))
                    {
                        continue;
                    }
                    obj.insert(
                        root.clone(),
                        serde_json::Value::Array(
                            off.iter()
                                .map(|p| serde_json::Value::String(p.key().into()))
                                .collect(),
                        ),
                    );
                }
                serde_json::Value::Object(obj).to_string()
            },
        ),
        ("verify_root", s.verify_root.clone()),
        ("verify_manifest", s.verify_manifest.clone()),
        ("verify_interval_days", s.verify_interval_days.to_string()),
        ("verify_last_at", s.verify_last_at.clone()),
        (
            "verify_last_ok",
            if s.verify_last_ok { "1" } else { "0" }.to_string(),
        ),
        ("verify_last_report", s.verify_last_report.clone()),
        ("lang", s.lang.code().to_string()),
        ("theme", s.theme.as_str().to_string()),
    ];
    for tool in crate::tools::ALL {
        pairs.push((tool.setting_key(), s.tools.get(tool).to_string()));
    }
    for (k, v) in pairs {
        conn.execute(
            "INSERT INTO settings(key,value) VALUES(?1,?2)
             ON CONFLICT(key) DO UPDATE SET value=excluded.value",
            params![k, v],
        )?;
    }
    Ok(())
}

/// Whether it is time to check the archive against the manifest (M10). Computed from the
/// settings, not from a timer: the application starts nothing itself, it only says "due".
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VerifyDue {
    /// No manifest is set — nothing to remind about.
    NoManifest,
    /// The interval is 0 — the owner switched the reminder off.
    Disabled,
    /// The term has not come yet; `days_left` is how many full days remain.
    NotYet { days_left: i64 },
    /// The term has passed; `days_over` is by how many days (0 — exactly due).
    Due { days_over: i64 },
}

/// RFC3339 → UTC. An empty or broken value means `None`.
pub fn parse_stamp(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    chrono::DateTime::parse_from_rfc3339(s.trim())
        .ok()
        .map(|d| d.with_timezone(&chrono::Utc))
}

/// The "due / not due" decision in one function: the same for the banner and for the tests.
/// `now` is passed explicitly, so "exactly due" and "an hour ago" are checked without waiting.
///
/// * no manifest → `NoManifest`;
/// * interval 0 → `Disabled`;
/// * the manifest has never been checked (empty or broken date) → `Due`;
/// * a date in the future (the clock was turned back) → `NotYet`: not due yet, no panic;
/// * otherwise — by calendar days.
pub fn verify_due(s: &Settings, now: chrono::DateTime<chrono::Utc>) -> VerifyDue {
    if s.verify_manifest.trim().is_empty() {
        return VerifyDue::NoManifest;
    }
    if s.verify_interval_days == 0 {
        return VerifyDue::Disabled;
    }
    let interval = chrono::Duration::days(i64::from(s.verify_interval_days));
    let Some(last) = parse_stamp(&s.verify_last_at) else {
        return VerifyDue::Due { days_over: 0 };
    };
    let elapsed = now.signed_duration_since(last);
    if elapsed >= interval {
        VerifyDue::Due {
            days_over: (elapsed - interval).num_days(),
        }
    } else {
        let left = (interval - elapsed).num_seconds();
        VerifyDue::NotYet {
            days_left: (left + 86_399) / 86_400,
        }
    }
}

/// One value from `settings` — for service keys like the dictionary version.
/// The interface language on the first launch (M21, ADR 0040): if the owner has not chosen it
/// yet, the system locale is taken and stored — afterwards the language is read from the DB, not guessed again.
pub fn ensure_lang(conn: &Connection) -> Result<()> {
    if get_setting(conn, "lang").is_none() {
        set_setting(conn, "lang", crate::i18n::Lang::from_env().code())?;
    }
    Ok(())
}

pub fn get_setting(conn: &Connection, key: &str) -> Option<String> {
    conn.query_row("SELECT value FROM settings WHERE key=?1", [key], |r| r.get(0))
        .ok()
}

pub fn set_setting(conn: &Connection, key: &str, value: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO settings(key,value) VALUES(?1,?2)
         ON CONFLICT(key) DO UPDATE SET value=excluded.value",
        params![key, value],
    )?;
    Ok(())
}
