//! A GUI on egui: the preview grid, duplicate groups, the timeline, starting an index pass.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::dupes::{self, Group};
use crate::embed;
use crate::faces;
use crate::i18n::{self, t, Lang};
use crate::index;
use crate::ocr;
use crate::organize;
use crate::player;
use crate::recycle;
use crate::storage;
use crate::tools::{self, Tool};
use crate::verify;
use anyhow::{Context, Result};
use chrono::Datelike;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

type SearchReply = Result<(embed::Model, Vec<(i64, f32)>, f32)>;
type EmbedReply = Result<(embed::EmbedStats, embed::RetagStats)>;
type OcrReply = Result<ocr::Stats>;
type TrashReply = Result<recycle::Report>;
type PhotoReply = (i64, Result<(usize, usize, Vec<u8>)>);
type IntegrityReply = (IntegrityJob, Result<IntegrityOutcome>);

enum FaceReply {
    Scan(Result<faces::Stats>),
    Cluster(Result<faces::ClusterStats>),
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Grid,
    Albums,
    Dupes,
    Calendar,
    Faces,
    Trash,
    Integrity,
}

/// What the "Integrity" screen does: compare two copies, snapshot a folder, or check a
/// folder against a snapshot. Three buttons — three background passes over the same CLI functions.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum IntegrityJob {
    Verify,
    Manifest,
    Check,
}

impl IntegrityJob {
    fn title(self, lang: i18n::Lang) -> &'static str {
        t(
            lang,
            match self {
                IntegrityJob::Verify => "comparing copies",
                IntegrityJob::Manifest => "writing the manifest",
                IntegrityJob::Check => "checking against the manifest",
            },
        )
    }
}

/// The finished result of a pass: it keeps both the report and the paths it was made for.
enum IntegrityOutcome {
    Verify {
        a: PathBuf,
        b: PathBuf,
        report: verify::Report,
    },
    Manifest {
        root: PathBuf,
        out: PathBuf,
        stats: verify::ManifestStats,
    },
    Check {
        root: PathBuf,
        file: PathBuf,
        report: verify::CheckReport,
    },
}

impl IntegrityOutcome {
    fn job(&self) -> IntegrityJob {
        match self {
            IntegrityOutcome::Verify { .. } => IntegrityJob::Verify,
            IntegrityOutcome::Manifest { .. } => IntegrityJob::Manifest,
            IntegrityOutcome::Check { .. } => IntegrityJob::Check,
        }
    }

    /// The pass was stopped by "Stop": some files were not checked, and that is not "matched".
    fn cancelled(&self) -> bool {
        match self {
            IntegrityOutcome::Verify { report, .. } => report.cancelled,
            IntegrityOutcome::Manifest { stats, .. } => stats.cancelled,
            IntegrityOutcome::Check { report, .. } => report.cancelled,
        }
    }

    /// Everything matched or the file was written. A cancel and a read error are not success.
    fn identical(&self) -> bool {
        match self {
            IntegrityOutcome::Verify { report, .. } => report.identical(),
            IntegrityOutcome::Manifest { stats, .. } => stats.written,
            IntegrityOutcome::Check { report, .. } => report.identical(),
        }
    }

    /// One line of verdict for the screen: in words, without guesses.
    fn verdict(&self, lang: i18n::Lang) -> String {
        match self {
            IntegrityOutcome::Verify { report, .. } => {
                if report.cancelled {
                    t(lang, "comparison stopped: not everything was checked").into()
                } else if report.identical() {
                    t(lang, "the copies match").into()
                } else {
                    i18n::fill(
                        lang,
                        "differences found: only in A {}, only in B {}, size {}, content {}, errors {}",
                        &[
                            report.only_a.len().to_string(),
                            report.only_b.len().to_string(),
                            report.size_diff.len().to_string(),
                            report.content_diff.len().to_string(),
                            report.errors().to_string(),
                        ],
                    )
                }
            }
            IntegrityOutcome::Manifest { stats, out, .. } => {
                if stats.written {
                    i18n::fill(
                        lang,
                        "manifest written: {} ({}) → {}",
                        &[
                            i18n::count(lang, stats.files, "file"),
                            db::human_in(lang, stats.bytes as i64),
                            out.display().to_string(),
                        ],
                    )
                } else if stats.cancelled {
                    t(lang, "manifest not written: the pass was stopped").into()
                } else {
                    i18n::fill(
                        lang,
                        "{}: {} {}",
                        &[
                            t(lang, "manifest not written").to_string(),
                            t(lang, "errors").to_string(),
                            stats.errors.to_string(),
                        ],
                    )
                }
            }
            IntegrityOutcome::Check { report, .. } => {
                if report.cancelled {
                    t(lang, "check stopped: not everything was checked").into()
                } else if report.identical() {
                    t(lang, "the folder matches the manifest").into()
                } else {
                    i18n::fill(
                        lang,
                        "the folder differs from the manifest: changed {}, missing in the folder {}, missing in the manifest {}, errors {}",
                        &[
                            report.changed.len().to_string(),
                            report.missing.len().to_string(),
                            report.extra.len().to_string(),
                            report.errors().to_string(),
                        ],
                    )
                }
            }
        }
    }

    /// The report text — the same `verify` functions the CLI prints.
    fn report(&self, all: bool) -> String {
        match self {
            IntegrityOutcome::Verify { a, b, report } => verify::verify_report(a, b, report, all),
            IntegrityOutcome::Manifest { root, out, stats } => {
                verify::manifest_report(root, Some(out), stats)
            }
            IntegrityOutcome::Check { root, file, report } => {
                verify::check_report(file, root, report, all)
            }
        }
    }

    fn status(&self, lang: i18n::Lang) -> String {
        let verdict = self.verdict(lang);
        let seconds = format!("{:.1}", self.seconds());
        match self {
            IntegrityOutcome::Verify { report, .. } => i18n::fill(
                lang,
                "{}; A {}, B {}; {} s",
                &[
                    verdict,
                    report.files_a.to_string(),
                    report.files_b.to_string(),
                    seconds,
                ],
            ),
            IntegrityOutcome::Manifest { .. } => {
                i18n::fill(lang, "{}, {} s", &[verdict, seconds])
            }
            IntegrityOutcome::Check { report, .. } => i18n::fill(
                lang,
                "{}; {} same; {} s",
                &[verdict, report.same.to_string(), seconds],
            ),
        }
    }

    /// The duration of the pass, whatever it was.
    fn seconds(&self) -> f64 {
        match self {
            IntegrityOutcome::Verify { report, .. } => report.seconds,
            IntegrityOutcome::Manifest { stats, .. } => stats.seconds,
            IntegrityOutcome::Check { report, .. } => report.seconds,
        }
    }

}

/// What exactly to start after the fields are checked: the folders are parsed into `PathBuf`.
enum IntegrityPlan {
    Verify(PathBuf, PathBuf),
    Manifest(PathBuf, PathBuf),
    Check(PathBuf, PathBuf),
}

/// The format in the grid: everything, photos only, video only (M16). One notion of "filter"
/// lives in the "Filter" section of the left panel and is repeated by a chip above the grid.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KindFilter {
    All,
    Images,
    Videos,
}

impl KindFilter {
    fn label(self, lang: i18n::Lang) -> &'static str {
        t(
            lang,
            match self {
                KindFilter::All => "All",
                KindFilter::Images => "Photos",
                KindFilter::Videos => "Videos",
            },
        )
    }

    /// The caption of the chip above the grid; `None` means no filter.
    fn chip(self, lang: i18n::Lang) -> Option<&'static str> {
        match self {
            KindFilter::All => None,
            KindFilter::Images => Some(t(lang, "photos only")),
            KindFilter::Videos => Some(t(lang, "videos only")),
        }
    }
}

/// What is cleaned in "Storage": the preview cache, the face crops or the index rows.
#[derive(Clone, Copy, PartialEq)]
enum StorageAction {
    Thumbs,
    Faces,
    Index,
}

/// The calendar gradation: year, month, week and day. A double click goes one layer down,
/// the tabs left of the heading go back up (docs/UI.md).
#[derive(Clone, Copy, PartialEq, Debug)]
enum CalScale {
    Year,
    Month,
    Week,
    Day,
}

impl CalScale {
    /// The layer below on a double click: year → month → week → day; there is nothing below a day.
    fn deeper(self) -> Option<CalScale> {
        match self {
            CalScale::Year => Some(CalScale::Month),
            CalScale::Month => Some(CalScale::Week),
            CalScale::Week => Some(CalScale::Day),
            CalScale::Day => None,
        }
    }
}

/// The textures for the full screen: the current frame, the thumbnail and the previous sharp frame.
struct ViewerTextures<'a> {
    full: Option<&'a (i64, egui::TextureHandle)>,
    preview: Option<&'a egui::TextureHandle>,
    stale: Option<&'a egui::TextureHandle>,
    /// A frame of the built-in player: while it exists it matters more than the still photo.
    video: Option<&'a egui::TextureHandle>,
    /// Video on pause: the frame is there, but the centre needs a ▶ button.
    paused: bool,
}

/// What the full screen draws this frame: the frame itself, the turn of the view and the faces
/// over it (M22.7).
struct ViewerFrame<'a> {
    textures: ViewerTextures<'a>,
    /// The turn of the view by 90° steps: 0, 90, 180 or 270 degrees clockwise.
    rot: u8,
    /// The faces of the file; empty until the face model has seen it.
    faces: &'a [db::FaceBox],
}

/// The full-screen viewer: one photo over the whole grid, with a slideshow and text.
struct Viewer {
    ids: Vec<i64>,
    idx: usize,
    zoom: f32,
    /// The offset of the zoomed frame from the centre, in screen points. On a file change and
    /// at the "fit" scale it is zero, the frame is centred (docs/UI.md).
    pan: egui::Vec2,
    /// The view turned by 90° steps (M22.7): only the view turns, the file on disk is unchanged.
    rotation: u8,
    /// The file card next to the frame: off by default, the frame gets the whole width.
    inspector: bool,
    /// The boxes of the found faces are drawn only on demand: the "Show faces" tool (M22.28).
    show_faces: bool,
    /// The face picked in the frame: its photos are listed in the right panel. `None` — the
    /// panel is closed, and a click on the empty part of the frame closes it (M22.28).
    face_focus: Option<FaceFocus>,
    /// The day of the current file and its neighbours: the series strip under the frame. Computed
    /// on a file change, not every frame — it walks the whole list (M22.7).
    day: Option<String>,
    series: Vec<StripItem>,
    show_text: bool,
    slideshow: bool,
    interval_s: u64,
    last_advance: std::time::Instant,
}

/// The face picked in the full screen (M22.28): its group and every photo that carries it. The
/// list is built once, on the click, and ordered by date — the panel is the story of one face.
#[derive(Clone)]
struct FaceFocus {
    /// The name of the group; empty while the group has no name.
    label: String,
    /// `None` while the face is not grouped yet: only the frame on screen is listed.
    cluster_id: Option<i64>,
    file_ids: Vec<i64>,
}

/// A neighbour in the series strip of the full screen: where it sits in the viewer order, the
/// file itself and its position inside its day (M22.7).
#[derive(Clone)]
struct StripItem {
    idx: usize,
    id: i64,
    thumb: Option<String>,
    video: bool,
    pos: usize,
}

/// A section of the "Settings" dialog: rare settings do not take the screen from the grid.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum SettingsTab {
    Interface,
    Model,
    Index,
    Tags,
    Faces,
    Storage,
    Tools,
}

impl SettingsTab {
    const ALL: [SettingsTab; 7] = [
        SettingsTab::Interface,
        SettingsTab::Model,
        SettingsTab::Index,
        SettingsTab::Tags,
        SettingsTab::Faces,
        SettingsTab::Storage,
        SettingsTab::Tools,
    ];

    /// The icon of the section: the mock-up's Material Symbols by name (`settings`,
    /// `psychology`, `folder_supervised`, `label`, `face`, `inventory_2`, `terminal`).
    fn icon(self) -> Icon {
        match self {
            SettingsTab::Interface => Icon::Settings,
            SettingsTab::Model => Icon::Psychology,
            SettingsTab::Index => Icon::FolderSpecial,
            SettingsTab::Tags => Icon::Label,
            SettingsTab::Faces => Icon::Face,
            SettingsTab::Storage => Icon::Inventory,
            SettingsTab::Tools => Icon::Terminal,
        }
    }

    fn label(self, lang: i18n::Lang) -> &'static str {
        t(
            lang,
            match self {
                SettingsTab::Interface => "Interface",
                SettingsTab::Model => "AI model",
                SettingsTab::Index => "Indexing & duplicates",
                SettingsTab::Tags => "Tags",
                SettingsTab::Faces => "People",
                SettingsTab::Storage => "Storage",
                SettingsTab::Tools => "External tools",
            },
        )
    }
}

/// What the album dialog does (M19): create, rename, move or delete.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AlbumDialog {
    New,
    Sub,
    Rename,
    Move,
    Delete,
}

impl AlbumDialog {
    fn title(self, lang: Lang) -> &'static str {
        match self {
            AlbumDialog::New => t(lang, "New album"),
            AlbumDialog::Sub => t(lang, "New sub-album"),
            AlbumDialog::Rename => t(lang, "Rename the album"),
            AlbumDialog::Move => t(lang, "Move the album"),
            AlbumDialog::Delete => t(lang, "Delete the album?"),
        }
    }
}

/// An action of the "Albums" panel: applied after the frame, when the texture borrows are
/// released (`M19`). The dialogs open separately.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum AlbumAction {
    Select(i64),
    /// Back to the root: the cards of the top level albums (M22.20).
    Root,
    Up,
    Down,
    AddPicked,
    RemovePicked,
    FileUp,
    FileDown,
}

/// The "add photos to an album" dialog (M22.20). It is the one path for adding a photo: from the
/// file card (one photo), from the selection of the grid (N photos) and from inside an album.
/// Nothing here opens a file: rows only.
struct AlbumPick {
    /// The files to add, in the order they were picked.
    files: Vec<i64>,
    /// The chosen target: 0 means "nothing chosen yet".
    target: i64,
    /// A new album at the root, made and filled by the same press. Empty means "use `target`".
    new_name: String,
    /// The filter of the album list inside the dialog.
    filter: String,
}

/// The kind of a click on a tile (M18): Ctrl adds or removes, Shift takes a range in grid order.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum PickClick {
    Plain,
    Toggle,
    Range,
}

/// Which click means which action. Ctrl and Cmd are the same action: the owner is on Linux,
/// but the code does not lean on the OS.
fn pick_kind(mods: &egui::Modifiers) -> PickClick {
    if mods.shift {
        PickClick::Range
    } else if mods.command || mods.ctrl {
        PickClick::Toggle
    } else {
        PickClick::Plain
    }
}

/// The year filter of the left panel and the chips row (M25.2): one year, or "all the years
/// older than the newest five in the cloud". The cloud never grows — the ellipsis is a filter,
/// not an expander.
#[derive(Clone, PartialEq, Eq, Debug)]
enum YearPick {
    Year(String),
    /// Everything strictly before this year: the boundary is the oldest year of the cloud.
    Older(String),
}

impl YearPick {
    /// Whether a file of this `taken_at` belongs to the pick. A file without a date belongs to
    /// no year at all.
    fn matches(&self, taken_at: Option<&str>) -> bool {
        let Some(year) = taken_at.and_then(|d| d.get(0..4)) else {
            return false;
        };
        match self {
            YearPick::Year(want) => year == want,
            YearPick::Older(bound) => year < bound.as_str(),
        }
    }
}

/// The set of files the selection was made in: the view, the filter, the search, the order.
/// While it stays the same the selection lives; a change of any field clears it (M18), so
/// that an action does not touch files that are no longer on the screen.
#[derive(Clone, PartialEq)]
struct PickScope {
    view: View,
    query: String,
    kind: KindFilter,
    tags: Vec<String>,
    year: Option<YearPick>,
    /// The two "missing" filters (M25.4): no date, no preview.
    no_date: bool,
    no_preview: bool,
    hits: Option<usize>,
    sort: db::GridSort,
    layout: db::GridLayout,
    /// The open album and the "All files" mode inside it (M19).
    album: Option<i64>,
    browse_all: bool,
}

/// The interface palette (M22, ADR 0044). Two themes from the "functional precision" mock-up:
/// dark (the main one) and light. The tokens are the only source of colour in the screens;
/// own `from_gray`/`from_rgb` remain only for text over a dimmed frame, where the background
/// is set by the photo, not by the theme.
#[derive(Clone, Copy)]
struct Theme {
    /// Tier 0: the window background and the grid canvas.
    canvas: egui::Color32,
    /// Tier 1: the panels, the top and bottom bars.
    panel: egui::Color32,
    /// Tier 2: the cards, the popup menus.
    card: egui::Color32,
    /// Tier 3: the input fields, the disabled buttons.
    field: egui::Color32,
    /// The 1 px line between panels and rows.
    border: egui::Color32,
    border_strong: egui::Color32,
    text: egui::Color32,
    text_med: egui::Color32,
    text_dim: egui::Color32,
    accent: egui::Color32,
    accent_hover: egui::Color32,
    on_accent: egui::Color32,
    accent_soft: egui::Color32,
    danger: egui::Color32,
    dark: bool,
}

/// The dark palette of the mock-up: cold graphites, amber is the only signal.
const DARK: Theme = Theme {
    canvas: egui::Color32::from_rgb(0x0c, 0x0e, 0x12),
    panel: egui::Color32::from_rgb(0x1a, 0x1c, 0x20),
    card: egui::Color32::from_rgb(0x1e, 0x20, 0x24),
    field: egui::Color32::from_rgb(0x28, 0x2a, 0x2e),
    border: egui::Color32::from_rgb(0x2b, 0x2e, 0x35),
    border_strong: egui::Color32::from_rgb(0x37, 0x3c, 0x4a),
    text: egui::Color32::from_rgb(0xe2, 0xe2, 0xe8),
    text_med: egui::Color32::from_rgb(0xa7, 0xaa, 0xb3),
    text_dim: egui::Color32::from_rgb(0x7b, 0x80, 0x8c),
    accent: egui::Color32::from_rgb(0xf5, 0x9e, 0x0b),
    accent_hover: egui::Color32::from_rgb(0xd9, 0x77, 0x06),
    on_accent: egui::Color32::from_rgb(0x11, 0x13, 0x17),
    accent_soft: egui::Color32::from_rgba_premultiplied(0x3a, 0x25, 0x02, 38),
    // M22.11: the mock-up's red `#ef4444` reads 4.34:1 on the card tone, below the 4.5 the danger
    // word needs at 13 px; the same hue a step brighter keeps the signal and passes on every ground.
    danger: egui::Color32::from_rgb(0xf4, 0x5c, 0x5c),
    dark: true,
};

/// The light palette of the mock-up: cold slate with no warm cast in the ground, so the canvas
/// does not tint a photograph. The amber is the design system's own `amber-focus` `#b45309`: the
/// drawn screen's `#d97706` reads 2.9:1 on the panel and 3.2:1 on white, below AA for the words it
/// colours (a picked chip, the active row, a confident tag); `#b45309` is 4.51:1 on the selection
/// fill and 5.02:1 on the card at the same hue (M22.11).
const LIGHT: Theme = Theme {
    canvas: egui::Color32::from_rgb(0xf8, 0xfa, 0xfc),
    panel: egui::Color32::from_rgb(0xf1, 0xf5, 0xf9),
    card: egui::Color32::from_rgb(0xff, 0xff, 0xff),
    field: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
    border: egui::Color32::from_rgb(0xe2, 0xe8, 0xf0),
    border_strong: egui::Color32::from_rgb(0xcb, 0xd5, 0xe1),
    text: egui::Color32::from_rgb(0x0f, 0x17, 0x2a),
    text_med: egui::Color32::from_rgb(0x47, 0x55, 0x69),
    text_dim: egui::Color32::from_rgb(0x64, 0x74, 0x8b),
    accent: egui::Color32::from_rgb(0xb4, 0x53, 0x09),
    accent_hover: egui::Color32::from_rgb(0x92, 0x40, 0x0e),
    on_accent: egui::Color32::from_rgb(0xff, 0xff, 0xff),
    accent_soft: egui::Color32::from_rgb(0xfe, 0xf3, 0xc7),
    // The light `DESIGN.md` error `#ba1a1a`: `#dc2626` reads 4.41:1 on the panel.
    danger: egui::Color32::from_rgb(0xba, 0x1a, 0x1a),
    dark: false,
};

/// The palette of the current frame. `settings.theme` chooses the theme, egui applies it.
fn theme(ui: &egui::Ui) -> Theme {
    if ui.visuals().dark_mode { DARK } else { LIGHT }
}

/// The accent: exactly "selected", "confident tag", "keep", a progress step.
fn accent(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().selection.stroke.color
}

/// Secondary text and service captions.
fn dim(ui: &egui::Ui) -> egui::Color32 {
    ui.visuals().widgets.noninteractive.fg_stroke.color
}

/// The playback row under the frame in the full screen: 32 px of icon buttons and the 6 px egui
/// puts before the row.
const VIEWER_PLAYER_H: f32 = 38.0;
/// The series strip under the frame: the air, the caps label with the day and the 64×48 thumbnail
/// row (measured 80 px, the budget keeps 7 px for the label's line height).
const VIEWER_SERIES_H: f32 = 87.0;
/// The biggest scale of the full screen: "1:1" on a 1920 px frame asks for more than 100 %.
const MAX_ZOOM: f32 = 8.0;
/// The thumbnail of the series strip: 64×48, the position in the corner.
const SERIES_THUMB_W: f32 = 64.0;
const SERIES_THUMB_H: f32 = 48.0;
/// How many neighbours the series strip holds: a window around the current file, so the strip
/// needs no scroll area of its own.
const SERIES_WINDOW: usize = 16;

/// Layout metrics: change only together with docs/UI.md.
const LEFT_W: f32 = 260.0;
/// The gap between tiles — 10 px from the mock-up (`gap-2.5`), not the former 2 px.
const TILE_GAP: f32 = 10.0;
/// The caption strip under a preview: the name and the date in monospaced digits.
const CELL_CAPTION_H: f32 = 22.0;
/// The height of a navigation item and of a filter row.
const NAV_H: f32 = 28.0;
/// The icon of a navigation row and of a settings section (M22.8).
const NAV_ICON: f32 = 14.0;
const RIGHT_W: f32 = 280.0;
/// The header band of the right panel and the air around the file card: the mock-up's 44 px
/// `h-11` row and its `p-3.5` (M22.16).
const PANEL_HEAD_H: f32 = 44.0;
const PANEL_PAD: f32 = 12.0;
/// The top bar is 32 px and the bottom one 28 px — mock-up metrics, not a matter of taste.
const TOP_H: f32 = 32.0;
const STATUS_H: f32 = 28.0;
/// The search field of the top bar is never wider than the mock-up's `max-w-xl` (576 px);
/// below that it takes the room between the mark and the tools.
const SEARCH_MAX_W: f32 = 576.0;
/// The "Tiles" slider of the top bar: the mock-up's compact pill, not the whole panel width.
const TILE_SLIDER_W: f32 = 84.0;
/// The row of the content panel above the grid: the mock-up's `h-11`.
const RIBBON_H: f32 = 44.0;
/// The mock-up radii (Tailwind in the drawn screens): `rounded` = 4 — buttons, chips,
/// badges; `rounded-md` = 6 — navigation items, menus; `rounded-lg` = 8 — tile cards,
/// search fields, dialogs. The prose of `DESIGN.md` about 0 px loses to the drawn screens —
/// the same way as in the palette (ADR 0044).
const RADIUS_CTRL: f32 = 4.0;
const RADIUS_ITEM: f32 = 6.0;
const RADIUS_CARD: f32 = 8.0;
/// The tooltip of the path/date field (M25.3): the filter is literal, and the content search lives
/// in another field. One source for the hint and the test.
const PATH_DATE_HINT: &str = "The field is literal: a case-insensitive part of the path or of the ISO date — “2024-05” finds May 2024, “vacation” finds a folder or a file with that word. A search by what is on the photo is in the top bar: “Semantic / By text”.";
/// The height of a year chip in the left panel's cloud (M25.2): a tag chip is 20 px, a year is
/// 18 px, and the cloud stays dense enough to be seen without a scroll.
const YEARS_CHIP_H: f32 = 18.0;
/// Below this window width the details move into the left panel: the grid needs 4 columns.
const DETAILS_MIN_W: f32 = 1200.0;
/// The gap inside the calendar grids: between the month cards of the year layer and between the
/// day cells of the month and week layers (the mock-up's `gap-1.5` / `gap-2`).
const CAL_GAP: f32 = 6.0;
/// A month card of the year layer: the header, the cover and the footer with the volume and the size.
const CAL_CARD_H: f32 = 92.0;
/// A day cell of the month layer: the number, the counter, up to three previews and the volume bar.
const CAL_DAY_H: f32 = 96.0;

// Scrolling (M22.2b). Below is the only place its profile is set: the wheel, the touchpad
// and the coast after the fingers lift. No `ScrollArea` sets values of its own.
/// One wheel notch — a third of the default window height (840 px): 280 px.
const WHEEL_NOTCH_PTS: f32 = 280.0;
/// The touchpad coast: the time constant of the speed decay, s (the distance = v·τ).
const COAST_DECAY_S: f32 = 0.35;
/// A short coast does not start: an accidental touch must not move the grid.
const COAST_STOP_PTS: f32 = 6.0;
/// The pause without touchpad events after which the gesture counts as over, s.
const GESTURE_END_S: f64 = 0.08;
/// Smoothing of the gesture speed estimate: the time constant, s.
const VEL_SMOOTH_S: f32 = 0.05;

/// The tile order options and their words on the screen (M17). "Album order" is M19.
const GRID_SORTS: [(db::GridSort, &str); 4] = [
    (db::GridSort::DateDesc, "Date: newest first"),
    (db::GridSort::DateAsc, "Date: oldest first"),
    (db::GridSort::Name, "Name"),
    (db::GridSort::SizeDesc, "Size: largest first"),
];

fn sort_label(sort: db::GridSort, lang: i18n::Lang) -> &'static str {
    match sort {
        db::GridSort::Album => t(lang, "Album order"),
        _ => GRID_SORTS
            .iter()
            .find(|(value, _)| *value == sort)
            .map(|(_, label)| t(lang, label))
            .unwrap_or(t(lang, "Date: newest first")),
    }
}

/// Date comparison: "no date" is always last, in both directions (M17).
fn cmp_date_none_last(a: Option<&str>, b: Option<&str>, desc: bool) -> std::cmp::Ordering {
    use std::cmp::Ordering;
    match (a, b) {
        (None, None) => Ordering::Equal,
        (None, Some(_)) => Ordering::Greater,
        (Some(_), None) => Ordering::Less,
        (Some(x), Some(y)) => {
            if desc {
                y.cmp(x)
            } else {
                x.cmp(y)
            }
        }
    }
}

/// Months in the chosen sort order; "No date" is always the last group (M17).
/// Inside a group the order comes from `ordered`, there is no second sort key.
fn month_groups(
    ordered: &[usize],
    items: &[db::FileRow],
    ascend: bool,
) -> Vec<(String, Vec<usize>)> {
    let mut by_month: std::collections::BTreeMap<String, Vec<usize>> =
        std::collections::BTreeMap::new();
    for &i in ordered {
        let key = items[i]
            .taken_at
            .as_deref()
            .map(|d| d.chars().take(7).collect::<String>())
            .filter(|k| k.len() == 7)
            .unwrap_or_else(|| "no date".into());
        by_month.entry(key).or_default().push(i);
    }
    let undated = by_month.remove("no date");
    let mut groups: Vec<(String, Vec<usize>)> = if ascend {
        by_month.into_iter().collect()
    } else {
        by_month.into_iter().rev().collect()
    };
    if let Some(ids) = undated {
        groups.push(("no date".into(), ids));
    }
    groups
}

/// The background of a selected item: the accent with transparency, not a solid fill.
fn selected_fill(ui: &egui::Ui) -> egui::Color32 {
    theme(ui).accent_soft
}

/// The source of the shooting date: Latin in the DB, in the interface language on the screen.
fn date_source(src: &str, lang: i18n::Lang) -> &'static str {
    match src {
        "exif" => "EXIF",
        "filename" => t(lang, "file name"),
        "mtime" => t(lang, "file time"),
        _ => t(lang, "source not set"),
    }
}

/// The GitHub theme of egui is not used: the palette is entirely ours (M22).
/// `dark` is switched only through `ThemeMode` in the settings, not from here.
fn style(ctx: &egui::Context) {
    apply_theme(ctx, db::ThemeMode::Dark);
}

/// Apply the palette and the rhythm (docs/UI.md § Layout). Zero radius, 1 px lines, no
/// shadows: depth is given by the tone step alone.
fn apply_theme(ctx: &egui::Context, mode: db::ThemeMode) {
    ctx.set_theme(match mode {
        db::ThemeMode::Dark => egui::ThemePreference::Dark,
        db::ThemeMode::Light => egui::ThemePreference::Light,
        db::ThemeMode::System => egui::ThemePreference::System,
    });
    let p = if ctx.theme() == egui::Theme::Light { LIGHT } else { DARK };
    let line = |c| egui::Stroke::new(1.0, c);
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size = egui::vec2(32.0, 26.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.indent = 16.0;
        s.spacing.slider_width = 120.0;
        s.visuals.dark_mode = p.dark;
        s.visuals.override_text_color = Some(p.text);
        s.visuals.weak_text_color = Some(p.text_med);
        s.visuals.panel_fill = p.panel;
        s.visuals.window_fill = p.card;
        // The input field of the light theme is white on a grey canvas; in the dark one it is the canvas.
        s.visuals.extreme_bg_color = if p.dark { p.canvas } else { p.card };
        s.visuals.error_fg_color = p.danger;
        s.visuals.warn_fg_color = p.accent_hover;
        s.visuals.faint_bg_color = p.card;
        s.visuals.code_bg_color = p.canvas;
        s.visuals.window_stroke = line(p.border_strong);
        s.visuals.hyperlink_color = p.accent;
        s.visuals.selection.bg_fill = p.accent_soft;
        s.visuals.selection.stroke = line(p.accent);
        let ctrl = egui::CornerRadius::same(RADIUS_CTRL as u8);
        s.visuals.window_corner_radius = egui::CornerRadius::same(RADIUS_CARD as u8);
        s.visuals.menu_corner_radius = egui::CornerRadius::same(RADIUS_ITEM as u8);
        s.visuals.widgets.noninteractive.corner_radius = ctrl;
        s.visuals.widgets.inactive.corner_radius = ctrl;
        s.visuals.widgets.hovered.corner_radius = ctrl;
        s.visuals.widgets.active.corner_radius = ctrl;
        s.visuals.widgets.open.corner_radius = ctrl;
        s.visuals.widgets.noninteractive.bg_fill = p.panel;
        s.visuals.widgets.noninteractive.weak_bg_fill = p.panel;
        s.visuals.widgets.noninteractive.bg_stroke = line(p.border);
        s.visuals.widgets.noninteractive.fg_stroke = line(p.text_med);
        s.visuals.widgets.inactive.bg_fill = p.field;
        s.visuals.widgets.inactive.weak_bg_fill = p.card;
        s.visuals.widgets.inactive.bg_stroke = line(p.border_strong);
        s.visuals.widgets.inactive.fg_stroke = line(p.text);
        s.visuals.widgets.hovered.bg_fill = p.field;
        s.visuals.widgets.hovered.weak_bg_fill = p.field;
        s.visuals.widgets.hovered.bg_stroke = line(p.accent);
        s.visuals.widgets.hovered.fg_stroke = line(p.text);
        s.visuals.widgets.active.bg_fill = p.accent_soft;
        s.visuals.widgets.active.weak_bg_fill = p.accent_soft;
        s.visuals.widgets.active.bg_stroke = line(p.accent);
        s.visuals.widgets.active.fg_stroke = line(p.text);
        s.visuals.widgets.open.bg_fill = p.card;
        s.visuals.widgets.open.weak_bg_fill = p.card;
        s.visuals.widgets.open.bg_stroke = line(p.border_strong);
        s.visuals.widgets.open.fg_stroke = line(p.text);
        s.visuals.window_shadow = egui::Shadow::NONE;
        s.visuals.popup_shadow = egui::Shadow::NONE;
        // The font size scale of the mock-up: 10 — a service caps caption, 11 — mono numbers,
        // 12 — body, 13 — dense text, 14 — subheading, 18 — a view heading.
        use egui::{FontFamily::Proportional, FontId, TextStyle};
        s.text_styles.insert(TextStyle::Heading, FontId::new(18.0, Proportional));
        s.text_styles.insert(TextStyle::Body, FontId::new(13.0, Proportional));
        s.text_styles.insert(TextStyle::Button, FontId::new(13.0, Proportional));
        s.text_styles.insert(TextStyle::Small, FontId::new(11.0, Proportional));
        s.text_styles
            .insert(TextStyle::Monospace, FontId::new(11.0, egui::FontFamily::Monospace));
        // Scrolling (M22.2b): a jump to an element (a month, a file, the focus) is fast and without a
        // jerk; the touchpad coast is one decay profile for every area.
        s.scroll_animation.points_per_second = 2400.0;
        s.scroll_animation.duration = egui::Rangef::new(0.10, 0.30);
        s.spacing.scroll.kinetic.decay_time = COAST_DECAY_S;
        s.spacing.scroll.kinetic.stop_distance = 0.5;
    });
    // A wheel notch: winit gives `Line`, egui multiplies it by this number.
    ctx.options_mut(|o| o.input_options.line_scroll_speed = WHEEL_NOTCH_PTS);
}

/// Scrolling with the wheel and the touchpad (M22.2b).
///
/// The wheel: one notch is `WHEEL_NOTCH_PTS` (egui converts `Line` into points itself, one
/// profile for the whole window). The touchpad: on Linux and Windows winit does not say what
/// scrolls (`MouseWheelSource::Unknown`), so egui gives neither inertia nor "rubber banding" —
/// the gesture arrives one to one and breaks off when the fingers lift. Here it is carried
/// through: the speed of the last events is kept, and after the pause the gesture continues
/// as a coast decaying with `COAST_DECAY_S` until the distance drops below `COAST_STOP_PTS`.
/// A new event (in either direction) cancels the coast — without a jump.
#[derive(Default)]
struct Scroll {
    /// The gesture speed, points/s.
    vel: egui::Vec2,
    /// The time of the last touchpad event, s (the egui clock).
    last_event: f64,
    /// Fingers on the touchpad: the gesture is going.
    touched: bool,
    /// The gesture is over, the coast is running: while `true`, the next frame is needed.
    coasting: bool,
}

impl Scroll {
    /// The frame events → the addition to the scroll of this frame. Returns `true` if the coast
    /// is still running and the next frame is needed (at rest no frames are requested).
    fn pump(&mut self, ctx: &egui::Context) -> bool {
        let mut gesture = egui::Vec2::ZERO;
        let mut wheel = false;
        // Ctrl/Cmd + scroll is a zoom (a scale gesture), not a touchpad gesture: no coast is
        // needed after it.
        let zoom_modifier = ctx.options(|o| o.input_options.zoom_modifier);
        let (time, dt) = ctx.input(|i| {
            let zooming = i.modifiers.matches_any(zoom_modifier);
            for e in &i.events {
                if let egui::Event::MouseWheel {
                    unit, delta, source, ..
                } = e
                {
                    // Exact (pixel) deltas without a source — a touchpad or a high-resolution wheel; line
                    // deltas — an ordinary wheel.
                    if !zooming
                        && *unit == egui::MouseWheelUnit::Point
                        && *source == egui::MouseWheelSource::Unknown
                    {
                        gesture += *delta;
                    } else {
                        wheel = true;
                    }
                }
            }
            (i.time, i.stable_dt.clamp(1.0 / 240.0, 0.1))
        });

        if gesture != egui::Vec2::ZERO {
            // Fingers on the touchpad: the frame goes one to one with the gesture, the coast is off.
            let a = 1.0 - (-dt / VEL_SMOOTH_S).exp();
            self.vel = self.vel * (1.0 - a) + gesture / dt * a;
            self.last_event = time;
            self.touched = true;
            self.coasting = false;
        } else if wheel {
            // The wheel steps on its own (egui smooths the notch); the touchpad coast is cancelled.
            self.touched = false;
            self.coasting = false;
            self.vel = egui::Vec2::ZERO;
        } else if self.touched && time - self.last_event > GESTURE_END_S {
            // The fingers lifted: the speed fades out over the pause and turns into a coast.
            self.touched = false;
            self.vel *= (-(time - self.last_event) as f32 / COAST_DECAY_S).exp();
            self.coasting = self.vel.length() * COAST_DECAY_S >= COAST_STOP_PTS;
            if !self.coasting {
                self.vel = egui::Vec2::ZERO;
            }
        }

        if !self.coasting {
            return false;
        }
        ctx.input_mut(|i| i.smooth_scroll_delta += self.vel * dt);
        self.vel *= (-dt / COAST_DECAY_S).exp();
        if self.vel.length() * COAST_DECAY_S < COAST_STOP_PTS {
            self.vel = egui::Vec2::ZERO;
            self.coasting = false;
        }
        self.coasting
    }
}

/// Monospaced digits: sizes, counters, dates, frame sizes (mock-up: mono-numeric).
fn num(ui: &egui::Ui, text: impl Into<String>, size: f32) -> egui::RichText {
    let family = ui
        .style()
        .text_styles
        .get(&egui::TextStyle::Monospace)
        .map(|f| f.family.clone())
        .unwrap_or(egui::FontFamily::Monospace);
    egui::RichText::new(text.into()).family(family).size(size)
}

/// A service caption of a section: 10 px, upper case, secondary colour.
fn caps_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(10.0)
            .color(dim(ui)),
    );
}

/// The 1 px border of a docked panel: a panel is separated by a line, not by a gap.
fn panel_edge(ui: &egui::Ui, left: bool) {
    let r = ui.max_rect();
    let x = if left { r.left() + 0.5 } else { r.right() - 0.5 };
    ui.painter()
        .vline(x, r.y_range(), egui::Stroke::new(1.0, theme(ui).border));
}

/// Cut the corners of a preview to the card radius: the picture is drawn rectangular, so the outer
/// corners are returned to the background under the card. A tile has a caption strip that closes
/// the bottom corners; a preview card has none, so it asks for all four.
fn round_preview(ui: &egui::Ui, rect: egui::Rect, radius: f32, bg: egui::Color32, bottom: bool) {
    let r = radius
        .min(rect.width() / 2.0)
        .min(rect.height() / 2.0);
    if r < 0.5 {
        return;
    }
    let arc = |anchor: egui::Pos2, center: egui::Pos2, a0: f32, a1: f32| {
        let mut pts = Vec::with_capacity(6);
        pts.push(anchor);
        for i in 0..=4 {
            let th = a0 + (a1 - a0) * i as f32 / 4.0;
            pts.push(center + egui::vec2(r * th.cos(), r * th.sin()));
        }
        pts
    };
    let (l, t, rt, b) = (rect.left(), rect.top(), rect.right(), rect.bottom());
    let nw = arc(
        egui::pos2(l, t),
        egui::pos2(l + r, t + r),
        std::f32::consts::PI,
        1.5 * std::f32::consts::PI,
    );
    let ne = arc(
        egui::pos2(rt, t),
        egui::pos2(rt - r, t + r),
        1.5 * std::f32::consts::PI,
        2.0 * std::f32::consts::PI,
    );
    let mut corners = vec![nw, ne];
    if bottom {
        corners.push(arc(
            egui::pos2(l, b),
            egui::pos2(l + r, b - r),
            std::f32::consts::PI,
            0.5 * std::f32::consts::PI,
        ));
        corners.push(arc(
            egui::pos2(rt, b),
            egui::pos2(rt - r, b - r),
            0.5 * std::f32::consts::PI,
            0.0,
        ));
    }
    for pts in corners {
        ui.painter()
            .add(egui::Shape::convex_polygon(pts, bg, egui::Stroke::NONE));
    }
}

/// A 1 px line across the whole panel width: depth is given by the line, not by a shadow.
fn hairline(ui: &mut egui::Ui) {
    let w = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(w, 1.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
}

/// A 1 × 16 px divider between the groups of one strip: a line, not an eyeballed gap.
fn hairline_vertical(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 16.0), egui::Sense::hover());
    ui.painter()
        .rect_filled(rect, 0.0, ui.visuals().widgets.noninteractive.bg_stroke.color);
}

/// A caps heading of a card block with a mono value at the right edge — "ALBUMS 2",
/// "AI TAGS CLIP 84%" (the mock-up's section rows).
fn caps_row(ui: &mut egui::Ui, text: &str, right: &str, accent_word: bool) {
    ui.horizontal(|ui| {
        caps_label(ui, text);
        if !right.is_empty() {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let color = if accent_word { accent(ui) } else { dim(ui) };
                ui.label(num(ui, right, 10.0).color(color));
            });
        }
    });
}

/// What a click on a chip of the file card asked for (M22.17): the word acts on the tag, the "×"
/// on its right edge removes it. One 20 px target with two zones, so a tag is never removed by a
/// click that meant "show me the others".
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
enum ChipClick {
    #[default]
    None,
    /// The word: the caller shows the photos that carry the tag or opens the album.
    Body,
    /// The "×": the caller removes the tag from the photo, from the album or the own tag.
    Remove,
}

/// A chip of the file card: the mock-up's tag and album chip — 20 px high, mono 11, an optional
/// 12 px glyph before the word (the album's folder, M22.16), the accent when it is the tag the grid
/// is filtered by. The word and the "×" on the right edge are two targets with their own hints
/// (M22.17).
fn card_chip(
    ui: &mut egui::Ui,
    text: &str,
    lead: Option<Icon>,
    selected: bool,
    body_hint: &str,
    remove_hint: &str,
) -> ChipClick {
    let th = theme(ui);
    let font = egui::FontId::monospace(11.0);
    let word = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), th.text_med);
    let cross = ui.painter().layout_no_wrap("×".to_owned(), font, th.text_med);
    let lead_w = if lead.is_some() { 12.0 + 5.0 } else { 0.0 };
    const PAD: f32 = 7.0;
    const GAP: f32 = 4.0;
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(
            PAD + lead_w + word.size().x + GAP + cross.size().x + PAD,
            20.0,
        ),
        egui::Sense::click(),
    );
    // The zone of the "×": the glyph, the gap before it and 2 px around it — 16 px of a 20 px chip,
    // so the two targets never overlap and the removal is not a mis-click away (M22.17).
    let cross_left = rect.right() - PAD - cross.size().x - 2.0;
    let over_cross = ui
        .ctx()
        .pointer_hover_pos()
        .is_some_and(|p| p.x >= cross_left && rect.contains(p));
    if ui.is_rect_visible(rect) {
        let (fill, border, color) = if selected {
            (th.accent_soft, th.accent, th.accent)
        } else if resp.hovered() {
            (th.card, th.border_strong, th.text)
        } else {
            (th.field, th.border, th.text_med)
        };
        let radius = egui::CornerRadius::same(RADIUS_CTRL as u8);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        let word_x = rect.left() + PAD + lead_w;
        if let Some(icon) = lead {
            paint_icon(
                ui.painter(),
                egui::pos2(rect.left() + PAD + 6.0, rect.center().y),
                12.0 * 0.30,
                icon,
                color,
            );
        }
        let y = rect.center().y - word.size().y / 2.0;
        let word_w = word.size().x;
        ui.painter().galley(egui::pos2(word_x, y), word, color);
        // The "×" turns red under the pointer: the only target of the chip that removes something.
        let cross_color = if over_cross { th.danger } else { color };
        ui.painter()
            .galley(egui::pos2(word_x + word_w + GAP, y), cross, cross_color);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let hint = if over_cross { remove_hint } else { body_hint };
    let resp = resp.on_hover_text(hint);
    if !resp.clicked() {
        return ChipClick::None;
    }
    match resp.interact_pointer_pos() {
        Some(at) if at.x >= cross_left => ChipClick::Remove,
        _ => ChipClick::Body,
    }
}

/// A "caption on the left — number on the right" row: numbers always at the right edge of the column.
fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    // A pair of 11 px labels is not a button: `horizontal` assumes a 26 px interactive row, which
    // costs 12 px of air per line and 48 px in the four rows of the left panel summary (M22.3).
    ui.scope(|ui| {
        ui.spacing_mut().interact_size.y = 16.0;
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(label).size(11.0).color(dim(ui)));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // The number always has its unit and is always monospaced: the column does not twitch.
                ui.label(num(ui, value, 11.0));
            });
        });
    });
}

/// A card row: a caption of 11 px in a 72 px column, a value of 12 px on the right, wrapped.
/// The PhotoPrism inspector lives on such pairs, not on a "caption above value" column.
fn meta_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(72.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(egui::RichText::new(label).size(11.0).color(dim(ui)));
            },
        );
        ui.add(egui::Label::new(egui::RichText::new(value).size(12.0)).wrap());
    });
    ui.add_space(4.0);
}

/// A navigation item: the whole panel width, a selection highlight, the word on the left, the number on the right.
fn nav_item(
    ui: &mut egui::Ui,
    current: &mut View,
    view: View,
    icon: Icon,
    text: &str,
    count: Option<String>,
) {
    let selected = *current == view;
    let th = theme(ui);
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NAV_H), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        // The active item: the card tone, the text in the accent and a 2 px accent edge on the
        // left, radius 6 px (M22.11). The field tone of the drawn screen reads 4.07:1 under the
        // light accent; the card tone is 5.02:1 in light and 7.60:1 in dark, and the same mark as
        // the section list of the settings dialog.
        let fill = if selected || resp.hovered() {
            th.card
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_ITEM as u8), fill);
        if selected {
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.0, rect.height())),
                0.0,
                th.accent,
            );
        }
        let color = if selected {
            th.accent
        } else if resp.hovered() {
            th.text
        } else {
            th.text_med
        };
        paint_icon(
            ui.painter(),
            rect.left_center() + egui::vec2(8.0 + NAV_ICON * 0.5, 0.0),
            NAV_ICON * 0.32,
            icon,
            color,
        );
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0 + NAV_ICON + 6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(13.0),
            color,
        );
        if let Some(count) = count {
            ui.painter().text(
                rect.right_center() + egui::vec2(-8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                count,
                egui::FontId::monospace(11.0),
                if selected { th.accent } else { th.text_dim },
            );
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    if resp.clicked() {
        *current = view;
    }
}

/// The heading of a navigation group: 10 px caps, the wide tracking of the mock-up (label-caps).
fn nav_section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(
        egui::RichText::new(text.to_uppercase())
            .size(10.0)
            .color(dim(ui)),
    );
    ui.add_space(2.0);
}

/// An item of the left list in the "Settings" dialog (M22.8): the icon and the word of the
/// section, the active row on the accent fill with a 2 px accent edge (`photocat_4`'s
/// `border-l-[3px]`, rounded down to the 2 px line of our layout), a mono value at the right edge —
/// the state of the section before a click, as the mock-up's "ViT-B/32" and "4.8 GB".
fn settings_nav_item(
    ui: &mut egui::Ui,
    selected: bool,
    icon: Icon,
    text: &str,
    value: Option<&str>,
) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NAV_H), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let th = theme(ui);
        let fill = if selected {
            selected_fill(ui)
        } else if resp.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_ITEM as u8), fill);
        let color = if selected {
            th.accent
        } else if resp.hovered() {
            th.text
        } else {
            th.text_med
        };
        if selected {
            // The edge of the active row: a 2 px accent bar on the left, inside the fill.
            ui.painter().rect_filled(
                egui::Rect::from_min_size(rect.left_top(), egui::vec2(2.0, rect.height())),
                0.0,
                th.accent,
            );
        }
        paint_icon(
            ui.painter(),
            rect.left_center() + egui::vec2(10.0 + NAV_ICON * 0.5, 0.0),
            NAV_ICON * 0.32,
            icon,
            color,
        );
        ui.painter().text(
            rect.left_center() + egui::vec2(10.0 + NAV_ICON + 6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(13.0),
            color,
        );
        // The value stands at the right edge only when it does not touch the word: a long section
        // name must not collide with its counter.
        if let Some(value) = value {
            let width = |text: &str, font: egui::FontId| {
                ui.painter()
                    .layout_no_wrap(text.to_string(), font, color)
                    .size()
                    .x
            };
            let label_w = width(text, egui::FontId::proportional(13.0));
            let value_w = width(value, egui::FontId::monospace(10.0));
            if 10.0 + NAV_ICON + 6.0 + label_w + 8.0 + value_w + 8.0 <= rect.width() {
                ui.painter().text(
                    rect.right_center() + egui::vec2(-8.0, 0.0),
                    egui::Align2::RIGHT_CENTER,
                    value,
                    egui::FontId::monospace(10.0),
                    if selected { th.accent } else { th.text_dim },
                );
            }
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// The file name on one line: the full path lives in the card, not in the status bar.
fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// A flat section of a panel: no `Frame::group` border, the content across the whole width.
/// A frame inside a frame competes with the photo for attention (M16, docs/UI.md § Layout).
fn block(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width();
    ui.scope(|ui| {
        ui.set_width(w);
        add(ui);
    });
    ui.add_space(8.0);
}

/// A block of content on the card tone: an 8 px rounding, a 1 px border, 8 px of air inside.
/// The screens the mock-up has no layout for are built from the same parts as the main one
/// (M22.9, `plan/M22-ui-spec.md` §3), so a group of pictures is a card, not a `Frame::group`.
fn card_block(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let th = theme(ui);
    // The frame adds its 8 px inner margin and its 1 px line on each side to the content: without
    // them counted a block grew 2 px past the panel and the window (M22.12's rule, found again
    // while checking the file card in M22.14).
    let width = ui.available_width() - 2.0 * (8.0 + 1.0);
    egui::Frame::new()
        .fill(th.card)
        .stroke(egui::Stroke::new(1.0, th.border))
        .corner_radius(egui::CornerRadius::same(RADIUS_CARD as u8))
        .inner_margin(egui::Margin::same(8))
        .show(ui, |ui| {
            ui.set_width(width.max(0.0));
            add(ui);
        });
}

/// The frame of a dialog (M22.10): the card tone, a 1 px line and the 8 px radius of a card, with
/// 16 px of air inside. `egui::Modal` defaults to `Frame::popup`, which is the frame of a menu — a
/// 6 px radius and 6 px of air — so without this a dialog would look like a dropdown.
fn dialog_frame(ctx: &egui::Context) -> egui::Frame {
    let th = if ctx.theme() == egui::Theme::Light {
        LIGHT
    } else {
        DARK
    };
    egui::Frame::new()
        .fill(th.card)
        .stroke(egui::Stroke::new(1.0, th.border))
        .corner_radius(egui::CornerRadius::same(RADIUS_CARD as u8))
        .inner_margin(egui::Margin::same(16))
}

/// The chip of an active filter: the word with "×"; a click removes only this filter (M16).
fn filter_chip(ui: &mut egui::Ui, text: &str, lang: Lang) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(format!("{text} ×")).size(12.0)).small(),
    )
    .on_hover_text(t(lang, "remove the filter"))
    .clicked()
}

/// A tag chip: 20 px high, mono 11, the word and its count after a middle dot, the picked chip in
/// the accent. The width comes from the text and not from the state, so a click never moves the
/// row (M22.3).
fn tag_chip(ui: &mut egui::Ui, text: &str, selected: bool) -> bool {
    cloud_chip(ui, text, selected, 20.0).clicked()
}

/// A year chip of the cloud (M25.2): the geometry of a tag chip with an 18 px height and no
/// number inside — the count lives in the hint. The width follows the text and never the state.
fn year_chip(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    cloud_chip(ui, text, selected, YEARS_CHIP_H)
}

/// The chip of a cloud: mono 11, the width from the text, the picked one on the accent. The
/// height separates a tag chip (20 px) from a year chip (18 px); nothing else differs, so the
/// two clouds cannot drift apart.
fn cloud_chip(ui: &mut egui::Ui, text: &str, selected: bool, height: f32) -> egui::Response {
    let th = theme(ui);
    let font = egui::FontId::monospace(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), th.text_med);
    let size = egui::vec2(galley.size().x + 14.0, height);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let (fill, border, color) = if selected {
            (th.accent_soft, th.accent, th.accent)
        } else if resp.hovered() {
            (th.card, th.border_strong, th.text)
        } else {
            (th.field, th.border, th.text_med)
        };
        let radius = egui::CornerRadius::same(RADIUS_CTRL as u8);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp
}

/// "2004–2019": the range of the years the "…" chip filters, or the only old year when there is
/// one (M25.2). `skip` is how many of the newest years the cloud already draws.
fn years_range(years: &[(String, usize)], skip: usize) -> String {
    let Some((newest_old, _)) = years.get(skip) else {
        return String::new();
    };
    let oldest = &years[years.len() - 1].0;
    if oldest == newest_old {
        oldest.clone()
    } else {
        format!("{oldest}–{newest_old}")
    }
}

/// A toggle chip of the filter block (M25.4): the 20 px chip of a tag with the word and the count
/// after a middle dot, the picked one on the accent, dimmed and dead at zero files, the reason in
/// the hint. Returns whether it was clicked while enabled.
fn toggle_chip(
    ui: &mut egui::Ui,
    text: &str,
    selected: bool,
    enabled: bool,
    hint: &str,
) -> bool {
    let th = theme(ui);
    let font = egui::FontId::monospace(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, th.text_med);
    let size = egui::vec2(galley.size().x + 14.0, 20.0);
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(size, sense);
    if ui.is_rect_visible(rect) {
        let (fill, border, color) = if !enabled {
            (th.field, th.border, th.text_dim)
        } else if selected {
            (th.accent_soft, th.accent, th.accent)
        } else if resp.hovered() {
            (th.card, th.border_strong, th.text)
        } else {
            (th.field, th.border, th.text_med)
        };
        let radius = egui::CornerRadius::same(RADIUS_CTRL as u8);
        ui.painter().rect_filled(rect, radius, fill);
        ui.painter().rect_stroke(
            rect,
            radius,
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }
    if enabled && resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let resp = resp.on_hover_text(hint);
    enabled && resp.clicked()
}

/// A snapshot of the active filters for the row of chips.
struct Chips {
    query: String,
    tags: Vec<String>,
    /// The year filter as a chip ("year: 2019" or "older than 2020"), M25.2.
    year: Option<String>,
    /// The two "missing" filters (M25.4).
    no_date: bool,
    no_preview: bool,
    kind: KindFilter,
    /// The language the chips are written in; the snapshot is taken once per frame.
    lang: i18n::Lang,
    hits: Option<usize>,
    /// The open album (M19): an "Album “Name”" chip with "×".
    album: Option<String>,
    /// The multi-select (M18): how many are picked, their volume and whether picking is allowed in this view.
    picked: usize,
    picked_bytes: i64,
    pickable: bool,
}

impl Chips {
    /// Whether anything narrows the grid: an empty row of chips costs height and says nothing
    /// (M22.19 — the row is drawn only while a filter is on).
    fn any(&self) -> bool {
        !self.query.is_empty()
            || !self.tags.is_empty()
            || self.year.is_some()
            || self.no_date
            || self.no_preview
            || self.album.is_some()
            || self.kind != KindFilter::All
            || self.hits.is_some()
    }
}

/// Which filters a click on a chip asks to remove.
#[derive(Default, Clone, Copy)]
struct FilterClear {
    query: bool,
    /// The index of the tag whose chip was clicked: one "×" removes exactly that tag (M25.1).
    tag_at: Option<usize>,
    /// "Reset" or a year-like reset: the whole tag filter goes.
    tags: bool,
    /// The year chip of the chips row: clear the year filter.
    year: bool,
    /// The two "missing" filters (M25.4).
    no_date: bool,
    no_preview: bool,
    kind: bool,
    hits: bool,
    /// Close the open album: go back to the album tree.
    album: bool,
    /// "Select all (M)" — mark what is visible without touching what the filter hides.
    pick_all: bool,
    /// "Clear the selection" — an empty selection, as on Esc.
    clear_pick: bool,
}

/// A short caption for a chip: a long path does not blow up the filter row.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// The row of the filters that are on: the "×" of a chip removes exactly that, "Reset" clears
/// them all. The row is drawn only while something is on (M22.19): an empty row costs height
/// and says nothing. The selection lives in its own floating bar at the bottom of the grid.
fn draw_filter_chips(ui: &mut egui::Ui, chips: &Chips) -> FilterClear {
    let lang = chips.lang;
    let mut clear = FilterClear::default();
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(6.0, 4.0);
        if !chips.query.is_empty() {
            let text = format!("{} {}", t(lang, "path/date:"), shorten(&chips.query, 24));
            if filter_chip(ui, &text, lang) {
                clear.query = true;
            }
        }
        if let Some(year) = &chips.year
            && filter_chip(ui, year, lang)
        {
            clear.year = true;
        }
        for (i, tag) in chips.tags.iter().enumerate() {
            let text = format!("{} {}", t(lang, "tag:"), shorten(tag, 24));
            if filter_chip(ui, &text, lang) {
                clear.tag_at = Some(i);
            }
        }
        if let Some(album) = &chips.album {
            let name = shorten(album, 22);
            let text = i18n::fill(lang, "Album “{}”", &[name]);
            if filter_chip(ui, &text, lang) {
                clear.album = true;
            }
        }
        if let Some(label) = chips.kind.chip(chips.lang)
            && filter_chip(ui, label, lang)
        {
            clear.kind = true;
        }
        if chips.no_date && filter_chip(ui, t(lang, "No date"), lang) {
            clear.no_date = true;
        }
        if chips.no_preview && filter_chip(ui, t(lang, "No preview"), lang) {
            clear.no_preview = true;
        }
        if let Some(n) = chips.hits
            && filter_chip(ui, &i18n::relevance(lang, n), lang)
        {
            clear.hits = true;
        }
        if ui
            .add(WordButton::small(Icon::Close, t(lang, "Reset")))
            .on_hover_text(t(lang, "removes every filter and the search"))
            .clicked()
        {
            clear = FilterClear {
                query: true,
                tags: true,
                year: true,
                no_date: true,
                no_preview: true,
                kind: true,
                hits: true,
                album: true,
                ..FilterClear::default()
            };
        }
    });
    clear
}

/// A grid group heading: the month at 14 px, the file count and the volume at 11 px (as in PhotoPrism).
/// A month without a date is a separate last group, not a junk key at the top of the list.
fn month_header(ui: &mut egui::Ui, key: &str, files: usize, bytes: i64, lang: i18n::Lang) {
    month_row(ui, key, files, bytes, lang);
    ui.add_space(4.0);
}

/// The month title and its facts, without the spacing around: the group heading and the pinned
/// heading (M22.4) draw the same words, so they cannot drift apart.
fn month_labels(key: &str, files: usize, bytes: i64, lang: i18n::Lang) -> (String, String) {
    let title = if key == "no date" {
        t(lang, "No date").to_string()
    } else {
        match (key.get(0..4), key.get(5..7)) {
            (Some(y), Some(m)) => match (y.parse::<i32>(), m.parse::<usize>()) {
                (Ok(y), Ok(m)) if (1..=12).contains(&m) => {
                    format!("{} {y}", i18n::month_name(lang, m))
                }
                _ => key.to_string(),
            },
            _ => key.to_string(),
        }
    };
    let summary = i18n::fill(
        lang,
        "· {}, {}",
        &[i18n::count(lang, files, "file"), db::human_in(lang, bytes)],
    );
    (title, summary)
}

/// One heading row: the month at 14 px strong, the facts at 11 px in the numeric family.
fn month_row(ui: &mut egui::Ui, key: &str, files: usize, bytes: i64, lang: i18n::Lang) {
    let (title, summary) = month_labels(key, files, bytes, lang);
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(14.0).strong());
        ui.label(num(ui, summary, 11.0).color(dim(ui)));
    });
}

/// A group heading as it was drawn this frame: where it lies and what it says. The pinned heading
/// (M22.4) needs the places to know which group is under the top edge of the grid area.
struct MonthHead {
    /// The top of the heading in window coordinates (it moves up with the scroll).
    top: f32,
    /// The heading with its spacing: the height the pinned heading takes.
    height: f32,
    key: String,
    files: usize,
    bytes: i64,
}

/// The month heading pinned at the top of the grid area (M22.4, the former M13e): while a group is
/// on screen its heading stays at the top edge, and the next group's heading pushes it out. It is
/// drawn over the grid on the canvas tone — not in a nested `ScrollArea`, so the grid keeps one
/// scroll of its own.
fn draw_sticky_month(ui: &mut egui::Ui, viewport: egui::Rect, heads: &[MonthHead], lang: i18n::Lang) {
    if heads.is_empty() || viewport.height() <= 1.0 {
        return;
    }
    let top = viewport.top();
    // The group whose heading has already gone above the top edge; the next group's heading is the
    // one that will push it out.
    let mut current: Option<&MonthHead> = None;
    let mut next_top = None;
    for head in heads {
        if head.top <= top + 0.5 {
            current = Some(head);
        } else {
            next_top = Some(head.top);
            break;
        }
    }
    let Some(current) = current else {
        return;
    };
    // The heading is still drawn in its own place (the first line of the group): nothing to pin yet.
    if current.top >= top - 0.5 {
        return;
    }
    let mut y = top;
    if let Some(next_top) = next_top {
        y = y.min(next_top - current.height);
    }
    // The next group's heading has pushed this one out completely.
    if y + current.height <= top {
        return;
    }
    let rect = egui::Rect::from_min_size(
        egui::pos2(viewport.left(), y),
        egui::vec2(viewport.width(), current.height),
    );
    // The tiles scroll under the heading: the canvas tone is opaque, so nothing shows through it.
    ui.painter()
        .with_clip_rect(viewport.intersect(ui.clip_rect()))
        .rect_filled(rect, 0.0, theme(ui).canvas);
    // The same row as the group heading, in a child area that does not advance the grid's cursor.
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
    month_row(&mut child, &current.key, current.files, current.bytes, lang);
}

/// The album path in the tree over the loaded nodes: "Dacha 2024 / May".
fn album_label(albums: &[db::AlbumNode], id: i64) -> String {
    let mut names = Vec::new();
    let mut at = id;
    for _ in 0..256 {
        let Some(node) = albums.iter().find(|n| n.id == at) else {
            break;
        };
        names.push(node.name.clone());
        if node.parent_id == 0 {
            break;
        }
        at = node.parent_id;
    }
    names.reverse();
    names.join(" / ")
}

/// Whether a node is visible at the current collapsing: all its parents are expanded.
fn album_visible(albums: &[db::AlbumNode], node: &db::AlbumNode, expanded: &BTreeSet<i64>) -> bool {
    let mut parent = node.parent_id;
    for _ in 0..256 {
        if parent == 0 {
            return true;
        }
        if !expanded.contains(&parent) {
            return false;
        }
        let Some(up) = albums.iter().find(|n| n.id == parent) else {
            return true;
        };
        parent = up.parent_id;
    }
    true
}

/// One row of the album tree (M22.24): a folder glyph, the name and the facts, in the selected
/// tone of the screen. The whole row is one click target — the glyph is not a second, dead spot.
fn album_tree_row(ui: &mut egui::Ui, text: &str, selected: bool) -> egui::Response {
    let th = theme(ui);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(60.0), 26.0),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        if selected {
            painter.rect_filled(
                rect,
                egui::CornerRadius::same(RADIUS_CTRL as u8),
                th.accent_soft,
            );
            painter.rect_filled(
                egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height())),
                0.0,
                th.accent,
            );
        } else if resp.hovered() {
            painter.rect_filled(
                rect,
                egui::CornerRadius::same(RADIUS_CTRL as u8),
                th.card,
            );
        }
        let color = if selected { th.accent } else { th.text };
        let icon = if selected { th.accent } else { th.text_med };
        paint_icon(
            painter,
            egui::pos2(rect.left() + 11.0, rect.center().y),
            14.0 * 0.34,
            Icon::Folder,
            icon,
        );
        painter.with_clip_rect(rect.shrink(2.0)).text(
            egui::pos2(rect.left() + 24.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            shorten(text, ((rect.width() - 30.0) / 7.0).max(6.0) as usize),
            egui::FontId::proportional(14.0),
            color,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    resp
}

/// The album tree: an indent by depth, a collapse triangle, the folder glyph and counters.
/// Returns the selected node; a click on the triangle does not change the selection.
fn draw_album_tree(
    ui: &mut egui::Ui,
    albums: &[db::AlbumNode],
    selected: Option<i64>,
    expanded: &mut BTreeSet<i64>,
    lang: i18n::Lang,
) -> Option<i64> {
    let mut chosen = None;
    for node in albums {
        if !album_visible(albums, node, expanded) {
            continue;
        }
        ui.horizontal(|ui| {
            ui.add_space(node.depth as f32 * 14.0);
            if node.subalbums > 0 {
                let open = expanded.contains(&node.id);
                if ui
                    .add(egui::Button::new(if open { "▾" } else { "▸" }).small())
                    .on_hover_text(if open {
                        t(lang, "collapse the sub-albums")
                    } else {
                        t(lang, "expand the sub-albums")
                    })
                    .clicked()
                {
                    if open {
                        expanded.remove(&node.id);
                    } else {
                        expanded.insert(node.id);
                    }
                }
            } else {
                ui.add_space(22.0);
            }
            let trashed = if node.trashed > 0 {
                format!(" · {} in the trash", node.trashed)
            } else {
                String::new()
            };
            let text = i18n::fill(
                lang,
                "{} · {}{} · {} sub-albums · {}",
                &[
                    node.name.to_string(),
                    i18n::count(lang, node.files as usize, "file"),
                    trashed,
                    node.subalbums.to_string(),
                    db::human_in(lang, node.bytes).to_string(),
                ],
            );
            let hint = if !node.note.trim().is_empty() {
                node.note.clone()
            } else if !node.covers.is_empty() {
                t(lang, "open the album; the cover shows its first photos in order").to_string()
            } else {
                t(lang, "open the album").to_string()
            };
            if album_tree_row(ui, &text, selected == Some(node.id))
                .on_hover_text(hint)
                .clicked()
            {
                chosen = Some(node.id);
            }
        });
    }
    chosen
}

/// The header of an open album (M22.6): the name at 14 px and the facts of its card next to it —
/// "N files · M sub-albums · size" in mono 11, with the app trash after them when it is not empty.
fn album_header(ui: &mut egui::Ui, node: &db::AlbumNode, lang: i18n::Lang) {
    let th = theme(ui);
    let mut facts = format!(
        "{} · {} · {}",
        i18n::count(lang, node.files as usize, "file"),
        i18n::count(lang, node.subalbums as usize, "sub_album"),
        db::human_in(lang, node.bytes)
    );
    if node.trashed > 0 {
        facts.push_str(&i18n::fill(
            lang,
            " · {} in the trash",
            &[node.trashed.to_string()],
        ));
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(&node.name)
                .size(14.0)
                .strong()
                .color(th.text),
        );
        ui.label(num(ui, facts, 11.0).color(th.text_med));
    });
}

/// The date of an album card (M22.20): the day of the last change from the unix seconds the DB
/// keeps, "18.05.2024"; an empty or broken stamp prints nothing instead of a wrong date.
fn album_date(stamp: &str) -> String {
    stamp
        .trim()
        .parse::<i64>()
        .ok()
        .and_then(|secs| chrono::DateTime::from_timestamp(secs, 0))
        .map(|t| t.with_timezone(&chrono::Local).format("%d.%m.%Y").to_string())
        .unwrap_or_default()
}

/// The size of an album card (M22.20, the mock-up's `photocat_3`): the width from the "Tiles"
/// slider, a 16:10 cover with 10 px of air, the name at 13 px and the facts in mono 10 — one card
/// model for every album and sub-album.
fn album_card_size(tile: f32) -> egui::Vec2 {
    const PAD: f32 = 10.0;
    let w = tile.clamp(100.0, 260.0);
    let cover = ((w - 2.0 * PAD) * 10.0 / 16.0).round();
    egui::vec2(w, PAD + cover + 8.0 + 18.0 + 4.0 + 14.0 + PAD)
}

/// A plate of text over a cover: the mock-up's badge. The background is a photograph, not the
/// theme, so the text keeps the one dimmed-frame tone of the window.
fn cover_badge(
    painter: &egui::Painter,
    anchor: egui::Pos2,
    align: egui::Align2,
    text: &str,
) {
    let galley = painter.layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(10.0),
        egui::Color32::from_gray(235),
    );
    let size = galley.size() + egui::vec2(10.0, 4.0);
    let plate = align.anchor_size(anchor, size);
    painter.rect_filled(plate, RADIUS_CTRL, egui::Color32::from_black_alpha(150));
    painter.galley(
        egui::pos2(plate.left() + 5.0, plate.center().y - galley.size().y / 2.0),
        galley,
        egui::Color32::from_gray(235),
    );
}

/// The cells of an album cover (M22.21): one photo fills the frame, two split it into columns,
/// three give the first a half and the others the right stack, four a 2×2 mosaic. Never more than
/// `db::ALBUM_COVERS` cells, however many previews the album has.
fn cover_cells(rect: egui::Rect, n: usize) -> Vec<egui::Rect> {
    const GAP: f32 = 2.0;
    let n = n.min(db::ALBUM_COVERS);
    let half_w = (rect.width() - GAP) / 2.0;
    let half_h = (rect.height() - GAP) / 2.0;
    let left = rect.left();
    let right = left + half_w + GAP;
    let top = rect.top();
    let bottom = top + half_h + GAP;
    match n {
        0 => Vec::new(),
        1 => vec![rect],
        2 => vec![
            egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(half_w, rect.height())),
            egui::Rect::from_min_size(egui::pos2(right, top), egui::vec2(half_w, rect.height())),
        ],
        3 => vec![
            egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(half_w, rect.height())),
            egui::Rect::from_min_size(egui::pos2(right, top), egui::vec2(half_w, half_h)),
            egui::Rect::from_min_size(egui::pos2(right, bottom), egui::vec2(half_w, half_h)),
        ],
        _ => vec![
            egui::Rect::from_min_size(egui::pos2(left, top), egui::vec2(half_w, half_h)),
            egui::Rect::from_min_size(egui::pos2(right, top), egui::vec2(half_w, half_h)),
            egui::Rect::from_min_size(egui::pos2(left, bottom), egui::vec2(half_w, half_h)),
            egui::Rect::from_min_size(egui::pos2(right, bottom), egui::vec2(half_w, half_h)),
        ],
    }
}

/// The mosaic of an album cover (M22.21): up to four previews, each filling its cell whole (the
/// sides that do not fit are cut evenly, M22.29), the `canvas` showing through the 2 px seams and
/// the cut corners of the well, which `bg` restores. Returns how many previews were drawn.
fn paint_cover_mosaic(
    ui: &egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    covers: &[&FileRow],
    rect: egui::Rect,
    bg: egui::Color32,
) -> usize {
    let cells = cover_cells(rect, covers.len());
    let mut drawn = 0;
    for (f, cell) in covers.iter().zip(cells) {
        if paint_thumb_as(ui, ctx, cs, f, cell, ThumbFit::Filling) {
            drawn += 1;
        }
    }
    if drawn > 0 {
        round_preview(ui, rect, RADIUS_CTRL, bg, true);
    }
    drawn
}

/// An album card (M22.20, the mock-up's `photocat_3`): a `card`-tone card with a 16:10 canvas
/// cover — a mosaic of up to four previews of its photos, each filling its cell without margins
/// (M22.29), the count of files in the corner, the count of sub-albums in the other one,
/// the name at 13 px and "size · date" in mono 10. A click opens the album; the open album keeps
/// the accent frame and the accent name.
#[allow(clippy::too_many_arguments)]
fn draw_album_cell(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    node: &db::AlbumNode,
    covers: &[&FileRow],
    tile: f32,
    open: bool,
    lang: i18n::Lang,
) -> bool {
    const PAD: f32 = 10.0;
    let th = theme(ui);
    let size = album_card_size(tile);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, RADIUS_CARD, th.card);
        // A 1 px line on every card — the accent one for the open album, the hovered one as well.
        let line = if open || resp.hovered() {
            th.accent
        } else {
            th.border
        };
        painter.rect_stroke(
            rect,
            RADIUS_CARD,
            egui::Stroke::new(if open { 2.0 } else { 1.0 }, line),
            egui::StrokeKind::Inside,
        );
        let cover_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + PAD, rect.top() + PAD),
            egui::vec2(size.x - 2.0 * PAD, size.y - 2.0 * PAD - 44.0),
        );
        painter.rect_filled(cover_rect, RADIUS_CTRL, th.canvas);
        let drawn = paint_cover_mosaic(ui, ctx, cs, covers, cover_rect, th.card) > 0;
        if !drawn {
            // An album without a snapshot keeps the empty canvas and a folder mark, not a placeholder.
            paint_icon(
                painter,
                cover_rect.center(),
                18.0 * 0.34,
                Icon::Folder,
                th.text_dim,
            );
        }
        // The two counters of the mock-up sit on the cover: the sub-albums at the top left, the
        // files at the bottom right.
        if node.subalbums > 0 {
            cover_badge(
                painter,
                cover_rect.left_top() + egui::vec2(6.0, 6.0),
                egui::Align2::LEFT_TOP,
                &i18n::count(lang, node.subalbums as usize, "sub_album"),
            );
        }
        cover_badge(
            painter,
            cover_rect.right_bottom() - egui::vec2(6.0, 6.0),
            egui::Align2::RIGHT_BOTTOM,
            &i18n::count(lang, node.files as usize, "file"),
        );
        let inner = size.x - 2.0 * PAD;
        let name_top = cover_rect.bottom() + 8.0;
        let facts_top = name_top + 18.0;
        // The texts are clipped to the card: a long name is cut with an ellipsis, not spilling out.
        let clipped = painter.with_clip_rect(rect.shrink(1.0));
        clipped.text(
            egui::pos2(rect.left() + PAD, name_top),
            egui::Align2::LEFT_TOP,
            shorten(&node.name, (inner / 7.0).max(6.0) as usize),
            egui::FontId::proportional(13.0),
            if open { th.accent } else { th.text },
        );
        let facts = format!(
            "{} · {}",
            db::human_in(lang, node.bytes),
            album_date(&node.updated_at)
        );
        clipped.text(
            egui::pos2(rect.left() + PAD, facts_top),
            egui::Align2::LEFT_TOP,
            shorten(&facts, (inner / 6.0).max(8.0) as usize),
            egui::FontId::monospace(10.0),
            th.text_med,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let hint = if !node.note.trim().is_empty() {
        node.note.clone()
    } else {
        i18n::fill(
            lang,
            "{} · {} · {} — open the album",
            &[
                i18n::count(lang, node.files as usize, "file"),
                i18n::count(lang, node.subalbums as usize, "sub_album"),
                db::human_in(lang, node.bytes).to_string(),
            ],
        )
    };
    resp.on_hover_text(hint).clicked()
}

/// The height of an album row in the list look (M22.20): a 16:10 cover 26 px high and text that
/// scales with the "Tiles" slider, the same as the file list.
fn album_row_height(tile: f32) -> f32 {
    (26.0 + tile * 0.12).round().clamp(36.0, 58.0)
}

/// An album in the list look (M22.20): a mosaic cover (M22.21), the name, the path and "N files ·
/// M sub-albums · size · date" in mono 10, with the accent edge on the open album. A click opens
/// it.
#[allow(clippy::too_many_arguments)]
fn draw_album_row(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    node: &db::AlbumNode,
    covers: &[&FileRow],
    tile: f32,
    open: bool,
    lang: i18n::Lang,
) -> bool {
    let th = theme(ui);
    let h = album_row_height(tile);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), h),
        egui::Sense::click(),
    );
    if ui.is_rect_visible(rect) {
        if open || resp.hovered() {
            let fill = if open { th.accent_soft } else { th.card };
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(RADIUS_CTRL as u8), fill);
        }
        if open {
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height()));
            ui.painter().rect_filled(bar, 0.0, th.accent);
        }
        let thumb_h = (h - 10.0).max(16.0);
        let thumb = egui::vec2((thumb_h * 16.0 / 10.0).round(), thumb_h);
        let tr = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(8.0, (h - thumb.y) / 2.0),
            thumb,
        );
        ui.painter().rect_filled(tr, RADIUS_CTRL, th.canvas);
        if paint_cover_mosaic(ui, ctx, cs, covers, tr, th.canvas) == 0 {
            paint_icon(
                ui.painter(),
                tr.center(),
                12.0 * 0.34,
                Icon::Folder,
                th.text_dim,
            );
        }
        let name_px = list_name_px(tile);
        let meta_px = list_meta_px(tile);
        let text_left = tr.right() + 10.0;
        let clip = egui::Rect::from_min_max(
            egui::pos2(text_left, rect.top()),
            egui::pos2(rect.right() - 10.0, rect.bottom()),
        );
        let painter = ui.painter().with_clip_rect(clip);
        let name_top = rect.center().y - name_px - 1.0;
        painter.text(
            egui::pos2(text_left, name_top),
            egui::Align2::LEFT_TOP,
            shorten(&node.name, ((clip.width() / name_px) * 1.6) as usize),
            egui::FontId::proportional(name_px),
            if open { th.accent } else { th.text },
        );
        let facts = i18n::fill(
            lang,
            "{} · {} · {} · {}",
            &[
                i18n::count(lang, node.files as usize, "file"),
                i18n::count(lang, node.subalbums as usize, "sub_album"),
                db::human_in(lang, node.bytes).to_string(),
                album_date(&node.updated_at),
            ],
        );
        painter.text(
            egui::pos2(text_left, name_top + name_px + 2.0),
            egui::Align2::LEFT_TOP,
            &facts,
            egui::FontId::monospace(meta_px),
            th.text_dim,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    let hint = if !node.note.trim().is_empty() {
        node.note.clone()
    } else {
        t(lang, "open the album").to_string()
    };
    resp.on_hover_text(hint).clicked()
}

/// The words of the album order menu (M22.20).
fn album_sort_label(sort: db::AlbumSort, lang: i18n::Lang) -> &'static str {
    t(
        lang,
        match sort {
            db::AlbumSort::Position => "Album order",
            db::AlbumSort::Updated => "By update",
            db::AlbumSort::Name => "By name",
            db::AlbumSort::Count => "By file count",
            db::AlbumSort::Size => "By size",
        },
    )
}

/// The previews of an album card (M22.21): up to four files in the album order, looked up among
/// the loaded rows by their preview path. A file outside the loaded rows gets no cell — an album
/// never invents a placeholder.
fn album_covers<'a>(
    node: &db::AlbumNode,
    by_thumb: &HashMap<&str, &'a FileRow>,
) -> Vec<&'a FileRow> {
    node.covers
        .iter()
        .filter_map(|thumb| by_thumb.get(thumb.as_str()).copied())
        .take(db::ALBUM_COVERS)
        .collect()
}

/// The cards (or the rows) of one album level (M22.20): 10 px apart, wrapped to the panel. Returns
/// the album of a click — the caller opens it.
#[allow(clippy::too_many_arguments)]
fn draw_album_cards(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    cards: &[db::AlbumNode],
    items: &[FileRow],
    tile: f32,
    selected: Option<i64>,
    list: bool,
    lang: i18n::Lang,
) -> Option<i64> {
    let mut open = None;
    // The preview path of every loaded row once: a card looks its mosaic up by it, so a level of
    // albums does not scan the whole grid for every cell.
    let by_thumb: HashMap<&str, &FileRow> = items
        .iter()
        .filter_map(|f| f.thumb.as_deref().map(|thumb| (thumb, f)))
        .collect();
    if list {
        for node in cards {
            let covers = album_covers(node, &by_thumb);
            if draw_album_row(
                ui,
                ctx,
                cs,
                node,
                &covers,
                tile,
                selected == Some(node.id),
                lang,
            ) {
                open = Some(node.id);
            }
        }
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(TILE_GAP, TILE_GAP);
            for node in cards {
                let covers = album_covers(node, &by_thumb);
                if draw_album_cell(
                    ui,
                    ctx,
                    cs,
                    node,
                    &covers,
                    tile,
                    selected == Some(node.id),
                    lang,
                ) {
                    open = Some(node.id);
                }
            }
        });
    }
    open
}

/// The albums of one level in the chosen order (M22.20): the children of `parent` whose name
/// contains `filter`, sorted by `sort`. The album order stays the data of the export.
fn album_cards_of(
    albums: &[db::AlbumNode],
    parent: i64,
    filter: &str,
    sort: db::AlbumSort,
) -> Vec<db::AlbumNode> {
    let needle = filter.trim().to_lowercase();
    let mut cards: Vec<db::AlbumNode> = albums
        .iter()
        .filter(|n| n.parent_id == parent)
        .filter(|n| needle.is_empty() || n.name.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    cards.sort_by(|a, b| db::album_order(a, b, sort));
    cards
}

/// The mock-up's dashed "+ Add" chip (M22.20): the one path that puts a photo into an album from
/// the file card. A dashed line says "a place to fill" without the weight of a button.
fn dashed_add_chip(ui: &mut egui::Ui, text: &str) -> egui::Response {
    let th = theme(ui);
    let galley = ui.painter().layout_no_wrap(
        text.to_owned(),
        egui::FontId::monospace(11.0),
        th.text_med,
    );
    let size = egui::vec2(galley.size().x + 16.0, 20.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let color = if resp.hovered() {
            th.accent
        } else {
            th.border_strong
        };
        let st = egui::Stroke::new(1.0, color);
        let r = rect.shrink(0.5);
        let corners = [r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()];
        for i in 0..4 {
            ui.painter().extend(egui::Shape::dashed_line(
                &[corners[i], corners[(i + 1) % 4]],
                st,
                4.0,
                3.0,
            ));
        }
        ui.painter().galley(
            egui::pos2(rect.left() + 8.0, rect.center().y - galley.size().y / 2.0),
            galley,
            if resp.hovered() {
                th.accent
            } else {
                th.text_med
            },
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    resp
}

/// The progress row of a background pass: `done/total`, "Stop" next to it.
/// Returns `true` if "Stop" was pressed.
fn progress_row(ui: &mut egui::Ui, name: &str, p: &Arc<Progress>, running: bool) -> bool {
    let (d, t) = (p.done(), p.total());
    let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
    let mut stop = false;
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.add(
            egui::ProgressBar::new(frac)
                .desired_width(320.0)
                .text(if t > 0 {
                    format!("{name}: {d}/{t}")
                } else {
                    p.msg()
                }),
        );
        if running && word_button(ui, Icon::Stop, "Stop").clicked() {
            stop = true;
        }
    });
    stop
}

/// The language of a window that is about to open. On the very first launch it comes from the system
/// locale and is stored once (`db::ensure_lang`), so the window, the CLI reports and the owner's
/// later choice always agree — before M24.2 the GUI read the default English and stored nothing,
/// while the CLI detected the locale, so a fresh install showed two languages at once.
fn first_lang(db_path: &Path) -> i18n::Lang {
    db::open(db_path)
        .map(|c| {
            let _ = db::ensure_lang(&c);
            db::load_settings(&c).lang
        })
        .unwrap_or_default()
}

pub fn run(explicit: Option<PathBuf>) -> Result<()> {
    let db_path = db::resolve_db(explicit.as_deref())?;
    let lang = first_lang(&db_path);
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 840.0])
            .with_title(t(lang, "photocat — a media archive cataloguer")),
        ..Default::default()
    };
    eframe::run_native(
        "photocat",
        opts,
        Box::new(move |cc| {
            // One style for the whole GUI (docs/UI.md § Layout) together with the scrolling profile.
            style(&cc.egui_ctx);
            Ok(Box::new(App::new(db_path.clone())))
        }),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

struct App {
    db_path: PathBuf,
    thumbs_dir: PathBuf,
    items: Vec<FileRow>,
    /// Files in the app trash: untouched on disk, hidden from the ordinary views.
    trash_items: Vec<FileRow>,
    /// Albums (M19): the tree, the open node and its files in album order.
    albums: Vec<db::AlbumNode>,
    album_selected: Option<i64>,
    album_expanded: BTreeSet<i64>,
    album_files: Vec<i64>,
    album_set: HashSet<i64>,
    album_trashed: usize,
    /// "All files" inside the "Albums" view: a temporary look to gather photos and press
    /// "Add selected". It is not saved as a setting.
    album_browse_all: bool,
    album_dialog: Option<AlbumDialog>,
    album_name_input: String,
    album_move_parent: i64,
    album_delete_text: String,
    /// The numbers of the delete confirmation: the albums and the file rows in mono 11 (M22.10).
    album_delete_facts: String,
    /// The albums of the selected file for the card: a cache for one file.
    file_albums: Vec<(i64, String)>,
    file_albums_for: Option<i64>,
    /// The filter of the album cards (M22.20): a substring of the album name. Not saved.
    album_filter: String,
    /// The "add photos to an album" dialog (M22.20): from the file card or from the selection.
    album_pick: Option<AlbumPick>,
    /// Exporting an album to disk (M20): the dialog, the plan tree and the background pass.
    export_open: bool,
    export_album: Option<i64>,
    export_out: String,
    export_name: String,
    export_error: String,
    export_plan: Option<organize::ExportPlan>,
    /// For which album, folder and template the plan was built: editing a field makes it stale.
    export_plan_for: Option<(i64, String, String)>,
    export_report: Option<organize::ExportReport>,
    export_progress: Option<Arc<Progress>>,
    export_rx: Option<mpsc::Receiver<organize::ExportReport>>,
    /// Integrity (M9): two folders, a manifest file and the result of the last pass.
    /// The background passes are the same as in the CLI (`verify`), plus "Stop" and progress in the top bar.
    integrity_a: String,
    integrity_b: String,
    integrity_manifest: String,
    integrity_show_all: bool,
    integrity_result: Option<IntegrityOutcome>,
    integrity_error: String,
    integrity_progress: Option<Arc<Progress>>,
    integrity_rx: Option<mpsc::Receiver<IntegrityReply>>,
    /// The manifest file exists already: overwriting only after a second press.
    integrity_overwrite: bool,
    /// The reminder to check (M10): the term is counted from `settings`, the banner starts nothing
    /// by itself. "Remind me later" is a decision for this session.
    verify_due: db::VerifyDue,
    verify_banner_hidden: bool,
    textures: HashMap<i64, egui::TextureHandle>,
    missing: HashSet<i64>,
    selected: Option<i64>,
    /// The multi-select of tiles (M18): the marked files. In memory only — empty after a restart.
    picked: BTreeSet<i64>,
    /// The anchor of Shift+click: the file the range is counted from in `App::ordered`.
    pick_anchor: Option<i64>,
    /// The set of files the selection was made in: a change of the view, filter, search or order
    /// clears it with a reason in the status bar.
    pick_scope: PickScope,
    view: View,
    groups: Vec<Group>,
    max_dist: u32,
    path_input: String,
    query: String,
    kind_filter: KindFilter,
    /// The year filter (M25.2): independent of the path/date field, so a typed word survives a
    /// click on a year. `None` means "all years".
    year_filter: Option<YearPick>,
    /// The two "missing" filters of the left panel (M25.4): only files without a date, only files
    /// whose preview is missing.
    only_no_date: bool,
    only_no_preview: bool,
    /// The files whose `thumb` path is recorded but whose preview is not on disk — read once per
    /// `reload`, so the filter does not stat the cache on every frame (M25.4).
    previews_missing: HashSet<i64>,
    progress: Option<Arc<Progress>>,
    rx: Option<mpsc::Receiver<Result<index::Stats>>>,
    /// CLIP tags: file_id → [(tag, weight)] descending.
    tags: HashMap<i64, Vec<(String, f32)>>,
    /// The tag dictionary of the active model: labels, prompts, order (M15.4).
    tag_defs: Vec<db::TagDef>,
    /// An edit of the dictionary is not recomputed yet: the "Recompute the tags" button is highlighted.
    tag_dict_dirty: bool,
    /// A new label and prompts in the dictionary editor.
    tag_new_label: String,
    tag_new_prompts: String,
    /// The path for exporting and importing the JSON dictionary; empty means a file in the cache.
    tag_json_path: String,
    /// Recomputing the tags over stored vectors (without running CLIP over the photos again).
    retag_rx: Option<mpsc::Receiver<Result<embed::RetagStats>>>,
    /// The weight distribution of every tag (descending): the percentile threshold of the filter
    /// is computed from it, without sorting on every frame.
    tag_dists: HashMap<String, Vec<f32>>,
    /// The owner's own tags: file_id → labels.
    user_tags: HashMap<i64, Vec<String>>,
    user_tag_input: String,
    /// The picked tags (M25.1): a file passes only when it carries every one of them above its
    /// own threshold. Sorted and without duplicates; empty means no tag filter.
    tag_filters: Vec<String>,
    semantic_query: String,
    /// The results of the semantic search in descending similarity.
    semantic_hits: Option<Vec<i64>>,
    /// The model is loaded after the first search; later queries go without a pause.
    ml_model: Option<embed::Model>,
    search_rx: Option<mpsc::Receiver<SearchReply>>,
    embed_progress: Option<Arc<Progress>>,
    embed_rx: Option<mpsc::Receiver<EmbedReply>>,
    /// The application settings (model, path, limits, thresholds).
    settings: db::Settings,
    /// The editable path to the models: applied by a button, not on every character.
    models_dir_input: String,
    /// Consent to download the weights (ADR 0015).
    download_consent: bool,
    download_progress: Option<Arc<Progress>>,
    download_rx: Option<mpsc::Receiver<Result<embed::DownloadReport>>>,
    /// The calendar: the selected month and day.
    cal_year: i32,
    cal_month: u32,
    cal_day: Option<String>,
    /// How many tags the owner hid on a file (for "restore").
    hidden_tags: HashMap<i64, usize>,
    /// The text of a photo: loaded for the selected file and cached.
    ocr_text: HashMap<i64, String>,
    /// The fragments of the found text with «» markers (for highlighting in the card).
    ocr_snippets: HashMap<i64, String>,
    /// Search the text on the photos (FTS5) instead of the meaning (CLIP).
    search_ocr: bool,
    ocr_files: i64,
    ocr_chars: i64,
    /// Faces: the groups, the faces of the selected group and the progress of a pass.
    face_clusters: Vec<db::FaceCluster>,
    face_views: Vec<db::FaceView>,
    face_textures: HashMap<i64, egui::TextureHandle>,
    face_missing: HashSet<i64>,
    face_selected_cluster: Option<i64>,
    face_label_input: String,
    face_images: usize,
    face_todo: usize,
    face_faces: i64,
    face_photos: i64,
    face_progress: Option<Arc<Progress>>,
    face_rx: Option<mpsc::Receiver<FaceReply>>,
    face_download_consent: bool,
    face_download_progress: Option<Arc<Progress>>,
    face_download_rx: Option<mpsc::Receiver<Result<faces::DownloadReport>>>,
    /// How many photos a batch OCR pass would take by the heuristic (a plan, not a run).
    ocr_candidates: usize,
    /// Of them — those without text yet; "Continue" shows exactly the remainder.
    ocr_remaining: usize,
    ocr_progress: Option<Arc<Progress>>,
    ocr_rx: Option<mpsc::Receiver<OcrReply>>,
    /// The tesseract version (the path from the settings or PATH): `None` means OCR is unavailable, the button is dimmed.
    tesseract: Option<String>,
    /// The editable paths to the programs: applied by a button after a test launch.
    tools_input: tools::Tools,
    /// The verified state of the programs (a version or an error) — they are not started on every frame.
    /// One line per program of `tools::ALL`: the version, the file found, the source.
    tool_states: Vec<String>,
    /// The "Settings" dialog: whether it is open and which section is shown.
    settings_open: bool,
    settings_tab: SettingsTab,
    /// The right details panel: it can be closed to free room for the calendar and the grid.
    details_open: bool,
    /// The tile and calendar size; kept in the settings.
    tile_size: f32,
    /// The full-screen viewer and the loading of a large frame in the background.
    viewer: Option<Viewer>,
    viewer_tex: Option<(i64, egui::TextureHandle)>,
    /// The faces of the frame in the full screen, read from the DB once per file (M22.7).
    viewer_faces: Option<(i64, Vec<db::FaceBox>)>,
    /// A pre-decoded frame of the selected file: the full screen opens without waiting.
    photo_cache: HashMap<i64, (usize, usize, Vec<u8>)>,
    photo_loading: HashSet<i64>,
    photo_jobs: VecDeque<i64>,
    photo_rx: Option<mpsc::Receiver<PhotoReply>>,
    /// The built-in player: video and sound in the application window (ADR 0030).
    video: Option<player::Player>,
    /// The player frame as a texture; refreshed from `poll_video`.
    video_tex: Option<egui::TextureHandle>,
    /// The fallback: an external ffplay when the built-in player refused.
    video_child: Option<std::process::Child>,
    /// When the video texture was last refreshed: filling the frame is the most expensive
    /// operation in a frame, so there is no point in doing it more than 30 times a second.
    video_tex_at: Option<std::time::Instant>,
    /// The "the decoder cannot keep up" message is shown once per file.
    video_lag_shown: bool,
    /// The video is over: the full-screen viewer moves to the next file.
    video_ended: bool,
    video_id: Option<i64>,
    /// The calendar: the gradation and the Monday of the selected week.
    cal_scale: CalScale,
    cal_week: chrono::NaiveDate,
    /// The faces selected for a batch naming.
    face_picked: HashSet<i64>,
    face_group_input: String,
    /// The groups marked for the "all together in one photo" search.
    face_picked_clusters: HashSet<i64>,
    face_together: bool,
    /// The confirmation of emptying the trash: without it nothing goes to the system trash.
    trash_confirm: bool,
    trash_progress: Option<Arc<Progress>>,
    trash_rx: Option<mpsc::Receiver<TrashReply>>,
    /// Folders and the built-in folder picker (without depending on the system dialog).
    roots: Vec<String>,
    /// How many indexed files lie in each root: the folder list says where the archive is. Counted
    /// when the index or the folder list changes, not on every frame of the dialog (M22.10).
    root_counts: BTreeMap<String, usize>,
    roots_open: bool,
    picker_open: bool,
    picker_current: PathBuf,
    picker_entries: Vec<PathBuf>,
    picker_error: String,
    /// The used space is counted on a button press, not on every frame.
    storage_usage: Option<storage::Usage>,
    storage_confirm: Option<StorageAction>,
    status: String,
    /// The theme allowed for the last frame: the palette is rebuilt only when it changes
    /// (a setting or the OS theme), not on every frame.
    theme_key: Option<egui::Theme>,
    /// Scrolling with the wheel and the touchpad: the speed, the coast, the pause (M22.2b).
    scroll: Scroll,
}

impl App {
    fn new(db_path: PathBuf) -> Self {
        let thumb_dir = db::cache_dir().join("thumbs");
        let mut me = Self {
            db_path,
            thumbs_dir: thumb_dir,
            items: Vec::new(),
            trash_items: Vec::new(),
            albums: Vec::new(),
            album_selected: None,
            album_expanded: BTreeSet::new(),
            album_files: Vec::new(),
            album_set: HashSet::new(),
            album_trashed: 0,
            album_browse_all: false,
            album_dialog: None,
            album_name_input: String::new(),
            album_move_parent: 0,
            album_delete_text: String::new(),
            album_delete_facts: String::new(),
            file_albums: Vec::new(),
            file_albums_for: None,
            album_filter: String::new(),
            album_pick: None,
            export_open: false,
            export_album: None,
            export_out: String::new(),
            export_name: organize::EXPORT_TEMPLATE.to_string(),
            export_error: String::new(),
            export_plan: None,
            export_plan_for: None,
            export_report: None,
            export_progress: None,
            export_rx: None,
            integrity_a: String::new(),
            integrity_b: String::new(),
            integrity_manifest: String::new(),
            integrity_show_all: false,
            integrity_result: None,
            integrity_error: String::new(),
            integrity_progress: None,
            integrity_rx: None,
            integrity_overwrite: false,
            verify_due: db::VerifyDue::NoManifest,
            verify_banner_hidden: false,
            textures: HashMap::new(),
            missing: HashSet::new(),
            selected: None,
            picked: BTreeSet::new(),
            pick_anchor: None,
            pick_scope: PickScope {
                view: View::Grid,
                query: String::new(),
                kind: KindFilter::All,
                tags: Vec::new(),
                year: None,
                no_date: false,
                no_preview: false,
                hits: None,
                sort: db::GridSort::DateDesc,
                layout: db::GridLayout::Months,
                album: None,
                browse_all: false,
            },
            view: View::Grid,
            groups: Vec::new(),
            max_dist: 6,
            path_input: String::new(),
            query: String::new(),
            kind_filter: KindFilter::All,
            year_filter: None,
            only_no_date: false,
            only_no_preview: false,
            previews_missing: HashSet::new(),
            progress: None,
            rx: None,
            tags: HashMap::new(),
            tag_defs: Vec::new(),
            tag_dict_dirty: false,
            tag_new_label: String::new(),
            tag_new_prompts: String::new(),
            tag_json_path: String::new(),
            retag_rx: None,
            tag_dists: HashMap::new(),
            user_tags: HashMap::new(),
            user_tag_input: String::new(),
            tag_filters: Vec::new(),
            semantic_query: String::new(),
            semantic_hits: None,
            ml_model: None,
            search_rx: None,
            embed_progress: None,
            embed_rx: None,
            settings: db::Settings::default(),
            models_dir_input: String::new(),
            download_consent: false,
            download_progress: None,
            download_rx: None,
            cal_year: 0,
            cal_month: 1,
            cal_day: None,
            face_clusters: Vec::new(),
            face_views: Vec::new(),
            face_textures: HashMap::new(),
            face_missing: HashSet::new(),
            face_selected_cluster: None,
            face_label_input: String::new(),
            face_images: 0,
            face_todo: 0,
            face_faces: 0,
            face_photos: 0,
            face_progress: None,
            face_rx: None,
            face_download_consent: false,
            face_download_progress: None,
            face_download_rx: None,
            hidden_tags: HashMap::new(),
            ocr_text: HashMap::new(),
            ocr_snippets: HashMap::new(),
            search_ocr: false,
            ocr_files: 0,
            ocr_chars: 0,
            ocr_candidates: 0,
            ocr_remaining: 0,
            ocr_progress: None,
            ocr_rx: None,
            tesseract: None,
            tools_input: tools::Tools::default(),
            tool_states: vec![String::new(); tools::ALL.len()],
            settings_open: false,
            settings_tab: SettingsTab::Interface,
            details_open: true,
            tile_size: 150.0,
            viewer: None,
            viewer_tex: None,
            viewer_faces: None,
            photo_cache: HashMap::new(),
            photo_loading: HashSet::new(),
            photo_jobs: VecDeque::new(),
            photo_rx: None,
            video: None,
            video_tex: None,
            video_child: None,
            video_tex_at: None,
            video_id: None,
            video_lag_shown: false,
            video_ended: false,
            cal_scale: CalScale::Month,
            cal_week: chrono::Local::now().date_naive(),
            face_picked: HashSet::new(),
            face_group_input: String::new(),
            face_picked_clusters: HashSet::new(),
            face_together: false,
            trash_confirm: false,
            trash_progress: None,
            trash_rx: None,
            roots: Vec::new(),
            root_counts: BTreeMap::new(),
            roots_open: false,
            picker_open: false,
            picker_current: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from)
                .or_else(|| std::env::current_dir().ok())
                .unwrap_or_else(|| PathBuf::from("/")),
            picker_entries: Vec::new(),
            picker_error: String::new(),
            storage_usage: None,
            storage_confirm: None,
            status: String::new(),
            theme_key: None,
            scroll: Scroll::default(),
        };
        me.reload();
        me
    }

    fn reload(&mut self) {
        let lang = self.settings.lang;
        self.textures.clear();
        self.missing.clear();
        self.photo_cache.clear();
        self.photo_jobs.clear();
        self.photo_loading.clear();
        self.viewer_tex = None;
        self.stop_video();
        self.face_textures.clear();
        self.face_missing.clear();
        self.semantic_hits = None;
        self.file_albums_for = None;
        self.ocr_text.clear();
        self.ocr_snippets.clear();
        match db::open(&self.db_path).and_then(|c| {
            let settings = db::load_settings(&c);
            let source = embed::tag_source(embed::active_entry(&settings));
            let rows = db::load_all(&c)?;
            let trashed = db::load_trashed(&c)?;
            let tags = db::load_tags(&c, &source)?;
            let user_tags = db::load_user_tags(&c)?;
            let hidden = db::load_hidden_counts(&c)?;
            let (ocr_files, ocr_chars) = db::ocr_summary(&c)?;
            let face_clusters = db::load_face_clusters(&c)?;
            let face_views = db::load_face_views(&c, None)?;
            let (face_faces, face_photos, _) = db::face_summary(&c)?;
            let (face_images, face_todo) = db::face_scan_plan(&c)?;
            Ok((
                settings,
                rows,
                trashed,
                tags,
                user_tags,
                hidden,
                ocr_files,
                ocr_chars,
                face_clusters,
                face_views,
                face_faces,
                face_photos,
                face_images,
                face_todo,
            ))
        }) {
            Ok((
                settings,
                rows,
                trashed,
                tags,
                user_tags,
                hidden,
                ocr_files,
                ocr_chars,
                face_clusters,
                face_views,
                face_faces,
                face_photos,
                face_images,
                face_todo,
            )) => {
                // The language comes from the settings that have just been read: the `lang` of
                // this frame is still the default one on the first load, and the first status line
                // stayed English in a Russian window (M22.12).
                let loaded_lang = settings.lang;
                self.status = i18n::fill(
                    loaded_lang,
                    "{} in the index, {}",
                    &[
                        i18n::count(loaded_lang, rows.len(), "file"),
                        db::human_in(loaded_lang, rows.iter().map(|r| r.size).sum()),
                    ],
                );
                self.settings = settings;
                self.tag_defs = embed::ensure_tag_defs(
                    &self.db_path,
                    embed::active_entry(&self.settings),
                )
                .unwrap_or_default();
                self.tools_input = self.settings.tools.clone();
                // Check tesseract and the other programs by the paths from the settings — once.
                self.tesseract = self.settings.tools.probe(Tool::Tesseract);
                self.refresh_tool_states();
                self.models_dir_input = self.settings.models_dir.to_string_lossy().to_string();
                self.max_dist = self.settings.max_dist;
                self.tile_size = self.settings.tile_size.clamp(100.0, 260.0);
                self.roots = self.settings.roots.clone();
                if self.path_input.trim().is_empty()
                    && let Some(first) = self.roots.first()
                {
                    self.path_input = first.clone();
                }
                // The "Integrity" screen knows the archive and the place for the manifest by default:
                // one press instead of three fields typed by hand. If the owner has already configured
                // a reminder (M10) — take exactly its folder and manifest.
                if !self.settings.verify_root.trim().is_empty() {
                    self.integrity_a = self.settings.verify_root.clone();
                }
                if self.integrity_a.trim().is_empty()
                    && let Some(first) = self.roots.first()
                {
                    self.integrity_a = first.clone();
                }
                if !self.settings.verify_manifest.trim().is_empty() {
                    self.integrity_manifest = self.settings.verify_manifest.clone();
                }
                if self.integrity_manifest.trim().is_empty() {
                    self.integrity_manifest = db::cache_dir()
                        .join("manifest.sha256")
                        .to_string_lossy()
                        .into_owned();
                }
                // "Time to check" is recomputed on load and after every pass, not on every frame:
                // the term changes once a day.
                self.verify_due = db::verify_due(&self.settings, chrono::Utc::now());
                self.items = rows;
                self.refresh_missing_previews();
                self.trash_items = trashed;
                self.tag_dists = embed::tag_distributions(&tags);
                self.tags = tags;
                self.user_tags = user_tags;
                self.hidden_tags = hidden;
                self.ocr_files = ocr_files;
                self.ocr_chars = ocr_chars;
                self.face_clusters = face_clusters;
                self.face_views = face_views;
                self.face_faces = face_faces;
                self.face_photos = face_photos;
                self.face_images = face_images;
                self.face_todo = face_todo;
                if !self
                    .face_clusters
                    .iter()
                    .any(|c| Some(c.id) == self.face_selected_cluster)
                {
                    self.face_selected_cluster = None;
                    self.face_label_input.clear();
                }
                if let Ok(plan) = ocr::plan(&self.db_path, false) {
                    self.ocr_candidates = plan.candidates;
                    self.ocr_remaining = plan.todo;
                }
                self.groups.clear();
                self.init_calendar();
                // The selection lives in memory: after the index is re-read only the files that still
                // exist remain in it, and the comparison set is fresh.
                let alive: HashSet<i64> = self.items.iter().map(|r| r.id).collect();
                self.picked.retain(|id| alive.contains(id));
                self.pick_anchor = self.pick_anchor.filter(|id| alive.contains(id));
                if self.selected.is_some_and(|id| !alive.contains(&id)) {
                    self.selected = None;
                }
                self.pick_scope = self.pick_scope_now();
            }
            Err(e) => {
                self.status = i18n::fill(lang, "index read error: {}", &[e.to_string()]);
            }
        }
        self.refresh_albums();
        self.refresh_root_counts();
    }

    /// The files of each root for the folder list: one pass over the index, not one per frame.
    fn refresh_root_counts(&mut self) {
        self.root_counts = self
            .roots
            .iter()
            .map(|root| {
                let n = self
                    .items
                    .iter()
                    .filter(|f| Path::new(&f.path).starts_with(root))
                    .count();
                (root.clone(), n)
            })
            .collect();
    }

    /// The album tree and the files of the open node (M19). DB rows only.
    fn refresh_albums(&mut self) {
        let lang = self.settings.lang;
        // The album contents changed — the "Albums" row in the card is read anew.
        self.file_albums_for = None;
        let loaded = db::open(&self.db_path).and_then(|c| {
            let tree = db::albums_tree(&c)?;
            let items = match self.album_selected {
                Some(id) if tree.iter().any(|n| n.id == id) => Some(db::album_items(&c, id)?),
                _ => None,
            };
            Ok((tree, items))
        });
        match loaded {
            Ok((tree, items)) => {
                if self.album_expanded.is_empty() {
                    for node in tree.iter().filter(|n| n.parent_id == 0) {
                        self.album_expanded.insert(node.id);
                    }
                }
                self.albums = tree;
                match items {
                    Some(rows) => self.set_album_rows(rows),
                    None => {
                        self.album_selected = None;
                        self.album_files.clear();
                        self.album_set.clear();
                        self.album_trashed = 0;
                        if self.settings.grid_sort == db::GridSort::Album {
                            self.settings.grid_sort = db::GridSort::DateDesc;
                            self.save_settings();
                        }
                    }
                }
            }
            Err(e) => self.status = i18n::fill(lang, "albums not read: {}", &[e.to_string()]),
        }
    }

    fn set_album_rows(&mut self, rows: Vec<FileRow>) {
        self.album_trashed = rows.iter().filter(|r| r.trashed_at.is_some()).count();
        self.album_files = rows
            .into_iter()
            .filter(|r| r.trashed_at.is_none())
            .map(|r| r.id)
            .collect();
        self.album_set = self.album_files.iter().copied().collect();
    }

    /// Open an album: show its files in album order and apply that order.
    fn album_open(&mut self, id: i64) {
        let lang = self.settings.lang;
        self.album_selected = Some(id);
        self.album_browse_all = false;
        // The parents are expanded, otherwise the selected node is not visible in the tree.
        let mut at = id;
        for _ in 0..256 {
            let Some(node) = self.albums.iter().find(|n| n.id == at) else {
                break;
            };
            if node.parent_id == 0 {
                break;
            }
            self.album_expanded.insert(node.parent_id);
            at = node.parent_id;
        }
        self.settings.grid_sort = db::GridSort::Album;
        self.save_settings();
        match db::open(&self.db_path).and_then(|c| db::album_items(&c, id)) {
            Ok(rows) => {
                let files = rows.iter().filter(|r| r.trashed_at.is_none()).count();
                let trashed = rows.len() - files;
                self.set_album_rows(rows);
                let path = self
                    .albums
                    .iter()
                    .find(|n| n.id == id)
                    .map(|n| n.name.clone())
                    .unwrap_or_default();
                self.status = i18n::fill(
                        lang,
                        "album “{}”: {}{} — album order",
                        &[path.to_string(), i18n::count(lang, files, "file"), if trashed > 0 {
                        i18n::fill(lang, ", {} in the trash", &[trashed.to_string()])
                    } else {
                        String::new()
                    }.to_string()]);
            }
            Err(e) => self.status = i18n::fill(lang, "the album is not open: {}", &[e.to_string()]),
        }
    }

    /// Close the album: the tree stays, the grid dims, the order returns to the date.
    fn album_close(&mut self) {
        self.album_selected = None;
        self.album_browse_all = false;
        self.album_files.clear();
        self.album_set.clear();
        self.album_trashed = 0;
        if self.settings.grid_sort == db::GridSort::Album {
            self.settings.grid_sort = db::GridSort::DateDesc;
            self.save_settings();
        }
    }

    fn album_node(&self, id: i64) -> Option<&db::AlbumNode> {
        self.albums.iter().find(|n| n.id == id)
    }

    /// How many albums and file rows go away with the node: for the confirmation.
    fn album_subtree(&self, id: i64) -> Vec<&db::AlbumNode> {
        let mut out = Vec::new();
        let mut seen = vec![id];
        if let Some(node) = self.album_node(id) {
            out.push(node);
        }
        loop {
            let mut grown = false;
            for node in &self.albums {
                if seen.contains(&node.parent_id) && !seen.contains(&node.id) {
                    seen.push(node.id);
                    out.push(node);
                    grown = true;
                }
            }
            if !grown {
                break;
            }
        }
        out
    }

    fn album_open_dialog(&mut self, dialog: AlbumDialog) {
        let lang = self.settings.lang;
        let id = match self.album_selected {
            Some(id) => id,
            None if dialog == AlbumDialog::New => 0,
            None => {
                self.status = t(lang, "pick an album in the tree first").into();
                return;
            }
        };
        self.album_name_input = match dialog {
            AlbumDialog::Rename => self
                .album_node(id)
                .map(|n| n.name.clone())
                .unwrap_or_default(),
            _ => String::new(),
        };
        self.album_move_parent = match dialog {
            AlbumDialog::Move => self.album_node(id).map(|n| n.parent_id).unwrap_or(0),
            _ => 0,
        };
        if dialog == AlbumDialog::Delete {
            let subtree = self.album_subtree(id);
            let rows: i64 = subtree.iter().map(|n| n.files).sum();
            let names = subtree
                .iter()
                .map(|n| format!("«{}»", n.name))
                .collect::<Vec<_>>()
                .join(", ");
            // The names and the caption stay words; the counters are a mono row of their own —
            // a number inside a translated sentence cannot be put into the numeric family (M22.10).
            let facts = format!(
                "{} · {}",
                i18n::count(lang, subtree.len(), "album"),
                i18n::count(lang, rows as usize, "file")
            );
            self.album_delete_text = i18n::fill(lang, "Will be deleted: {}", &[names]);
            self.album_delete_facts = facts;
        }
        self.album_dialog = Some(dialog);
    }

    /// The confirmed action of the dialog. Every operation is DB rows, not files.
    fn album_commit_dialog(&mut self, dialog: AlbumDialog) {
        let lang = self.settings.lang;
        let selected = self.album_selected;
        let result: Result<String> = (|| {
            let conn = db::open(&self.db_path)?;
            match dialog {
                AlbumDialog::New | AlbumDialog::Sub => {
                    let parent = if dialog == AlbumDialog::New {
                        0
                    } else {
                        selected.unwrap_or(0)
                    };
                    let id = db::create_album(
                        &conn,
                        parent,
                        &self.album_name_input,
                        "",
                    )?;
                    self.album_selected = Some(id);
                    Ok(i18n::fill(lang, "album created: {}", &[db::album_path(&conn, id)?.to_string()]))
                }
                AlbumDialog::Rename => {
                    let id = selected.context(t(lang, "no album picked"))?;
                    db::rename_album(&conn, id, &self.album_name_input)?;
                    Ok(format!(
                        "album renamed: {}",
                        db::album_path(&conn, id)?
                    ))
                }
                AlbumDialog::Move => {
                    let id = selected.context(t(lang, "no album picked"))?;
                    db::move_album(&conn, id, self.album_move_parent, None)?;
                    Ok(format!(
                        "album moved: {}",
                        db::album_path(&conn, id)?
                    ))
                }
                AlbumDialog::Delete => {
                    let id = selected.context(t(lang, "no album picked"))?;
                    let albums = db::delete_album(&conn, id)?;
                    Ok(i18n::fill(
                        lang,
                        "{} albums deleted; the files on disk are untouched",
                        &[albums.to_string()],
                    ))
                }
            }
        })();
        self.album_dialog = None;
        match result {
            Ok(msg) => {
                if matches!(dialog, AlbumDialog::Delete) {
                    self.album_selected = None;
                }
                self.refresh_albums();
                if let Some(id) = self.album_selected
                    && matches!(dialog, AlbumDialog::New | AlbumDialog::Sub | AlbumDialog::Rename | AlbumDialog::Move)
                {
                    self.album_open(id);
                }
                self.status = msg;
            }
            Err(e) => self.status = i18n::fill(lang, "the album is not changed: {}", &[e.to_string()]),
        }
    }

    /// The album export dialog (M20). The plan is built by a button, the copying happens in
    /// a background thread; nothing is written into the archive folder or past the copies in `--out`.
    fn export_dialog_open(&mut self, album_id: i64) {
        self.export_open = true;
        self.export_album = Some(album_id);
        self.export_error.clear();
        self.export_plan = None;
        self.export_plan_for = None;
        self.export_report = None;
        if self.export_name.trim().is_empty() {
            self.export_name = organize::EXPORT_TEMPLATE.to_string();
        }
    }

    /// Build the plan from the current fields. DB reads and a `--out` check only.
    fn export_build(&self) -> Result<organize::ExportPlan> {
        let lang = self.settings.lang;
        let album = self.export_album.context(t(lang, "no album picked"))?;
        let out = self.export_out.trim();
        if out.is_empty() {
            anyhow::bail!(t(lang, "give the target folder — it must exist already"));
        }
        let out = PathBuf::from(out);
        organize::check_out_dir(&out, &self.settings.roots)?;
        let template = {
            let t = self.export_name.trim();
            if t.is_empty() {
                organize::EXPORT_TEMPLATE.to_string()
            } else {
                t.to_string()
            }
        };
        organize::validate_export_template(&template)?;
        let conn = db::open(&self.db_path)?;
        let data = db::album_export_data(&conn, album)?;
        let tags = if template.contains("{tag}") {
            db::load_visible_tags(&conn)?
        } else {
            HashMap::new()
        };
        let mut opts = organize::ExportOptions::new(&out);
        opts.name = template;
        opts.tags = tags;
        opts.tag_show = self.settings.tag_show;
        opts.tag_margin = self.settings.tag_margin;
        organize::plan_album(&organize::export_albums(data), &opts)
    }

    fn export_plan_key(&self) -> (i64, String, String) {
        (
            self.export_album.unwrap_or(0),
            self.export_out.trim().to_string(),
            self.export_name.trim().to_string(),
        )
    }

    fn export_show_plan(&mut self) {
        match self.export_build() {
            Ok(plan) => {
                self.export_plan = Some(plan);
                self.export_plan_for = Some(self.export_plan_key());
                self.export_error.clear();
            }
            Err(e) => {
                self.export_plan = None;
                self.export_plan_for = None;
                self.export_error = format!("{e:#}");
            }
        }
    }

    /// Start the copying: the plan is rebuilt if the fields changed after it.
    fn export_start(&mut self) {
        let lang = self.settings.lang;
        if self.export_rx.is_some() {
            return;
        }
        if self.export_plan.is_none() || self.export_plan_for.as_ref() != Some(&self.export_plan_key())
        {
            self.export_show_plan();
        }
        let Some(plan) = self.export_plan.clone() else {
            if self.export_error.is_empty() {
                self.export_error = t(lang, "build the plan first").into();
            }
            return;
        };
        let progress = Arc::new(Progress::default());
        let worker = progress.clone();
        let (tx, rx) = mpsc::channel();
        let lang = self.settings.lang;
        std::thread::spawn(move || {
            let report = organize::apply_export(&plan, lang, Some(&worker));
            let _ = tx.send(report);
        });
        self.export_progress = Some(progress);
        self.export_rx = Some(rx);
        self.export_report = None;
        self.export_error.clear();
        self.status = t(lang, "the album export runs in the background").into();
    }

    /// Check the fields of the "Integrity" screen and parse the paths. Read only: not a single
    /// file is created here — the manifest is written in the background by a button.
    fn integrity_plan(&self, job: IntegrityJob) -> Result<IntegrityPlan, String> {
        let lang = self.settings.lang;
        let dir = |s: &str, what: &str| -> Result<PathBuf, String> {
            let name = s.trim();
            if name.is_empty() {
                return Err(format!("{} {what}", t(lang, "please give")));
            }
            let p = PathBuf::from(name);
            if !p.is_dir() {
                return Err(format!("{what} {}: {name}", t(lang, "is not a folder")));
            }
            Ok(p)
        };
        let dir_a = t(lang, "folder A (archive)");
        let manifest_hint = t(lang, "manifest file");
        match job {
            IntegrityJob::Verify => {
                let a = dir(&self.integrity_a, dir_a)?;
                let b = dir(&self.integrity_b, t(lang, "folder B (copy)"))?;
                let ca = a.canonicalize().unwrap_or_else(|_| a.clone());
                let cb = b.canonicalize().unwrap_or_else(|_| b.clone());
                if ca == cb {
                    return Err(t(
                        lang,
                        "folder A and folder B are the same path; nothing to compare",
                    )
                    .into());
                }
                Ok(IntegrityPlan::Verify(a, b))
            }
            IntegrityJob::Manifest => {
                let a = dir(&self.integrity_a, dir_a)?;
                let m = self.integrity_manifest.trim();
                if m.is_empty() {
                    return Err(t(
                        lang,
                        "give the manifest file: the path it will be written to",
                    )
                    .into());
                }
                let out = PathBuf::from(m);
                if out.is_dir() {
                    return Err(format!(
                        "{manifest_hint} — {}: {m}",
                        t(lang, "that is a folder")
                    ));
                }
                if let Some(parent) = out.parent()
                    && !parent.as_os_str().is_empty()
                    && !parent.is_dir()
                {
                    return Err(format!(
                        "{}: {}",
                        t(lang, "the folder for the manifest does not exist"),
                        parent.display()
                    ));
                }
                Ok(IntegrityPlan::Manifest(a, out))
            }
            IntegrityJob::Check => {
                let a = dir(&self.integrity_a, dir_a)?;
                let m = self.integrity_manifest.trim();
                if m.is_empty() {
                    return Err(t(lang, "give the manifest file to check against").into());
                }
                let file = PathBuf::from(m);
                if !file.is_file() {
                    return Err(format!("{}: {m}", t(lang, "manifest file not found")));
                }
                Ok(IntegrityPlan::Check(a, file))
            }
        }
    }

    /// Start a background pass. An existing manifest is not overwritten without a second
    /// press: returns `true` if a confirmation was asked for instead of starting.
    fn integrity_start(&mut self, job: IntegrityJob) {
        let lang = self.settings.lang;
        if self.integrity_rx.is_some() {
            self.status = t(lang, "a pass is already running: wait for it or press “Stop”").into();
            return;
        }
        if job == IntegrityJob::Manifest && !self.integrity_overwrite {
            let path = Path::new(self.integrity_manifest.trim());
            if path.is_file() {
                self.integrity_overwrite = true;
                self.integrity_error = match lang {
                    i18n::Lang::En => format!(
                        "the manifest already exists: {}. Press “Overwrite the manifest” to replace \
                         it whole, or “Cancel”.",
                        path.display()
                    ),
                    i18n::Lang::Ru => i18n::fill(
                        lang,
                        "the manifest file exists already: {}. Press “Overwrite the manifest” to replace it whole, or “Cancel”.",
                        &[path.display().to_string()]
                    ),
                };
                self.status =
                    t(lang, "manifest not written: the overwrite needs a confirmation").into();
                return;
            }
        }
        let plan = match self.integrity_plan(job) {
            Ok(plan) => plan,
            Err(e) => {
                self.integrity_error = e.clone();
                self.status = format!("{}: {e}", job.title(lang));
                return;
            }
        };
        // The manifest and the folder are remembered in the settings before the start (M10): if the
        // pass fails, the reminder still knows what to check and where.
        let remembered = match &plan {
            IntegrityPlan::Manifest(root, out) => Some((root.clone(), out.clone())),
            IntegrityPlan::Check(root, file) => Some((root.clone(), file.clone())),
            IntegrityPlan::Verify(..) => None,
        };
        if let Some((root, manifest)) = remembered {
            self.settings.verify_root = root.to_string_lossy().into_owned();
            self.settings.verify_manifest = manifest.to_string_lossy().into_owned();
            if let Err(e) =
                db::open(&self.db_path).and_then(|c| db::save_settings(&c, &self.settings))
            {
                self.integrity_error =
                    format!("{}: {e}", t(lang, "reminder settings not saved"));
            }
            self.verify_due = db::verify_due(&self.settings, chrono::Utc::now());
        }
        let jobs = self.settings.index_jobs;
        let progress = Arc::new(Progress::default());
        let worker = progress.clone();
        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let res = match plan {
                IntegrityPlan::Verify(a, b) => verify::verify(&a, &b, jobs, lang, Some(worker)).map(
                    |mut report| {
                        report.lang = lang;
                        IntegrityOutcome::Verify { a, b, report }
                    },
                ),
                IntegrityPlan::Manifest(root, out) => {
                    verify::manifest(&root, Some(&out), jobs, lang, Some(worker)).map(|mut stats| {
                        stats.lang = lang;
                        IntegrityOutcome::Manifest { root, out, stats }
                    })
                }
                IntegrityPlan::Check(root, file) => {
                    verify::check_manifest(&root, &file, jobs, lang, Some(worker)).map(|mut report| {
                        report.lang = lang;
                        IntegrityOutcome::Check { root, file, report }
                    })
                }
            };
            let _ = tx.send((job, res));
        });
        self.integrity_progress = Some(progress);
        self.integrity_rx = Some(rx);
        self.integrity_result = None;
        self.integrity_error.clear();
        self.status = format!("{}: {}", job.title(lang), t(lang, "running in the background"));
    }

    /// The outcome of a finished pass for the reminder (M10): the date, "ok" and the full report.
    /// Written only for a manifest and a check — comparing two copies is not tied to a term;
    /// a cancel does not count as a pass, and the previous successful report stays intact.
    /// The report goes to `<cache>/photocat/verify-last.txt` (nothing is written into the archive).
    fn note_integrity(&mut self, outcome: &IntegrityOutcome) -> Option<String> {
        if outcome.job() == IntegrityJob::Verify || outcome.cancelled() {
            return None;
        }
        let now = chrono::Utc::now();
        let report = outcome.report(true);
        self.settings.verify_root = self.integrity_a.trim().to_string();
        self.settings.verify_manifest = self.integrity_manifest.trim().to_string();
        self.settings.verify_last_at = now.to_rfc3339();
        self.settings.verify_last_ok = outcome.identical();
        self.settings.verify_last_report = report.clone();
        self.verify_due = db::verify_due(&self.settings, now);
        let mut warn = None;
        if let Err(e) = db::open(&self.db_path).and_then(|c| db::save_settings(&c, &self.settings))
        {
            warn = Some(format!(
                "{}: {e}",
                t(self.settings.lang, "the outcome was not saved in the settings")
            ));
        }
        let path = db::cache_dir().join("verify-last.txt");
        if let Err(e) = std::fs::write(&path, &report) {
            warn = Some(format!(
                "{} {}: {e}",
                t(self.settings.lang, "report not written to"),
                path.display()
            ));
        }
        warn
    }

    /// The "Integrity" screen: the fields, three buttons, progress in the top bar and the report.
    /// Drawn before the grid: it needs neither the tiles nor the preview cache.
    fn integrity_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.add_space(6.0);
        ui.label(egui::RichText::new(t(lang, "Integrity")).size(18.0));
        ui.label(
            num(ui, t(lang, "comparing copies and a sha256 manifest"), 12.0).color(dim(ui)),
        );
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(t(
                lang,
                "The comparison only reads: sizes first, sha256 only for files of the same size. The manifest is a sha256sum file for future checks; it is written by a button only. Nothing is written into the archive folder.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        ui.add_space(8.0);
        let mut a_changed = false;
        let mut manifest_changed = false;
        egui::Grid::new("integrity-fields")
            .num_columns(2)
            .spacing([8.0, 6.0])
            .show(ui, |ui| {
                ui.label(format!("{}:", t(lang, "Folder A (archive)")));
                a_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.integrity_a)
                            .font(egui::TextStyle::Monospace)
                            .hint_text(t(lang, "/home/…/Photos from the phone"))
                            .desired_width(460.0),
                    )
                    .changed();
                ui.end_row();
                ui.label(format!("{}:", t(lang, "Folder B (copy)")));
                ui.add(
                    egui::TextEdit::singleline(&mut self.integrity_b)
                        .font(egui::TextStyle::Monospace)
                        .hint_text(t(lang, "/mnt/backup/Photos from the phone"))
                        .desired_width(460.0),
                );
                ui.end_row();
                ui.label(format!("{}:", t(lang, "Manifest file")));
                manifest_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.integrity_manifest)
                            .font(egui::TextStyle::Monospace)
                            .hint_text(t(lang, "<cache>/photocat/manifest.sha256"))
                            .desired_width(460.0),
                    )
                    .changed();
                ui.end_row();
            });
        if a_changed {
            // A new folder — a new report: the old verdict does not apply to it.
            self.integrity_result = None;
        }
        if manifest_changed {
            // The confirmation referred to the old path — reset it so as not to overwrite someone
            // else's file at the new one.
            self.integrity_overwrite = false;
            self.integrity_error.clear();
            self.integrity_result = None;
        }
        ui.add_space(4.0);
        ui.checkbox(&mut self.integrity_show_all, t(lang, "Show all paths"));
        ui.add_space(4.0);
        let running = self.integrity_rx.is_some();
        let manifest_label = t(
            lang,
            if self.integrity_overwrite {
                "Overwrite the manifest"
            } else {
                "Write the manifest"
            },
        );
        let mut verify_clicked = false;
        let mut manifest_clicked = false;
        let mut check_clicked = false;
        let mut cancel_overwrite = false;
        let mut copy = false;
        let mut copy_last = false;
        ui.horizontal(|ui| {
            let b = WordButton::primary(Icon::CompareArrows, t(lang, "Compare the copies"));
            if ui
                .add_enabled(!running, b)
                .on_hover_text(t(lang, "compare folder A and folder B byte by byte"))
                .clicked()
            {
                verify_clicked = true;
            }
            if ui
                .add_enabled(!running, WordButton::new(Icon::Description, manifest_label))
                .on_hover_text(t(lang, "sha256 of every file in folder A into the manifest"))
                .clicked()
            {
                manifest_clicked = true;
            }
            if ui
                .add_enabled(
                    !running,
                    WordButton::new(Icon::Verified, t(lang, "Check against the manifest")),
                )
                .on_hover_text(t(lang, "compare folder A with the saved manifest"))
                .clicked()
            {
                check_clicked = true;
            }
            if self.integrity_overwrite
                && word_button(ui, Icon::Close, t(lang, "Cancel")).clicked()
            {
                cancel_overwrite = true;
            }
        });
        if running {
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "the pass is running: progress and “Stop” are in the top bar",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
        }
        // The reminder (M10): the term lives in the settings, the check is started only by a button
        // or by the "Check" button at the banner. 0 days means deliberately off.
        ui.add_space(4.0);
        let mut interval_changed = false;
        ui.horizontal(|ui| {
            ui.label(t(lang, "Remind me to check every"));
            interval_changed = ui
                .add(
                    egui::DragValue::new(&mut self.settings.verify_interval_days)
                        .range(0..=3650)
                        .suffix(t(lang, " days")),
                )
                .on_hover_text(t(
                    lang,
                    "the term counts from the last complete pass over the archive",
                ))
                .changed();
            ui.label(
                egui::RichText::new(t(lang, "0 — do not remind"))
                    .size(11.0)
                    .color(dim(ui)),
            );
        });
        if interval_changed {
            let value = self.settings.verify_interval_days.to_string();
            if let Err(e) = db::open(&self.db_path)
                .and_then(|c| db::set_setting(&c, "verify_interval_days", &value))
            {
                self.integrity_error =
                    format!("{}: {e}", t(lang, "reminder interval not saved"));
            }
            self.verify_due = db::verify_due(&self.settings, chrono::Utc::now());
        }
        let last = if self.settings.verify_manifest.trim().is_empty() {
            t(lang, "no manifest set — the reminder stays quiet").to_string()
        } else if self.settings.verify_last_at.trim().is_empty() {
            t(lang, "no pass has happened yet").to_string()
        } else if let Some(stamp) = db::parse_stamp(&self.settings.verify_last_at) {
            let when = stamp
                .with_timezone(&chrono::Local)
                .format("%Y-%m-%d %H:%M")
                .to_string();
            let outcome = t(
                lang,
                if self.settings.verify_last_ok {
                    "matched"
                } else {
                    "there were differences"
                },
            );
            i18n::fill(lang, "the last pass: {} · {}", &[when.to_string(), outcome.to_string()])
        } else {
            t(lang, "the date of the last pass is unclear — we will remind on launch").to_string()
        };
        ui.label(egui::RichText::new(last).size(11.0).color(dim(ui)));
        let (next, urgent) = match self.verify_due {
            db::VerifyDue::NoManifest => {
                (t(lang, "reminder: no manifest set").to_string(), false)
            }
            db::VerifyDue::Disabled => {
                (t(lang, "reminder disabled (interval 0)").to_string(), false)
            }
            db::VerifyDue::NotYet { days_left } => (
                format!(
                    "{} {days_left} {}",
                    t(lang, "next check in"),
                    t(lang, "days")
                ),
                false,
            ),
            db::VerifyDue::Due { days_over: 0 } => {
                (t(lang, "time to check: overdue").to_string(), true)
            }
            db::VerifyDue::Due { days_over } => (
                format!(
                    "{} {days_over} {}",
                    t(lang, "time to check: overdue"),
                    t(lang, "days ago")
                ),
                true,
            ),
        };
        ui.label(
            egui::RichText::new(next)
                .size(11.0)
                .color(if urgent { accent(ui) } else { dim(ui) }),
        );
        if !self.settings.verify_last_report.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!(
                        "{}: {}",
                        t(lang, "report of the last pass"),
                        db::cache_dir().join("verify-last.txt").display()
                    ))
                    .size(11.0)
                    .color(dim(ui)),
                );
                if word_button(ui, Icon::ContentCopy, t(lang, "Copy the last report")).clicked() {
                    copy_last = true;
                }
            });
        }
        if !self.integrity_error.is_empty() {
            ui.label(
                egui::RichText::new(&self.integrity_error)
                    .size(11.0)
                    .color(ui.visuals().error_fg_color),
            );
        }
        let verdict = self.integrity_result.as_ref().map(|r| {
            (
                r.verdict(lang),
                r.identical(),
                r.cancelled(),
                r.report(self.integrity_show_all),
                r.job().title(lang),
            )
        });
        if let Some((verdict, ok, cancelled, report, job)) = verdict {
            ui.add_space(6.0);
            ui.separator();
            let color = if cancelled {
                dim(ui)
            } else if ok {
                accent(ui)
            } else {
                ui.visuals().error_fg_color
            };
            ui.label(
                egui::RichText::new(&verdict)
                    .size(14.0)
                    .color(color),
            );
            ui.horizontal(|ui| {
                if word_button(ui, Icon::ContentCopy, t(lang, "Copy the report")).clicked() {
                    copy = true;
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{job}: {}",
                        t(lang, "the same report the CLI prints; nothing was written into the archive")
                    ))
                    .size(11.0)
                    .color(dim(ui)),
                );
            });
            ui.add_space(4.0);
            let h = (ui.available_height() - 12.0).max(160.0);
            egui::ScrollArea::both()
                .id_salt("integrity-report")
                .max_height(h)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(egui::RichText::new(&report).monospace().size(11.0));
                });
        }
        if cancel_overwrite {
            self.integrity_overwrite = false;
            self.integrity_error.clear();
            self.status = t(lang, "manifest overwrite cancelled").into();
        }
        if copy {
            if let Some(r) = &self.integrity_result {
                ui.ctx().copy_text(r.report(self.integrity_show_all));
            }
            self.status = t(lang, "report copied to the clipboard").into();
        }
        if copy_last && !self.settings.verify_last_report.is_empty() {
            ui.ctx().copy_text(self.settings.verify_last_report.clone());
            self.status = t(lang, "the last pass report was copied to the clipboard").into();
        }
        if verify_clicked {
            self.integrity_start(IntegrityJob::Verify);
        }
        if manifest_clicked {
            self.integrity_start(IntegrityJob::Manifest);
        }
        if check_clicked {
            self.integrity_start(IntegrityJob::Check);
        }
    }

    /// The actions of the "Albums" panel. Errors go to the status bar, no data is lost.
    fn album_apply(&mut self, action: AlbumAction) {
        let lang = self.settings.lang;
        // Opening an album and going back to the root need no open album: the root of the albums is
        // the card grid, and a card there must open its album (M22.21).
        match action {
            AlbumAction::Select(id) => {
                self.album_open(id);
                return;
            }
            AlbumAction::Root => {
                self.album_close();
                return;
            }
            _ => {}
        }
        let Some(id) = self.album_selected else {
            return;
        };
        let res: Result<String> = (|| {
            let conn = db::open(&self.db_path)?;
            match action {
                AlbumAction::Select(_) | AlbumAction::Root => unreachable!(),
                AlbumAction::Up | AlbumAction::Down => {
                    let node = self.album_node(id).cloned().context(t(lang, "there is no album"))?;
                    let position = if action == AlbumAction::Up {
                        (node.position - 1).max(0)
                    } else {
                        node.position + 1
                    };
                    db::move_album(&conn, id, node.parent_id, Some(position))?;
                    Ok(i18n::fill(lang, "album moved: {}", &[db::album_path(&conn, id)?.to_string()]))
                }
                AlbumAction::AddPicked => {
                    let ids: Vec<i64> = self.picked.iter().copied().collect();
                    if ids.is_empty() {
                        return Ok(t(lang, "nothing is picked").into());
                    }
                    let (added, already) = db::add_to_album(&conn, id, &ids)?;
                    Ok(i18n::fill(
                        lang,
                        "{} added to the album, {} were there already; {} files now",
                        &[
                            added.to_string(),
                            already.to_string(),
                            db::album_items(&conn, id)?.len().to_string(),
                        ],
                    ))
                }
                AlbumAction::RemovePicked => {
                    let ids: Vec<i64> = self
                        .picked
                        .iter()
                        .copied()
                        .filter(|file| self.album_set.contains(file))
                        .collect();
                    if ids.is_empty() {
                        return Ok(t(lang, "there is nothing to remove from the album").into());
                    }
                    let removed = db::remove_from_album(&conn, id, &ids)?;
                    Ok(i18n::fill(
                        lang,
                        "{} removed from the album; {} left",
                        &[
                            removed.to_string(),
                            db::album_items(&conn, id)?.len().to_string(),
                        ],
                    ))
                }
                AlbumAction::FileUp | AlbumAction::FileDown => {
                    let Some(file) = self.selected else {
                        return Ok(t(lang, "pick a photo in the grid first").into());
                    };
                    let delta = if action == AlbumAction::FileUp { -1 } else { 1 };
                    let moved = db::move_in_album(&conn, id, file, delta)?;
                    Ok(if moved {
                        t(lang, "the album order is changed").into()
                    } else {
                        t(lang, "the file is at the edge already").into()
                    })
                }
            }
        })();
        match res {
            Ok(msg) => {
                if !msg.is_empty() {
                    self.status = msg;
                }
                self.refresh_albums();
            }
            Err(e) => self.status = i18n::fill(lang, "the album is not changed: {}", &[e.to_string()]),
        }
    }

    /// Open the "add photos to an album" dialog for a set of files (M22.20). The target starts at
    /// the open album when there is one: inside an album the owner adds photos to that album in
    /// most cases. An empty set does nothing.
    fn album_pick_open(&mut self, files: Vec<i64>) {
        if files.is_empty() {
            return;
        }
        let target = self.album_selected.unwrap_or(0);
        self.album_pick = Some(AlbumPick {
            files,
            target,
            new_name: String::new(),
            filter: String::new(),
        });
    }

    /// The confirmed add: rows in `album_files` only, the files on disk are untouched. A typed
    /// name makes a new root album and fills it in the same press, so "collect into a new album"
    /// is one step.
    fn album_pick_commit(&mut self) {
        let lang = self.settings.lang;
        let Some(pick) = self.album_pick.take() else {
            return;
        };
        let res: Result<String> = (|| {
            let conn = db::open(&self.db_path)?;
            let target = if pick.new_name.trim().is_empty() {
                pick.target
            } else {
                db::create_album(&conn, 0, &pick.new_name, "")?
            };
            if target == 0 {
                anyhow::bail!(t(lang, "pick an album or type a name for a new one"));
            }
            let (added, already) = db::add_to_album(&conn, target, &pick.files)?;
            let path = db::album_path(&conn, target)?;
            Ok(i18n::fill(
                lang,
                "{} added to “{}”, {} were there already",
                &[added.to_string(), path, already.to_string()],
            ))
        })();
        match res {
            Ok(msg) => self.status = msg,
            Err(e) => {
                self.status = i18n::fill(lang, "the album is not changed: {}", &[e.to_string()])
            }
        }
        self.file_albums_for = None;
        self.refresh_albums();
    }

    /// The dialog itself: the tree of albums, a filter, and the field for a new album. A press on
    /// the primary button adds the photos to the chosen album (or to the new one).
    fn album_pick_modal(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        let Some(pick) = self.album_pick.as_ref() else {
            return;
        };
        let mut target = pick.target;
        let mut new_name = pick.new_name.clone();
        let mut filter = pick.filter.clone();
        let mut confirm = false;
        let mut cancel = false;
        let albums = self.albums.clone();
        let things = i18n::count(lang, pick.files.len(), "shot");
        let modal = egui::Modal::new(egui::Id::unique("photocat-album-pick"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new(t(lang, "Add to an album")).size(14.0).strong());
            ui.label(num(ui, things.clone(), 11.0).color(dim(ui)));
            ui.add_space(6.0);
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "The photos go to the end of the album. The files on disk stay untouched.",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            ui.add_space(6.0);
            if albums.is_empty() {
                ui.label(t(
                    lang,
                    "There are no albums yet: type a name and the album is made with these photos.",
                ));
            } else {
                let field = ui.add(
                    egui::TextEdit::singleline(&mut filter)
                        .hint_text(t(lang, "Filter the albums"))
                        .desired_width(300.0),
                );
                field.on_hover_text(t(lang, "a part of the album name"));
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .id_salt("album-pick-list")
                    .max_height(220.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let needle = filter.trim().to_lowercase();
                        for node in &albums {
                            let path = album_label(&albums, node.id);
                            if !needle.is_empty() && !path.to_lowercase().contains(&needle) {
                                continue;
                            }
                            let picked = target == node.id;
                            let text = i18n::fill(
                                lang,
                                "{} · {}",
                                &[path, i18n::count(lang, node.files as usize, "file")],
                            );
                            let row = ui.selectable_label(picked, text);
                            if row.clicked() {
                                target = node.id;
                            }
                        }
                    });
            }
            ui.add_space(6.0);
            ui.label(t(lang, "or a new album:"));
            ui.add(
                egui::TextEdit::singleline(&mut new_name)
                    .hint_text(t(lang, "For printing, Summer 2024"))
                    .desired_width(360.0),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let label = i18n::fill(lang, "Add {}", std::slice::from_ref(&things));
                if ui
                    .add(WordButton::primary(Icon::Add, label))
                    .on_hover_text(t(
                        lang,
                        "add the photos to the chosen album or to a new one; a repeat does not duplicate rows",
                    ))
                    .clicked()
                {
                    confirm = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if resp.backdrop_response.clicked() {
            cancel = true;
        }
        if cancel {
            self.album_pick = None;
            return;
        }
        if let Some(pick) = self.album_pick.as_mut() {
            pick.target = target;
            pick.new_name = new_name;
            pick.filter = filter;
        }
        if confirm {
            self.album_pick_commit();
        }
    }

    /// Open the calendar on the month of the freshest file.
    fn init_calendar(&mut self) {
        if self.cal_year != 0 {
            return;
        }
        let date = self
            .items
            .iter()
            .filter_map(|r| r.taken_at.as_deref())
            .max();
        if let Some(date) = date {
            let y = date.get(0..4).and_then(|v| v.parse().ok()).unwrap_or(2024);
            let m = date.get(5..7).and_then(|v| v.parse().ok()).unwrap_or(1);
            self.cal_year = y;
            self.cal_month = m;
            if let Some(d) = parse_date(date) {
                self.cal_week = monday_of(d);
            }
        } else {
            self.cal_year = 2024;
            self.cal_month = 1;
            self.cal_week = monday_of(chrono::Local::now().date_naive());
        }
    }

    fn save_settings(&mut self) {
        let lang = self.settings.lang;
        let res = db::open(&self.db_path).and_then(|c| db::save_settings(&c, &self.settings));
        match res {
            Ok(()) => self.status = t(lang, "settings saved").into(),
            Err(e) => self.status = format!("{}: {e}", t(lang, "settings not saved")),
        }
    }

    /// The "Settings" dialog: rarely needed, but always one click away from the top bar.
    fn settings_modal(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        if !self.settings_open {
            return;
        }
        let mut close = false;
        let mut tab = self.settings_tab;
        // What every section holds right now — the mono value at the right edge of its row.
        let mut section_notes: std::collections::BTreeMap<SettingsTab, String> =
            std::collections::BTreeMap::new();
        section_notes.insert(
            SettingsTab::Interface,
            self.settings.lang.code().to_uppercase(),
        );
        let entry = embed::active_entry(&self.settings);
        section_notes.insert(
            SettingsTab::Model,
            format!("{} {}", entry.dim, t(lang, "dims")),
        );
        section_notes.insert(SettingsTab::Index, self.items.len().to_string());
        section_notes.insert(
            SettingsTab::Tags,
            self.tag_defs.iter().filter(|d| d.enabled).count().to_string(),
        );
        section_notes.insert(
            SettingsTab::Faces,
            format!("{:.2}", self.settings.face_threshold),
        );
        section_notes.insert(
            SettingsTab::Tools,
            format!(
                "{}/{}",
                self.tool_states.iter().filter(|s| !s.is_empty()).count(),
                tools::ALL.len()
            ),
        );
        let modal = egui::Modal::new(egui::Id::unique("photocat-settings"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(760.0, 620.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_size(egui::vec2(760.0, 620.0));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new(t(lang, "Settings")).size(14.0));
                    });
                    ui.add_space(8.0);
                    egui::Panel::bottom("settings-footer").show(ui, |ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(t(
                                    lang,
                                    "Changes are saved immediately; originals are never modified.",
                                ))
                                .size(11.0)
                                .color(dim(ui)),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(12.0);
                                    if word_button(ui, Icon::Close, t(lang, "Close")).clicked()
                                    {
                                        close = true;
                                    }
                                },
                            );
                        });
                        ui.add_space(4.0);
                    });
                    egui::Panel::left("settings-nav")
                        .exact_size(200.0)
                        .resizable(false)
                        .show(ui, |ui| {
                            ui.add_space(8.0);
                            caps_label(ui, t(lang, "Settings sections"));
                            ui.add_space(4.0);
                            for section in SettingsTab::ALL {
                                if settings_nav_item(
                                    ui,
                                    tab == section,
                                    section.icon(),
                                    section.label(lang),
                                    section_notes.get(&section).map(String::as_str),
                                ) {
                                    tab = section;
                                }
                            }
                        });
                    egui::ScrollArea::vertical()
                        .id_salt("settings-scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.add_space(12.0);
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.vertical(|ui| {
                                    ui.set_max_width(520.0);
                                    self.settings_section(ui, tab);
                                });
                            });
                        });
                },
            );
        });
        self.settings_tab = tab;
        if close
            || resp.backdrop_response.clicked()
            || ctx.input(|i| i.key_pressed(egui::Key::Escape))
        {
            self.settings_open = false;
        }
    }

    /// The list of folders: add a new one, remove from the index (not from disk), index them all.
    fn roots_modal(&mut self, ctx: &egui::Context) {
        if !self.roots_open {
            return;
        }
        let lang = self.settings.lang;
        let mut close = false;
        let mut add = false;
        let mut index_all = false;
        let mut remove: Option<String> = None;
        let roots = self.roots.clone();
        // How many indexed files lie in each root: the folder list says where the archive is.
        let counts: Vec<usize> = roots
            .iter()
            .map(|root| self.root_counts.get(root).copied().unwrap_or(0))
            .collect();
        let modal = egui::Modal::new(egui::Id::unique("photocat-roots"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new(t(lang, "Archive folders")).size(14.0));
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "You can index several folders. “Add a folder…” offers indexing straight away; “Remove from the index” deletes DB rows only — the files on disk are untouched.",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            ui.add_space(8.0);
            if roots.is_empty() {
                ui.label(t(lang, "No folders yet — add one with the button below."));
            }
            egui::ScrollArea::vertical()
                .id_salt("roots-list")
                .max_height(260.0)
                .show(ui, |ui| {
                    for (i, root) in roots.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(root);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .add(WordButton::small(
                                            Icon::Remove,
                                            t(lang, "Remove from the index"),
                                        ))
                                        .on_hover_text(t(lang, "files on disk are not deleted"))
                                        .clicked()
                                    {
                                        remove = Some(root.clone());
                                    }
                                    let n = counts.get(i).copied().unwrap_or(0);
                                    ui.label(
                                        num(ui, i18n::count(lang, n, "file"), 11.0)
                                            .color(dim(ui)),
                                    );
                                },
                            );
                        });
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if word_button(ui, Icon::CreateNewFolder, t(lang, "Add a folder…")).clicked() {
                    add = true;
                }
                let b = WordButton::primary(Icon::Sync, t(lang, "Index all"));
                if ui
                    .add_enabled(!roots.is_empty(), b)
                    .clicked()
                {
                    index_all = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Close")).clicked() {
                    close = true;
                }
            });
        });
        if resp.backdrop_response.clicked() {
            close = true;
        }
        if close {
            self.roots_open = false;
        }
        if add {
            self.roots_open = false;
            self.picker_open = true;
            self.refresh_picker();
        }
        if index_all {
            self.roots_open = false;
            self.start_index();
        }
        if let Some(root) = remove {
            match db::remove_root(&self.db_path, &root) {
                Ok(n) => {
                    self.roots.retain(|r| r != &root);
                    self.settings.roots = self.roots.clone();
                    self.save_settings();
                    self.reload();
                    self.status = format!(
                        "{}: {} {n}; {}",
                        t(lang, "folder removed from the index"),
                        t(lang, "rows deleted"),
                        t(lang, "the files on disk are intact")
                    );
                }
                Err(e) => {
                    self.status =
                        format!("{}: {e}", t(lang, "the folder was not removed from the index"))
                }
            }
        }
    }

    fn refresh_picker(&mut self) {
        self.picker_entries.clear();
        self.picker_error.clear();
        match std::fs::read_dir(&self.picker_current) {
            Ok(rd) => {
                for e in rd.flatten() {
                    let p = e.path();
                    if e.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                        self.picker_entries.push(p);
                    }
                }
                self.picker_entries.sort();
            }
            Err(e) => {
                self.picker_error = format!(
                    "{} {}: {e}",
                    t(self.settings.lang, "cannot read"),
                    self.picker_current.display()
                )
            }
        }
    }

    /// The built-in folder picker: no dependency on the system dialog and no typing a path by hand.
    fn picker_modal(&mut self, ctx: &egui::Context) {
        if !self.picker_open {
            return;
        }
        let lang = self.settings.lang;
        let mut close = false;
        let mut nav: Option<PathBuf> = None;
        let mut add = false;
        let mut add_and_index = false;
        let current = self.picker_current.clone();
        let entries = self.picker_entries.clone();
        let error = self.picker_error.clone();
        let modal = egui::Modal::new(egui::Id::unique("photocat-picker"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new(t(lang, "Add a folder")).size(14.0));
            ui.label(
                egui::RichText::new(current.display().to_string())
                    .size(11.0)
                    .color(dim(ui)),
            );
            if !error.is_empty() {
                ui.label(egui::RichText::new(error).size(12.0).color(dim(ui)));
            }
            ui.horizontal(|ui| {
                if let Some(parent) = current.parent()
                    && word_button(ui, Icon::ArrowUp, t(lang, "Up")).clicked()
                {
                    nav = Some(parent.to_path_buf());
                }
                let b = WordButton::primary(Icon::Add, t(lang, "Add and index"));
                if ui
                    .add(b)
                    .clicked()
                {
                    add = true;
                    add_and_index = true;
                }
                if word_button(ui, Icon::Add, t(lang, "Add without indexing")).clicked() {
                    add = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Cancel")).clicked() {
                    close = true;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("picker-list")
                .max_height(320.0)
                .show(ui, |ui| {
                    if entries.is_empty() {
                        ui.label(t(lang, "No subfolders."));
                    }
                    for p in &entries {
                        let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("?");
                        if word_button(ui, Icon::Folder, format!("{name}/")).clicked() {
                            nav = Some(p.clone());
                        }
                    }
                });
        });
        if resp.backdrop_response.clicked() {
            close = true;
        }
        if close {
            self.picker_open = false;
        }
        if let Some(p) = nav {
            self.picker_current = p;
            self.refresh_picker();
        }
        if add {
            let path = self.picker_current.to_string_lossy().to_string();
            if !self.roots.iter().any(|r| r == &path) {
                self.roots.push(path.clone());
                self.settings.roots = self.roots.clone();
                self.path_input = path.clone();
                self.refresh_root_counts();
                self.save_settings();
                self.status = format!("{}: {path}", t(lang, "folder added"));
            }
            self.picker_open = false;
            if add_and_index {
                self.start_index();
            }
        }
    }

    /// The confirmation of cleaning the cache or the index: the archive and the models are untouched.
    fn storage_modal(&mut self, ctx: &egui::Context) {
        let Some(action) = self.storage_confirm else {
            return;
        };
        let lang = self.settings.lang;
        let (title, text, button) = match action {
            StorageAction::Thumbs => (
                t(lang, "Clear the previews?"),
                t(
                    lang,
                    "Only the files in <cache>/photocat/thumbs are deleted. The grid shows “no preview” until the folder is indexed again. The originals are unchanged.",
                ),
                t(lang, "Clear the previews"),
            ),
            StorageAction::Faces => (
                t(lang, "Clear the face crops?"),
                t(
                    lang,
                    "The crops and the face rows in the DB are deleted; the groups disappear. “Find faces” creates them again. The photos and originals are unchanged.",
                ),
                t(lang, "Clear the faces"),
            ),
            StorageAction::Index => (
                t(lang, "Reset the index?"),
                t(
                    lang,
                    "The DB rows are deleted: files, tags, text, vectors and faces. The settings, the models and every file on disk stay. Indexing is needed again afterwards.",
                ),
                t(lang, "Reset the index"),
            ),
        };
        let mut confirmed = false;
        let mut cancel = false;
        let modal = egui::Modal::new(egui::Id::unique("photocat-storage"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new(title).size(14.0));
            ui.add_space(4.0);
            ui.label(text);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let icon = match action {
                    StorageAction::Index => Icon::Delete,
                    StorageAction::Thumbs | StorageAction::Faces => Icon::Cleaning,
                };
                let b = WordButton::primary(icon, button);
                if ui.add(b).clicked() {
                    confirmed = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if resp.backdrop_response.clicked() {
            cancel = true;
        }
        if cancel {
            self.storage_confirm = None;
        }
        if confirmed {
            self.storage_confirm = None;
            let res = match action {
                StorageAction::Thumbs => storage::clear_thumbs(&self.db_path),
                StorageAction::Faces => storage::clear_faces(&self.db_path),
                StorageAction::Index => {
                    storage::reset_index(&self.db_path).map(|()| storage::Cleared::default())
                }
            };
            match res {
                Ok(c) => {
                    self.storage_usage = None;
                    self.selected = None;
                    self.groups.clear();
                    self.reload();
                    self.status = match action {
                        StorageAction::Index => {
                            t(lang, "index reset; index a folder now").into()
                        }
                        _ => format!("{}: {c}", t(lang, "cache cleared")),
                    };
                }
                Err(e) => {
                    self.status = format!("{}: {e:#}", t(lang, "cleaning failed"))
                }
            }
        }
    }

    /// The album dialog (M19): the name, the parent choice or the delete confirmation.
    /// Not a single button here opens or changes files on disk.
    fn album_modal(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        let Some(dialog) = self.album_dialog else {
            return;
        };
        let mut name = self.album_name_input.clone();
        let mut parent = self.album_move_parent;
        let mut confirm = false;
        let mut cancel = false;
        let selected = self.album_selected;
        // Own descendants are unavailable in the parent list: a cycle is forbidden both in the DB and here.
        let banned: HashSet<i64> = selected
            .map(|id| self.album_subtree(id).iter().map(|n| n.id).collect())
            .unwrap_or_default();
        let parents: Vec<(i64, String)> = std::iter::once((0i64, t(lang, "Root").to_string()))
            .chain(
                self.albums
                    .iter()
                    .filter(|n| !banned.contains(&n.id))
                    .map(|n| (n.id, album_label(&self.albums, n.id))),
            )
            .collect();
        let delete_text = self.album_delete_text.clone();
        let delete_facts = self.album_delete_facts.clone();
        let modal = egui::Modal::new(egui::Id::unique("photocat-album"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.label(egui::RichText::new(dialog.title(lang)).size(14.0));
            ui.add_space(6.0);
            match dialog {
                AlbumDialog::Delete => {
                    ui.label(&delete_text);
                    ui.label(num(ui, &delete_facts, 11.0).color(dim(ui)));
                    ui.label(
                        egui::RichText::new(t(
                            lang,
                            "The album rows are removed; the files stay on disk.",
                        ))
                        .size(12.0),
                    );
                }
                AlbumDialog::Move => {
                    ui.label(
                        t(lang, "A new parent. Own sub-albums are unavailable in the list — otherwise there would be a cycle."),
                    );
                    let current = parents
                        .iter()
                        .find(|(id, _)| *id == parent)
                        .map(|(_, label)| label.as_str())
                        .unwrap_or(t(lang, "Root"));
                    egui::ComboBox::from_id_salt("album-parent")
                        .selected_text(current)
                        .width(360.0)
                        .show_ui(ui, |ui| {
                            for (id, label) in &parents {
                                ui.selectable_value(&mut parent, *id, label);
                            }
                        });
                }
                _ => {
                    ui.label(t(lang, "The album name: up to 80 characters, unique among siblings regardless of case."));
                    let field = ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text(t(lang, "Dacha 2024, May, For printing"))
                            .desired_width(360.0),
                    );
                    field.request_focus();
                }
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let label = match dialog {
                    AlbumDialog::New | AlbumDialog::Sub => t(lang, "Create"),
                    AlbumDialog::Rename => t(lang, "Save"),
                    AlbumDialog::Move => t(lang, "Move"),
                    AlbumDialog::Delete => t(lang, "Delete the album"),
                };
                let icon = match dialog {
                    AlbumDialog::New | AlbumDialog::Sub => Icon::CreateNewFolder,
                    AlbumDialog::Rename => Icon::Edit,
                    AlbumDialog::Move => Icon::MoveTo,
                    AlbumDialog::Delete => Icon::Delete,
                };
                let b = WordButton::primary(icon, label);
                if ui.add(b).clicked() {
                    confirm = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if resp.backdrop_response.clicked() {
            cancel = true;
        }
        self.album_name_input = name;
        self.album_move_parent = parent;
        if cancel {
            self.album_dialog = None;
        }
        if confirm {
            self.album_commit_dialog(dialog);
        }
    }

    /// The album export dialog (M20): the folder, the template, the plan tree and the report.
    /// Closing the window does not cancel a pass already started — "Stop" is in the top bar.
    fn export_modal(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        if !self.export_open {
            return;
        }
        let mut out = self.export_out.clone();
        let mut name = self.export_name.clone();
        let mut show_plan = false;
        let mut start = false;
        let mut close = false;
        let mut copy = false;
        let title = self
            .export_album
            .map(|id| album_label(&self.albums, id))
            .unwrap_or_else(|| t(lang, "no album picked").into());
        let plan = self.export_plan.clone();
        let report_text = self
            .export_report
            .as_ref()
            .map(|r| r.to_string())
            .unwrap_or_default();
        let errors: Vec<String> = self
            .export_report
            .as_ref()
            .map(|r| r.errors.clone())
            .unwrap_or_default();
        let running = self.export_rx.is_some();
        let progress_text = self.export_progress.as_ref().map(|p| {
            let (d, total) = (p.done(), p.total());
            if total > 0 {
                i18n::fill(lang, "export: {}/{}", &[d.to_string(), total.to_string()])
            } else {
                t(lang, "exporting…").to_string()
            }
        });
        let error = self.export_error.clone();
        let files = plan.as_ref().map(|p| p.files()).unwrap_or(0);
        let modal = egui::Modal::new(egui::Id::unique("photocat-export"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(640.0);
            ui.label(egui::RichText::new(t(lang, "Export the album to disk")).size(14.0));
            ui.label(i18n::fill(lang, "Album “{}”", std::slice::from_ref(&title)));
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "File copies: the structure and the order repeat the album. The originals and the archive folder do not change; nothing in the target is overwritten or deleted.",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            ui.add_space(6.0);
            egui::Grid::new("export-fields")
                .num_columns(2)
                .spacing([8.0, 6.0])
                .show(ui, |ui| {
                    ui.label(t(lang, "Folder:"));
                    ui.add(
                        egui::TextEdit::singleline(&mut out)
                            .hint_text(t(lang, "/mnt/disk/export — the folder exists already"))
                            .desired_width(440.0),
                    );
                    ui.end_row();
                    ui.label(t(lang, "File name:"));
                    ui.add(
                        egui::TextEdit::singleline(&mut name)
                            .hint_text(organize::EXPORT_TEMPLATE)
                            .desired_width(440.0),
                    );
                    ui.end_row();
                });
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "Placeholders: {n} {date} {time} {camera} {tag} {orig} {ext} {hash8}. {n} is the position in the album; sub-album folders carry no numbers.",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if word_button(ui, Icon::Preview, t(lang, "Show the plan")).clicked() {
                    show_plan = true;
                }
                if ui
                    .add_enabled(
                        !running,
                        WordButton::primary(
                            Icon::Download,
                            i18n::fill(lang, "Export {} files", &[files.to_string()]),
                        ),
                    )
                    .on_hover_text(t(lang, "copies, not links; a repeated pass duplicates nothing"))
                    .clicked()
                {
                    start = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Close")).clicked() {
                    close = true;
                }
            });
            if let Some(text) = &progress_text {
                ui.label(num(ui, text, 11.0).color(dim(ui)));
            }
            if !error.is_empty() {
                ui.label(
                    egui::RichText::new(&error)
                        .size(11.0)
                        .color(ui.visuals().error_fg_color),
                );
            }
            if let Some(plan) = &plan {
                ui.add_space(6.0);
                ui.label(
                    egui::RichText::new(plan.summary(self.settings.lang))
                        .size(11.0)
                        .color(dim(ui)),
                );
                egui::ScrollArea::both()
                    .id_salt("export-plan")
                    .max_height(240.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(plan.outline(160))
                                .monospace()
                                .size(11.0),
                        );
                    });
            }
            if !report_text.is_empty() {
                ui.add_space(6.0);
                hairline(ui);
                ui.add_space(6.0);
                // A report is read line by line and copied as is: the numeric family, 11 px (M22.10).
                egui::ScrollArea::both()
                    .id_salt("export-report")
                    .max_height(160.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.label(num(ui, &report_text, 11.0));
                    });
                for line in errors.iter().take(5) {
                    ui.label(num(ui, line, 11.0).color(dim(ui)));
                }
                if word_button(ui, Icon::ContentCopy, t(lang, "Copy the report")).clicked() {
                    copy = true;
                }
            }
        });
        if resp.backdrop_response.clicked() {
            close = true;
        }
        self.export_out = out;
        self.export_name = name;
        if show_plan {
            self.export_show_plan();
        }
        if start {
            self.export_start();
        }
        if copy {
            ctx.copy_text(report_text.clone());
            self.status = t(lang, "the export report is copied to the clipboard").into();
        }
        if close {
            self.export_open = false;
        }
    }

    /// The confirmation of emptying the trash: the only action that removes a file from disk.
    fn trash_modal(&mut self, ctx: &egui::Context) {
        if !self.trash_confirm {
            return;
        }
        let mut confirmed = false;
        let mut cancel = false;
        let n = self.trash_items.len();
        let bytes: i64 = self.trash_items.iter().map(|r| r.size).sum();
        let lang = self.settings.lang;
        let facts = format!("{} · {}", i18n::count(lang, n, "file"), db::human_in(lang, bytes));
        let modal = egui::Modal::new(egui::Id::unique("photocat-trash"))
            .frame(dialog_frame(ctx));
        let resp = modal.show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new(t(lang, "Empty the trash?")).size(14.0));
            ui.add_space(4.0);
            ui.label(num(ui, &facts, 11.0).color(dim(ui)));
            ui.label(t(lang, "Will be moved to the OS trash."));
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "Nothing is deleted for good inside the app: the files stay in the OS trash until you empty it yourself. The originals will disappear from their folders in the archive — the app cannot undo that.",
                ))
                .size(12.0),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                // The one path that touches the disk wears the danger border, not the accent (M22.9).
                let b = WordButton::danger(Icon::Delete, t(lang, "Move to the OS trash"));
                if ui
                    .add(b)
                    .clicked()
                {
                    confirmed = true;
                }
                if word_button(ui, Icon::Close, t(lang, "Cancel")).clicked() {
                    cancel = true;
                }
            });
        });
        if resp.backdrop_response.clicked() {
            cancel = true;
        }
        if cancel {
            self.trash_confirm = false;
        }
        if confirmed {
            self.trash_confirm = false;
            self.start_trash();
        }
    }

    fn start_trash(&mut self) {
        if self.trash_rx.is_some() {
            return;
        }
        let items: Vec<(i64, String, i64)> = self
            .trash_items
            .iter()
            .map(|r| (r.id, r.path.clone(), r.size))
            .collect();
        if items.is_empty() {
            return;
        }
        let db_path = self.db_path.clone();
        let progress = Arc::new(Progress::default());
        self.trash_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.trash_rx = Some(rx);
        let lang = self.settings.lang;
        self.status = i18n::fill(lang, "trash: moving {} files to the OS trash…", &[items.len().to_string()]);
        std::thread::spawn(move || {
            let r = recycle::empty(&db_path, &items, lang, Some(progress));
            let _ = tx.send(r);
        });
    }

    fn settings_section(&mut self, ui: &mut egui::Ui, tab: SettingsTab) {
        let lang = self.settings.lang;
        ui.label(egui::RichText::new(tab.label(lang)).size(14.0));
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(8.0);
        match tab {
            SettingsTab::Interface => self.settings_interface(ui),
            SettingsTab::Model => self.settings_model(ui),
            SettingsTab::Index => self.settings_index(ui),
            SettingsTab::Tags => self.settings_tags(ui),
            SettingsTab::Faces => self.settings_faces(ui),
            SettingsTab::Storage => self.settings_storage(ui),
            SettingsTab::Tools => self.settings_tools(ui),
        }
    }

    /// The "Interface" section: the only place where the language is chosen (M21, ADR 0040).
    /// The choice applies at once and is saved in `settings.lang`, without an "Apply" button:
    /// to change one's mind it is enough to pick another language in the same list.
    fn settings_interface(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(
            egui::RichText::new(t(lang, "Interface language"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let mut chosen = self.settings.lang;
        egui::ComboBox::from_id_salt("lang_pick")
            .width(240.0)
            .selected_text(chosen.label())
            .show_ui(ui, |ui| {
                for l in i18n::Lang::ALL {
                    ui.selectable_value(&mut chosen, l, l.label());
                }
            });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(t(
                lang,
                "The choice is saved immediately. English is the project's main language, Russian stays second; screens not translated yet fall back to Russian.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        if chosen != self.settings.lang {
            self.settings.lang = chosen;
            self.save_settings();
        }
        // The theme (M22): the two palettes of the mock-up and following the OS theme. The choice is visible in the same frame.
        ui.add_space(12.0);
        hairline(ui);
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t(lang, "Interface theme"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let mut mode = self.settings.theme;
        ui.horizontal(|ui| {
            for (value, label) in [
                (db::ThemeMode::Dark, t(lang, "Dark")),
                (db::ThemeMode::Light, t(lang, "Light")),
                (db::ThemeMode::System, t(lang, "Follow the system")),
            ] {
                ui.selectable_value(&mut mode, value, label);
            }
        });
        ui.add_space(4.0);
        ui.label(
            egui::RichText::new(t(
                lang,
                "Dark is the main one: snapshots are looked at in the evening. Light is the same grid on white, for daytime work. The theme changes neither files nor the database.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        if mode != self.settings.theme {
            self.settings.theme = mode;
            self.save_settings();
            self.theme_key = None;
        }
    }

    fn settings_model(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let current = embed::active_entry(&self.settings);
        let current_id = current.id;
        let mut chosen = current_id;
        ui.label(egui::RichText::new(t(lang, "Model")).size(12.0).color(dim(ui)));
        ui.add_space(2.0);
        egui::ComboBox::from_id_salt("model_pick")
            .width(380.0)
            .selected_text(format!(
                "{} — {} {} · {}",
                t(lang, current.title),
                current.dim,
                t(lang, "dims"),
                db::human_in(lang, current.bytes() as i64)
            ))
            .show_ui(ui, |ui| {
                for e in embed::catalog() {
                    ui.selectable_value(
                        &mut chosen,
                        e.id,
                        format!(
                            "{} — {} {} · {} · {} · {}",
                            t(lang, e.title),
                            e.dim,
                            t(lang, "dims"),
                            db::human_in(lang, e.bytes() as i64),
                            e.license,
                            t(lang, e.note)
                        ),
                    );
                }
            });
        if chosen != current_id {
            self.settings.model_id = chosen.to_string();
            self.ml_model = None;
            self.semantic_hits = None;
            self.save_settings();
            self.reload();
        }
        ui.add_space(8.0);

        ui.label(
            egui::RichText::new(t(lang, "Model state"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let state = if embed::model_present(&self.settings) {
            format!(
                "{} · {}",
                t(lang, "Downloaded · verified"),
                db::human_in(
                    lang,
                    embed::model_bytes_in(&self.settings.models_dir, current)
                )
            )
        } else {
            t(lang, "Not downloaded").to_string()
        };
        ui.label(egui::RichText::new(state).size(12.0).color(
            if embed::model_present(&self.settings) {
                accent(ui)
            } else {
                dim(ui)
            },
        ));
        if word_button(ui, Icon::CheckCircle, t(lang, "Check the model")).clicked() {
            match embed::check_model(&self.settings) {
                Ok(()) => {
                    self.status = i18n::fill(lang, "the model {} is verified: the sha256 matches", &[t(lang, embed::active_entry(&self.settings).title).to_string()])
                }
                Err(e) => self.status = format!("{}: {e:#}", t(lang, "the model is not ready")),
            }
        }
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new(t(lang, "Model folder"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.models_dir_input)
                    .hint_text(t(lang, "where to download the weights"))
                    .desired_width(380.0),
            );
            if word_button(ui, Icon::Check, t(lang, "Apply the path")).clicked() {
                let p = self.models_dir_input.trim();
                if p.is_empty() {
                    self.status = t(lang, "the model path is empty").into();
                } else {
                    self.settings.models_dir = PathBuf::from(p);
                    self.save_settings();
                    self.reload();
                }
            }
        });
        ui.label(
            egui::RichText::new(self.settings.models_dir.display().to_string())
                .size(11.0)
                .color(dim(ui)),
        );
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new(t(lang, "Download limit"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::Slider::new(&mut self.settings.max_model_mb, 50..=2000)
                    .suffix(t(lang, " MiB"))
                    .step_by(10.0),
            );
            if r.drag_stopped() || r.lost_focus() {
                self.save_settings();
            }
        });
        ui.label(
            egui::RichText::new(t(
                lang,
                "A model over the limit is not downloaded. If the chosen one is unavailable, the downloader looks for a compatible one within the limit.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        ui.add_space(8.0);
        for e in embed::catalog() {
            let present = embed::model_present_in(&self.settings.models_dir, e);
            let fits = e.fits(self.settings.max_model_mb);
            let state = if present {
                t(lang, "in place")
            } else if fits {
                t(lang, "not downloaded")
            } else {
                t(lang, "over the limit")
            };
            ui.label(
                egui::RichText::new(format!(
                    "{} — {} {} · {} · {} · {}",
                    t(lang, e.title),
                    e.dim,
                    t(lang, "dims"),
                    db::human_in(lang, e.bytes() as i64),
                    e.license,
                    state
                ))
                .size(12.0),
            );
        }
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new(t(lang, "Network consent"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        ui.checkbox(
            &mut self.download_consent,
            t(
                lang,
                "I allow the model weights to be downloaded (photos, paths and queries are not sent)",
            ),
        );
        ui.label(
            egui::RichText::new(t(
                lang,
                "The network is used only when “Download the model” is pressed. No user data is sent.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        ui.add_space(8.0);
        let busy = self.download_rx.is_some();
        ui.horizontal(|ui| {
            let b = WordButton::primary(Icon::Download, t(lang, "Download the model"));
            if ui
                .add_enabled(self.download_consent && !busy, b)
                .clicked()
            {
                self.start_download();
            }
            if ui
                .add_enabled(
                    self.embed_rx.is_none() && embed::model_present(&self.settings),
                    WordButton::new(Icon::AutoAwesome, t(lang, "Compute tags")),
                )
                .clicked()
            {
                self.start_embed();
            }
        });
        if !self.download_consent {
            ui.label(
                egui::RichText::new(t(lang, "the button is disabled: network consent is required"))
                    .size(11.0)
                    .color(dim(ui)),
            );
        }
        if let Some(p) = &self.download_progress {
            let (done, total) = (p.done(), p.total());
            let frac = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(380.0)
                    .text(if total > 0 {
                        format!("{}: {done}/{total}", t(lang, "files:"))
                    } else {
                        p.msg()
                    }),
            );
            ui.label(egui::RichText::new(p.msg()).size(11.0).color(dim(ui)));
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t(
                lang,
                "Tags and vectors belong to the model: after switching models they must be computed again.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
    }

    fn settings_index(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(
            egui::RichText::new(t(lang, "Indexing threads"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let r = ui.add(
            egui::DragValue::new(&mut self.settings.index_jobs)
                .range(1..=64)
                .suffix(format!(" {}", t(lang, "threads"))),
        );
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(t(lang, "Duplicate threshold"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let r = ui.add(
            egui::Slider::new(&mut self.settings.max_dist, 0..=16)
                .text(t(lang, "bits out of 64")),
        );
        if r.drag_stopped() || r.lost_focus() {
            self.max_dist = self.settings.max_dist;
            self.save_settings();
        }
        ui.label(
            egui::RichText::new(t(
                lang,
                "More bits — more similar photos in a group. Exact copies are found at any threshold.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
    }

    fn settings_tags(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(
            egui::RichText::new(t(lang, "Tag filter threshold"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        self.tag_filter_slider(ui);
        let hits = self
            .tags
            .values()
            .filter(|scores| {
                scores
                    .iter()
                    .any(|(tag, score)| *score >= self.tag_threshold(tag))
            })
            .count();
        let above = if self.tags.is_empty() {
            t(lang, "tags are not computed yet").to_string()
        } else {
            match lang {
                i18n::Lang::En => format!(
                    "above the threshold: {hits} of {} files with tags; the percentile is \
                     computed per tag",
                    self.tags.len()
                ),
                i18n::Lang::Ru => i18n::fill(
                    lang,
                    "above the threshold: {} of {} files with tags; the percentile is counted per tag",
                    &[self.tags.len().to_string()]
                ),
            }
        };
        ui.label(egui::RichText::new(above).size(11.0).color(dim(ui)));
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(t(lang, "Confident tag threshold (cosine)"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let r = ui.add(
            egui::Slider::new(&mut self.settings.tag_show, 0.10..=0.50).step_by(0.01),
        );
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.label(
            egui::RichText::new(t(
                lang,
                "This is about the card, OCR and the {tag} in the showcase, not the filter: a tag is highlighted only when it is above the threshold and clearly ahead of the second one.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(t(lang, "Margin over the second tag"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let r = ui.add(egui::Slider::new(&mut self.settings.tag_margin, 0.0..=0.10));
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t(
                lang,
                "A lower threshold — more tags and more mistakes. “Remove” on a tag in the card hides it for one photo and survives a recompute.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        self.settings_tag_dictionary(ui);
    }

    /// The dictionary editor (M15.4): labels and prompts are stored in the DB per model.
    /// An edit does not recompute the photos: the image vectors do not depend on the prompts.
    fn settings_tag_dictionary(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t(lang, "Tag dictionary"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.label(
            egui::RichText::new(t(
                lang,
                "The label and the prompts live in the DB per model. Editing prompts does not re-read photos: the vectors are already stored, only the weights are recomputed.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        if self.tag_defs.is_empty() {
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "the dictionary is not seeded yet — it appears after the first “Compute tags”",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
        }
        let mut edited: Option<db::TagDef> = None;
        let mut removed: Option<String> = None;
        for d in self.tag_defs.iter_mut() {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui
                    .checkbox(&mut d.enabled, "")
                    .on_hover_text(t(
                        lang,
                        "a disabled tag is not computed and not shown in the filter",
                    ))
                    .changed()
                {
                    edited = Some(d.clone());
                }
                if ui
                    .add(egui::TextEdit::singleline(&mut d.label).desired_width(130.0))
                    .on_hover_text(t(
                        lang,
                        "the label in Russian: it is shown in the filter and in the card",
                    ))
                    .changed()
                {
                    edited = Some(d.clone());
                }
                let mut pct = d.min_percentile;
                if ui
                    .add(
                        egui::DragValue::new(&mut pct)
                            .range(0..=100)
                            .speed(1)
                            .suffix("%"),
                    )
                    .on_hover_text(t(lang, "the minimum percentile for showing this tag"))
                    .changed()
                {
                    d.min_percentile = pct;
                    edited = Some(d.clone());
                }
                if word_button(ui, Icon::Delete, t(lang, "Remove from the dictionary")).clicked()
                {
                    removed = Some(d.id.clone());
                }
            });
            let mut text = d.prompts.join("\n");
            if ui
                .add(
                    egui::TextEdit::multiline(&mut text)
                        .desired_rows(1)
                        .desired_width(f32::INFINITY)
                        .hint_text(t(
                            lang,
                            "prompts: one per line; 2–3 general templates",
                        )),
                )
                .changed()
            {
                d.prompts = text
                    .lines()
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect();
                edited = Some(d.clone());
            }
        }
        if let Some(d) = edited {
            self.save_tag_def_row(&d);
        }
        if let Some(id) = removed {
            self.delete_tag_def_row(&id);
        }
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_new_label)
                    .desired_width(150.0)
                    .hint_text(t(lang, "a new label, e.g. “Wardrobe”")),
            );
            ui.add(
                egui::TextEdit::singleline(&mut self.tag_new_prompts)
                    .desired_width(260.0)
                    .hint_text(t(lang, "prompt: “a photo of a wardrobe”")),
            );
        });
        let mut add = false;
        let mut retag = false;
        let mut export = false;
        let mut import = false;
        let mut reset = false;
        ui.horizontal(|ui| {
            add = ui
                .add_enabled(
                    !self.tag_new_label.trim().is_empty(),
                    WordButton::new(Icon::Add, t(lang, "Add to the dictionary")),
                )
                .on_hover_text(t(
                    lang,
                    "then “Recompute tags”: the weights are recomputed from the stored vectors",
                ))
                .clicked();
            let can = self.retag_rx.is_none() && embed::model_present(&self.settings);
            let title = if self.tag_dict_dirty {
                format!("{} •", t(lang, "Recompute tags"))
            } else {
                t(lang, "Recompute tags").to_string()
            };
            retag = ui
                .add_enabled(can, WordButton::new(Icon::Refresh, title))
                .on_hover_text(t(
                    lang,
                    "recompute the tag weights from the vectors already stored, without running CLIP over the photos again",
                ))
                .clicked();
            export = ui
                .add(WordButton::new(Icon::Download, t(lang, "Export JSON")))
                .on_hover_text(t(
                    lang,
                    "the dictionary without paths or photos — it can be moved to another machine",
                ))
                .clicked();
            import = ui
                .add(WordButton::new(Icon::Upload, t(lang, "Import JSON")))
                .on_hover_text(t(
                    lang,
                    "replace the model dictionary with a file; the old weights disappear after a recompute",
                ))
                .clicked();
            reset = ui
                .add(WordButton::new(Icon::Restore, t(lang, "Restore the defaults")))
                .on_hover_text(t(
                    lang,
                    "the 14 general tags from the code; your edits will be replaced",
                ))
                .clicked();
        });
        ui.add(
            egui::TextEdit::singleline(&mut self.tag_json_path)
                .desired_width(f32::INFINITY)
                .hint_text(t(
                    lang,
                    "the JSON file for export/import; empty — <cache>/photocat/tags.json",
                )),
        );
        if self.tag_dict_dirty {
            ui.label(
                egui::RichText::new(t(lang, "the dictionary has changed — press “Recompute tags”"))
                    .size(11.0)
                    .color(accent(ui)),
            );
        }
        if add {
            self.add_tag_def();
        }
        if retag {
            self.start_retag();
        }
        if export {
            self.export_tag_defs();
        }
        if import {
            self.import_tag_defs();
        }
        if reset {
            self.reset_tag_defs();
        }
    }

    fn save_tag_def_row(&mut self, d: &db::TagDef) {
        let lang = self.settings.lang;
        let r = db::open(&self.db_path).and_then(|c| db::save_tag_def(&c, d));
        match r {
            Ok(()) => {
                self.tag_dict_dirty = true;
                self.status = i18n::fill(lang, "dictionary: “{}” saved", std::slice::from_ref(&d.label));
            }
            Err(e) => {
                // The label may have collided with a taken one: show the dictionary from the DB, not our copy.
                if let Ok(c) = db::open(&self.db_path)
                    && let Ok(defs) =
                        db::load_tag_defs(&c, embed::active_entry(&self.settings).id)
                {
                    self.tag_defs = defs;
                }
                self.status = i18n::fill(lang, "the dictionary is not saved: {}", &[e.to_string()]);
            }
        }
    }

    fn add_tag_def(&mut self) {
        let lang = self.settings.lang;
        let label = self.tag_new_label.trim().to_string();
        if label.is_empty() {
            return;
        }
        if self.tag_defs.iter().any(|d| d.label == label) {
            self.status = i18n::fill(lang, "the tag “{}” is in the dictionary already", std::slice::from_ref(&label));
            return;
        }
        let n = self
            .tag_defs
            .iter()
            .filter_map(|d| d.id.strip_prefix("custom-")?.parse::<u32>().ok())
            .max()
            .unwrap_or(0)
            + 1;
        let prompts: Vec<String> = self
            .tag_new_prompts
            .lines()
            .map(str::trim)
            .filter(|p| !p.is_empty())
            .map(str::to_string)
            .collect();
        let d = db::TagDef {
            model: embed::active_entry(&self.settings).id.to_string(),
            id: format!("custom-{n}"),
            label,
            prompts,
            enabled: true,
            position: self.tag_defs.iter().map(|d| d.position).max().unwrap_or(0) + 1,
            min_percentile: 0,
        };
        let r = db::open(&self.db_path).and_then(|c| db::save_tag_def(&c, &d));
        match r {
            Ok(()) => {
                self.tag_new_label.clear();
                self.tag_new_prompts.clear();
                self.tag_defs.push(d.clone());
                self.tag_dict_dirty = true;
                self.status = i18n::fill(
                    lang,
                    "the tag “{}” is added — press “Recompute the tags”",
                    std::slice::from_ref(&d.label),
                );
            }
            Err(e) => self.status = format!("{}: {e}", t(lang, "tag not added")),
        }
    }

    fn delete_tag_def_row(&mut self, id: &str) {
        let lang = self.settings.lang;
        let model = embed::active_entry(&self.settings).id.to_string();
        let r = db::open(&self.db_path)
            .and_then(|c| db::delete_tag_def(&c, &model, id));
        match r {
            Ok(_) => {
                self.tag_defs.retain(|d| d.id != id);
                self.tag_dict_dirty = true;
                self.status = t(
                    lang,
                    "the tag is removed from the dictionary — its weights disappear after “Recompute tags”",
                )
                .into();
            }
            Err(e) => self.status = format!("{}: {e}", t(lang, "tag not removed")),
        }
    }

    fn reset_tag_defs(&mut self) {
        let lang = self.settings.lang;
        let entry = embed::active_entry(&self.settings);
        let r = embed::reset_to_defaults(&self.db_path, entry).map(|defs| {
            self.tag_defs = defs;
        });
        match r {
            Ok(()) => {
                self.tag_dict_dirty = true;
                self.status = t(
                    lang,
                    "the dictionary is back to the default — press “Recompute tags”",
                )
                .into();
            }
            Err(e) => self.status = format!("{}: {e}", t(lang, "dictionary not reset")),
        }
    }

    fn tag_json_file(&self) -> PathBuf {
        let p = self.tag_json_path.trim();
        if p.is_empty() {
            db::cache_dir().join("tags.json")
        } else {
            PathBuf::from(p)
        }
    }

    fn export_tag_defs(&mut self) {
        let lang = self.settings.lang;
        let path = self.tag_json_file();
        let json = embed::tag_defs_to_json(&self.tag_defs);
        match std::fs::write(&path, json) {
            Ok(()) => {
                self.tag_json_path = path.to_string_lossy().to_string();
                self.status = match lang {
                    i18n::Lang::En => format!(
                        "dictionary exported: {} ({})",
                        path.display(),
                        i18n::count(
                            lang,
                            self.tag_defs.len(),"tag")
                    ),
                    i18n::Lang::Ru => format!(
                        "dictionary exported: {} ({} tags)",
                        path.display(),
                        self.tag_defs.len()
                    ),
                };
            }
            Err(e) => self.status = format!("{}: {e}", t(lang, "dictionary not exported")),
        }
    }

    fn import_tag_defs(&mut self) {
        let lang = self.settings.lang;
        let path = self.tag_json_file();
        let model = embed::active_entry(&self.settings).id.to_string();
        let r = std::fs::read_to_string(&path)
            .map_err(|e| {
                anyhow::anyhow!(
                    "{} {}: {e}",
                    t(lang, "cannot read"),
                    path.display()
                )
            })
            .and_then(|text| embed::tag_defs_from_json(&model, &text))
            .and_then(|defs| {
                embed::import_defs(&self.db_path, &model, &defs)?;
                Ok(defs)
            });
        match r {
            Ok(defs) => {
                self.tag_json_path = path.to_string_lossy().to_string();
                self.tag_defs = defs;
                self.tag_dict_dirty = true;
                self.status = i18n::fill(lang, "the dictionary is loaded: {} tags — press “Recompute the tags”", &[self.tag_defs.len().to_string()]);
            }
            Err(e) => self.status = format!("{}: {e}", t(lang, "dictionary not loaded")),
        }
    }

    fn start_retag(&mut self) {
        if self.retag_rx.is_some() {
            return;
        }
        let lang = self.settings.lang;
        let db_path = self.db_path.clone();
        let progress = Arc::new(Progress::default());
        self.embed_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.retag_rx = Some(rx);
        self.status = t(lang, "CLIP: recomputing tags from the stored vectors…").into();
        std::thread::spawn(move || {
            let r = embed::retag(&db_path, 8, Some(progress));
            let _ = tx.send(r);
        });
    }

    fn settings_faces(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let present = faces::model_present(&self.settings);
        ui.label(
            egui::RichText::new(format!(
                "{} · {} · {}",
                faces::MODEL_ID,
                db::human_in(lang, faces::model_bytes(&self.settings)),
                if present {
                    t(lang, "in place")
                } else {
                    t(lang, "not downloaded")
                }
            ))
            .size(12.0)
            .color(if present { accent(ui) } else { dim(ui) }),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new(t(lang, "Face merge threshold (cosine)"))
                .size(12.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let r =
            ui.add(egui::Slider::new(&mut self.settings.face_threshold, 0.20..=0.70).step_by(0.01));
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.label(
            egui::RichText::new(t(
                lang,
                "A higher threshold — fewer false merges, but one person may split into groups. The SCRFD and ArcFace models are ONNX, local.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        ui.add_space(12.0);
        ui.checkbox(
            &mut self.face_download_consent,
            t(lang, "I allow the face weights to be downloaded (photos are not sent)"),
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            let b = WordButton::primary(Icon::Download, t(lang, "Download the face model"));
            if ui
                .add_enabled(
                    self.face_download_consent && self.face_download_rx.is_none(),
                    b,
                )
                .clicked()
            {
                self.start_face_download();
            }
            if word_button(ui, Icon::CheckCircle, t(lang, "Check the face model")).clicked() {
                match faces::check_model(&self.settings) {
                    Ok(()) => {
                        self.status =
                            t(lang, "the face model is checked: sha256 matched").into()
                    }
                    Err(e) => {
                        self.status = format!("{}: {e:#}", t(lang, "the face model is not ready"))
                    }
                }
            }
        });
        if !self.face_download_consent {
            ui.label(
                egui::RichText::new(t(lang, "the button is disabled: network consent is required"))
                    .size(11.0)
                    .color(dim(ui)),
            );
        }
        if let Some(p) = &self.face_download_progress {
            let (done, total) = (p.done(), p.total());
            let frac = if total > 0 {
                done as f32 / total as f32
            } else {
                0.0
            };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(380.0)
                    .text(if total > 0 {
                        format!("{}: {done}/{total}", t(lang, "face files:"))
                    } else {
                        p.msg()
                    }),
            );
            ui.label(egui::RichText::new(p.msg()).size(11.0).color(dim(ui)));
        }
    }

    fn settings_storage(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let bytes: i64 = self.items.iter().map(|r| r.size).sum();
        let trashed = self.trash_items.len();
        let index = i18n::fill(
            lang,
            "{}, {} · {} in the trash",
            &[
                i18n::count(lang, self.items.len(), "file"),
                db::human_in(lang, bytes),
                trashed.to_string(),
            ],
        );
        meta_row(ui, t(lang, "Index"), &index);
        meta_row(ui, t(lang, "DB"), &self.db_path.display().to_string());
        meta_row(ui, t(lang, "Cache"), &db::cache_dir().display().to_string());
        meta_row(
            ui,
            t(lang, "Models"),
            &format!(
                "{} · {}",
                self.settings.models_dir.display(),
                db::human_in(
                    lang,
                    embed::model_bytes_in(
                        &self.settings.models_dir,
                        embed::active_entry(&self.settings)
                    )
                )
            ),
        );
        meta_row(
            ui,
            t(lang, "People"),
            &format!(
                "{} · {}",
                faces::MODEL_ID,
                db::human_in(lang, faces::model_bytes(&self.settings))
            ),
        );
        ui.add_space(8.0);
        if word_button(ui, Icon::Calculate, t(lang, "Compute the used space")).clicked() {
            self.storage_usage = Some(storage::usage(&self.db_path, &self.settings.models_dir));
        }
        if let Some(u) = &self.storage_usage {
            stat_row(ui, t(lang, "Database:"), &db::human_in(lang, u.db_bytes as i64));
            stat_row(ui, t(lang, "Previews:"), &db::human_in(lang, u.thumbs_bytes as i64));
            stat_row(ui, t(lang, "Face crops:"), &db::human_in(lang, u.faces_bytes as i64));
            stat_row(ui, t(lang, "Models:"), &db::human_in(lang, u.models_bytes as i64));
            stat_row(ui, t(lang, "Whole cache:"), &db::human_in(lang, u.total_bytes as i64));
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t(lang, "Cleaning the cache and the index"))
                .size(11.0)
                .color(dim(ui)),
        );
        if ui
            .add(WordButton::new(Icon::Cleaning, t(lang, "Clear the previews…")))
            .on_hover_text(t(lang, "only <cache>/photocat/thumbs; the archive is untouched"))
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Thumbs);
        }
        if ui
            .add(WordButton::new(Icon::Cleaning, t(lang, "Clear the face crops…")))
            .on_hover_text(t(
                lang,
                "the crops and the face rows; after that “Find faces” runs again",
            ))
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Faces);
        }
        if ui
            .add(WordButton::new(Icon::Delete, t(lang, "Reset the index…")))
            .on_hover_text(t(
                lang,
                "DB rows: files, tags, OCR, vectors, faces; the originals are intact",
            ))
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Index);
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(t(
                lang,
                "photocat never writes into the archive folder: only DB rows and previews in the cache. The date showcase is a separate command with an explicit confirmation.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
    }

    fn settings_tools(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(
            egui::RichText::new(t(
                lang,
                "Empty — the program is looked up in PATH. A path is checked by running it when saved; without the program only its part is disabled.",
            ))
            .size(11.0)
            .color(dim(ui)),
        );
        ui.add_space(8.0);
        for (i, tool) in tools::ALL.into_iter().enumerate() {
            ui.label(egui::RichText::new(tool.name()).size(12.0).color(dim(ui)));
            ui.add_space(2.0);
            let input = self.tools_input.get_mut(tool);
            ui.add(
                egui::TextEdit::singleline(input)
                    .hint_text(t(lang, "empty — from PATH"))
                    .desired_width(380.0),
            );
            ui.label(
                egui::RichText::new(self.tool_states.get(i).map(String::as_str).unwrap_or(""))
                    .size(11.0)
                    .color(dim(ui)),
            );
            ui.label(
                egui::RichText::new(format!(
                    "{}: {}",
                    t(lang, "without it"),
                    tool.without(lang)
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            ui.add_space(8.0);
        }
        if ui
            .add(WordButton::new(
                Icon::CheckCircle,
                t(lang, "Check and save the programs"),
            ))
            .clicked()
        {
            self.apply_tools();
        }
    }

    fn reset_filters(&mut self) {
        let lang = self.settings.lang;
        self.query.clear();
        self.kind_filter = KindFilter::All;
        self.tag_filters.clear();
        self.year_filter = None;
        self.only_no_date = false;
        self.only_no_preview = false;
        self.semantic_hits = None;
        self.ocr_snippets.clear();
        self.status = t(lang, "the filters are reset").into();
    }

    /// The top bar of `photocat_1`: the mark on the left, one search field in the middle (the
    /// mode switch, the query, "Search", "×") and the tools on the right — the tile size, the
    /// state of the archive, the details toggle and "Settings". The folder and "Index" live in
    /// the left panel: they are work with the archive, not a search (M22.19).
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let th = theme(ui);
        let row = ui.max_rect();
        let row_h = TOP_H - 6.0;
        let gap = 8.0;

        // The three sectors are measured, not guessed: whatever the language, the search stays in
        // the middle and the tools keep to the right edge.
        let brand_w = 20.0 + 8.0 + text_width(ui, "photocat", 14.0);
        let settings_w = WordButton::new(Icon::Settings, t(lang, "Settings")).size(ui).0;
        let state = self.sync_word(lang);
        let status_w = 8.0 + 6.0 + 6.0 + text_width(ui, &state, 11.0) + 8.0;
        let zoom_w = 8.0 + 14.0 + 6.0 + TILE_SLIDER_W + 8.0;
        let with_state = row.width() >= 1100.0;
        // The details toggle has nothing to toggle on the "People" screen: its right panel always
        // holds the group list (M22.26).
        let details_toggle = self.view != View::Faces;
        let mut right_w = settings_w + zoom_w + gap;
        if details_toggle {
            right_w += 28.0 + gap;
        }
        if with_state {
            right_w += status_w + gap;
        }

        // Left: the mark.
        let left = egui::Rect::from_min_max(
            row.min,
            egui::pos2(row.left() + brand_w + 4.0, row.bottom()),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| {
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                icon_mark_color(ui, Icon::PhotoLibrary, 20.0, th.accent);
                ui.add_space(2.0);
                ui.label(egui::RichText::new("photocat").size(14.0).strong());
            });
        });

        // Right: the tools, from the right edge (Settings, details, state, tile size).
        let right = egui::Rect::from_min_max(
            egui::pos2(row.right() - right_w, row.top()),
            egui::pos2(row.right(), row.bottom()),
        );
        ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = gap;
                ui.set_min_height(row_h);
                if word_button(ui, Icon::Settings, t(lang, "Settings")).clicked() {
                    self.settings_open = true;
                }
                if details_toggle {
                    let hint = if self.details_open {
                        t(lang, "Hide details")
                    } else {
                        t(lang, "Details")
                    };
                    if toggle_icon_button(ui, Icon::DockToRight, 28.0, self.details_open, hint)
                        .clicked()
                    {
                        self.details_open = !self.details_open;
                    }
                }
                if with_state {
                    let facts = i18n::fill(
                        lang,
                        "{} files · {} with EXIF",
                        &[
                            self.items.len().to_string(),
                            self.items
                                .iter()
                                .filter(|r| r.taken_src == "exif")
                                .count()
                                .to_string(),
                        ],
                    );
                    let (rect, resp) =
                        ui.allocate_exact_size(egui::vec2(status_w, 28.0), egui::Sense::hover());
                    ui.painter().rect(
                        rect,
                        egui::CornerRadius::same(RADIUS_CTRL as u8),
                        th.card,
                        egui::Stroke::new(1.0, th.border),
                        egui::StrokeKind::Inside,
                    );
                    let dot = egui::pos2(rect.left() + 11.0, rect.center().y);
                    ui.painter().circle_filled(dot, 3.0, th.accent);
                    ui.painter().text(
                        egui::pos2(dot.x + 9.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        &state,
                        egui::FontId::proportional(11.0),
                        th.text_med,
                    );
                    resp.on_hover_text(facts);
                }
                self.tile_pill(ui);
            });
        });

        // The middle: one search field, `max-w-xl`, centred between the mark and the tools.
        let x0 = row.left() + brand_w + 16.0;
        let x1 = row.right() - right_w - 16.0;
        if x1 - x0 > 180.0 {
            let avail = x1 - x0;
            let w = avail.min(SEARCH_MAX_W);
            let x = x0 + (avail - w) * 0.5;
            let rect = egui::Rect::from_min_max(
                egui::pos2(x, row.top()),
                egui::pos2(x + w, row.bottom()),
            );
            ui.scope_builder(egui::UiBuilder::new().max_rect(rect), |ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(w, row.height()),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| self.search_field(ui, w),
                );
            });
        }
        self.pass_rows(ui);
        let r = ui.max_rect();
        ui.painter()
            .hline(r.x_range(), r.bottom() - 0.5, egui::Stroke::new(1.0, th.border));
    }

    /// One word for the state of the archive: a pass is running or everything is indexed. The
    /// numbers live in the hint of the pill, the dot says "working" without a sentence.
    fn sync_word(&self, lang: i18n::Lang) -> String {
        if self.busy() {
            t(lang, "Syncing").to_string()
        } else {
            t(lang, "Synced").to_string()
        }
    }

    /// Whether a background pass is running: any of them makes the top pill say "Syncing".
    fn busy(&self) -> bool {
        self.rx.is_some()
            || self.embed_rx.is_some()
            || self.ocr_rx.is_some()
            || self.face_rx.is_some()
            || self.trash_rx.is_some()
            || self.export_rx.is_some()
            || self.integrity_rx.is_some()
    }

    /// The search field of the top bar: the mock-up's one rounded well with the search glyph, the
    /// mode switch, the query, "Search" and "×", instead of a row of separate buttons.
    fn search_field(&mut self, ui: &mut egui::Ui, w: f32) {
        let lang = self.settings.lang;
        let th = theme(ui);
        let (mode_icon, mode_text) = if self.search_ocr {
            (Icon::TextFields, t(lang, "By text"))
        } else {
            (Icon::AutoAwesome, t(lang, "Semantic"))
        };
        let mode_w = WordButton::new(mode_icon, mode_text).compact().size(ui).0 + 12.0;
        let search_w = WordButton::primary(Icon::Search, t(lang, "Search"))
            .compact()
            .size(ui)
            .0;
        let inner = (w - 14.0).max(80.0);
        let field_w = (inner - 14.0 - 6.0 - mode_w - 6.0 - search_w - 6.0 - 20.0).max(40.0);
        let ready = if self.search_ocr {
            self.ocr_files > 0
        } else {
            embed::model_present(&self.settings)
        };
        let hint = if self.search_ocr {
            if self.ocr_files > 0 {
                t(lang, "receipt, invoice, 2024")
            } else {
                t(lang, "no recognized text yet")
            }
        } else if ready {
            t(lang, "cake, sea, furniture, document")
        } else {
            t(lang, "model not downloaded")
        };
        let mut text_mode = self.search_ocr;
        let mut submit = false;
        let mut clear = false;
        let mut enter = false;
        egui::Frame::new()
            .fill(th.canvas)
            .stroke(egui::Stroke::new(1.0, th.border))
            .corner_radius(RADIUS_CARD)
            .inner_margin(egui::Margin::symmetric(6, 5))
            .show(ui, |ui| {
                ui.set_width(inner);
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.spacing_mut().interact_size.y = 20.0;
                    icon_mark_color(ui, Icon::Search, 14.0, th.text_dim);
                    let mode = ui
                        .add(WordButton::new(mode_icon, mode_text).compact())
                        .on_hover_text(if text_mode {
                            t(lang, "search the recognized text")
                        } else {
                            t(lang, "search by the content of the photos")
                        });
                    egui::Popup::menu(&mode).show(|ui| {
                        if ui
                            .selectable_label(!text_mode, t(lang, "Semantic"))
                            .clicked()
                        {
                            text_mode = false;
                            ui.close();
                        }
                        if ui
                            .selectable_label(text_mode, t(lang, "By text"))
                            .clicked()
                        {
                            text_mode = true;
                            ui.close();
                        }
                    });
                    icon_mark_color(ui, Icon::ArrowDown, 10.0, th.text_dim);
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.semantic_query)
                            .hint_text(hint)
                            .desired_width(field_w),
                    );
                    enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui
                        .add_enabled(
                            ready,
                            WordButton::primary(Icon::Search, t(lang, "Search")).compact(),
                        )
                        .clicked()
                    {
                        submit = true;
                    }
                    if icon_button(ui, Icon::Close, 20.0, t(lang, "Clear the search")).clicked() {
                        clear = true;
                    }
                });
            });
        if text_mode != self.search_ocr {
            self.search_ocr = text_mode;
            self.semantic_hits = None;
            self.ocr_snippets.clear();
            self.status = if text_mode {
                t(lang, "search by text recognized on photos").into()
            } else {
                t(lang, "semantic search over photo content").into()
            };
        }
        if clear {
            self.semantic_query.clear();
            self.semantic_hits = None;
            self.ocr_snippets.clear();
            self.status = t(lang, "search cleared").into();
        }
        if submit || (enter && ready) {
            self.start_search();
        }
    }

    /// The compact "Tiles" pill of the top bar: the glyph and the slider, no number (the value is
    /// in the hint), so the control costs about 120 px of the strip (M22.19).
    fn tile_pill(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let th = theme(ui);
        let hint = t(lang, tile_hint(self.settings.cell_style));
        egui::Frame::new()
            .fill(th.card)
            .stroke(egui::Stroke::new(1.0, th.border))
            .corner_radius(egui::CornerRadius::same(RADIUS_CTRL as u8))
            .inner_margin(egui::Margin::symmetric(8, 4))
            .show(ui, |ui| {
                // The pill sits in a right-to-left tool row; the glyph and the slider of the
                // mock-up read left to right, and a 26 px slider would make the pill 36 px tall.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.spacing_mut().interact_size.y = 16.0;
                    ui.spacing_mut().slider_width = TILE_SLIDER_W;
                    icon_mark_color(ui, Icon::GridView, 14.0, th.text_dim);
                    let size = ui.add(
                        egui::Slider::new(&mut self.tile_size, 100.0..=260.0)
                            .step_by(10.0)
                            .show_value(false),
                    );
                    if size.drag_stopped() || size.lost_focus() {
                        self.settings.tile_size = self.tile_size;
                        self.save_settings();
                    }
                    size.on_hover_text(hint);
                });
            });
    }

    /// The passes of the archive are the only third row: it exists only while one is running.
    fn pass_rows(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        if let Some(p) = &self.progress {
            let running = self.rx.is_some();
            if progress_row(ui, t(lang, "Indexing"), p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.embed_progress {
            let running = self.embed_rx.is_some();
            if progress_row(ui, "CLIP", p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.ocr_progress {
            let running = self.ocr_rx.is_some();
            if progress_row(ui, "OCR", p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.face_progress {
            let running = self.face_rx.is_some();
            if progress_row(ui, t(lang, "People"), p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.trash_progress {
            let running = self.trash_rx.is_some();
            if progress_row(ui, t(lang, "Trash"), p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.export_progress {
            let running = self.export_rx.is_some();
            if progress_row(ui, t(lang, "Export"), p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.integrity_progress {
            let running = self.integrity_rx.is_some();
            if progress_row(ui, t(lang, "Integrity"), p, running) {
                p.cancel();
            }
        }
        self.verify_banner(ui);
    }

    /// The "Time to check the archive" banner (M10). It starts nothing by itself: "Check" is a
    /// press, after which the ordinary background check against the manifest runs.
    fn verify_banner(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        if self.verify_banner_hidden {
            return;
        }
        let db::VerifyDue::Due { days_over } = self.verify_due else {
            return;
        };
        let manifest = std::path::Path::new(&self.settings.verify_manifest)
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.settings.verify_manifest.clone());
        let last = db::parse_stamp(&self.settings.verify_last_at)
            .map(|t| t.with_timezone(&chrono::Local))
            .map(|t| t.format("%Y-%m-%d").to_string());
        let mut open = false;
        let mut later = false;
        ui.add_space(2.0);
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(t(lang, "Time to check the archive"))
                    .size(13.0)
                    .strong()
                    .color(accent(ui)),
            );
            let fact = match last {
                Some(date) if days_over > 0 => i18n::fill(
                    lang,
                    "manifest {} · the last pass {} · due {} days ago",
                    &[manifest.to_string(), date.to_string(), days_over.to_string()],
                ),
                Some(date) => i18n::fill(lang, "manifest {} · the last pass {} · the term is over", &[manifest.to_string(), date.to_string()]),
                None => i18n::fill(lang, "manifest {} · there has been no pass yet", std::slice::from_ref(&manifest)),
            };
            ui.label(egui::RichText::new(fact).size(11.0).color(dim(ui)));
            if ui
                .add(WordButton::new(Icon::Verified, t(lang, "Check now")))
                .on_hover_text(t(lang, "read the archive and compare it with the saved manifest"))
                .clicked()
            {
                open = true;
            }
            if ui
                .add(WordButton::new(Icon::Close, t(lang, "Remind me later")))
                .on_hover_text(t(lang, "hide the banner until the next launch"))
                .clicked()
            {
                later = true;
            }
        });
        if open {
            self.start_reminder_check();
        }
        if later {
            self.verify_banner_hidden = true;
            self.status = t(lang, "reminder postponed until the next launch").into();
        }
    }

    /// The "Check" button at the banner: the same background check against the manifest, only the
    /// fields are filled from the settings — the owner does not have to look for the "Integrity" screen.
    fn start_reminder_check(&mut self) {
        if !self.settings.verify_root.trim().is_empty() {
            self.integrity_a = self.settings.verify_root.clone();
        }
        self.integrity_manifest = self.settings.verify_manifest.clone();
        self.view = View::Integrity;
        self.integrity_start(IntegrityJob::Check);
    }

    /// The 28 px bottom bar: the dot and the last fact on the left, the counters of the index and
    /// the selected file (name · resolution · size) on the right — the mock-up's status line
    /// (M22.19). The numbers are monospaced, so a counter does not jostle its neighbours while
    /// indexing.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let th = theme(ui);
        let top = ui.max_rect();
        ui.painter()
            .hline(top.x_range(), top.top() + 0.5, egui::Stroke::new(1.0, th.border));
        ui.horizontal(|ui| {
            ui.set_min_height(STATUS_H - 6.0);
            let (dot, _) = ui.allocate_exact_size(egui::vec2(6.0, 6.0), egui::Sense::hover());
            ui.painter().circle_filled(dot.center(), 3.0, th.accent);
            ui.label(
                egui::RichText::new(if self.status.is_empty() {
                    t(lang, "Ready")
                } else {
                    &self.status
                })
                .size(11.0)
                .color(th.text_med),
            );
            ui.add_space(4.0);
            hairline_vertical(ui);
            ui.add_space(4.0);
            let total = i18n::count(lang, self.items.len(), "file");
            let bytes: i64 = self.items.iter().map(|r| r.size).sum();
            let facts = format!("{total} · {}", db::human_in(lang, bytes));
            ui.label(num(ui, facts, 10.0).color(th.text_dim));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(f) = self
                    .selected
                    .and_then(|id| self.items.iter().find(|r| r.id == id))
                {
                    // The selected file: the name, the frame, the size — the three facts the
                    // mock-up puts in its footer.
                    let resolution = if f.width > 0 && f.height > 0 {
                        format!(" · {}×{}", f.width, f.height)
                    } else {
                        String::new()
                    };
                    let text = format!(
                        "{}{} · {}",
                        file_name(&f.path),
                        resolution,
                        db::human_in(lang, f.size)
                    );
                    ui.label(num(ui, text, 10.0).color(th.text_med));
                } else if !self.picked.is_empty() {
                    let picked: i64 = self
                        .items
                        .iter()
                        .filter(|r| self.picked.contains(&r.id))
                        .map(|r| r.size)
                        .sum();
                    let text = i18n::fill(
                        lang,
                        "{} selected · {}",
                        &[
                            self.picked.len().to_string(),
                            db::human_in(lang, picked).to_string(),
                        ],
                    );
                    ui.label(num(ui, text, 10.0).color(th.text_dim));
                }
            });
        });
    }

    /// A snapshot of the filters for the row of chips: the chips are drawn without access to `self`,
    /// because at that moment the grid already holds `&mut self.textures`. The removal applies after the frame.
    fn chips_snapshot(&self) -> Chips {
        // The album chip is shown only in its own view: in "All files" the grid is not narrowed by the
        // album, although the selected node remains, to return to it with one click.
        let album = (self.view == View::Albums)
            .then(|| {
                self.album_selected.and_then(|id| {
                    self.albums
                        .iter()
                        .find(|n| n.id == id)
                        .map(|n| n.name.clone())
                })
            })
            .flatten();
        let lang = self.settings.lang;
        let year = self.year_filter.as_ref().map(|pick| match pick {
            YearPick::Year(y) => format!("{} {y}", t(lang, "year:")),
            YearPick::Older(bound) => {
                i18n::fill(lang, "older than {}", std::slice::from_ref(bound))
            }
        });
        Chips {
            query: self.query.trim().to_string(),
            tags: self.tag_filters.clone(),
            year,
            no_date: self.only_no_date,
            no_preview: self.only_no_preview,
            kind: self.kind_filter,
            lang: self.settings.lang,
            hits: self.semantic_hits.as_ref().map(|h| h.len()),
            album,
            picked: self.picked.len(),
            picked_bytes: self
                .items
                .iter()
                .filter(|r| self.picked.contains(&r.id))
                .map(|r| r.size)
                .sum(),
            // A selection exists only in the grid: the duplicates, faces and trash views have a choice of their own.
            pickable: self.view == View::Grid
                || (self.view == View::Albums && self.album_selected.is_some()),
        }
    }

    /// The left panel: the map of the application. The navigation and the filter are pinned, the view context scrolls.
    fn left_panel(&mut self, ui: &mut egui::Ui, show_card: bool, show_faces: bool) {
        let lang = self.settings.lang;
        // The folder and "Start indexing" are work with the archive, not a search: they live here, while
        // the top bar stays a single 32 px row (mock-up: working folder block).
        ui.add_space(2.0);
        // The mock-up's catalog block: the caps heading with a bare "+" on the right — the way
        // to several folders and to dropping one from the index (the worded path is the dialog).
        ui.horizontal(|ui| {
            caps_label(ui, t(lang, "Folder"));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if icon_button(
                    ui,
                    Icon::Add,
                    20.0,
                    t(lang, "add several folders, drop a folder from the index"),
                )
                .clicked()
                {
                    self.roots_open = true;
                }
            });
        });
        ui.add_space(2.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.path_input)
                .hint_text(t(lang, "path to a folder; several — with the “+” button"))
                .desired_width(f32::INFINITY),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(4.0);
        let index = WordButton::primary(Icon::Sync, t(lang, "Start indexing"));
        ui.horizontal(|ui| {
            if ui.add(index).clicked() || enter {
                self.start_index();
            }
            if word_button(ui, Icon::Refresh, t(lang, "Refresh")).clicked() {
                self.reload();
            }
        });
        ui.add_space(4.0);
        hairline(ui);

        // The map of the application — two groups with numbers on the right (PhotoPrism: Library / Tools).
        // The number explains the price of an item before a click: 103 groups, 617 faces, 3 in the trash.
        nav_section(ui, t(lang, "LIBRARY"));
        let was_albums = self.view == View::Albums;
        nav_item(
            ui,
            &mut self.view,
            View::Grid,
            Icon::PhotoLibrary,
            t(lang, "All files"),
            Some(self.items.len().to_string()),
        );
        nav_item(
            ui,
            &mut self.view,
            View::Albums,
            Icon::CollectionsBookmark,
            t(lang, "Albums"),
            Some(self.albums.len().to_string()),
        );
        nav_item(
            ui,
            &mut self.view,
            View::Calendar,
            Icon::CalendarMonth,
            t(lang, "Calendar"),
            None,
        );
        nav_item(
            ui,
            &mut self.view,
            View::Faces,
            Icon::Face,
            t(lang, "People"),
            Some(self.face_clusters.len().to_string()),
        );
        nav_section(ui, t(lang, "TOOLS"));
        nav_item(
            ui,
            &mut self.view,
            View::Dupes,
            Icon::BurstMode,
            t(lang, "Duplicates & series"),
            Some(self.groups.len().to_string()),
        );
        nav_item(
            ui,
            &mut self.view,
            View::Integrity,
            Icon::Verified,
            t(lang, "Integrity"),
            None,
        );
        nav_item(
            ui,
            &mut self.view,
            View::Trash,
            Icon::Delete,
            t(lang, "Trash"),
            Some(self.trash_items.len().to_string()),
        );
        // "Album order" lives only inside an open album: outside it the sort returns to the date,
        // so that the "All files" grid does not stand still silently.
        if self.view != View::Albums && self.settings.grid_sort == db::GridSort::Album {
            self.settings.grid_sort = db::GridSort::DateDesc;
            self.save_settings();
        }
        if !was_albums
            && self.view == View::Albums
            && let Some(id) = self.album_selected
        {
            self.album_open(id);
        }

        ui.add_space(8.0);
        hairline(ui);
        nav_section(ui, t(lang, "FILTER"));
        ui.label(
            egui::RichText::new(t(lang, "Search by path or date"))
                .size(11.0)
                .color(dim(ui)),
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text(t(lang, "2024-05, vacation"))
                .desired_width(f32::INFINITY),
        )
        .on_hover_text(t(lang, PATH_DATE_HINT));
        ui.add_space(4.0);
        // The format is one choice instead of a checkbox: everything, photos or video (PhotoPrism: Images/Video).
        ui.horizontal(|ui| {
            for (kind, icon) in [
                (KindFilter::All, Icon::Apps),
                (KindFilter::Images, Icon::Photo),
                (KindFilter::Videos, Icon::Videocam),
            ] {
                if ui
                    .add(
                        WordButton::picked(icon, kind.label(lang), self.kind_filter == kind)
                            .compact(),
                    )
                    .clicked()
                {
                    self.kind_filter = kind;
                }
            }
        });
        ui.add_space(4.0);
        // The two "missing" filters (M25.4): the numbers of the summary become filters. They take
        // the 20 px chip of a tag, narrow the grid together with the rest and with each other, and
        // are dead at zero — a filter that promises nothing does not invite a click.
        let no_date = self.items.iter().filter(|r| r.taken_at.is_none()).count();
        let no_preview = self
            .items
            .iter()
            .filter(|r| self.has_no_preview(r))
            .count();
        let preview_hint = t(
            lang,
            "the preview could not be built or is no longer in the cache: HEIC/AVIF (the decoder cannot read them), a broken file, a video without a cover (no ffmpeg or an ffmpeg error), or previews cleared in Settings → Storage. Index the folder again to rebuild them",
        );
        let mut date_click = false;
        let mut preview_click = false;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            let date_text = format!("{} · {no_date}", t(lang, "No date"));
            if toggle_chip(
                ui,
                &date_text,
                self.only_no_date,
                no_date > 0,
                t(lang, "only the files without a date"),
            ) {
                date_click = true;
            }
            let preview_text = format!("{} · {no_preview}", t(lang, "No preview"));
            if toggle_chip(
                ui,
                &preview_text,
                self.only_no_preview,
                no_preview > 0,
                preview_hint,
            ) {
                preview_click = true;
            }
        });
        if date_click {
            self.only_no_date = !self.only_no_date;
        }
        if preview_click {
            self.only_no_preview = !self.only_no_preview;
        }
        ui.add_space(4.0);

        // In a wide window the summary is pressed to the bottom of the panel. In a narrow one the card and
        // the summary go into a common scroll: otherwise the card is left with a 40 px strip.
        if !show_card {
            egui::Panel::bottom("left-stats")
                .resizable(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    hairline(ui);
                    self.stats_block(ui);
                });
        }
        egui::ScrollArea::vertical()
            .id_salt("left-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // The years are the fast road into the archive: the five newest with their counts,
                // a click narrows the date filter to one year (M22.3). They open the scrolling part
                // and are the first thing under the pinned filter, so they need no scroll of their
                // own; the model block follows a wheel notch lower, as before.
                // The date filter drives the grid and the calendar; other screens have a context of
                // their own (groups, faces, the trash), so the years are not drawn there.
                let years = self.year_counts();
                if matches!(self.view, View::Grid | View::Calendar) && !years.is_empty() {
                    nav_section(ui, t(lang, "YEARS"));
                    self.years_block(ui, &years);
                    hairline(ui);
                }
                match self.view {
                    View::Dupes => self.dupes_controls(ui),
                    View::Faces => self.faces_controls(ui, show_faces),
                    _ => {}
                }
                self.model_block(ui);
                self.ocr_block(ui);
                if show_card {
                    ui.add_space(8.0);
                    hairline(ui);
                    self.file_card(ui, self.selected);
                    ui.add_space(8.0);
                    hairline(ui);
                    self.stats_block(ui);
                }
            });
        panel_edge(ui, false);
    }

    /// The years of the archive as a wrapped cloud (M25.2): the five newest as 18 px chips and,
    /// when the index holds older years, a sixth "…" chip that filters exactly those older years.
    /// No number is drawn inside a chip — the count lives in the hint — and "All years" is the way
    /// back, drawn only while a pick is on, so the panel spends no room on an empty filter.
    fn years_block(&mut self, ui: &mut egui::Ui, years: &[(String, usize)]) {
        let lang = self.settings.lang;
        const CLOUD: usize = 5;
        let cloud = &years[..years.len().min(CLOUD)];
        let older = (years.len() > CLOUD).then(|| {
            let bound = years[CLOUD - 1].0.clone();
            let count: usize = years[CLOUD..].iter().map(|(_, n)| *n).sum();
            (bound, count)
        });
        let mut set: Option<Option<YearPick>> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            if self.year_filter.is_some()
                && year_chip(ui, t(lang, "All years"), false)
                    .on_hover_text(t(lang, "clear the year filter"))
                    .clicked()
            {
                set = Some(None);
            }
            for (year, count) in cloud {
                let pick = YearPick::Year(year.clone());
                let selected = self.year_filter.as_ref() == Some(&pick);
                let hint = format!("{year} · {}", i18n::count(lang, *count, "file"));
                if year_chip(ui, year, selected)
                    .on_hover_text(hint)
                    .clicked()
                {
                    // A second click on the picked year clears it: a toggle, as a chip behaves.
                    set = Some(if selected { None } else { Some(pick) });
                }
            }
            if let Some((bound, count)) = &older {
                let pick = YearPick::Older(bound.clone());
                let selected = self.year_filter.as_ref() == Some(&pick);
                let hint = i18n::fill(
                    lang,
                    "older than {} · {}",
                    &[bound.clone(), i18n::count(lang, *count, "file")],
                );
                let hint = format!("{hint} · {}", years_range(years, CLOUD));
                if year_chip(ui, "…", selected)
                    .on_hover_text(hint)
                    .clicked()
                {
                    set = Some(if selected { None } else { Some(pick) });
                }
            }
        });
        if let Some(pick) = set {
            self.set_year(pick);
        }
    }

    /// The years of the index with the number of files: the newest first, every year of the
    /// archive — the cloud takes the five newest and knows whether the rest exists (M25.2). A
    /// file without a date belongs to no year and is not counted.
    fn year_counts(&self) -> Vec<(String, usize)> {
        let mut map: BTreeMap<String, usize> = BTreeMap::new();
        for r in &self.items {
            let Some(year) = r.taken_at.as_deref().and_then(|d| d.get(0..4)) else {
                continue;
            };
            if year.len() == 4 && year.bytes().all(|b| b.is_ascii_digit()) {
                *map.entry(year.to_string()).or_default() += 1;
            }
        }
        let mut out: Vec<(String, usize)> = map.into_iter().collect();
        out.sort_by(|a, b| b.0.cmp(&a.0));
        out
    }

    /// A year or "the older years" clicked in the panel: the filter is its own (`year_filter`),
    /// so a typed path/date word survives the click (M25.2, M25.3). A semantic search and the
    /// text snippets are dropped — a year is a date, not words.
    fn set_year(&mut self, pick: Option<YearPick>) {
        self.year_filter = pick;
        self.semantic_hits = None;
        self.ocr_snippets.clear();
    }

    /// The context of the "Duplicates and series" view: the threshold, "Find groups", the summary and the plan export.
    fn dupes_controls(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        block(ui, |ui| {
            ui.label(
                egui::RichText::new(t(lang, "Duplicates & series"))
                    .size(11.0)
                    .color(dim(ui)),
            );
            ui.add_space(4.0);
            let threshold = i18n::fill(lang, "Similarity threshold: {} bits out of 64", &[self.max_dist.to_string()]);
            ui.label(egui::RichText::new(threshold));
            let resp = ui.add(egui::Slider::new(&mut self.max_dist, 0..=16));
            if resp.changed() {
                self.settings.max_dist = self.max_dist;
                self.save_settings();
            }
            let b = WordButton::primary(Icon::BurstMode, t(lang, "Find groups"));
            if ui
                .add(b)
                .clicked()
            {
                let t0 = std::time::Instant::now();
                self.groups = dupes::find_groups(&self.items, self.max_dist);
                let waste: i64 = self.groups.iter().map(|g| g.waste).sum();
                self.status = i18n::fill(lang, "{} groups, {} duplicates, in {} s", &[self.groups.len().to_string(), db::human_in(lang, waste).to_string(), format!("{:.1}", t0.elapsed().as_secs_f32())]);
            }
            let sum = dupes::summary(&self.items, &self.groups);
            let summary = i18n::fill(lang, "{} in the groups, {} marked spare, {} will be freed", &[sum.groups.to_string(), sum.marked_drop.to_string(), db::human_in(lang, sum.freed).to_string()]);
            ui.label(egui::RichText::new(summary).size(11.0).color(dim(ui)));
            let has_groups = !self.groups.is_empty();
            ui.add_space(4.0);
            if ui
                .add_enabled(has_groups, WordButton::new(Icon::Download, t(lang, "Export the plan")))
                .on_hover_text(t(
                    lang,
                    "write a CSV into the app cache: group, keep_path, drop_path, reason, wasted_bytes; a list to review, not a deletion",
                ))
                .clicked()
            {
                let path = db::cache_dir().join("plan.csv");
                let csv = dupes::plan_csv(&self.items, &self.groups, self.settings.lang);
                let rows = csv.lines().count().saturating_sub(1);
                match std::fs::write(&path, &csv) {
                    Ok(()) => {
                        self.status = {
                                i18n::fill(
                                    lang,
                                    "the plan is written: {} ({} rows)",
                                    &[path.display().to_string(), rows.to_string()],
                                )
                            }
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", t(lang, "plan not written"));
                    }
                }
            }
            if ui
                .add_enabled(has_groups, WordButton::new(Icon::ContentCopy, t(lang, "Copy the list")))
                .on_hover_text(t(lang, "the whole CSV plan to the clipboard"))
                .clicked()
            {
                ui.ctx()
                    .copy_text(dupes::plan_csv(&self.items, &self.groups, self.settings.lang));
                self.status = t(lang, "plan copied to the clipboard").into();
            }
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "“keep” picks the best frame of the group: it stays, the rest are marked spare. No file is deleted; the decision lives in the DB and lands in the plan.",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "“Export the plan” writes a CSV into the app cache — a report to review, not a deletion.",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            let no_sharp = self
                .items
                .iter()
                .filter(|r| r.kind == "image" && r.sharpness.is_none())
                .count();
            if no_sharp > 0 {
                let note = {
                        i18n::fill(
                            lang,
                            "the sharpness is not computed for {} photos — index the folder",
                            &[no_sharp.to_string()],
                        )
                    };
                ui.label(egui::RichText::new(note).size(11.0).color(dim(ui)));
            }
        });
    }

    /// The context of the "Faces" view: the threshold, the pass over the photos, the regrouping and
    /// the "all together" search. The list of groups is the right panel of the screen (M22.26);
    /// only a narrow window keeps it here.
    fn faces_controls(&mut self, ui: &mut egui::Ui, show_lists: bool) {
        let lang = self.settings.lang;
        block(ui, |ui| {
            ui.label(
                egui::RichText::new(t(lang, "People"))
                    .size(11.0)
                    .color(dim(ui)),
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!(
                "{} {:.2}",
                t(lang, "Merge threshold:"),
                self.settings.face_threshold
            )));
            let r = ui.add(
                egui::Slider::new(&mut self.settings.face_threshold, 0.20..=0.70).step_by(0.01),
            );
            if r.drag_stopped() || r.lost_focus() {
                self.save_settings();
            }
            let model = faces::model_present(&self.settings);
            if !model {
                ui.label(
                    egui::RichText::new(t(lang, "face model not downloaded"))
                        .size(11.0)
                        .color(dim(ui)),
                );
                if word_button(ui, Icon::Settings, t(lang, "Face settings")).clicked() {
                    self.settings_open = true;
                    self.settings_tab = SettingsTab::Faces;
                }
            }
            let found = i18n::fill(lang, "{} faces in {} photos, {} groups", &[self.face_faces.to_string(), self.face_photos.to_string(), self.face_clusters.len().to_string()]);
            ui.label(egui::RichText::new(found).size(11.0).color(dim(ui)));
            let todo = {
                    i18n::fill(lang, "{} of {} photos have no faces yet", &[self.face_todo.to_string(), self.face_images.to_string()])
                };
            ui.label(egui::RichText::new(todo).size(11.0).color(dim(ui)));
            ui.add_space(4.0);
            if self.face_rx.is_some() {
                ui.label(t(lang, "reclustering…"));
                if let Some(p) = &self.face_progress
                    && word_button(ui, Icon::Stop, t(lang, "Stop")).clicked()
                {
                    p.cancel();
                    self.status = t(lang, "people: stopping after the current photos…").into();
                }
            } else {
                let scan_label = if self.face_todo < self.face_images {
                    format!("{} ({})", t(lang, "Resume"), self.face_todo)
                } else {
                    format!("{} ({})", t(lang, "Find faces"), self.face_todo)
                };
                if ui
                    .add_enabled(
                        model && self.face_todo > 0,
                        WordButton::new(
                            if self.face_todo < self.face_images {
                                Icon::Play
                            } else {
                                Icon::Face
                            },
                            scan_label,
                        ),
                    )
                    .on_hover_text(t(
                        lang,
                        "a long pass: detection and embeddings run locally, no network; photos already scanned are skipped by sha256",
                    ))
                    .clicked()
                {
                    self.start_faces();
                }
                if ui
                    .add_enabled(
                        self.face_faces > 0,
                        WordButton::new(Icon::Refresh, t(lang, "Rebuild groups")),
                    )
                    .on_hover_text(t(
                        lang,
                        "the threshold applies to faces; manual names and splits are kept",
                    ))
                    .clicked()
                {
                    self.start_face_clusters();
                }
            }
            // The list of groups is the right panel of the screen; only a narrow window keeps it
            // here (M22.26).
            if show_lists {
                ui.add_space(8.0);
                ui.label(
                    egui::RichText::new(t(lang, "Groups"))
                        .size(11.0)
                        .color(dim(ui)),
                );
                egui::ScrollArea::vertical()
                    .id_salt("face-clusters")
                    .max_height(220.0)
                    .show(ui, |ui| self.face_list_rows(ui));
            }
            // "All together": the photos that hold at least one face from each marked group.
            if self.face_picked_clusters.len() >= 2 {
                let mut per_file: HashMap<i64, HashSet<i64>> = HashMap::new();
                for f in &self.face_views {
                    if let Some(cid) = f.cluster_id
                        && self.face_picked_clusters.contains(&cid)
                    {
                        per_file.entry(f.file_id).or_default().insert(cid);
                    }
                }
                let n = per_file
                    .values()
                    .filter(|set| self.face_picked_clusters.iter().all(|c| set.contains(c)))
                    .count();
                if ui
                    .add_enabled(
                        n > 0,
                        WordButton::new(
                            Icon::Face,
                            if self.face_together {
                                format!("{} ({n})", t(lang, "Show the photos (together)"))
                            } else {
                                format!("{} ({n})", t(lang, "Photos with everyone together"))
                            },
                        ),
                    )
                    .on_hover_text(t(
                        lang,
                        "show only photos that have a face from every marked group",
                    ))
                    .clicked()
                {
                    self.face_together = !self.face_together;
                }
            } else {
                self.face_together = false;
            }
        });
    }

    /// The rows of the group list: "All people", then every group with a tick for the "all
    /// together" search and the best face of the group as its icon (M22.26).
    fn face_list_rows(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let clusters = self.face_clusters.clone();
        // The faces are ordered by quality, so the first one of a group is its icon.
        let mut best: HashMap<i64, (i64, Option<String>)> = HashMap::new();
        for f in &self.face_views {
            if let Some(cid) = f.cluster_id {
                best.entry(cid).or_insert((f.id, f.thumb.clone()));
            }
        }
        if ui
            .selectable_label(
                self.face_selected_cluster.is_none(),
                format!("{} · {}", t(lang, "All people"), self.face_faces),
            )
            .clicked()
        {
            self.face_selected_cluster = None;
            self.face_label_input.clear();
        }
        for c in clusters {
            let label = if c.label.is_empty() {
                t(lang, "no name").to_string()
            } else {
                c.label.clone()
            };
            ui.horizontal(|ui| {
                let mut mark = self.face_picked_clusters.contains(&c.id);
                if ui
                    .checkbox(&mut mark, "")
                    .on_hover_text(t(lang, "mark the group for the “all together” search"))
                    .changed()
                {
                    if mark {
                        self.face_picked_clusters.insert(c.id);
                    } else {
                        self.face_picked_clusters.remove(&c.id);
                    }
                }
                let (face_id, thumb) = best
                    .get(&c.id)
                    .map(|(id, thumb)| (Some(*id), thumb.as_deref()))
                    .unwrap_or((None, None));
                self.face_avatar(ui, face_id, thumb, 20.0);
                let line = format!(
                    "{label} · {}",
                    i18n::count(lang, c.photos as usize, "photo")
                );
                if ui
                    .selectable_label(self.face_selected_cluster == Some(c.id), line)
                    .clicked()
                {
                    self.face_selected_cluster = Some(c.id);
                    self.face_label_input = c.label;
                }
            });
        }
    }

    /// The icon of a group row: its best face. The texture goes to the same cache the tiles use;
    /// a group without a crop keeps the face glyph (M22.26).
    fn face_avatar(
        &mut self,
        ui: &mut egui::Ui,
        face_id: Option<i64>,
        thumb: Option<&str>,
        size: f32,
    ) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
        let Some(face_id) = face_id else {
            paint_icon(ui.painter(), rect.center(), size * 0.30, Icon::Face, dim(ui));
            return;
        };
        if !self.face_textures.contains_key(&face_id) && !self.face_missing.contains(&face_id) {
            let dir = db::cache_dir().join("faces");
            match thumb.and_then(|rel| {
                load_texture(ui.ctx(), &dir.join(rel), &format!("face{face_id}"))
            }) {
                Some(t) => {
                    self.face_textures.insert(face_id, t);
                }
                None => {
                    self.face_missing.insert(face_id);
                }
            }
        }
        match self.face_textures.get(&face_id) {
            Some(t) => {
                ui.painter().with_clip_rect(rect).image(
                    t.id(),
                    fit_rect(rect, t.size_vec2()),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            None => paint_icon(ui.painter(), rect.center(), size * 0.30, Icon::Face, dim(ui)),
        }
    }

    /// The right panel of the "People" screen: the list of groups, not the card of a file (M22.26).
    fn faces_panel(&mut self, ui: &mut egui::Ui) {
        panel_edge(ui, true);
        let lang = self.settings.lang;
        let th = theme(ui);
        // The same 44 px header band as the details panel, with the face glyph instead of "File
        // details" and no "×": this list is the screen's content, it does not open or close.
        let w = ui.available_width();
        let (band, _) = ui.allocate_exact_size(egui::vec2(w, PANEL_HEAD_H), egui::Sense::hover());
        ui.painter().rect_filled(band, 0.0, th.card);
        ui.painter().hline(
            band.x_range(),
            band.bottom() - 0.5,
            egui::Stroke::new(1.0, th.border),
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(band.shrink2(egui::vec2(PANEL_PAD, 0.0))),
            |ui| {
                ui.horizontal_centered(|ui| {
                    icon_mark(ui, Icon::Face, 14.0);
                    ui.label(
                        egui::RichText::new(t(lang, "Groups"))
                            .size(12.0)
                            .strong(),
                    );
                });
            },
        );
        egui::ScrollArea::vertical()
            .id_salt("face-groups")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                egui::Frame::NONE
                    .inner_margin(egui::Margin::same(PANEL_PAD as i8))
                    .show(ui, |ui| self.face_list_rows(ui));
            });
    }

    /// The model block: the state, "Compute tags", the filter by tag.
    fn model_block(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        block(ui, |ui| {
            ui.label(
                egui::RichText::new(t(lang, "AI model"))
                    .size(11.0)
                    .color(dim(ui)),
            );
            ui.add_space(2.0);
            if embed::model_present(&self.settings) {
                let entry = embed::active_entry(&self.settings);
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        t(lang, entry.title),
                        db::human_in(lang, embed::model_bytes_in(&self.settings.models_dir, entry))
                    ))
                    .size(11.0)
                    .color(dim(ui)),
                );
                let defs = self.tag_defs.iter().filter(|d| d.enabled).count().max(1);
                let hint = match lang {
                    i18n::Lang::En => format!(
                        "run CLIP locally over the previews: {defs} approximate categories; \
                         look for a specific thing with free-word semantic search; \
                         nothing is sent anywhere"
                    ),
                    i18n::Lang::Ru => i18n::fill(
                        lang,
                        "run CLIP locally over the previews: {} approximate categories; look for the thing you need with a word search; nothing is sent anywhere",
                        &[self.tag_defs.iter().filter(|d| d.enabled).count().to_string()],
                    ),
                };
                if ui
                    .add_enabled(
                        self.embed_rx.is_none(),
                        WordButton::new(Icon::AutoAwesome, t(lang, "Compute tags")),
                    )
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.start_embed();
                }
                let tagged = self
                    .items
                    .iter()
                    .filter(|r| {
                        self.tags.get(&r.id).is_some_and(|t| {
                            embed::confident_tag_with(
                                t,
                                self.settings.tag_show,
                                self.settings.tag_margin,
                            )
                            .is_some()
                        })
                    })
                    .count();
                ui.label(
                    egui::RichText::new(format!(
                        "{} {tagged}",
                        t(lang, "with a confident tag:")
                    ))
                    .size(11.0)
                    .color(dim(ui)),
                );
                let free = i18n::fill(
                    lang,
                    "a free word search understands any words, not only the {} tags",
                    &[defs.to_string()],
                );
                ui.label(
                    egui::RichText::new(free).size(11.0).color(dim(ui)),
                );
                // The tag cloud under its own heading (M25.1). The number next to a tag is how
                // many photos carry it at the current threshold.
                self.tags_heading(ui);
                let counts = self.tag_chips(ui);
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(t(lang, "Filter threshold"))
                        .size(11.0)
                        .color(dim(ui)),
                );
                self.tag_filter_slider(ui);
                if !self.tags.is_empty() && counts.iter().all(|n| *n == 0) {
                    ui.label(
                        egui::RichText::new(t(
                            lang,
                            "no tag passes the threshold — lower the slider",
                        ))
                        .size(11.0)
                        .color(dim(ui)),
                    );
                }
                if let Some(hits) = &self.semantic_hits {
                    ui.label(
                        egui::RichText::new(format!(
                            "{}: {}",
                            t(lang, "search"),
                            i18n::count(
                                lang,
                                hits.len(),"file")
                        ))
                        .size(11.0)
                        .color(dim(ui)),
                    );
                }
            } else {
                ui.label(
                    egui::RichText::new(t(lang, "model not downloaded"))
                        .size(11.0)
                        .color(dim(ui)),
                );
            }
            if word_button(ui, Icon::Settings, t(lang, "Model settings")).clicked() {
                self.settings_open = true;
                self.settings_tab = SettingsTab::Model;
            }
        });
    }

    /// The block of text on the photos: how much is recognized and how much is left.
    fn ocr_block(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        block(ui, |ui| {
            ui.label(
                egui::RichText::new(t(lang, "Text on the photo"))
                    .size(11.0)
                    .color(dim(ui)),
            );
            ui.add_space(2.0);
            if self.tesseract.is_none() {
                ui.label(
                    egui::RichText::new(t(
                        lang,
                        "tesseract does not start — set its path in “Settings”",
                    ))
                    .size(11.0)
                    .color(dim(ui)),
                );
            }
            let images = self.items.iter().filter(|r| r.kind == "image").count();
            let with_text = i18n::fill(
                lang,
                "with text: {} of {} photos",
                &[self.ocr_files.to_string(), images.to_string()],
            );
            ui.label(egui::RichText::new(with_text).size(11.0).color(dim(ui)));
            let candidates = i18n::fill(lang, "{} candidates, {} left", &[self.ocr_candidates.to_string(), self.ocr_remaining.to_string()]);
            ui.label(egui::RichText::new(candidates).size(11.0).color(dim(ui)));
            ui.add_space(4.0);
            if self.ocr_rx.is_some() {
                if word_button(ui, Icon::Stop, t(lang, "Stop")).clicked()
                    && let Some(p) = &self.ocr_progress
                {
                    p.cancel();
                    self.status = t(lang, "OCR: stopping after the current files…").into();
                }
            } else {
                let label = if self.ocr_remaining < self.ocr_candidates {
                    format!("{} ({})", t(lang, "Resume"), self.ocr_remaining)
                } else {
                    format!(
                        "{} ({})",
                        t(lang, "Recognize documents"),
                        self.ocr_candidates
                    )
                };
                if ui
                    .add_enabled(
                        self.tesseract.is_some() && self.ocr_remaining > 0,
                        WordButton::new(
                            if self.ocr_remaining < self.ocr_candidates {
                                Icon::Play
                            } else {
                                Icon::TextFields
                            },
                            label,
                        ),
                    )
                    .on_hover_text(t(
                        lang,
                        "a long pass: documents and screenshots without a camera and with a confident tag; a single photo — with the button in the file card",
                    ))
                    .clicked()
                {
                    self.start_ocr(None);
                }
            }
        });
    }

    /// The summary at the bottom of the left panel: numbers with captions only, no actions.
    fn stats_block(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        ui.label(
            egui::RichText::new(t(lang, "Summary"))
                .size(11.0)
                .color(dim(ui)),
        );
        ui.add_space(2.0);
        let n = self.items.len();
        let bytes: i64 = self.items.iter().map(|r| r.size).sum();
        stat_row(ui, t(lang, "Files:"), &n.to_string());
        stat_row(ui, t(lang, "Size:"), &db::human_in(lang, bytes));
        // "No date" and "No preview" are the counts of the two filter chips above (M25.4): a number
        // printed twice would drift apart the moment the two are computed differently.
        let no_cover = self
            .items
            .iter()
            .filter(|r| r.kind == "video" && r.thumb.is_none())
            .count();
        if no_cover > 0 {
            stat_row(ui, t(lang, "Videos without a cover:"), &no_cover.to_string());
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "press “Index” — ffmpeg will create the covers",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
        }
        ui.add_space(4.0);
    }

    /// The right panel: details and actions for the selected file, in the composition of
    /// `photocat_12` (M22.16): a 44 px header band closed by a line and the card below it.
    fn details_panel(&mut self, ui: &mut egui::Ui) {
        panel_edge(ui, true);
        let lang = self.settings.lang;
        let th = theme(ui);
        // The mock-up's header: the card tone, a 1 px line under it, the info glyph and the
        // title of 12 px. The close target is the mock-up's "×"; the worded way back is
        // "Details" in the top bar (docs/UI.md § Layout).
        let w = ui.available_width();
        let (band, _) = ui.allocate_exact_size(egui::vec2(w, PANEL_HEAD_H), egui::Sense::hover());
        ui.painter().rect_filled(band, 0.0, th.card);
        ui.painter().hline(
            band.x_range(),
            band.bottom() - 0.5,
            egui::Stroke::new(1.0, th.border),
        );
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(band.shrink2(egui::vec2(PANEL_PAD, 0.0))),
            |ui| {
                ui.horizontal_centered(|ui| {
                    icon_mark(ui, Icon::Info, 14.0);
                    ui.label(
                        egui::RichText::new(t(lang, "File details"))
                            .size(12.0)
                            .strong(),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(
                            ui,
                            Icon::Close,
                            22.0,
                            t(lang, "free up room for the grid and calendar"),
                        )
                        .clicked()
                        {
                            self.details_open = false;
                        }
                    });
                });
            },
        );
        egui::ScrollArea::vertical()
            .id_salt("details-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| self.file_card(ui, self.selected));
    }

    /// The file card: a preview, metadata, tags and text. One for the whole interface:
    /// the right panel, the narrow window and the inspector of the full screen call it (M22.7).
    /// The mock-up keeps 12 px of air around it, so no block touches the panel's lines (M22.16).
    fn file_card(&mut self, ui: &mut egui::Ui, shown: Option<i64>) {
        egui::Frame::NONE
            .inner_margin(egui::Margin::same(PANEL_PAD as i8))
            .show(ui, |ui| self.file_card_content(ui, shown));
    }

    fn file_card_content(&mut self, ui: &mut egui::Ui, shown: Option<i64>) {
        let lang = self.settings.lang;
        let Some(id) = shown else {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "Select a photo in the grid — its date, camera, tags and text will appear here.",
                ))
                .size(12.0)
                .color(dim(ui)),
            );
            return;
        };
        let Some(f) = self.items.iter().find(|r| r.id == id).cloned() else {
            return;
        };

        // The text of a photo is read from the DB once per file, not on every frame.
        if !self.ocr_text.contains_key(&id)
            && !self.ocr_snippets.contains_key(&id)
            && let Ok(conn) = db::open(&self.db_path)
            && let Ok(text) = db::load_ocr(&conn, id)
        {
            self.ocr_text.insert(id, text.unwrap_or_default());
        }

        let th = theme(ui);
        let w = ui.available_width();
        // The mock-up's preview card: a 4:3 well of the canvas tone, an 8 px radius and a 1 px line.
        // The snapshot is fitted whole, never cropped (the owner's rule of M22.2).
        let h = (w * 0.75).min(210.0).round();
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter().rect_filled(rect, RADIUS_CARD, th.canvas);
        let well = rect.shrink(1.0);
        let ctx = ui.ctx().clone();
        let mut cells = Cells {
            textures: &mut self.textures,
            missing: &mut self.missing,
            thumbs_dir: &self.thumbs_dir,
            selected: self.selected,
            picked: &self.picked,
            clicked: None,
            open_clicked: None,
            show_keep: false,
            keep_clicked: None,
            trash_mode: false,
            restore_clicked: None,
            tile: self.tile_size,
            lang,
            budget: 4,
            bg: th.canvas,
            cell_style: db::CellStyle::Tiles,
            time_only: false,
        };
        if paint_thumb(ui, &ctx, &mut cells, &f, well) {
            round_preview(ui, well, RADIUS_CARD, th.canvas, true);
        } else {
            let txt = match f.kind.as_str() {
                "video" => t(lang, "▶ video").to_string(),
                _ => t(lang, "no preview").to_string(),
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                txt,
                egui::FontId::proportional(12.0),
                dim(ui),
            );
        }
        ui.painter().rect_stroke(
            rect,
            RADIUS_CARD,
            egui::Stroke::new(1.0, th.border),
            egui::StrokeKind::Inside,
        );
        // The badge of the mock-up in the corner of the preview: the duration of a video, the
        // format of a still ("JPG", "HEIC") — the mock-up's "CR2 RAW · 14-bit" without the
        // RAW metadata the index does not read (M22.16).
        let badge = if f.kind == "video" {
            Some(match f.duration_s {
                Some(d) => format!("▶ {}", db::duration(d)),
                None => t(lang, "▶ video").to_string(),
            })
        } else {
            Path::new(&f.path)
                .extension()
                .and_then(|e| e.to_str())
                .map(|e| e.to_uppercase())
        };
        if let Some(txt) = badge {
            let galley = ui
                .painter()
                .layout_no_wrap(txt, egui::FontId::monospace(10.0), egui::Color32::from_gray(235));
            let badge = egui::Rect::from_min_size(
                egui::pos2(
                    rect.left() + 6.0,
                    rect.bottom() - 6.0 - galley.size().y - 4.0,
                ),
                egui::vec2(galley.size().x + 10.0, galley.size().y + 4.0),
            );
            ui.painter()
                .rect_filled(badge, RADIUS_CTRL, egui::Color32::from_black_alpha(150));
            ui.painter().galley(
                egui::pos2(badge.left() + 5.0, badge.center().y - galley.size().y / 2.0),
                galley,
                egui::Color32::from_gray(235),
            );
        }
        ui.add_space(8.0);
        // The mock-up's identity row: the name in the numeric family on the left, the size on the
        // right; the path is its caption below.
        // The name is cut to the room the size leaves, otherwise a long name widens the card
        // (and with it the whole right panel) past its 280 px and the grid jumps (M22.25).
        let name = file_name(&f.path);
        let size_text = db::human_in(lang, f.size);
        let size_w = mono_width(ui, &size_text, 11.0);
        let name_cap = ((w - size_w - 8.0) / 7.0).max(6.0) as usize;
        ui.horizontal(|ui| {
            ui.add(
                egui::Label::new(
                    num(ui, shorten(&name, name_cap), 12.0)
                        .strong()
                        .color(accent(ui)),
                ),
            )
            .on_hover_text(&name);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(num(ui, &size_text, 11.0).color(dim(ui)));
            });
        });
        ui.add(egui::Label::new(num(ui, &f.path, 11.0).color(dim(ui))).wrap());
        if self.picked.len() > 1 {
            let bytes: i64 = self
                .items
                .iter()
                .filter(|r| self.picked.contains(&r.id))
                .map(|r| r.size)
                .sum();
            ui.label(
                egui::RichText::new(format!(
                    "{} {} · {}",
                    t(lang, "selected"),
                    self.picked.len(),
                    db::human_in(lang, bytes)
                ))
                .size(11.0)
                .color(accent(ui)),
            );
        }
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            // The mock-up puts the actions of the card right under the preview as compact
            // buttons of 28 px; here they carry words and take the 20 px row, so both fit the
            // 255 px of the card in either language (M22.16).
            if ui
                .add(WordButton::small(Icon::FitScreen, t(lang, "Open full screen")))
                .on_hover_text(t(lang, "double-clicking a tile does the same"))
                .clicked()
            {
                self.open_viewer(id);
            }
            let b = WordButton::danger(Icon::Delete, t(lang, "To trash")).compact();
            if ui
                .add(b)
                .on_hover_text(t(
                    lang,
                    "move from the usual views to the app trash; the file on disk is unchanged",
                ))
                .clicked()
            {
                match db::mark_trashed(&self.db_path, id) {
                    Ok(()) => {
                        self.selected = None;
                        self.reload();
                        self.status =
                            t(lang, "file moved to the app trash; untouched on disk").into();
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", t(lang, "file not moved to the trash"))
                    }
                }
            }
        });
        ui.add_space(6.0);

        // The mock-up's shooting-parameters card: the camera, the date and the resolution in two
        // columns, the coordinates and the checksum. The exposure badges of the mock-up are not
        // built: the index keeps the camera name only (plan/M22-ui-spec.md §5).
        let date = format!(
            "{} ({})",
            f.taken_at
                .as_deref()
                .map(|d| d.replace('T', " ").chars().take(16).collect::<String>())
                .unwrap_or_else(|| "—".into()),
            date_source(&f.taken_src, lang)
        );
        let resolution = format!("{}\u{d7}{}", f.width, f.height);
        card_block(ui, |ui| {
            caps_label(ui, t(lang, "Shooting params"));
            ui.add_space(2.0);
            if let Some(c) = &f.camera {
                ui.horizontal(|ui| {
                    icon_mark(ui, Icon::Photo, 14.0);
                    ui.label(egui::RichText::new(c).size(12.0));
                });
            }
            ui.add_space(2.0);
            ui.horizontal_top(|ui| {
                let col = ((ui.available_width() - 8.0) / 2.0).max(60.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(col, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        caps_label(ui, t(lang, "Date and time"));
                        ui.add(egui::Label::new(num(ui, &date, 11.0)).wrap());
                    },
                );
                ui.allocate_ui_with_layout(
                    egui::vec2(col, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        caps_label(ui, t(lang, "Resolution"));
                        let mut value = resolution.clone();
                        if f.kind == "video"
                            && let Some(d) = f.duration_s
                        {
                            value.push_str(&format!(" · ▶ {}", db::duration(d)));
                        }
                        ui.add(egui::Label::new(num(ui, value, 11.0)).wrap());
                    },
                );
            });
            if let (Some(a), Some(b)) = (f.lat, f.lon) {
                ui.add_space(2.0);
                caps_label(ui, t(lang, "Location"));
                // The mock-up puts the pin before the coordinates (M22.16); the place name of
                // the mock-up needs a geocoder, which the offline rule forbids.
                ui.horizontal(|ui| {
                    icon_mark(ui, Icon::Location, 14.0);
                    ui.add(egui::Label::new(num(ui, format!("{a:.5}, {b:.5}"), 11.0)).wrap());
                });
            }
            ui.add_space(4.0);
            hairline(ui);
            ui.add_space(2.0);
            let sum = format!("SHA256 {}…", &f.sha256[..16.min(f.sha256.len())]);
            ui.label(num(ui, sum, 10.0).color(dim(ui)));
        });

        // The albums of the file (M19): chips in the inspector, the "×" removes only the album
        // row. Nothing changes on disk. Since M22.20 the section is always here, with the mock-up's
        // dashed "+ Add": the one way to put a photo into an album without leaving the grid.
        if self.file_albums_for != Some(id) {
            self.file_albums_for = Some(id);
            self.file_albums = db::open(&self.db_path)
                .and_then(|c| db::album_refs_of_file(&c, id))
                .unwrap_or_default();
        }
        {
            let list = self.file_albums.clone();
            ui.add_space(8.0);
            caps_row(ui, t(lang, "Albums"), &list.len().to_string(), false);
            ui.add_space(2.0);
            let mut remove_from: Option<i64> = None;
            let mut open_album: Option<i64> = None;
            let mut add_to_album = false;
            ui.horizontal_wrapped(|ui| {
                for (album_id, path) in &list {
                    let body = t(lang, "open the album in the Albums view");
                    let remove = t(
                        lang,
                        "remove the photo from this album; the file on disk is unchanged",
                    );
                    match card_chip(ui, &shorten(path, 30), Some(Icon::Folder), false, body, remove)
                    {
                        ChipClick::Body => open_album = Some(*album_id),
                        ChipClick::Remove => remove_from = Some(*album_id),
                        ChipClick::None => {}
                    }
                }
                if dashed_add_chip(ui, t(lang, "+ Add"))
                    .on_hover_text(t(
                        lang,
                        "put this photo into an album; the file on disk is unchanged",
                    ))
                    .clicked()
                {
                    add_to_album = true;
                }
            });
            if add_to_album {
                self.album_pick_open(vec![id]);
            }
            if let Some(album_id) = open_album {
                // A click on the word goes to the album; only the "×" changes a row (M22.17).
                self.view = View::Albums;
                self.album_open(album_id);
            }
            if let Some(album_id) = remove_from {
                match db::open(&self.db_path)
                    .and_then(|c| db::remove_from_album(&c, album_id, &[id]))
                {
                    Ok(n) => {
                        self.file_albums_for = None;
                        self.refresh_albums();
                        self.status =
                            format!("{}: {n}", t(lang, "photo removed from the album"));
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", t(lang, "cannot remove from the album"))
                    }
                }
            }
        }

        // Confidence comes from the margin of the first tag over the second (threshold and margin
        // in the settings). The mock-up's section row: the caps heading on the left, "CLIP" and the
        // weight of the leading tag on the right; the tags themselves are chips.
        let confident = self.tags.get(&id).and_then(|tags| {
            embed::confident_tag_with(tags, self.settings.tag_show, self.settings.tag_margin)
                .map(|(tag, _)| tag.to_string())
        });
        let mut hide: Option<String> = None;
        if let Some(tags) = self.tags.get(&id).cloned() {
            let lead = tags.first().map(|(_, s)| *s).unwrap_or(0.0);
            let clip = if confident.is_some() {
                format!("CLIP {:.0}%", lead * 100.0)
            } else {
                "CLIP".to_string()
            };
            ui.add_space(8.0);
            caps_row(ui, t(lang, "AI tags"), &clip, confident.is_some());
            match &confident {
                Some(tag) => {
                    ui.label(
                        egui::RichText::new(format!("{} {tag}", t(lang, "tag:")))
                            .size(11.0)
                            .color(dim(ui)),
                    );
                }
                None if !tags.is_empty() => {
                    ui.label(
                        egui::RichText::new(t(lang, "tag: not sure — the top weights are close"))
                            .size(11.0)
                            .color(dim(ui)),
                    );
                }
                None => {}
            }
            ui.add_space(2.0);
            let remove_hint = t(
                lang,
                "remove the tag from this photo; the original does not change",
            );
            // A click on the word filters the grid by the tag; the accent marks the tags the grid
            // is filtered by, not "the confident one" (M22.17).
            let mut toggle: Option<String> = None;
            let body_hint = |chosen: bool| {
                if chosen {
                    t(lang, "remove this tag from the filter")
                } else {
                    t(lang, "add this tag to the filter")
                }
            };
            let top = if confident.is_some() { tags.len() } else { 3.min(tags.len()) };
            ui.horizontal_wrapped(|ui| {
                for (tag, score) in tags.iter().take(top) {
                    let pct = self.tag_percentile(tag, *score);
                    let label = format!(
                        "{} {:.0}%",
                        i18n::word(lang, tag),
                        score * 100.0
                    );
                    let chosen = self.tag_picked(tag);
                    let body = body_hint(chosen);
                    let (body, remove) = match pct {
                        Some(p) => (
                            format!("{body} · {score:.2} · {p}%"),
                            format!("{remove_hint} · {score:.2} · {p}%"),
                        ),
                        None => (
                            format!("{body} · {score:.2}"),
                            format!("{remove_hint} · {score:.2}"),
                        ),
                    };
                    match card_chip(ui, &label, None, chosen, &body, &remove) {
                        ChipClick::Body => toggle = Some(tag.clone()),
                        ChipClick::Remove => hide = Some(tag.clone()),
                        ChipClick::None => {}
                    }
                }
            });
            if confident.is_none() && tags.len() > top {
                egui::CollapsingHeader::new(format!("{} ({})", t(lang, "all tags"), tags.len()))
                    .id_salt(("card_tags", id))
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for (tag, score) in tags.iter().skip(top) {
                                let pct = self.tag_percentile(tag, *score);
                                let label = format!(
                                    "{} {:.0}%",
                                    i18n::word(lang, tag),
                                    score * 100.0
                                );
                                let chosen = self.tag_picked(tag);
                                let body = body_hint(chosen);
                                let (body, remove) = match pct {
                                    Some(p) => (
                                        format!("{body} · {score:.2} · {p}%"),
                                        format!("{remove_hint} · {score:.2} · {p}%"),
                                    ),
                                    None => (
                                        format!("{body} · {score:.2}"),
                                        format!("{remove_hint} · {score:.2}"),
                                    ),
                                };
                                match card_chip(ui, &label, None, chosen, &body, &remove) {
                                    ChipClick::Body => toggle = Some(tag.clone()),
                                    ChipClick::Remove => hide = Some(tag.clone()),
                                    ChipClick::None => {}
                                }
                            }
                        });
                    });
            }
            if let Some(tag) = toggle {
                self.toggle_tag(&tag);
            }
        } else {
            ui.add_space(8.0);
            caps_row(ui, t(lang, "AI tags"), "CLIP", false);
            ui.label(
                egui::RichText::new(t(lang, "tag: not computed"))
                    .size(11.0)
                    .color(dim(ui)),
            );
        }
        if let Some(tag) = hide {
            match db::hide_tag(&self.db_path, id, &tag) {
                Ok(()) => {
                    if let Some(tags) = self.tags.get_mut(&id) {
                        tags.retain(|(t, _)| t != &tag);
                    }
                    *self.hidden_tags.entry(id).or_insert(0) += 1;
                    self.status =
                        format!("{}: «{tag}»", t(lang, "tag removed from this photo"));
                }
                Err(e) => self.status = format!("{}: {e}", t(lang, "tag not removed")),
            }
        }
        let hidden = self.hidden_tags.get(&id).copied().unwrap_or(0);
        if hidden > 0
            && ui
                .add(WordButton::new(
                    Icon::Restore,
                    format!("{} ({hidden})", t(lang, "restore hidden tags")),
                ))
                .clicked()
        {
            match db::unhide_tags(&self.db_path, id) {
                Ok(_) => {
                    self.reload();
                    self.status = t(lang, "hidden tags restored").into();
                }
                Err(e) => self.status = format!("{}: {e}", t(lang, "tags not restored")),
            }
        }

        ui.add_space(8.0);
        let own = self.user_tags.get(&id).cloned().unwrap_or_default();
        caps_row(ui, t(lang, "My tags"), &own.len().to_string(), false);
        if own.is_empty() {
            ui.label(
                egui::RichText::new(t(lang, "none yet"))
                    .size(11.0)
                    .color(dim(ui)),
            );
        }
        let mut remove_own: Option<String> = None;
        let remove_hint = t(lang, "remove");
        let mut own_toggle: Option<String> = None;
        ui.horizontal_wrapped(|ui| {
            for tag in &own {
                let chosen = self.tag_picked(tag);
                let body = if chosen {
                    t(lang, "remove this tag from the filter")
                } else {
                    t(lang, "add this tag to the filter")
                };
                match card_chip(ui, tag, None, chosen, body, remove_hint) {
                    ChipClick::Body => own_toggle = Some(tag.clone()),
                    ChipClick::Remove => remove_own = Some(tag.clone()),
                    ChipClick::None => {}
                }
            }
        });
        if let Some(tag) = own_toggle {
            self.toggle_tag(&tag);
        }
        if let Some(tag) = remove_own {
            match db::remove_user_tag(&self.db_path, id, &tag) {
                Ok(()) => {
                    if let Some(tags) = self.user_tags.get_mut(&id) {
                        tags.retain(|t| t != &tag);
                    }
                    self.status = format!("{}: «{tag}»", t(lang, "own tag removed"));
                }
                Err(e) => self.status = format!("{}: {e}", t(lang, "tag not removed")),
            }
        }
        // The mock-up's inline field: the "#", the tag and a compact "+" in one framed row
        // (M22.16). The row has to fit the 280 px panel: the field takes what the button leaves.
        // The button is an icon-only 22 px square since M22.18 — a word took 140 px of the row and
        // left the field short.
        let mut add_tag = false;
        egui::Frame::new()
            .fill(th.field)
            .stroke(egui::Stroke::new(1.0, th.border))
            .corner_radius(egui::CornerRadius::same(RADIUS_CTRL as u8))
            // 22 px of content, 2 px above and below and the 1 px line on each side: the
            // mock-up's `h-7` (28 px) exactly (M22.18).
            .inner_margin(egui::Margin::symmetric(8, 2))
            .show(ui, |ui| {
                // A frame hugs its content: the row has to take the whole card, or the field is
                // narrower than the block above it.
                let w = ui.available_width();
                ui.set_width(w);
                // The "#" glyph and the 8 px that egui puts before the field, the "+" and the gap
                // before it: the field takes what is left and never widens the panel (M22.12).
                const ADD_W: f32 = 22.0;
                const HASH_W: f32 = 16.0;
                const GAP: f32 = 8.0;
                // The theme gives a text edit the height of a button (26 px) and aligns its text
                // to the top: inside the framed row the text stuck to the upper edge and the
                // field looked short and crooked (M22.18). The row sets its own floor and centres
                // the text, so "#", the tag and the "+" share one line.
                ui.spacing_mut().interact_size.y = ADD_W;
                let width = (w - HASH_W - ADD_W - GAP).max(40.0);
                ui.horizontal(|ui| {
                    ui.label(num(ui, "#", 12.0).color(dim(ui)));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.user_tag_input)
                            .frame(egui::Frame::NONE)
                            .align(egui::Align2::LEFT_CENTER)
                            .hint_text(t(lang, "your own tag, e.g. furniture"))
                            .desired_width(width),
                    );
                    add_tag = icon_button(ui, Icon::Add, ADD_W, t(lang, "Add tag")).clicked();
                });
            });
        if add_tag {
            let tag = self.user_tag_input.trim().to_string();
            if !tag.is_empty() {
                match db::add_user_tag(&self.db_path, id, &tag) {
                    Ok(()) => {
                        let tags = self.user_tags.entry(id).or_default();
                        tags.push(tag.clone());
                        tags.sort();
                        self.user_tag_input.clear();
                        self.status = format!("{}: «{tag}»", t(lang, "own tag added"));
                    }
                    Err(e) => {
                        self.status = format!("{}: {e}", t(lang, "tag not added"))
                    }
                }
            }
        }

        if f.kind == "image" {
            ui.add_space(8.0);
            let engine = if self.tesseract.is_some() { "tesseract" } else { "" };
            caps_row(ui, t(lang, "Text on the photo"), engine, false);
            let label = if self.ocr_text.get(&id).is_some_and(|t| !t.trim().is_empty()) {
                t(lang, "Recognize again")
            } else {
                t(lang, "Recognize text")
            };
            if ui
                .add_enabled(
                    self.tesseract.is_some() && self.ocr_rx.is_none(),
                    WordButton::new(
                        if self.ocr_text.get(&id).is_some_and(|t| !t.trim().is_empty()) {
                            Icon::Refresh
                        } else {
                            Icon::TextFields
                        },
                        label,
                    ),
                )
                .on_hover_text(t(lang, "this photo only; the original is unchanged"))
                .clicked()
            {
                self.start_ocr(Some(id));
            }
            if self.tesseract.is_none() {
                ui.label(
                    egui::RichText::new(t(lang, "button disabled: tesseract does not start"))
                        .size(11.0)
                        .color(dim(ui)),
                );
            }
            let shown = self
                .ocr_snippets
                .get(&id)
                .cloned()
                .or_else(|| self.ocr_text.get(&id).cloned())
                .filter(|t| !t.trim().is_empty());
            if let Some(text) = shown {
                ui.add_space(4.0);
                block(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("card-text")
                        .max_height(220.0)
                        .show(ui, |ui| {
                            ui.label(highlight_job(ui, &text));
                        });
                });
                if ui
                    .add(WordButton::new(Icon::TextFields, t(lang, "Show the full text")))
                    .on_hover_text(t(lang, "full screen: the frame and all recognized text"))
                    .clicked()
                {
                    self.open_viewer(id);
                    if let Some(v) = &mut self.viewer {
                        v.show_text = true;
                    }
                }
            }
        }
    }

    /// Going from a face to the photo: not into the common grid but into the calendar day it lies in.
    /// Without a date — the common grid with this file selected.
    fn open_from_faces(&mut self, file_id: i64) {
        let lang = self.settings.lang;
        // The old search and filter must not hide the photo inside a calendar day.
        self.reset_filters();
        self.select_one(file_id);
        // The filters were removed on purpose: that is no reason to drop the selection just made.
        self.pick_scope = self.pick_scope_now();
        let date = self
            .items
            .iter()
            .find(|r| r.id == file_id)
            .and_then(|r| r.taken_at.as_deref())
            .and_then(parse_date);
        match date {
            Some(d) => {
                self.cal_year = d.year();
                self.cal_month = d.month();
                self.cal_day = Some(d.format("%Y-%m-%d").to_string());
                self.cal_week = monday_of(d);
                self.cal_scale = CalScale::Month;
                self.view = View::Calendar;
                self.status = i18n::fill(lang, "the photo is open in the calendar: {}", &[d.format("%Y-%m-%d").to_string()]);
            }
            None => {
                self.view = View::Grid;
                self.status = t(lang, "the photo has no date — it is open in all files, selected").into();
            }
        }
    }

    /// Queue a frame for background decoding. `front` — the current frame matters more than its neighbours.
    fn request_photo(&mut self, id: i64, front: bool) {
        if self.photo_cache.contains_key(&id) || self.photo_loading.contains(&id) {
            return;
        }
        let Some(f) = self
            .items
            .iter()
            .chain(self.trash_items.iter())
            .find(|r| r.id == id)
        else {
            return;
        };
        if f.kind != "image" {
            return;
        }
        self.photo_loading.insert(id);
        if front {
            self.photo_jobs.push_front(id);
        } else {
            self.photo_jobs.push_back(id);
        }
        while self.photo_jobs.len() > 8 {
            if let Some(old) = self.photo_jobs.pop_back() {
                self.photo_loading.remove(&old);
            }
        }
        self.pump_photo();
    }

    /// One background decoder at a time: the queue does not let dozens of threads eat the memory.
    fn pump_photo(&mut self) {
        if self.photo_rx.is_some() {
            return;
        }
        while let Some(id) = self.photo_jobs.pop_front() {
            if self.photo_cache.contains_key(&id) {
                self.photo_loading.remove(&id);
                continue;
            }
            let Some(f) = self
                .items
                .iter()
                .chain(self.trash_items.iter())
                .find(|r| r.id == id)
            else {
                self.photo_loading.remove(&id);
                continue;
            };
            let path = PathBuf::from(&f.path);
            let (tx, rx) = mpsc::channel();
            self.photo_rx = Some(rx);
            std::thread::spawn(move || {
                let res = derive::fast_view(&path, 1920);
                let _ = tx.send((id, res));
            });
            return;
        }
    }

    /// The current frame, the next, the previous and one past the next — for paging and the slideshow.
    fn request_around(&mut self, ids: &[i64], idx: usize) {
        if ids.is_empty() {
            return;
        }
        let n = ids.len();
        self.request_photo(ids[idx], true);
        if n > 1 {
            self.request_photo(ids[(idx + 1) % n], false);
        }
        if n > 2 {
            self.request_photo(ids[(idx + n - 1) % n], false);
        }
        if n > 3 {
            self.request_photo(ids[(idx + 2) % n], false);
        }
    }

    /// Preloading the selected file (a click on a tile before the double click).
    fn prefetch_photo(&mut self, id: i64) {
        self.request_photo(id, true);
    }

    fn trim_photo_cache(&mut self) {
        let current = self.viewer.as_ref().and_then(|v| v.ids.get(v.idx)).copied();
        while self.photo_cache.len() > 4 {
            let Some(k) = self
                .photo_cache
                .keys()
                .find(|k| Some(**k) != current)
                .copied()
            else {
                break;
            };
            self.photo_cache.remove(&k);
        }
    }

    fn poll_photo(&mut self, ctx: &egui::Context) {
        let res = {
            let Some(rx) = &self.photo_rx else { return };
            rx.try_recv()
        };
        match res {
            Ok((id, Ok((w, h, raw)))) => {
                self.photo_loading.remove(&id);
                self.photo_rx = None;
                let current = self.viewer.as_ref().and_then(|v| v.ids.get(v.idx)).copied();
                if current == Some(id) {
                    let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &raw);
                    let tex = ctx.load_texture(
                        format!("viewer{id}"),
                        color,
                        egui::TextureOptions::LINEAR,
                    );
                    self.viewer_tex = Some((id, tex));
                } else {
                    self.photo_cache.insert(id, (w, h, raw));
                    self.trim_photo_cache();
                }
                if let Some(sel) = self.selected
                    && sel != id
                    && self.viewer.is_none()
                {
                    self.prefetch_photo(sel);
                }
                self.pump_photo();
            }
            Ok((id, Err(e))) => {
                self.photo_loading.remove(&id);
                self.photo_rx = None;
                self.status = format!("{}: {e:#}", t(self.settings.lang, "the frame was not decoded"));
                self.pump_photo();
            }
            Err(mpsc::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(30));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                self.photo_rx = None;
                self.pump_photo();
            }
        }
    }

    /// The built-in player plays the video: the frame into a texture, the sound through cpal.
    /// If the file is beyond it (a codec out of scope, a broken container) — we honestly
    /// yield to an external `ffplay` instead of showing a black screen.
    fn start_video_playback(&mut self, f: &FileRow) {
        let lang = self.settings.lang;
        self.stop_video();
        self.video_lag_shown = false;
        self.video_id = Some(f.id);
        match player::Player::open(Path::new(&f.path), player::Options::default()) {
            Ok(p) => {
                self.status = format!(
                    "{}: {}{}",
                    t(lang, "video"),
                    f.path,
                    if p.has_audio() {
                        t(lang, " (with sound)")
                    } else {
                        t(lang, " (without sound)")
                    }
                );
                self.video = Some(p);
            }
            Err(e) => {
                self.status = format!(
                    "{} ({e:#}); {}",
                    t(lang, "the built-in player could not open it"),
                    t(lang, "opening ffplay")
                );
                let tools = self.settings.tools.clone();
                match tools::ffplay(&tools, Path::new(&f.path)) {
                    Ok(child) => {
                        self.video_child = Some(child);
                    }
                    Err(e2) => {
                        self.status = format!(
                            "{} ({e2:#}); {}",
                            t(lang, "ffplay did not start"),
                            t(lang, "the system player")
                        );
                        match tools::open_path(Path::new(&f.path)) {
                            Ok(()) => {
                                self.status = format!(
                                    "{}: {}",
                                    t(lang, "opening in the system player"),
                                    f.path
                                )
                            }
                            Err(e3) => {
                                self.status =
                                    format!("{}: {e3:#}", t(lang, "cannot open the video"))
                            }
                        }
                        self.video_id = None;
                    }
                }
            }
        }
    }

    fn stop_video(&mut self) {
        self.video_lag_shown = false;
        self.video_ended = false;
        self.video_tex_at = None;
        if let Some(mut p) = self.video.take() {
            p.stop();
        }
        self.video_tex = None;
        if let Some(mut child) = self.video_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
        self.video_id = None;
    }

    /// The player frame → a texture. While the video plays the window is repainted 30 times a second.
    fn poll_video(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        if let Some(child) = &mut self.video_child
            && child.try_wait().ok().flatten().is_some()
        {
            self.video_child = None;
            self.video_id = None;
            self.status = t(lang, "ffplay: the video is closed").into();
        }
        let Some(p) = self.video.as_ref() else { return };
        let due = self
            .video_tex_at
            .is_none_or(|t| t.elapsed() >= std::time::Duration::from_millis(25));
        if due && let Some(frame) = p.take_frame() {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [frame.width, frame.height],
                &frame.rgba,
            );
            match &mut self.video_tex {
                Some(tex) => tex.set(image, egui::TextureOptions::LINEAR),
                None => {
                    self.video_tex = Some(ctx.load_texture(
                        "photocat-video",
                        image,
                        egui::TextureOptions::LINEAR,
                    ))
                }
            }
            self.video_tex_at = Some(std::time::Instant::now());
        }
        if p.audio_lag() && !self.video_lag_shown {
            self.video_lag_shown = true;
            self.status = t(
                lang,
                "the decoder cannot keep up with the sound: sound is off, the video runs slower",
            )
            .into();
        }
        if let Some(e) = p.error() {
            self.status = format!("{}: {e}", t(lang, "the video stopped"));
            self.video = None;
            self.video_id = None;
            self.video_tex = None;
            return;
        }
        if p.is_finished() {
            self.status = t(lang, "the video has finished").into();
            // A finished file gives way to the next one by itself (see `viewer_modal`).
            self.video_ended = self.viewer.is_some();
            self.video = None;
            self.video_id = None;
            self.video_tex = None;
            if self.video_ended {
                ctx.request_repaint();
            }
            return;
        }
        if p.is_playing() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    /// The list for paging in the full-screen viewer: the current view and the current filter.
    fn viewer_ids(&self) -> Vec<i64> {
        match self.view {
            View::Faces if self.face_together && self.face_picked_clusters.len() >= 2 => {
                let mut per_file: HashMap<i64, HashSet<i64>> = HashMap::new();
                for f in &self.face_views {
                    if let Some(cid) = f.cluster_id
                        && self.face_picked_clusters.contains(&cid)
                    {
                        per_file.entry(f.file_id).or_default().insert(cid);
                    }
                }
                per_file
                    .into_iter()
                    .filter(|(_, set)| self.face_picked_clusters.iter().all(|c| set.contains(c)))
                    .map(|(id, _)| id)
                    .collect()
            }
            View::Faces => {
                let mut seen = HashSet::new();
                self.face_views
                    .iter()
                    .filter(|f| {
                        self.face_selected_cluster.is_none()
                            || f.cluster_id == self.face_selected_cluster
                    })
                    .filter(|f| seen.insert(f.file_id))
                    .map(|f| f.file_id)
                    .collect()
            }
            View::Trash => self.trash_items.iter().map(|r| r.id).collect(),
            View::Calendar => {
                let ids = self
                    .ordered(&self.filtered())
                    .into_iter()
                    .map(|i| self.items[i].id);
                match &self.cal_day {
                    Some(day) => ids
                        .filter(|id| {
                            self.items
                                .iter()
                                .find(|r| r.id == *id)
                                .and_then(|r| r.taken_at.as_deref())
                                .is_some_and(|d| d.starts_with(day.as_str()))
                        })
                        .collect(),
                    None => ids.collect(),
                }
            }
            _ => self
                .ordered(&self.filtered())
                .into_iter()
                .map(|i| self.items[i].id)
                .collect(),
        }
    }

    fn open_viewer(&mut self, file_id: i64) {
        self.stop_video();
        self.video_ended = false;
        let mut ids = self.viewer_ids();
        if !ids.contains(&file_id) {
            ids.push(file_id);
        }
        let idx = ids.iter().position(|&id| id == file_id).unwrap_or(0);
        self.request_around(&ids, idx);
        let mut viewer = Viewer {
            ids,
            idx,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            rotation: 0,
            inspector: false,
            show_faces: false,
            face_focus: None,
            day: None,
            series: Vec::new(),
            show_text: false,
            slideshow: false,
            interval_s: 3,
            last_advance: std::time::Instant::now(),
        };
        self.set_series(&mut viewer);
        self.viewer = Some(viewer);
        self.viewer_tex = None;
    }

    /// Changing the file in the full-screen viewer.
    ///
    /// The video is stopped without fail: otherwise the frame of the old video stays on the
    /// screen over the new photo, and paging looks like "nothing is shown".
    fn viewer_step(&mut self, v: &mut Viewer, step: i32) {
        if step == 0 || v.ids.len() <= 1 {
            return;
        }
        self.stop_video();
        v.idx = (v.idx as i64 + step as i64).rem_euclid(v.ids.len() as i64) as usize;
        v.last_advance = std::time::Instant::now();
        // A new frame is shown from the centre: someone else's offset on another photo looks
        // like a cropped frame for no reason.
        v.pan = egui::Vec2::ZERO;
        // A sideways-looking frame was turned on purpose for one file; the next one is shown
        // as it is stored (M22.7).
        v.rotation = 0;
        // The panel lists the photos of the face picked on the previous frame: another photo is
        // another face, so the panel closes (M22.28).
        v.face_focus = None;
        self.set_series(v);
    }

    /// Jump to a file of the series strip: the same as a step, but straight to the sixth
    /// neighbour; the video is stopped and the frame is centred and turned back (M22.7).
    fn viewer_goto(&mut self, v: &mut Viewer, idx: usize) {
        if idx >= v.ids.len() || idx == v.idx {
            return;
        }
        self.stop_video();
        v.idx = idx;
        v.last_advance = std::time::Instant::now();
        v.pan = egui::Vec2::ZERO;
        v.rotation = 0;
        v.face_focus = None;
        self.set_series(v);
    }

    /// The photos of the face picked in the full screen (M22.28): every photo of the same group,
    /// oldest first. A face that is not grouped yet lists only the frame on screen.
    fn face_focus_for(&self, face: &db::FaceBox, current: i64) -> FaceFocus {
        let mut file_ids: Vec<i64> = Vec::new();
        if let Some(cid) = face.cluster_id {
            for f in &self.face_views {
                if f.cluster_id == Some(cid) && !file_ids.contains(&f.file_id) {
                    file_ids.push(f.file_id);
                }
            }
        }
        if file_ids.is_empty() {
            file_ids.push(current);
        }
        let date_of = |id: i64| {
            self.items
                .iter()
                .chain(self.trash_items.iter())
                .find(|r| r.id == id)
                .and_then(|r| r.taken_at.clone())
        };
        file_ids.sort_by(|a, b| {
            cmp_date_none_last(date_of(*a).as_deref(), date_of(*b).as_deref(), false)
                .then(a.cmp(b))
        });
        FaceFocus {
            label: face.label.clone(),
            cluster_id: face.cluster_id,
            file_ids,
        }
    }

    /// Keep the series strip of the viewer in step with the file on screen (M22.7).
    fn set_series(&self, v: &mut Viewer) {
        let (day, series) = self.viewer_series(v);
        v.day = day;
        v.series = series;
    }

    /// The neighbours of the current file inside its day — the "series" of the full screen
    /// (M22.7): the snapshots of one date, in the order of the viewer. The strip is a window
    /// around the current file, so it needs no scroll area of its own.
    ///
    /// A file without a date has no series: there is nothing to stand next to.
    fn viewer_series(&self, v: &Viewer) -> (Option<String>, Vec<StripItem>) {
        let day_of = |id: i64| {
            self.items
                .iter()
                .chain(self.trash_items.iter())
                .find(|r| r.id == id)
                .and_then(|r| r.taken_at.as_deref())
                .map(|d| d.chars().take(10).collect::<String>())
        };
        let Some(day) = v.ids.get(v.idx).copied().and_then(day_of) else {
            return (None, Vec::new());
        };
        let mut all: Vec<StripItem> = Vec::new();
        let mut pos = 0usize;
        let mut cur_pos = 0usize;
        for (idx, id) in v.ids.iter().copied().enumerate() {
            if day_of(id).as_deref() != Some(day.as_str()) {
                continue;
            }
            let Some(f) = self
                .items
                .iter()
                .chain(self.trash_items.iter())
                .find(|r| r.id == id)
            else {
                continue;
            };
            pos += 1;
            if idx == v.idx {
                cur_pos = pos;
            }
            all.push(StripItem {
                idx,
                id,
                thumb: f.thumb.clone(),
                video: f.kind == "video",
                pos,
            });
        }
        let start = cur_pos
            .saturating_sub(SERIES_WINDOW / 2)
            .min(all.len().saturating_sub(SERIES_WINDOW));
        let series = all.into_iter().skip(start).take(SERIES_WINDOW).collect();
        (Some(day), series)
    }

    fn viewer_modal(&mut self, ctx: &egui::Context) {
        let Some(mut v) = self.viewer.take() else {
            return;
        };
        if v.ids.is_empty() {
            return;
        }
        let lang = self.settings.lang;
        v.idx = v.idx.min(v.ids.len() - 1);
        // The video is over — show the next file by ourselves: otherwise the viewer stands on the
        // last frame and it seems that paging does not work.
        let ended = std::mem::take(&mut self.video_ended);
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut step = 0i32;
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            step = 1;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            step = -1;
        }
        // An arrow pressed in the same frame wins: one step per frame.
        if step == 0 && ended && v.idx + 1 < v.ids.len() {
            step = 1;
        }
        self.viewer_step(&mut v, step);
        if v.slideshow
            && v.last_advance.elapsed() >= std::time::Duration::from_secs(v.interval_s)
        {
            let next = v.ids[(v.idx + 1) % v.ids.len()];
            // The video needs no waiting: the cover frame is shown, there is no full decode.
            let is_video = self
                .items
                .iter()
                .chain(self.trash_items.iter())
                .find(|r| r.id == next)
                .is_some_and(|r| r.kind == "video");
            let ready = is_video
                || self.photo_cache.contains_key(&next)
                || self.viewer_tex.as_ref().is_some_and(|(t, _)| *t == next);
            if ready {
                self.viewer_step(&mut v, 1);
            } else {
                // Do not show mush: wait for the ready next frame.
                self.request_photo(next, true);
            }
        }
        let id = v.ids[v.idx];
        let Some(f) = self
            .items
            .iter()
            .chain(self.trash_items.iter())
            .find(|r| r.id == id)
            .cloned()
        else {
            self.status = t(lang, "file not found in the index — viewer closed").into();
            return;
        };
        self.request_around(&v.ids, v.idx);
        if v.show_text
            && !self.ocr_text.contains_key(&id)
            && let Ok(conn) = db::open(&self.db_path)
            && let Ok(text) = db::load_ocr(&conn, id)
        {
            self.ocr_text.insert(id, text.unwrap_or_default());
        }
        let need_full = self
            .viewer_tex
            .as_ref()
            .map(|(t, _)| *t != id)
            .unwrap_or(true);
        if need_full
            && let Some((w, h, raw)) = self.photo_cache.remove(&id)
        {
            let color = egui::ColorImage::from_rgba_unmultiplied([w, h], &raw);
            let tex = ctx.load_texture(
                format!("viewer{id}"),
                color,
                egui::TextureOptions::LINEAR,
            );
            self.viewer_tex = Some((id, tex));
        }
        if !self.textures.contains_key(&id)
            && !self.missing.contains(&id)
            && let Some(rel) = f.thumb.as_ref()
            && let Some(t) = load_texture(
                ctx,
                &self.thumbs_dir.join(rel),
                &format!("t{id}"),
            )
        {
            self.textures.insert(id, t);
        }
        let mut open_video = false;
        let mut start_video = false;
        let mut stop_video = false;
        let mut toggle_video = false;
        let mut seek_video: Option<f64> = None;
        let mut volume_video: Option<f32> = None;
        let mut step_click = 0i32;
        let video_active =
            self.video_id == Some(id) && (self.video.is_some() || self.video_child.is_some());
        // Space is the same as ▶: play and pause for video, a slideshow for a photo.
        if ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            if f.kind == "video" {
                if video_active && self.video.is_some() {
                    toggle_video = true;
                } else if !video_active {
                    start_video = true;
                }
            } else {
                v.slideshow = !v.slideshow;
                v.last_advance = std::time::Instant::now();
            }
        }
        if v.slideshow || video_active {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
        // A player frame belongs to a particular file: without the check the old video is drawn
        // over the new photo and the file change is not visible.
        // The textures are cloned out of `self`: the inspector draws the file card, which needs
        // the whole `App`, so the modal closure borrows it mutably (M22.7).
        let video_frame = self
            .video_tex
            .as_ref()
            .filter(|_| self.video_id == Some(id))
            .cloned();
        let paused = video_active && self.video.as_ref().is_some_and(|p| !p.is_playing());
        let full_tex = self.viewer_tex.as_ref().filter(|(t, _)| *t == id).cloned();
        // A video has no "full frame": there it is the cover frame, not a loading state.
        let stale_tex = self
            .viewer_tex
            .as_ref()
            .filter(|(t, _)| *t != id && f.kind != "video")
            .map(|(_, t)| t.clone());
        let preview_tex = self.textures.get(&id).cloned();
        // The faces of the frame: read once per file, only when the model has run (M22.7).
        if self.viewer_faces.as_ref().map(|(fid, _)| *fid) != Some(id) {
            let boxes = db::open(&self.db_path)
                .and_then(|c| db::load_face_boxes(&c, id))
                .unwrap_or_default();
            self.viewer_faces = Some((id, boxes));
        }
        let faces = self
            .viewer_faces
            .as_ref()
            .filter(|(fid, _)| *fid == id)
            .map(|(_, b)| b.clone())
            .unwrap_or_default();
        // The boxes are drawn only on demand (M22.28): the tool "Show faces" decides.
        let shown_faces: &[db::FaceBox] = if v.show_faces { &faces } else { &[] };
        // The neighbours of the same day: the strip under the frame, computed on a file change.
        // The thumbnails come from the same cache as the grid: a file that has just been on the
        // screen costs nothing.
        for item in &v.series {
            if self.textures.contains_key(&item.id) || self.missing.contains(&item.id) {
                continue;
            }
            let loaded = item.thumb.as_ref().and_then(|rel| {
                load_texture(ctx, &self.thumbs_dir.join(rel), &format!("t{}", item.id))
            });
            match loaded {
                Some(t) => {
                    self.textures.insert(item.id, t);
                }
                None => {
                    self.missing.insert(item.id);
                }
            }
        }
        let mut new_zoom = v.zoom;
        let mut new_pan = v.pan;
        let mut new_text = v.show_text;
        let mut new_slideshow = v.slideshow;
        let mut new_interval = v.interval_s;
        let mut new_rotation = v.rotation;
        let mut new_inspector = v.inspector;
        let mut new_show_faces = v.show_faces;
        let mut new_focus = v.face_focus.clone();
        let mut want_one_to_one = false;
        let mut goto: Option<usize> = None;
        let mut image_clicked = false;
        let mut face_click: Option<i64> = None;
        let mut panel_open: Option<i64> = None;
        let mut panel_close = false;
        let mut reopen: Option<i64> = None;
        if ctx.input(|i| i.key_pressed(egui::Key::I)) {
            new_inspector = !v.inspector;
        }

        let screen = ctx.content_rect();
        let resp = egui::Modal::new(egui::Id::unique("photocat-viewer")).show(ctx, |ui| {
            let min_size = screen.size() - egui::vec2(48.0, 48.0);
            ui.set_min_size(min_size);
            let avail = ui.available_size();
            let chrome_top = ui.cursor().top();
            // The column is decided by what is on the screen at the start of the frame: a face
            // picked now opens its panel on the next frame, when the body has made room for it
            // (M22.28).
            let insp_w = if v.inspector || v.face_focus.is_some() {
                RIGHT_W + 16.0
            } else {
                0.0
            };
            let left_w = (avail.x - insp_w).max(200.0);
            let strip_h = if v.series.len() > 1 {
                VIEWER_SERIES_H
            } else {
                0.0
            };
            // The chrome flows by itself, so a narrow window may wrap the tools row without
            // clipping it; the body takes what is left (M22.7).
                // The header: the way back, the name, the facts in mono, the position in the
                // accent (M22.7).
                ui.horizontal(|ui| {
                    if ui
                        .add(WordButton::new(Icon::ArrowBack, t(lang, "Back to the grid")))
                        .on_hover_text(t(lang, "close the full screen — Esc"))
                        .clicked()
                    {
                        close = true;
                    }
                    ui.label(egui::RichText::new(file_name(&f.path)).size(14.0));
                    let mut facts =
                        format!("{} · {}×{}", db::human_in(lang, f.size), f.width, f.height);
                    if f.kind == "video"
                        && let Some(d) = f.duration_s
                    {
                        facts.push_str(&format!(" · ▶ {}", db::duration(d)));
                    }
                    let date = f
                        .taken_at
                        .as_deref()
                        .map(|d| d.replace('T', " ").chars().take(16).collect::<String>())
                        .unwrap_or_else(|| t(lang, "no date").into());
                    ui.label(num(ui, format!("{date} · {facts}"), 11.0).color(dim(ui)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        // The mock-up's "×": the close target of the whole screen, next to the
                        // position counter (M22.28).
                        if icon_button(
                            ui,
                            Icon::Close,
                            22.0,
                            t(lang, "close the full screen — Esc"),
                        )
                        .clicked()
                        {
                            close = true;
                        }
                        ui.label(
                            num(ui, i18n::position(lang, v.idx + 1, v.ids.len()), 11.0)
                                .color(accent(ui)),
                        );
                    });
                });
                hairline(ui);
                // The tools row: a word and a hint for every tool, no icon-only path (M22.7).
                ui.horizontal_wrapped(|ui| {
                    if ui
                        .add(WordButton::new(Icon::ZoomOut, t(lang, "Zoom out")))
                        .on_hover_text(t(lang, "100 % fits the frame into the window"))
                        .clicked()
                    {
                        new_zoom = (v.zoom - 0.25).max(0.5);
                    }
                    ui.label(num(ui, format!("{:.0} %", v.zoom * 100.0), 11.0));
                    if ui
                        .add(WordButton::new(Icon::ZoomIn, t(lang, "Zoom in")))
                        .on_hover_text(t(lang, "above 100 % the frame is dragged with the mouse"))
                        .clicked()
                    {
                        new_zoom = (v.zoom + 0.25).min(MAX_ZOOM);
                    }
                    if ui
                        .add(WordButton::new(Icon::FitScreen, t(lang, "Fit")))
                        .on_hover_text(t(lang, "the whole frame in the window"))
                        .clicked()
                    {
                        new_zoom = 1.0;
                        new_pan = egui::Vec2::ZERO;
                    }
                    if ui
                        .add_enabled(
                            full_tex.is_some(),
                            WordButton::new(Icon::OneToOne, t(lang, "1:1")),
                        )
                        .on_hover_text(t(lang, "one image pixel per screen pixel"))
                        .clicked()
                    {
                        want_one_to_one = true;
                    }
                    if ui
                        .add(WordButton::new(Icon::Rotate, t(lang, "Rotate")))
                        .on_hover_text(t(
                            lang,
                            "turn the view by 90°; the file on disk is unchanged",
                        ))
                        .clicked()
                    {
                        new_rotation = (v.rotation + 1) % 4;
                    }
                    if v.zoom > 1.0 {
                        ui.label(
                            egui::RichText::new(t(lang, "drag to move the frame"))
                                .size(11.0)
                                .color(dim(ui)),
                        );
                    }
                    if ui
                        .add(WordButton::picked(
                            Icon::TextFields,
                            t(lang, "Text on the photo"),
                            v.show_text,
                        ))
                        .clicked()
                    {
                        new_text = !v.show_text;
                    }
                    // The face boxes are a tool, not a permanent overlay: off by default, one
                    // word and the silhouette glyph turn them on (M22.28).
                    let faces_hint = if faces.is_empty() {
                        t(lang, "no faces found in this file")
                    } else {
                        t(lang, "the boxes of the found faces — a click on a face lists its photos")
                    };
                    if ui
                        .add_enabled(
                            !faces.is_empty(),
                            WordButton::picked(Icon::Face, t(lang, "Show faces"), v.show_faces),
                        )
                        .on_hover_text(faces_hint)
                        .clicked()
                    {
                        new_show_faces = !v.show_faces;
                    }
                    if f.kind != "video"
                        && word_button(ui, Icon::OpenInNew, t(lang, "In the system viewer"))
                            .clicked()
                    {
                        open_video = true;
                    }
                    if ui
                        .add(WordButton::picked(
                            Icon::Info,
                            t(lang, "Inspector"),
                            v.inspector,
                        ))
                        .on_hover_text(t(lang, "the file card next to the frame — I"))
                        .clicked()
                    {
                        new_inspector = !v.inspector;
                    }
                });
            // The chrome may take two rows on a narrow window: the body follows what it used up,
            // measured from the cursor, so nothing is clipped (M22.7).
            let chrome_h = ui.cursor().top() - chrome_top;
            ui.add_space(4.0);
            let body_h = (min_size.y - chrome_h - 4.0).max(120.0);
            let frame_h = (body_h - VIEWER_PLAYER_H - strip_h).max(120.0);
            // 1:1 — one image pixel per screen pixel: the frame is fitted at `fit`, so the scale
            // that shows it as it is stored is `1 / fit` (M22.7).
            if want_one_to_one
                && let Some((_, t)) = full_tex.as_ref()
            {
                new_zoom = one_to_one_zoom(
                    rotated_size(t.size_vec2(), v.rotation),
                    egui::vec2(left_w, frame_h),
                );
            }
            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(left_w, body_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let mut image_resp = None;
                        if v.show_text {
                            ui.horizontal(|ui| {
                                let img_w = (left_w * 0.62).max(160.0);
                                ui.allocate_ui(egui::vec2(img_w, frame_h), |ui| {
                                    let (r, face) = draw_viewer_image(
                                        ui,
                                        ViewerFrame {
                                            textures: ViewerTextures {
                                                full: full_tex.as_ref(),
                                                preview: preview_tex.as_ref(),
                                                stale: stale_tex.as_ref(),
                                                video: video_frame.as_ref(),
                                                paused,
                                            },
                                            rot: v.rotation,
                                            faces: shown_faces,
                                        },
                                        &f,
                                        v.zoom,
                                        &mut new_pan,
                                        egui::vec2(img_w, frame_h),
                                        lang,
                                    );
                                    image_resp = Some(r);
                                    if face.is_some() {
                                        face_click = face;
                                    }
                                });
                                ui.separator();
                                egui::ScrollArea::vertical()
                                    .id_salt("viewer-text")
                                    .max_height(frame_h)
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        ui.label(
                                            egui::RichText::new(t(lang, "Text on the photo"))
                                                .size(11.0)
                                                .color(dim(ui)),
                                        );
                                        let text =
                                            self.ocr_text.get(&id).cloned().unwrap_or_default();
                                        if text.trim().is_empty() {
                                            ui.label(
                                                egui::RichText::new(t(
                                                    lang,
                                                    "No text recognized. The “Recognize text” button is in the file card.",
                                                ))
                                                .size(12.0)
                                                .color(dim(ui)),
                                            );
                                        } else {
                                            ui.add(egui::Label::new(text).wrap());
                                        }
                                    });
                            });
                        } else {
                            let (r, face) = draw_viewer_image(
                                ui,
                                ViewerFrame {
                                    textures: ViewerTextures {
                                        full: full_tex.as_ref(),
                                        preview: preview_tex.as_ref(),
                                        stale: stale_tex.as_ref(),
                                        video: video_frame.as_ref(),
                                        paused,
                                    },
                                    rot: v.rotation,
                                    faces: shown_faces,
                                },
                                &f,
                                v.zoom,
                                &mut new_pan,
                                egui::vec2(left_w, frame_h),
                                lang,
                            );
                            image_resp = Some(r);
                            if face.is_some() {
                                face_click = face;
                            }
                        }
                        // A click on the video frame is like a big ▶ button: play or pause.
                        if f.kind == "video" && image_resp.as_ref().is_some_and(|r| r.clicked()) {
                            if video_active {
                                toggle_video = true;
                            } else {
                                start_video = true;
                            }
                        }
                        image_clicked = image_resp.as_ref().is_some_and(|r| r.clicked());
                        // A click on a face opens its photos in the right panel; a click on the
                        // empty part of the frame closes the panel (M22.28).
                        if let Some(fid) = face_click.take() {
                            if let Some(face) = shown_faces.iter().find(|b| b.id == fid) {
                                new_focus = Some(self.face_focus_for(face, id));
                            }
                        } else if image_clicked && f.kind != "video" {
                            new_focus = None;
                        }
                        // The playback bar: paging, play and pause, seeking, the volume.
                        ui.horizontal_wrapped(|ui| {
                            if icon_button(ui, Icon::Prev, 32.0, t(lang, "previous file — ←"))
                                .clicked()
                            {
                                step_click = -1;
                            }
                            let playing =
                                video_active && self.video.as_ref().is_some_and(|p| p.is_playing());
                            if f.kind == "video" {
                                if video_active && self.video.is_none() {
                                    ui.label(
                                        egui::RichText::new(t(lang, "ffplay in its own window"))
                                            .size(11.0)
                                            .color(dim(ui)),
                                    );
                                } else if icon_button(
                                    ui,
                                    if playing { Icon::Pause } else { Icon::Play },
                                    32.0,
                                    if video_active {
                                        t(lang, "pause and resume — space")
                                    } else {
                                        t(lang, "play the video — space")
                                    },
                                )
                                .clicked()
                                {
                                    if video_active {
                                        toggle_video = true;
                                    } else {
                                        start_video = true;
                                    }
                                }
                            } else if icon_button(
                                ui,
                                if v.slideshow { Icon::Pause } else { Icon::Play },
                                32.0,
                                t(lang, "slideshow — space"),
                            )
                            .clicked()
                            {
                                new_slideshow = !v.slideshow;
                            }
                            if icon_button(ui, Icon::Next, 32.0, t(lang, "next file — →")).clicked()
                            {
                                step_click = 1;
                            }
                            if f.kind == "video" {
                                if video_active && let Some(p) = self.video.as_ref() {
                                    let dur = p.duration_s().max(0.001);
                                    let mut pos = p.position_s().min(dur);
                                    ui.label(
                                        num(
                                            ui,
                                            format!(
                                                "{} / {}",
                                                db::duration(pos),
                                                db::duration(dur)
                                            ),
                                            11.0,
                                        )
                                        .color(dim(ui)),
                                    );
                                    let w = (ui.available_width() - 220.0).max(160.0);
                                    let resp = ui.add_sized(
                                        [w, 18.0],
                                        egui::Slider::new(&mut pos, 0.0..=dur)
                                            .show_value(false)
                                            .trailing_fill(true),
                                    );
                                    if resp.dragged() || resp.clicked() {
                                        seek_video = Some(pos);
                                    }
                                    icon_mark(ui, Icon::Volume, 18.0);
                                    let mut vol = p.volume();
                                    if ui
                                        .add_sized(
                                            [90.0, 18.0],
                                            egui::Slider::new(&mut vol, 0.0..=1.0)
                                                .show_value(false),
                                        )
                                        .changed()
                                    {
                                        volume_video = Some(vol);
                                    }
                                    if !p.has_audio() {
                                        ui.label(
                                            egui::RichText::new(t(lang, "no device"))
                                                .size(11.0)
                                                .color(dim(ui)),
                                        );
                                    }
                                }
                                if video_active
                                    && ui
                                        .add(WordButton::picked(
                                            Icon::Stop,
                                            t(lang, "Stop the video"),
                                            true,
                                        ))
                                        .on_hover_text(t(
                                            lang,
                                            "stop and go back to the poster frame",
                                        ))
                                        .clicked()
                                {
                                    stop_video = true;
                                }
                            } else {
                                if ui
                                    .add(WordButton::picked(
                                        Icon::Slideshow,
                                        if v.slideshow {
                                            t(lang, "Stop the slideshow")
                                        } else {
                                            t(lang, "Slideshow")
                                        },
                                        v.slideshow,
                                    ))
                                    .on_hover_text(t(
                                        lang,
                                        "space — start/stop; walks the current list",
                                    ))
                                    .clicked()
                                {
                                    new_slideshow = !v.slideshow;
                                }
                                ui.label(
                                    egui::RichText::new(t(lang, "interval:")).size(11.0).color(dim(ui)),
                                );
                                for secs in [3u64, 5, 10] {
                                    if ui
                                        .selectable_label(
                                            v.interval_s == secs,
                                            i18n::seconds(lang, secs),
                                        )
                                        .clicked()
                                    {
                                        new_interval = secs;
                                    }
                                }
                            }
                        });
                        // The series of the day: the neighbours with their position in it.
                        if strip_h > 0.0 {
                            goto = draw_series_strip(
                                ui,
                                v.day.as_deref(),
                                &v.series,
                                v.idx,
                                &self.textures,
                                lang,
                            );
                        }
                    },
                );
                if insp_w > 0.0 {
                    ui.add_space(8.0);
                    ui.vertical(|ui| {
                        ui.set_width(RIGHT_W);
                        if let Some(focus) = &new_focus {
                            let (open, close_panel) = self.viewer_face_panel(ui, focus, id);
                            panel_open = open;
                            panel_close = close_panel;
                        } else if v.inspector {
                            panel_edge(ui, true);
                            ui.add_space(4.0);
                            egui::ScrollArea::vertical()
                                .id_salt("viewer-card")
                                .auto_shrink([false, false])
                                .show(ui, |ui| self.file_card(ui, Some(id)));
                        }
                    });
                }
            });
        });
        if resp.backdrop_response.clicked() {
            close = true;
        }
        v.zoom = new_zoom;
        v.pan = new_pan;
        v.show_text = new_text;
        v.rotation = new_rotation;
        v.inspector = new_inspector;
        if v.slideshow != new_slideshow {
            v.last_advance = std::time::Instant::now();
        }
        v.slideshow = new_slideshow;
        v.interval_s = new_interval;
        v.show_faces = new_show_faces;
        if !v.show_faces || panel_close {
            new_focus = None;
        }
        v.face_focus = new_focus;
        if open_video {
            match tools::open_path(Path::new(&f.path)) {
                Ok(()) => {
                    self.status = format!(
                        "{}: {}",
                        t(lang, "opening in the system player"),
                        f.path
                    )
                }
                Err(e) => self.status = format!("{}: {e:#}", t(lang, "cannot open")),
            }
        }
        if let Some(p) = self.video.as_ref() {
            if toggle_video {
                p.toggle();
            }
            if let Some(t) = seek_video {
                p.seek(t);
            }
            if let Some(v) = volume_video {
                p.set_volume(v);
            }
        }
        if stop_video {
            self.stop_video();
        }
        if start_video {
            self.start_video_playback(&f);
        }
        // The ←/→ buttons page after the drawing: the frame of the new file comes next frame.
        if step_click != 0 {
            self.viewer_step(&mut v, step_click);
            ctx.request_repaint();
        }
        // A click in the series strip opens that file at once.
        if let Some(idx) = goto {
            self.viewer_goto(&mut v, idx);
            ctx.request_repaint();
        }
        // A photo picked in the face panel opens in place when the walk holds it, otherwise the
        // viewer is rebuilt around it (M22.28).
        if let Some(fid) = panel_open {
            match v.ids.iter().position(|&x| x == fid) {
                Some(idx) => {
                    self.viewer_goto(&mut v, idx);
                    ctx.request_repaint();
                }
                None => reopen = Some(fid),
            }
        }
        if let Some(fid) = reopen {
            self.open_viewer(fid);
            return;
        }
        if close {
            self.stop_video();
            self.viewer = None;
            self.viewer_tex = None;
            self.viewer_faces = None;
        } else {
            self.request_around(&v.ids, v.idx);
            self.viewer = Some(v);
        }
    }

    /// The right panel of the full screen: every photo that carries the picked face, oldest
    /// first (M22.28). A click on a row opens that photo; the "×" in the band closes the panel.
    /// Returns the photo picked in the list and whether the band asked to close.
    fn viewer_face_panel(
        &mut self,
        ui: &mut egui::Ui,
        focus: &FaceFocus,
        current: i64,
    ) -> (Option<i64>, bool) {
        let lang = self.settings.lang;
        let th = theme(ui);
        panel_edge(ui, true);
        let w = ui.available_width();
        let (band, _) = ui.allocate_exact_size(egui::vec2(w, PANEL_HEAD_H), egui::Sense::hover());
        ui.painter().rect_filled(band, 0.0, th.card);
        ui.painter().hline(
            band.x_range(),
            band.bottom() - 0.5,
            egui::Stroke::new(1.0, th.border),
        );
        let mut close = false;
        ui.scope_builder(
            egui::UiBuilder::new().max_rect(band.shrink2(egui::vec2(PANEL_PAD, 0.0))),
            |ui| {
                ui.horizontal_centered(|ui| {
                    icon_mark(ui, Icon::Face, 14.0);
                    let name = if focus.label.is_empty() {
                        t(lang, "face")
                    } else {
                        focus.label.as_str()
                    };
                    ui.label(egui::RichText::new(name).size(12.0).strong());
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if icon_button(ui, Icon::Close, 22.0, t(lang, "close the face panel"))
                            .clicked()
                        {
                            close = true;
                        }
                    });
                });
            },
        );
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        let open = egui::Frame::NONE
            .inner_margin(egui::Margin::same(PANEL_PAD as i8))
            .show(ui, |ui| {
                ui.label(
                    num(ui, i18n::count(lang, focus.file_ids.len(), "photo"), 11.0)
                        .color(dim(ui)),
                );
                if focus.cluster_id.is_none() {
                    ui.label(
                        egui::RichText::new(t(lang, "This face is not in a group yet."))
                            .size(11.0)
                            .color(dim(ui)),
                    );
                }
                ui.add_space(6.0);
                let rows: Vec<db::FileRow> = focus
                    .file_ids
                    .iter()
                    .filter_map(|id| {
                        self.items
                            .iter()
                            .chain(self.trash_items.iter())
                            .find(|r| r.id == *id)
                            .cloned()
                    })
                    .collect();
                let mut open = None;
                egui::ScrollArea::vertical()
                    .id_salt("viewer-face-photos")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for f in &rows {
                            let (rect, resp) = ui.allocate_exact_size(
                                egui::vec2(ui.available_width(), 56.0),
                                egui::Sense::click(),
                            );
                            ui.painter().rect_filled(rect, 4.0, th.card);
                            let p = ui.painter().with_clip_rect(rect.shrink(1.0));
                            let thumb = egui::Rect::from_min_size(
                                rect.min + egui::vec2(4.0, 4.0),
                                egui::vec2(64.0, 48.0),
                            );
                            if !self.textures.contains_key(&f.id)
                                && !self.missing.contains(&f.id)
                            {
                                let loaded = f.thumb.as_ref().and_then(|rel| {
                                    load_texture(
                                        ui.ctx(),
                                        &self.thumbs_dir.join(rel),
                                        &format!("t{}", f.id),
                                    )
                                });
                                match loaded {
                                    Some(t) => {
                                        self.textures.insert(f.id, t);
                                    }
                                    None => {
                                        self.missing.insert(f.id);
                                    }
                                }
                            }
                            if let Some(t) = self.textures.get(&f.id) {
                                p.image(
                                    t.id(),
                                    fit_rect(thumb, t.size_vec2()),
                                    uv,
                                    egui::Color32::WHITE,
                                );
                                // The tone under the cut corners is the card the row lies on.
                                round_preview(ui, thumb, 4.0, th.card, false);
                            }
                            let text_x = thumb.right() + 8.0;
                            let name = file_name(&f.path);
                            p.text(
                                egui::pos2(text_x, rect.top() + 6.0),
                                egui::Align2::LEFT_TOP,
                                name.clone(),
                                egui::FontId::proportional(12.0),
                                th.text_med,
                            );
                            let date = f
                                .taken_at
                                .as_deref()
                                .map(|d| d.chars().take(10).collect::<String>())
                                .unwrap_or_else(|| t(lang, "no date").to_string());
                            p.text(
                                egui::pos2(text_x, rect.top() + 24.0),
                                egui::Align2::LEFT_TOP,
                                date,
                                egui::FontId::monospace(10.0),
                                th.text_dim,
                            );
                            p.text(
                                egui::pos2(text_x, rect.top() + 38.0),
                                egui::Align2::LEFT_TOP,
                                db::human_in(lang, f.size),
                                egui::FontId::monospace(10.0),
                                th.text_dim,
                            );
                            if f.id == current {
                                ui.painter().rect_stroke(
                                    rect,
                                    4.0,
                                    egui::Stroke::new(1.5, accent(ui)),
                                    egui::StrokeKind::Inside,
                                );
                            }
                            let resp = resp
                                .on_hover_cursor(egui::CursorIcon::PointingHand)
                                .on_hover_text(name);
                            if resp.clicked() {
                                open = Some(f.id);
                            }
                        }
                    });
                open
            })
            .inner;
        (open, close)
    }

    /// One pass over the programs: the version, the file found, the source. Run on reload and on save.
    fn refresh_tool_states(&mut self) {
        let lang = self.settings.lang;
        self.tool_states = tools::ALL.iter().map(|&tool| {
            let where_ = match self.settings.tools.locate(tool) {
                Some(p) => format!(
                    "{} ({})",
                    p.display(),
                    self.settings.tools.source(tool, lang)
                ),
                None => format!(
                    "{} ({})",
                    self.settings.tools.program(tool),
                    self.settings.tools.source(tool, lang)
                ),
            };
            match self.settings.tools.version(tool) {
                Ok(v) => format!(
                    "{} · {where_} · {}",
                    short_version(&v),
                    tool.purpose(lang)
                ),
                Err(e) => format!(
                    "{}: {e:#} · {where_} · {}",
                    t(lang, "does not work"),
                    tool.purpose(lang)
                ),
            }
        }).collect();
    }

    /// The path to a program is saved only after a successful version launch.
    /// The error stays in the status bar (at the bottom of the window), not in the console.
    fn apply_tools(&mut self) {
        let lang = self.settings.lang;
        let mut problems = Vec::new();
        let mut checked = Vec::new();
        for tool in tools::ALL {
            if !self.tools_input.configured(tool) {
                continue;
            }
            match self.tools_input.version(tool) {
                Ok(v) => checked.push(format!("{} {}", tool.name(), short_version(&v))),
                Err(e) => problems.push(format!("{e:#}")),
            }
        }
        if !problems.is_empty() {
            self.status = format!(
                "{}: {}",
                t(lang, "programs not saved"),
                problems.join("; ")
            );
            return;
        }
        self.settings.tools = self.tools_input.clone();
        self.save_settings();
        self.tesseract = self.settings.tools.probe(Tool::Tesseract);
        self.refresh_tool_states();
        self.status = if checked.is_empty() {
            t(lang, "programs: all taken from PATH").into()
        } else {
            format!(
                "{}: {}",
                t(lang, "programs saved and checked"),
                checked.join(", ")
            )
        };
    }

    fn start_download(&mut self) {
        let lang = self.settings.lang;
        if self.download_rx.is_some() || !self.download_consent {
            return;
        }
        let settings = self.settings.clone();
        let requested = settings.model_id.clone();
        let progress = Arc::new(Progress::default());
        self.download_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.download_rx = Some(rx);
        self.status = t(lang, "downloading a model: weights only, no user data").into();
        std::thread::spawn(move || {
            let r = embed::download_model(&settings, &requested, true, Some(progress));
            let _ = tx.send(r);
        });
    }

    /// The only place where the tile order is decided (M17).
    /// In a semantic search the search sets the order, otherwise `settings.grid_sort` does.
    /// The key is total and stable: the date (or the name or size), then `id`, so equal dates
    /// are not reshuffled between frames. The grid, the full screen and the selection (M18)
    /// take their order only from here.
    fn ordered(&self, visible: &[usize]) -> Vec<usize> {
        let mut out = visible.to_vec();
        if self.semantic_hits.is_some() {
            // `filtered()` has already given the indices in descending similarity.
            return out;
        }
        let items = &self.items;
        match self.settings.grid_sort {
            db::GridSort::DateDesc => out.sort_by(|&a, &b| {
                let (ra, rb) = (&items[a], &items[b]);
                cmp_date_none_last(ra.taken_at.as_deref(), rb.taken_at.as_deref(), true)
                    .then(ra.id.cmp(&rb.id))
            }),
            db::GridSort::DateAsc => out.sort_by(|&a, &b| {
                let (ra, rb) = (&items[a], &items[b]);
                cmp_date_none_last(ra.taken_at.as_deref(), rb.taken_at.as_deref(), false)
                    .then(ra.id.cmp(&rb.id))
            }),
            db::GridSort::Name => out.sort_by(|&a, &b| {
                let (ra, rb) = (&items[a], &items[b]);
                file_name(&ra.path)
                    .to_lowercase()
                    .cmp(&file_name(&rb.path).to_lowercase())
                    .then(ra.path.cmp(&rb.path))
                    .then(ra.id.cmp(&rb.id))
            }),
            db::GridSort::SizeDesc => out.sort_by(|&a, &b| {
                let (ra, rb) = (&items[a], &items[b]);
                rb.size.cmp(&ra.size).then(ra.id.cmp(&rb.id))
            }),
            db::GridSort::Album => {
                // Album order is data (M19). Outside an open album or in the temporary "All files"
                // look it reads as the date ↓.
                if self.view == View::Albums
                    && self.album_selected.is_some()
                    && !self.album_browse_all
                {
                    let rank: HashMap<i64, usize> = self
                        .album_files
                        .iter()
                        .enumerate()
                        .map(|(n, id)| (*id, n))
                        .collect();
                    out.sort_by_key(|&i| {
                        (rank.get(&items[i].id).copied().unwrap_or(usize::MAX), items[i].id)
                    });
                } else {
                    out.sort_by(|&a, &b| {
                        let (ra, rb) = (&items[a], &items[b]);
                        cmp_date_none_last(
                            ra.taken_at.as_deref(),
                            rb.taken_at.as_deref(),
                            true,
                        )
                        .then(ra.id.cmp(&rb.id))
                    });
                }
            }
        }
        out
    }

    /// The current set of files the selection lives in (M18).
    fn pick_scope_now(&self) -> PickScope {
        PickScope {
            view: self.view,
            query: self.query.trim().to_lowercase(),
            kind: self.kind_filter,
            tags: self.tag_filters.clone(),
            year: self.year_filter.clone(),
            no_date: self.only_no_date,
            no_preview: self.only_no_preview,
            hits: self.semantic_hits.as_ref().map(|h| h.len()),
            sort: self.settings.grid_sort,
            layout: self.settings.grid_layout,
            album: self.album_selected,
            browse_all: self.album_browse_all,
        }
    }

    /// A change of the view, filter, query, search or order clears the selection: an action
    /// must not touch files that are no longer on the screen. The status bar says why (M18).
    fn sync_pick_scope(&mut self) {
        let lang = self.settings.lang;
        let now = self.pick_scope_now();
        if now == self.pick_scope {
            return;
        }
        let reason = if now.view != self.pick_scope.view {
            t(lang, "the view changed")
        } else if now.query != self.pick_scope.query {
            t(lang, "the folder and date filter changed")
        } else if now.kind != self.pick_scope.kind {
            t(lang, "the photo and video filter changed")
        } else if now.tags != self.pick_scope.tags {
            t(lang, "the tag filter changed")
        } else if now.year != self.pick_scope.year {
            t(lang, "the year filter changed")
        } else if now.no_date != self.pick_scope.no_date
            || now.no_preview != self.pick_scope.no_preview
        {
            t(lang, "the no-date and no-preview filters changed")
        } else if now.hits != self.pick_scope.hits {
            t(lang, "the search changed")
        } else if now.album != self.pick_scope.album {
            if now.album.is_none() {
                t(lang, "the album was closed")
            } else {
                t(lang, "another album was opened")
            }
        } else if now.browse_all != self.pick_scope.browse_all {
            t(lang, "the grid of the album changed")
        } else {
            t(lang, "the order changed")
        };
        self.pick_scope = now;
        self.clear_picked(&i18n::fill(lang, "the selection is cleared: {}", &[reason.to_string()]));
    }

    /// Clear the selection. The fact goes to the status bar only if there was something to
    /// clear: otherwise the action stays silent and does not overwrite someone else's message.
    fn clear_picked(&mut self, message: &str) {
        if self.picked.is_empty() {
            return;
        }
        self.picked.clear();
        self.pick_anchor = None;
        self.status = message.to_string();
    }

    /// Select one file: it is also the last one selected for the inspector.
    fn select_one(&mut self, id: i64) {
        self.selected = Some(id);
        self.picked.clear();
        self.picked.insert(id);
        self.pick_anchor = Some(id);
    }

    /// A click on a tile: a plain one selects one, Ctrl adds or removes, Shift takes a range
    /// in the current grid order (`order` — the ids in the order of `App::ordered`).
    fn pick_clicked(&mut self, id: i64, click: PickClick, order: &[i64]) {
        let lang = self.settings.lang;
        match click {
            PickClick::Plain => self.select_one(id),
            PickClick::Toggle => {
                if !self.picked.remove(&id) {
                    self.picked.insert(id);
                }
                self.pick_anchor = Some(id);
                if self.picked.is_empty() {
                    self.selected = None;
                    self.status = t(lang, "the selection is cleared").into();
                } else {
                    // A removed tile is no longer "the last selected": take one of the remaining.
                    self.selected = if self.picked.contains(&id) {
                        Some(id)
                    } else {
                        self.picked.iter().next_back().copied()
                    };
                }
            }
            PickClick::Range => {
                let anchor = self.pick_anchor.unwrap_or(id);
                match (
                    order.iter().position(|&x| x == anchor),
                    order.iter().position(|&x| x == id),
                ) {
                    (Some(a), Some(b)) => {
                        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
                        self.picked = order[lo..=hi].iter().copied().collect();
                        self.selected = Some(id);
                        // The anchor is not moved: the next Shift+click continues from it.
                    }
                    // The anchor is not in the current set (the filter changed) — a plain click.
                    _ => self.select_one(id),
                }
            }
        }
    }

    /// "Select all": only what is visible now (filter, search and order come from `ordered`).
    fn pick_all(&mut self) {
        let order: Vec<i64> = self
            .ordered(&self.filtered())
            .iter()
            .map(|&i| self.items[i].id)
            .collect();
        let Some(first) = order.first().copied() else {
            return;
        };
        let bytes: i64 = self
            .items
            .iter()
            .filter(|r| order.contains(&r.id))
            .map(|r| r.size)
            .sum();
        self.picked = order.iter().copied().collect();
        self.pick_anchor = Some(first);
        if !self.selected.is_some_and(|id| self.picked.contains(&id)) {
            self.selected = Some(first);
        }
        let lang = self.settings.lang;
        self.status = i18n::fill(
            lang,
            "{} selected · {}",
            &[
                self.picked.len().to_string(),
                db::human_in(self.settings.lang, bytes),
            ],
        );
    }

    /// Esc clears the selection, Ctrl+A takes what was found — only in the grid, when the
    /// keyboard is not taken by an input field and no dialog or full screen is open.
    fn pick_keys(&mut self, ctx: &egui::Context) {
        let lang = self.settings.lang;
        let modal = self.viewer.is_some()
            || self.settings_open
            || self.roots_open
            || self.picker_open
            || self.storage_confirm.is_some()
            || self.trash_confirm;
        let grid_active = self.view == View::Grid
            || (self.view == View::Albums && self.album_selected.is_some());
        if modal || !grid_active || ctx.text_edit_focused() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.clear_picked(t(lang, "the selection is cleared"));
        }
        if ctx.input(|i| i.modifiers.command && i.key_pressed(egui::Key::A)) {
            self.pick_all();
        }
    }

    /// How the file grid is shown. On the Albums screen the "Cards / List" switch of the album
    /// control row drives the photos too, so one switch changes the album cards and its snapshots
    /// (M22.23); everywhere else the cell view of the ribbon decides.
    fn grid_cell_style(&self) -> db::CellStyle {
        if self.view == View::Albums {
            if self.settings.album_list {
                db::CellStyle::List
            } else {
                db::CellStyle::Tiles
            }
        } else {
            self.settings.cell_style
        }
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        let album_scope = self.view == View::Albums
            && self.album_selected.is_some()
            && !self.album_browse_all;
        let mut out: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, r)| !album_scope || self.album_set.contains(&r.id))
            .filter(|(_, r)| match self.kind_filter {
                KindFilter::All => true,
                KindFilter::Images => r.kind == "image",
                KindFilter::Videos => r.kind == "video",
            })
            .filter(|(_, r)| {
                q.is_empty()
                    || r.path.to_lowercase().contains(&q)
                    || r.taken_at.clone().unwrap_or_default().contains(&q)
            })
            .filter(|(_, r)| match &self.year_filter {
                None => true,
                Some(pick) => pick.matches(r.taken_at.as_deref()),
            })
            .filter(|(_, r)| !self.only_no_date || r.taken_at.is_none())
            .filter(|(_, r)| !self.only_no_preview || self.has_no_preview(r))
            .filter(|(_, r)| {
                self.tag_filters.iter().all(|want| {
                    self.user_tags
                        .get(&r.id)
                        .is_some_and(|tags| tags.iter().any(|t| t == want))
                        || self.tags.get(&r.id).is_some_and(|t| {
                            embed::tag_above(t, want, self.tag_threshold(want))
                        })
                })
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(hits) = &self.semantic_hits {
            let rank: HashMap<i64, usize> =
                hits.iter().enumerate().map(|(n, id)| (*id, n)).collect();
            out.retain(|&i| rank.contains_key(&self.items[i].id));
            out.sort_by_key(|&i| rank.get(&self.items[i].id).copied().unwrap_or(usize::MAX));
        }
        out
    }

    /// Whether this file has no preview (M25.4): either the index never built one, or the preview
    /// it names is no longer on disk.
    fn has_no_preview(&self, r: &db::FileRow) -> bool {
        r.thumb.is_none() || self.previews_missing.contains(&r.id)
    }

    /// The files whose preview is recorded but missing on disk. Read once per `reload`, so the
    /// filter does not stat the cache on every frame (M25.4).
    fn refresh_missing_previews(&mut self) {
        let thumbs = self.thumbs_dir.clone();
        self.previews_missing = self
            .items
            .iter()
            .filter(|r| {
                r.thumb
                    .as_deref()
                    .is_some_and(|rel| !thumbs.join(rel).exists())
            })
            .map(|r| r.id)
            .collect();
    }

    /// Whether the tag filter holds this tag (M25.1).
    fn tag_picked(&self, tag: &str) -> bool {
        self.tag_filters.iter().any(|t| t == tag)
    }

    /// A click on a tag: it joins the filter, a second click removes it. The set stays sorted, so
    /// the chips row and the panel draw the picked tags in one order (M25.1).
    fn toggle_tag(&mut self, tag: &str) {
        match self.tag_filters.iter().position(|t| t == tag) {
            Some(i) => {
                self.tag_filters.remove(i);
            }
            None => {
                self.tag_filters.push(tag.to_string());
                self.tag_filters.sort();
                self.tag_filters.dedup();
            }
        }
    }

    fn clear_tag_filters(&mut self) {
        self.tag_filters.clear();
    }

    /// The "Tags" heading of the cloud and the reset beside it (M25.1): the bare 20 px target
    /// stands where the filter is set and is drawn only while something is picked — the folder
    /// block's "+" pattern, so an empty filter spends no room on a reset.
    fn tags_heading(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let mut reset = false;
        ui.horizontal(|ui| {
            caps_label(ui, t(lang, "Tags"));
            if !self.tag_filters.is_empty() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if icon_button(ui, Icon::Close, 20.0, t(lang, "reset the tag filter")).clicked()
                    {
                        reset = true;
                    }
                });
            }
        });
        if reset {
            self.clear_tag_filters();
        }
    }

    /// The tags as chips (M22.3): the word with the count of photos after a middle dot, the
    /// picked one in the accent, a click adds the tag to the filter and a second click removes it
    /// (M25.1). The dictionary does not need a menu; the count next to a word honestly says
    /// "zero" before a click. Returns the counts, so the model block can warn that no tag passes
    /// the threshold.
    fn tag_chips(&mut self, ui: &mut egui::Ui) -> Vec<usize> {
        let lang = self.settings.lang;
        let labels = self.tag_labels();
        let counts = self.tag_counts();
        let mut own_tags: Vec<String> = self.user_tags.values().flatten().cloned().collect();
        own_tags.sort();
        own_tags.dedup();
        let mut clicked: Vec<String> = Vec::new();
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            for (label, n) in labels.iter().zip(&counts) {
                let chosen = self.tag_filters.iter().any(|t| t == label);
                let text = format!("{} · {n}", i18n::word(lang, label));
                if tag_chip(ui, &text, chosen) {
                    clicked.push(label.clone());
                }
            }
            for label in &own_tags {
                let chosen = self.tag_filters.iter().any(|t| t == label);
                let n = self
                    .user_tags
                    .values()
                    .filter(|tags| tags.iter().any(|t| t == label))
                    .count();
                let text = format!("{}: {label} · {n}", t(lang, "mine"));
                if tag_chip(ui, &text, chosen) {
                    clicked.push(label.clone());
                }
            }
        });
        for tag in clicked {
            self.toggle_tag(&tag);
        }
        counts
    }

    /// The cut-off threshold in the filter is a percentile over the chosen tag (ADR 0032).
    /// 100% is the strongest photo of this tag, 0% all of them, 70% the top 30%.
    fn tag_filter_slider(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        if self.tags.is_empty() {
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "tags are not computed yet — press “Compute tags”",
                ))
                .size(11.0)
                .color(dim(ui)),
            );
            return;
        }
        let mut pct = self.settings.tag_show_pct;
        let r = ui
            .add(
                egui::Slider::new(&mut pct, 0..=100)
                    .custom_formatter(|v, _| format!("{v:.0}%")),
            )
            .on_hover_text(t(
                lang,
                "100% — the strongest photo of this tag, 0% — all; 70% — the top 30%",
            ));
        if r.changed() {
            self.settings.tag_show_pct = pct;
        }
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        if self.tags.len() < 100 {
            let few = i18n::fill(lang, "few tags ({} files) — the percentile is noisy, the raw threshold {} is used", &[self.tags.len().to_string(), format!("{:.2}", self.settings.tag_show)]);
            ui.label(egui::RichText::new(few).size(11.0).color(dim(ui)));
        }
    }

    /// The raw threshold for a tag at the current percentile. On a small folder the
    /// percentile is noisy, so the raw confident-tag threshold applies there.
    /// The dictionary may set a minimum percentile for a single tag (M15.4).
    fn tag_threshold(&self, tag: &str) -> f32 {
        if self.tags.len() < 100 {
            return self.settings.tag_show;
        }
        let pct = self
            .tag_defs
            .iter()
            .find(|d| d.label == tag)
            .map(|d| self.settings.tag_show_pct.max(d.min_percentile))
            .unwrap_or(self.settings.tag_show_pct);
        self.tag_dists
            .get(tag)
            .and_then(|d| embed::percentile_threshold(d, pct))
            .unwrap_or(self.settings.tag_show)
    }

    /// The percentile of a tag weight in the index: 100 is the strongest photo of this tag.
    /// Not shown on a small folder (it is noisy there), only the weight remains.
    fn tag_percentile(&self, tag: &str, score: f32) -> Option<u32> {
        if self.tags.len() < 100 {
            return None;
        }
        let d = self.tag_dists.get(tag)?;
        score_percentile(d, score)
    }

    /// The labels for the filter: the enabled tags of the dictionary in order, then the old
    /// labels that are no longer in the dictionary — so computed tags do not disappear.
    fn tag_labels(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .tag_defs
            .iter()
            .filter(|d| d.enabled)
            .map(|d| d.label.clone())
            .collect();
        for scores in self.tags.values() {
            for (tag, _) in scores {
                if !out.iter().any(|l| l == tag) {
                    out.push(tag.clone());
                }
            }
        }
        out
    }

    /// How many photos per tag of the dictionary at its percentile threshold: the number next
    /// to a tag honestly says "zero" instead of surprising after a click.
    fn tag_counts(&self) -> Vec<usize> {
        let labels = self.tag_labels();
        let mut counts = vec![0usize; labels.len()];
        for scores in self.tags.values() {
            for (tag, score) in scores {
                if *score < self.tag_threshold(tag) {
                    continue;
                }
                if let Some(i) = labels.iter().position(|l| l == tag) {
                    counts[i] += 1;
                }
            }
        }
        counts
    }

    fn start_embed(&mut self) {
        let lang = self.settings.lang;
        if self.embed_rx.is_some() {
            return;
        }
        let db_path = self.db_path.clone();
        let progress = Arc::new(Progress::default());
        self.embed_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.embed_rx = Some(rx);
        self.status = t(lang, "CLIP: computing the embeddings and the tags…").into();
        std::thread::spawn(move || {
            let r = embed::embed_and_retag(&db_path, 8, 8, Some(progress));
            let _ = tx.send(r);
        });
    }



    /// `Some(id)` — this photo only (the main action), `None` — a batch pass over the candidates.
    fn start_ocr(&mut self, file: Option<i64>) {
        let lang = self.settings.lang;
        if self.ocr_rx.is_some() || self.tesseract.is_none() {
            return;
        }
        let db_path = self.db_path.clone();
        let progress = Arc::new(Progress::default());
        self.ocr_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.ocr_rx = Some(rx);
        self.status = match file {
            Some(id) => {
                let name = self
                    .items
                    .iter()
                    .find(|r| r.id == id)
                    .and_then(|r| {
                        Path::new(&r.path)
                            .file_name()
                            .and_then(|s| s.to_str())
                            .map(str::to_string)
                    })
                    .unwrap_or_else(|| t(lang, "the selected photo").into());
                i18n::fill(lang, "OCR: recognizing the text of {}…", &[name])
            }
            None => i18n::fill(lang, "OCR: a batch pass over {} candidates…", &[self.ocr_candidates.to_string()]),
        };
        std::thread::spawn(move || {
            let r = match file {
                Some(id) => ocr::run_files(&db_path, &[id], Some(progress)),
                None => {
                    let jobs = std::thread::available_parallelism()
                        .map(|n| n.get())
                        .unwrap_or(4)
                        .clamp(2, 8);
                    ocr::run(&db_path, jobs, false, Some(progress))
                }
            };
            let _ = tx.send(r);
        });
    }

    fn start_search(&mut self) {
        let lang = self.settings.lang;
        let q = self.semantic_query.trim().to_string();
        if q.is_empty() {
            self.semantic_hits = None;
            self.ocr_snippets.clear();
            return;
        }
        self.ocr_snippets.clear();
        if self.search_ocr {
            let t = std::time::Instant::now();
            let res = db::open(&self.db_path).and_then(|c| db::search_ocr(&c, &q, 200));
            match res {
                Ok(hits) => {
                    self.status = i18n::fill(lang, "text search “{}”: {} files in {} ms", &[q.to_string(), hits.len().to_string(), t.elapsed().as_millis().to_string()]);
                    self.ocr_snippets = hits.iter().cloned().collect();
                    self.semantic_hits = Some(hits.into_iter().map(|(id, _)| id).collect());
                }
                Err(e) => self.status = i18n::fill(lang, "the text search failed: {}", &[e.to_string()]),
            }
            return;
        }
        if !embed::model_present(&self.settings) {
            self.status = t(lang, "the word search is unavailable: the model is not downloaded (see the Settings)").into();
            return;
        }
        if let Some(model) = self.ml_model.as_mut() {
            let t = std::time::Instant::now();
            match embed::search(&self.db_path, model, &q, 200) {
                Ok(hits) => {
                    self.status = i18n::fill(lang, "word search “{}”: {} files in {} ms", &[q.to_string(), hits.len().to_string(), t.elapsed().as_millis().to_string()]);
                    self.semantic_hits = Some(hits.into_iter().map(|(id, _)| id).collect());
                }
                Err(e) => self.status = i18n::fill(lang, "the search failed: {}", &[e.to_string()]),
            }
            return;
        }
        if self.search_rx.is_some() {
            return;
        }
        let db_path = self.db_path.clone();
        let settings = self.settings.clone();
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        self.status = i18n::fill(lang, "CLIP: loading the model for the query “{}”…", std::slice::from_ref(&q));
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let res = embed::Model::load(&settings, 8).and_then(|mut model| {
                let hits = embed::search(&db_path, &mut model, &q, 200)?;
                let ms = t.elapsed().as_secs_f32() * 1000.0;
                Ok((model, hits, ms))
            });
            let _ = tx.send(res);
        });
    }

    fn start_faces(&mut self) {
        if self.face_rx.is_some() || !faces::model_present(&self.settings) {
            return;
        }
        let db_path = self.db_path.clone();
        let threshold = self.settings.face_threshold;
        let progress = Arc::new(Progress::default());
        self.face_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.face_rx = Some(rx);
        let lang = self.settings.lang;
        self.status = i18n::fill(
            lang,
            "faces: {} models, the threshold {}…",
            &[faces::MODEL_ID.to_string(), format!("{threshold:.2}")],
        );
        std::thread::spawn(move || {
            let jobs = std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(4)
                .clamp(2, 8);
            let r = faces::run(&db_path, jobs, threshold, Some(progress));
            let _ = tx.send(FaceReply::Scan(r));
        });
    }

    fn start_face_clusters(&mut self) {
        if self.face_rx.is_some() {
            return;
        }
        let db_path = self.db_path.clone();
        let threshold = self.settings.face_threshold;
        let (tx, rx) = mpsc::channel();
        self.face_rx = Some(rx);
        self.status = t(self.settings.lang, "people: rebuilding groups…").into();
        std::thread::spawn(move || {
            let r = faces::recluster(&db_path, threshold);
            let _ = tx.send(FaceReply::Cluster(r));
        });
    }

    fn start_face_download(&mut self) {
        if self.face_download_rx.is_some() || !self.face_download_consent {
            return;
        }
        let settings = self.settings.clone();
        let progress = Arc::new(Progress::default());
        self.face_download_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.face_download_rx = Some(rx);
        self.status = t(
            self.settings.lang,
            "downloading the face model: weights only, no photos",
        )
        .into();
        std::thread::spawn(move || {
            let r = faces::download_model(&settings, true, Some(progress));
            let _ = tx.send(r);
        });
    }

    fn start_index(&mut self) {
        let lang = self.settings.lang;
        let mut roots: Vec<PathBuf> = self.roots.iter().map(PathBuf::from).collect();
        let typed = self.path_input.trim();
        if !typed.is_empty() && !roots.iter().any(|r| r == Path::new(typed)) {
            roots.push(PathBuf::from(typed));
        }
        if roots.is_empty() {
            self.status =
                t(lang, "give a folder in the top bar or add one with the “Folders” button").into();
            return;
        }
        let missing: Vec<String> = roots
            .iter()
            .filter(|r| !r.is_dir())
            .map(|r| r.display().to_string())
            .collect();
        roots.retain(|r| r.is_dir());
        if roots.is_empty() {
            self.status = i18n::fill(lang, "not a folder: {}", &[missing.join(", ").to_string()]);
            return;
        }
        let db_path = self.db_path.clone();
        let tools = self.settings.tools.clone();
        let jobs = self.settings.index_jobs;
        let lang = self.settings.lang;
        let progress = Arc::new(Progress::default());
        self.progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.status = if roots.len() == 1 {
            i18n::fill(lang, "indexing {}…", &[roots[0].display().to_string()])
        } else {
            i18n::fill(lang, "indexing {} folders…", &[roots.len().to_string()])
        };
        if !missing.is_empty() {
            self.status.push_str(&i18n::fill(lang, " (skipped: {})", &[missing.join(", ").to_string()]));
        }
        std::thread::spawn(move || {
            let t0 = std::time::Instant::now();
            let mut total = 0usize;
            let mut indexed = 0usize;
            let mut skipped = 0usize;
            let mut errors = missing.len();
            let mut bytes = 0i64;
            let mut cancelled = false;
            for root in roots {
                progress.reset();
                match index::run(&root, jobs, &db_path, &tools, lang, Some(progress.clone())) {
                    Ok(st) => {
                        total += st.total;
                        indexed += st.indexed;
                        skipped += st.skipped;
                        errors += st.errors;
                        bytes += st.bytes;
                        cancelled |= st.cancelled;
                        if st.cancelled {
                            break;
                        }
                    }
                    Err(e) => {
                        errors += 1;
                        eprintln!(
                            "{}",
                            i18n::fill(
                                lang,
                                "indexing {}: {}",
                                &[root.display().to_string(), e.to_string()],
                            )
                        );
                    }
                }
            }
            let _ = tx.send(Ok(index::Stats {
                total,
                indexed,
                skipped,
                errors,
                bytes,
                seconds: t0.elapsed().as_secs_f64(),
                cancelled,
                lang,
            }));
        });
    }
}

/// A face tile: the same metric as a file (150×150 + a caption), but the frame is an aligned
/// face rather than a file preview. So it has its own texture loading, not `draw_cell`.
struct FaceCells {
    textures: HashMap<i64, egui::TextureHandle>,
    missing: HashSet<i64>,
    dir: PathBuf,
    selected_file: Option<i64>,
    split_clicked: Option<i64>,
    open_clicked: Option<i64>,
    viewer_clicked: Option<i64>,
    /// The selected faces: a name can be given to several at once.
    picked: HashSet<i64>,
    tile: f32,
}

fn draw_face_cell(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut FaceCells,
    f: &db::FaceView,
    lang: i18n::Lang,
    can_split: bool,
) {
    let w = cs.tile.clamp(100.0, 260.0);
    let th = theme(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w + 26.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, w));
    let mut button_clicked = false;
    let mut pointer_on_button = false;
    if ui.is_rect_visible(rect) {
        // A face tile is a card of the same palette as a file tile: the card tone, an 8 px radius
        // and a mono 10 caption (M22.9). The metrics stay 150×150 + 26 (docs/UI.md § Metrics).
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_CARD as u8), th.card);
        if !cs.textures.contains_key(&f.id) && !cs.missing.contains(&f.id) {
            match f
                .thumb
                .as_ref()
                .and_then(|rel| load_texture(ctx, &cs.dir.join(rel), &format!("face{}", f.id)))
            {
                Some(t) => {
                    cs.textures.insert(f.id, t);
                }
                None => {
                    cs.missing.insert(f.id);
                }
            }
        }
        if let Some(t) = cs.textures.get(&f.id) {
            ui.painter().image(
                t.id(),
                fit_rect(img_rect, t.size_vec2()),
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            round_preview(ui, img_rect, RADIUS_CARD, th.card, false);
        } else {
            ui.painter().text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                t(lang, "no frame"),
                egui::FontId::proportional(12.0),
                dim(ui),
            );
        }
        let date = f
            .taken_at
            .as_deref()
            .map(|d| d.chars().take(10).collect::<String>())
            .unwrap_or_else(|| t(lang, "no date").to_string());
        // The caption of a face tile: the date in mono 10 on the left, the detection score in
        // mono 10 `text_dim` on the right — the same caption band as a file tile.
        ui.painter().text(
            egui::pos2(rect.left() + 3.0, img_rect.bottom() + 3.0),
            egui::Align2::LEFT_TOP,
            date,
            egui::FontId::monospace(10.0),
            th.text_med,
        );
        ui.painter().text(
            egui::pos2(rect.right() - 3.0, img_rect.bottom() + 3.0),
            egui::Align2::RIGHT_TOP,
            format!("{:.2}", f.score),
            egui::FontId::monospace(10.0),
            th.text_dim,
        );
        // A false merge is split right on the tile, without going to another screen. The target
        // is relevant only inside one group: in "All people" there is nothing to split off
        // (M22.26).
        if can_split {
            let btn = egui::Rect::from_min_size(
                egui::pos2(img_rect.right() - 30.0, img_rect.top() + 6.0),
                egui::vec2(24.0, 24.0),
            );
            let r = tile_icon_target(
                ui,
                egui::Id::unique(("face-split", f.id)),
                btn,
                Icon::NotHim,
                false,
                t(lang, "split this face into a new group"),
            );
            pointer_on_button |= r.hovered();
            if r.clicked() {
                cs.split_clicked = Some(f.id);
                button_clicked = true;
            }
        }
        // A batch naming: mark several faces and give them one name. An empty circle before the
        // pick, the tick in the accent after it (M22.26).
        let pick = egui::Rect::from_min_size(
            egui::pos2(img_rect.left() + 6.0, img_rect.top() + 6.0),
            egui::vec2(24.0, 24.0),
        );
        let is_picked = cs.picked.contains(&f.id);
        let r = tile_icon_target(
            ui,
            egui::Id::unique(("face-pick", f.id)),
            pick,
            if is_picked {
                Icon::CheckCircle
            } else {
                Icon::Pick
            },
            is_picked,
            t(lang, "mark this face for a shared name"),
        );
        pointer_on_button |= r.hovered();
        if r.clicked() {
            if is_picked {
                cs.picked.remove(&f.id);
            } else {
                cs.picked.insert(f.id);
            }
            button_clicked = true;
        }
        if is_picked {
            ui.painter().rect_stroke(
                img_rect,
                egui::CornerRadius::same(RADIUS_CARD as u8),
                egui::Stroke::new(2.0, accent(ui)),
                egui::StrokeKind::Inside,
            );
        }
    }
    if cs.selected_file == Some(f.file_id) {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(RADIUS_CARD as u8),
            egui::Stroke::new(2.0, accent(ui)),
            egui::StrokeKind::Inside,
        );
    }
    if resp.clicked() && !button_clicked && !pointer_on_button {
        cs.open_clicked = Some(f.file_id);
    }
    if resp.double_clicked() {
        cs.viewer_clicked = Some(f.file_id);
    }
    if resp.hovered()
        && let Some(t) = cs.textures.get(&f.id).cloned()
    {
        resp.on_hover_ui_at_pointer(|ui| {
            let big = egui::load::SizedTexture::new(t.id(), t.size_vec2());
            ui.add(egui::Image::new(big).max_size(egui::vec2(300.0, 300.0)));
            ui.label(&f.path);
        });
    }
}

/// A round icon target over a tile: the tick circle, the "not him" slash and the trash's restore
/// glyph. The scrim keeps the glyph readable on any crop; the picked state takes the accent fill
/// (M22.26).
fn tile_icon_target(
    ui: &mut egui::Ui,
    id: egui::Id,
    rect: egui::Rect,
    icon: Icon,
    active: bool,
    hint: &str,
) -> egui::Response {
    let resp = ui
        .interact(rect, id, egui::Sense::click())
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hint);
    if ui.is_rect_visible(rect) {
        let th = theme(ui);
        let bg = if active {
            th.accent
        } else if resp.hovered() {
            egui::Color32::from_black_alpha(200)
        } else {
            egui::Color32::from_black_alpha(150)
        };
        ui.painter()
            .circle_filled(rect.center(), rect.width() * 0.5, bg);
        let color = if active {
            th.on_accent
        } else {
            egui::Color32::WHITE
        };
        paint_icon(ui.painter(), rect.center(), rect.width() * 0.30, icon, color);
    }
    resp
}

/// A short version caption: the first line and the first three words ("ffmpeg version 9.0.2").
fn short_version(line: &str) -> String {
    line.split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// A weight percentile over a descending distribution: the share of photos with a lower weight.
/// The strongest photo of a tag is 100 (after rounding), the weakest 0.
fn score_percentile(sorted_desc: &[f32], score: f32) -> Option<u32> {
    if sorted_desc.is_empty() {
        return None;
    }
    let below = sorted_desc.iter().filter(|s| **s < score).count();
    Some((below as f64 * 100.0 / sorted_desc.len() as f64).round() as u32)
}


struct Cells<'a> {
    /// The background of the parent under a tile (the grid canvas or a duplicate-group card).
    bg: egui::Color32,
    /// How the files are shown: the mock-up tile, rows by proportion or a list (M22).
    cell_style: db::CellStyle,
    /// A day strip in the calendar: every row falls on one date, so the caption shows the time
    /// instead of the date — the only thing that tells the snapshots of a day apart (M22.5).
    time_only: bool,
    textures: &'a mut HashMap<i64, egui::TextureHandle>,
    missing: &'a mut HashSet<i64>,
    thumbs_dir: &'a Path,
    selected: Option<i64>,
    /// The multi-select (M18): the marked tiles get an outline and a check mark without hovering.
    picked: &'a BTreeSet<i64>,
    /// A click on a tile with the kind of the click (Ctrl/Shift) — `App` decides.
    clicked: Option<(i64, PickClick)>,
    /// A double click is the full-screen viewer.
    open_clicked: Option<i64>,
    /// Show the "keep" button on the tile (the "Duplicates" view).
    show_keep: bool,
    /// "Keep" was pressed on a file — apply the decision to its group.
    keep_clicked: Option<i64>,
    /// The "Trash" view: a "restore" button and a "in the trash" mark on the tile.
    trash_mode: bool,
    restore_clicked: Option<i64>,
    /// The tile size, px: set by the slider in the left panel.
    tile: f32,
    /// The interface language: the captions on a tile ("no date", "spare") go through `t()`.
    lang: i18n::Lang,
    /// How many new textures to decode per frame: the rest on the next frame, otherwise fast
    /// scrolling runs into JPEG decoding on the UI thread.
    budget: usize,
}

fn load_texture(ctx: &egui::Context, path: &Path, name: &str) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, &img.into_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

/// Fit the picture into the tile square keeping its proportions: margins remain at the edges.
fn fit_rect(bounds: egui::Rect, size: egui::Vec2) -> egui::Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
    egui::Rect::from_center_size(bounds.center(), size * scale)
}

/// The tile filling: the picture covers the whole frame keeping its proportions, so the sides that
/// do not fit are cut evenly on both edges (never stretched) — the mock-up's 4:3 crop (M22.15).
/// The result is wider or taller than the frame, so a painter of this picture clips it to the frame.
fn cover_rect(bounds: egui::Rect, size: egui::Vec2) -> egui::Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width() / size.x).max(bounds.height() / size.y);
    egui::Rect::from_center_size(bounds.center(), size * scale)
}

/// The offset of a zoomed frame: the frame does not leave the edges of the area, and when it
/// is fitted (one of the sides is not bigger than the area) it is centred along that side.
fn clamp_pan(pan: egui::Vec2, image: egui::Vec2, area: egui::Rect) -> egui::Vec2 {
    let room = ((image - area.size()) / 2.0).max(egui::Vec2::ZERO);
    egui::vec2(pan.x.clamp(-room.x, room.x), pan.y.clamp(-room.y, room.y))
}

/// How a preview sits in the frame it is drawn into.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ThumbFit {
    /// The whole frame: the picture is fitted and the margins remain (the calendar, the series
    /// strip, the file card — a preview is never cropped there).
    Whole,
    /// The whole frame: the picture fills it edge to edge and the sides that do not fit are cut
    /// evenly (the grid tile, M22.15; a cell of an album cover, M22.29).
    Filling,
}

/// One way to draw a preview: the grid, the calendar and the day thumbnails all use it.
/// Returns `true` if the picture was drawn.
fn paint_thumb(
    ui: &egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    f: &FileRow,
    rect: egui::Rect,
) -> bool {
    paint_thumb_as(ui, ctx, cs, f, rect, ThumbFit::Whole)
}

/// A preview that fills its frame: used by the grid tile only (M22.15).
fn paint_thumb_filling(
    ui: &egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    f: &FileRow,
    rect: egui::Rect,
) -> bool {
    paint_thumb_as(ui, ctx, cs, f, rect, ThumbFit::Filling)
}

fn paint_thumb_as(
    ui: &egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    f: &FileRow,
    rect: egui::Rect,
    fit: ThumbFit,
) -> bool {
    if !cs.textures.contains_key(&f.id) && !cs.missing.contains(&f.id) {
        if cs.budget == 0 {
            ctx.request_repaint();
            return false;
        }
        cs.budget -= 1;
        match f
            .thumb
            .as_ref()
            .and_then(|rel| load_texture(ctx, &cs.thumbs_dir.join(rel), &format!("t{}", f.id)))
        {
            Some(t) => {
                cs.textures.insert(f.id, t);
            }
            None => {
                cs.missing.insert(f.id);
            }
        }
    }
    if let Some(t) = cs.textures.get(&f.id) {
        let size = t.size_vec2();
        let dst = match fit {
            ThumbFit::Whole => fit_rect(rect, size),
            ThumbFit::Filling => cover_rect(rect, size),
        };
        // A filling picture is bigger than the frame on one side: the frame clips the cut-off part.
        let painter = ui.painter().with_clip_rect(ui.clip_rect().intersect(rect));
        painter.image(
            t.id(),
            dst,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        true
    } else {
        false
    }
}

fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let m = month as i32 - 1 + delta;
    let y = year + m.div_euclid(12);
    let m = m.rem_euclid(12) as u32 + 1;
    (y.clamp(1900, 2199), m)
}

/// A date from `taken_at` (`YYYY-MM-DD[THH:MM]`) without a panic on garbage.
fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s.get(0..10)?, "%Y-%m-%d").ok()
}

/// The Monday of the week the date falls into.
fn monday_of(d: chrono::NaiveDate) -> chrono::NaiveDate {
    d - chrono::Duration::days(d.weekday().num_days_from_monday() as i64)
}

fn days_in_month(year: i32, month: u32) -> u32 {
    let (ny, nm) = shift_month(year, month, 1);
    let next = chrono::NaiveDate::from_ymd_opt(ny, nm, 1);
    let first = chrono::NaiveDate::from_ymd_opt(year, month, 1);
    match (first, next) {
        (Some(a), Some(b)) => (b - a).num_days() as u32,
        _ => 30,
    }
}

fn first_weekday_monday(year: i32, month: u32) -> u32 {
    chrono::NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| d.weekday().num_days_from_monday())
        .unwrap_or(0)
}

fn month_key(year: i32, month: u32) -> String {
    format!("{year:04}-{month:02}")
}

/// The nearest month in the direction of `delta` that has files.
/// Empty months are not shown: paging through them is pointless.
fn shift_to_file_month(
    months: &BTreeSet<String>,
    year: i32,
    month: u32,
    delta: i32,
) -> Option<(i32, u32)> {
    let mut cur = (year, month);
    for _ in 0..1200 {
        cur = shift_month(cur.0, cur.1, delta);
        if months.contains(&month_key(cur.0, cur.1)) {
            return Some(cur);
        }
    }
    None
}

/// The nearest month with files to the given one (backwards first, then forwards).
fn nearest_file_month(months: &BTreeSet<String>, year: i32, month: u32) -> Option<(i32, u32)> {
    for d in 0..1200 {
        let back = shift_month(year, month, -d);
        if months.contains(&month_key(back.0, back.1)) {
            return Some(back);
        }
        let forward = shift_month(year, month, d);
        if months.contains(&month_key(forward.0, forward.1)) {
            return Some(forward);
        }
    }
    None
}

/// A calendar cell: the card tone with a 1 px line while the cell holds files, the accent fill and
/// a 1 px accent frame while it is picked (the month, the day), nothing while it is empty — so the
/// archive's shape is visible without reading the numbers (M22.5). Returns the palette of the
/// frame, because the number, the counter and the bar inside are drawn in its tones.
fn cal_cell(ui: &egui::Ui, rect: egui::Rect, radius: f32, has_files: bool, picked: bool) -> Theme {
    let th = theme(ui);
    let corners = egui::CornerRadius::same(radius as u8);
    let (fill, stroke) = if picked {
        (th.accent_soft, egui::Stroke::new(1.0, th.accent))
    } else if has_files {
        (th.card, egui::Stroke::new(1.0, th.border))
    } else {
        // An empty cell keeps the panel tone instead of vanishing: the month stays a grid, and the
        // gaps of the archive are seen instead of searched for.
        (th.panel, egui::Stroke::NONE)
    };
    if fill != egui::Color32::TRANSPARENT {
        ui.painter().rect_filled(rect, corners, fill);
    }
    if stroke != egui::Stroke::NONE {
        ui.painter()
            .rect_stroke(rect, corners, stroke, egui::StrokeKind::Inside);
    }
    th
}

/// The volume of a calendar cell as a bar: the share of the fullest month of the year or the
/// fullest day of the month — the mock-up's 4 px strip under the cell.
fn cal_bar(ui: &egui::Ui, rect: egui::Rect, share: f32) {
    let th = theme(ui);
    let corners = egui::CornerRadius::same(2);
    ui.painter().rect_filled(rect, corners, th.field);
    let w = rect.width() * share.clamp(0.0, 1.0);
    if w >= 1.0 {
        let filled = egui::Rect::from_min_size(rect.min, egui::vec2(w, rect.height()));
        ui.painter().rect_filled(filled, corners, th.accent);
    }
}

/// The title of a day: "Wed, 01.05.2024"; a key that is not a date is shown as it is.
fn day_title(lang: i18n::Lang, key: &str) -> String {
    match parse_date(key) {
        Some(d) => format!(
            "{}, {}",
            i18n::weekday(lang, d.weekday().num_days_from_monday() as usize),
            d.format("%d.%m.%Y")
        ),
        None => key.to_string(),
    }
}

/// The heading of a day: the date at 14 px and its facts at 11 px in the numeric family — the same
/// words as a month heading, so the calendar and the grid speak alike.
fn day_heading(ui: &mut egui::Ui, title: &str, files: usize, bytes: i64, lang: i18n::Lang) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(14.0).strong());
        let facts = i18n::fill(
            lang,
            "· {}, {}",
            &[i18n::count(lang, files, "file"), db::human_in(lang, bytes)],
        );
        ui.label(num(ui, facts, 11.0).color(dim(ui)));
    });
    ui.add_space(4.0);
}

/// The snapshots of one day under the calendar (M22.5): the date and its facts, then the files as
/// a list with the time instead of the full date. Within one day the date part is noise, and the
/// list is the view that shows the time and the size side by side; the owner's cell view is
/// overridden for this strip only, so every other screen keeps his choice.
fn day_strip(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    key: &str,
    idxs: &[usize],
    items: &[FileRow],
    lang: i18n::Lang,
) {
    let bytes: i64 = idxs.iter().map(|&i| items[i].size).sum();
    day_heading(ui, &day_title(lang, key), idxs.len(), bytes, lang);
    let style = cs.cell_style;
    cs.cell_style = db::CellStyle::List;
    cs.time_only = true;
    let row: Vec<&FileRow> = idxs.iter().map(|&i| &items[i]).collect();
    draw_files(ui, ctx, cs, &row);
    cs.time_only = false;
    cs.cell_style = style;
}

/// A text fragment with the matches: FTS5 marks them with « », the UI paints them with the accent.
fn highlight_job(ui: &egui::Ui, text: &str) -> egui::text::LayoutJob {
    let normal = theme(ui).text;
    let accent_color = accent(ui);
    let mut job = egui::text::LayoutJob::default();
    job.wrap.max_width = ui.available_width().max(120.0);
    let mut matched = false;
    let mut segment = String::new();
    fn flush(
        segment: &mut String,
        matched: bool,
        normal: egui::Color32,
        accent: egui::Color32,
        job: &mut egui::text::LayoutJob,
    ) {
        if segment.is_empty() {
            return;
        }
        job.append(
            segment,
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(12.0),
                color: if matched { accent } else { normal },
                ..Default::default()
            },
        );
        segment.clear();
    }
    for ch in text.chars() {
        match ch {
            '«' | '»' => {
                flush(&mut segment, matched, normal, accent_color, &mut job);
                matched = ch == '«';
            }
            _ => segment.push(ch),
        }
    }
    flush(&mut segment, matched, normal, accent_color, &mut job);
    job
}

/// A geometric icon of the window (M22.8). The names are the mock-up's Material Symbols, the
/// shapes are drawn by the painter: photocat bundles no icon font and no image, the binary does
/// not grow, and an icon always stands next to its word (docs/UI.md § Icons). The playback signs
/// (`Play`, `Pause`, `Prev`, `Next`, `Volume`) were the first of them and keep their geometry.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Icon {
    // Actions of the shell.
    Search,
    Close,
    Settings,
    DockToRight,
    Folder,
    FolderSpecial,
    CreateNewFolder,
    Sync,
    Refresh,
    // The map of the application.
    PhotoLibrary,
    Photo,
    Videocam,
    CollectionsBookmark,
    CalendarMonth,
    CalendarToday,
    Face,
    BurstMode,
    Verified,
    // Views and lists.
    Apps,
    GridView,
    ViewAgenda,
    ViewList,
    Sort,
    // Words that repeat everywhere.
    Add,
    Remove,
    Edit,
    MoveTo,
    ContentCopy,
    Download,
    Upload,
    Check,
    CheckCircle,
    // The face tile: an empty tick circle and a head with a slash (M22.26).
    Pick,
    NotHim,
    Delete,
    Restore,
    Stop,
    ArrowUp,
    ArrowDown,
    ArrowBack,
    ChevronLeft,
    ChevronRight,
    // The full screen and the files.
    ZoomIn,
    ZoomOut,
    FitScreen,
    OneToOne,
    Rotate,
    OpenInNew,
    Info,
    Location,
    Slideshow,
    TextFields,
    Preview,
    CompareArrows,
    Description,
    // Sections of the settings and the model.
    Psychology,
    Label,
    Inventory,
    Terminal,
    AutoAwesome,
    Calculate,
    Cleaning,
    // The player.
    Prev,
    Next,
    Play,
    Pause,
    Volume,
}

/// The stroke of a drawn icon. Not 1 px: a 1 px line of accent would be read as a selection
/// frame by the calendar check, and it is too thin to see at 14 px.
const ICON_W: f32 = 1.5;

/// An icon button of the playback bar: 32 px, the tooltip is mandatory.
fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, hint: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let resp = resp
        .on_hover_text(hint)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let visuals = ui.style().interact(&resp);
    ui.painter().rect_filled(rect, 4.0, visuals.bg_fill);
    paint_icon(
        ui.painter(),
        rect.center(),
        size * 0.30,
        icon,
        visuals.fg_stroke.color,
    );
    resp
}

/// An icon without an action — a caption next to the slider (the volume).
fn icon_mark(ui: &mut egui::Ui, icon: Icon, size: f32) {
    icon_mark_color(ui, icon, size, ui.visuals().text_color());
}

/// The same mark in a given tone: the brand glyph of the top bar is the accent.
fn icon_mark_color(ui: &mut egui::Ui, icon: Icon, size: f32, color: egui::Color32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_icon(ui.painter(), rect.center(), size * 0.30, icon, color);
}

/// The width of a mono word: the selection bar and the status line are built of numbers.
fn mono_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::monospace(size),
            egui::Color32::WHITE,
        )
        .size()
        .x
}

/// The width of a word in the UI family: the top bar centres the search between the mark and
/// the tools, and the widths depend on the language.
fn text_width(ui: &egui::Ui, text: &str, size: f32) -> f32 {
    ui.painter()
        .layout_no_wrap(
            text.to_owned(),
            egui::FontId::proportional(size),
            egui::Color32::WHITE,
        )
        .size()
        .x
}

/// A 28 px square that stays lit while the thing it toggles is on (the right panel): the
/// mock-up's active state, and the only icon-only target of the top bar (docs/UI.md § Layout).
fn toggle_icon_button(
    ui: &mut egui::Ui,
    icon: Icon,
    size: f32,
    on: bool,
    hint: &str,
) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let resp = resp
        .on_hover_text(hint)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let th = theme(ui);
    let (fill, stroke, color) = if on {
        (th.accent_soft, egui::Stroke::new(1.0, th.accent), th.accent)
    } else if resp.hovered() {
        (th.field, egui::Stroke::new(1.0, th.border_strong), th.text)
    } else {
        (th.card, egui::Stroke::new(1.0, th.border), th.text_med)
    };
    ui.painter().rect(
        rect,
        egui::CornerRadius::same(RADIUS_CTRL as u8),
        fill,
        stroke,
        egui::StrokeKind::Inside,
    );
    paint_icon(ui.painter(), rect.center(), size * 0.30, icon, color);
    resp
}

/// Which accent a word button carries: the ordinary field tone, the one main action of the
/// screen (the accent fill), the OS-trash path (the danger tone) or a picked segment of a switch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum WordKind {
    Normal,
    Primary,
    Danger,
}

/// A button with an icon and a word: one widget for every action of the window (M22.8). The
/// button keeps the geometry of `egui::Button` (26 px high, 10 px padding, 4 px radius) and adds
/// a 14 px icon 6 px before the word; the word stays, an icon is never the only path
/// (docs/UI.md § Layout).
struct WordButton {
    icon: Icon,
    text: String,
    kind: WordKind,
    selected: bool,
    small: bool,
}

impl WordButton {
    fn new(icon: Icon, text: impl Into<String>) -> Self {
        Self {
            icon,
            text: text.into(),
            kind: WordKind::Normal,
            selected: false,
            small: false,
        }
    }

    /// The one main action of the screen: the accent fill, the text on the accent.
    fn primary(icon: Icon, text: impl Into<String>) -> Self {
        Self {
            kind: WordKind::Primary,
            ..Self::new(icon, text)
        }
    }

    /// A destructive path: the danger border, no fill (the OS trash only, AGENTS.md §1).
    fn danger(icon: Icon, text: impl Into<String>) -> Self {
        Self {
            kind: WordKind::Danger,
            ..Self::new(icon, text)
        }
    }

    /// A segment of a switch or a filter: picked means the accent, the width follows the word
    /// and not the state, so the row never moves on a click.
    fn picked(icon: Icon, text: impl Into<String>, selected: bool) -> Self {
        Self {
            selected,
            ..Self::new(icon, text)
        }
    }

    /// A 20 px button of a dense row (the section list of the settings dialog).
    fn small(icon: Icon, text: impl Into<String>) -> Self {
        Self {
            small: true,
            ..Self::new(icon, text)
        }
    }

    /// The same button in the 20 px rhythm of a strip switch.
    fn compact(mut self) -> Self {
        self.small = true;
        self
    }

    fn size(&self, ui: &egui::Ui) -> (f32, f32, f32, f32) {
        let font = if self.small { 11.0 } else { 13.0 };
        let icon = if self.small { 12.0 } else { 14.0 };
        let pad = if self.small { 6.0 } else { 10.0 };
        let h = if self.small { 20.0 } else { 26.0 };
        let w = ui
            .painter()
            .layout_no_wrap(
                self.text.clone(),
                egui::FontId::proportional(font),
                egui::Color32::WHITE,
            )
            .size()
            .x;
        (pad + icon + 6.0 + w + pad, h, icon, font)
    }
}

impl egui::Widget for WordButton {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let (w, h, icon_size, font) = self.size(ui);
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
        let th = theme(ui);
        let visuals = if self.selected {
            ui.style().interact_selectable(&resp, true)
        } else {
            *ui.style().interact(&resp)
        };
        // Every role reads its tones from the palette, never from a literal colour.
        let (fill, stroke, color) = match self.kind {
            WordKind::Primary => (
                if resp.hovered() {
                    th.accent_hover
                } else {
                    th.accent
                },
                egui::Stroke::NONE,
                th.on_accent,
            ),
            WordKind::Danger => (
                egui::Color32::TRANSPARENT,
                egui::Stroke::new(1.0, th.danger),
                th.danger,
            ),
            WordKind::Normal if self.selected => (
                ui.visuals().selection.bg_fill,
                egui::Stroke::NONE,
                ui.visuals().selection.stroke.color,
            ),
            WordKind::Normal => (visuals.weak_bg_fill, visuals.bg_stroke, visuals.fg_stroke.color),
        };
        let color = if ui.is_enabled() {
            color
        } else {
            th.text_dim
        };
        if ui.is_rect_visible(rect) {
            let p = ui.painter();
            p.rect(
                rect,
                egui::CornerRadius::same(RADIUS_CTRL as u8),
                fill,
                stroke,
                egui::StrokeKind::Inside,
            );
            let pad = if self.small { 6.0 } else { 10.0 };
            let icon_c = egui::pos2(rect.left() + pad + icon_size * 0.5, rect.center().y);
            paint_icon(p, icon_c, icon_size * 0.34, self.icon, color);
            p.text(
                egui::pos2(icon_c.x + icon_size * 0.5 + 6.0, rect.center().y),
                egui::Align2::LEFT_CENTER,
                &self.text,
                egui::FontId::proportional(font),
                color,
            );
        }
        if resp.hovered() && ui.is_enabled() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
        resp
    }
}

/// A plain button with an icon: `word_button(ui, Icon::Add, "Add tag")`.
fn word_button(ui: &mut egui::Ui, icon: Icon, text: impl Into<String>) -> egui::Response {
    ui.add(WordButton::new(icon, text))
}

/// The geometry of an icon: `s` is half the height of the shape, `c` its centre.
fn paint_icon(p: &egui::Painter, c: egui::Pos2, s: f32, icon: Icon, color: egui::Color32) {
    let pt = |x: f32, y: f32| egui::pos2(c.x + x * s, c.y + y * s);
    let st = egui::Stroke::new(ICON_W, color);
    let line = |a: egui::Pos2, b: egui::Pos2| {
        p.line_segment([a, b], st);
    };
    let seg = |pts: Vec<egui::Pos2>| {
        p.add(egui::Shape::line(pts, st));
    };
    let frame = |pts: Vec<egui::Pos2>| {
        p.add(egui::Shape::closed_line(pts, st));
    };
    let fill = |pts: Vec<egui::Pos2>| {
        p.add(egui::Shape::convex_polygon(
            pts,
            color,
            egui::Stroke::NONE,
        ));
    };
    let boxed = |x0: f32, y0: f32, x1: f32, y1: f32| {
        p.rect_stroke(
            egui::Rect::from_min_max(pt(x0, y0), pt(x1, y1)),
            1.0,
            st,
            egui::StrokeKind::Inside,
        );
    };
    let block = |x0: f32, y0: f32, x1: f32, y1: f32| {
        p.rect_filled(egui::Rect::from_min_max(pt(x0, y0), pt(x1, y1)), 1.0, color);
    };
    let disc = |x: f32, y: f32, r: f32| {
        p.circle_filled(pt(x, y), r * s, color);
    };
    let ring = |x: f32, y: f32, r: f32| {
        p.circle_stroke(pt(x, y), r * s, st);
    };
    let arc = |cx: f32, cy: f32, r: f32, a0: f32, a1: f32| {
        let pts: Vec<egui::Pos2> = (0..=20)
            .map(|i| {
                let a = a0 + (a1 - a0) * i as f32 / 20.0;
                pt(cx + r * a.cos(), cy + r * a.sin())
            })
            .collect();
        seg(pts)
    };
    // An arrowhead whose tip is `a` and which points along `d` (in icon units).
    let head = |a: egui::Pos2, d: egui::Vec2| {
        let len = d.length().max(0.001);
        let d = d / len;
        let n = egui::vec2(-d.y, d.x);
        fill(vec![
            a,
            a - d * s * 0.62 + n * s * 0.42,
            a - d * s * 0.62 - n * s * 0.42,
        ]);
    };
    let head_at = |radius: f32, angle: f32, dir: f32| {
        let a = pt(radius * angle.cos(), radius * angle.sin());
        head(a, egui::vec2(-angle.sin() * dir, angle.cos() * dir));
    };
    let folder = |plus: bool, sparkle: bool| {
        frame(vec![
            pt(-0.95, -0.5),
            pt(-0.3, -0.5),
            pt(-0.12, -0.25),
            pt(0.95, -0.25),
            pt(0.95, 0.75),
            pt(-0.95, 0.75),
        ]);
        if plus {
            line(pt(0.0, -0.02), pt(0.0, 0.52));
            line(pt(-0.28, 0.25), pt(0.28, 0.25));
        }
        if sparkle {
            fill(vec![
                pt(0.0, -0.02),
                pt(0.16, 0.25),
                pt(0.0, 0.52),
                pt(-0.16, 0.25),
            ]);
        }
    };
    match icon {
        Icon::Search => {
            ring(-0.15, -0.15, 0.62);
            line(pt(0.32, 0.32), pt(0.95, 0.95));
        }
        Icon::ZoomIn | Icon::ZoomOut => {
            ring(-0.1, -0.1, 0.6);
            line(pt(0.36, 0.36), pt(0.95, 0.95));
            line(pt(-0.42, -0.1), pt(0.22, -0.1));
            if matches!(icon, Icon::ZoomIn) {
                line(pt(-0.1, -0.42), pt(-0.1, 0.22));
            }
        }
        Icon::Close => {
            line(pt(-0.7, -0.7), pt(0.7, 0.7));
            line(pt(-0.7, 0.7), pt(0.7, -0.7));
        }
        Icon::Add => {
            line(pt(0.0, -0.85), pt(0.0, 0.85));
            line(pt(-0.85, 0.0), pt(0.85, 0.0));
        }
        Icon::Remove => {
            line(pt(-0.85, 0.0), pt(0.85, 0.0));
        }
        Icon::Check => seg(vec![pt(-0.85, 0.05), pt(-0.25, 0.65), pt(0.9, -0.7)]),
        // The mock-up's `sort`: three rails, the shortest at the bottom.
        Icon::Sort => {
            line(pt(-0.9, -0.55), pt(0.9, -0.55));
            line(pt(-0.9, 0.0), pt(0.45, 0.0));
            line(pt(-0.9, 0.55), pt(0.0, 0.55));
        }
        Icon::CheckCircle => {
            ring(0.0, 0.0, 0.92);
            seg(vec![pt(-0.45, 0.02), pt(-0.12, 0.38), pt(0.5, -0.38)]);
        }
        // The tick of a face tile before it is picked (M22.26).
        Icon::Pick => ring(0.0, 0.0, 0.9),
        // "Not him": a head and shoulders with a slash across them (M22.26).
        Icon::NotHim => {
            ring(0.0, -0.32, 0.38);
            arc(
                0.0,
                0.88,
                0.62,
                std::f32::consts::PI,
                2.0 * std::f32::consts::PI,
            );
            line(pt(-0.82, -0.82), pt(0.82, 0.82));
        }
        // The mock-up's `tune` (the icon of the settings dialog there): three rails with a knob
        // each. A 14 px gear reads as a sun, a rail reads at once.
        Icon::Settings => {
            for (y, x) in [(-0.6, -0.3), (0.0, 0.25), (0.6, -0.1)] {
                line(pt(-0.95, y), pt(x - 0.26, y));
                line(pt(x + 0.26, y), pt(0.95, y));
                disc(x, y, 0.22);
            }
        }
        Icon::DockToRight => {
            boxed(-0.95, -0.8, 0.95, 0.8);
            block(0.3, -0.8, 0.95, 0.8);
        }
        Icon::Folder => folder(false, false),
        Icon::FolderSpecial => folder(false, true),
        Icon::CreateNewFolder => folder(true, false),
        Icon::Sync => {
            arc(0.0, 0.0, 0.8, 0.15 * std::f32::consts::PI, 0.85 * std::f32::consts::PI);
            head_at(0.8, 0.85 * std::f32::consts::PI, 1.0);
            arc(0.0, 0.0, 0.8, 1.15 * std::f32::consts::PI, 1.85 * std::f32::consts::PI);
            head_at(0.8, 1.85 * std::f32::consts::PI, 1.0);
        }
        Icon::Refresh | Icon::Rotate => {
            arc(0.0, 0.0, 0.8, 0.2 * std::f32::consts::PI, 1.9 * std::f32::consts::PI);
            head_at(0.8, 1.9 * std::f32::consts::PI, 1.0);
        }
        Icon::Restore => {
            arc(0.0, 0.0, 0.8, 0.1 * std::f32::consts::PI, 1.8 * std::f32::consts::PI);
            head_at(0.8, 0.1 * std::f32::consts::PI, -1.0);
        }
        Icon::PhotoLibrary => {
            boxed(-0.95, -0.85, 0.45, 0.35);
            boxed(-0.45, -0.35, 0.95, 0.85);
            disc(-0.15, -0.05, 0.13);
            seg(vec![pt(-0.35, 0.72), pt(0.05, 0.32), pt(0.4, 0.6), pt(0.85, 0.15)]);
        }
        Icon::Photo => {
            boxed(-0.95, -0.75, 0.95, 0.75);
            disc(-0.42, -0.28, 0.16);
            seg(vec![
                pt(-0.9, 0.55),
                pt(-0.25, -0.1),
                pt(0.15, 0.3),
                pt(0.45, 0.0),
                pt(0.9, 0.45),
            ]);
        }
        Icon::Videocam => {
            boxed(-0.95, -0.55, 0.3, 0.55);
            fill(vec![pt(0.35, -0.15), pt(0.95, -0.6), pt(0.95, 0.6), pt(0.35, 0.15)]);
        }
        Icon::CollectionsBookmark => {
            block(-0.85, -0.8, -0.45, 0.85);
            block(-0.2, -0.8, 0.2, 0.85);
            block(0.45, -0.8, 0.85, 0.4);
        }
        Icon::CalendarMonth | Icon::CalendarToday => {
            boxed(-0.95, -0.75, 0.95, 0.9);
            block(-0.95, -0.75, 0.95, -0.4);
            if matches!(icon, Icon::CalendarToday) {
                disc(0.0, 0.25, 0.24);
            } else {
                for x in [-0.5, 0.0, 0.5] {
                    disc(x, 0.1, 0.15);
                    disc(x, 0.55, 0.15);
                }
            }
        }
        Icon::Face => {
            ring(0.0, 0.0, 0.92);
            disc(-0.33, -0.25, 0.16);
            disc(0.33, -0.25, 0.16);
            arc(0.0, -0.05, 0.48, 0.6, std::f32::consts::PI - 0.6);
        }
        Icon::BurstMode => {
            boxed(-0.95, -0.95, 0.25, 0.25);
            boxed(-0.55, -0.55, 0.65, 0.65);
            boxed(-0.15, -0.15, 0.95, 0.95);
        }
        Icon::Verified => {
            frame(vec![
                pt(0.0, -0.95),
                pt(0.85, -0.6),
                pt(0.75, 0.3),
                pt(0.0, 0.95),
                pt(-0.75, 0.3),
                pt(-0.85, -0.6),
            ]);
            seg(vec![pt(-0.4, 0.0), pt(-0.1, 0.32), pt(0.45, -0.35)]);
        }
        Icon::Delete => {
            line(pt(-0.85, -0.55), pt(0.85, -0.55));
            line(pt(-0.3, -0.55), pt(-0.3, -0.85));
            line(pt(0.3, -0.55), pt(0.3, -0.85));
            frame(vec![
                pt(-0.65, -0.55),
                pt(-0.5, 0.9),
                pt(0.5, 0.9),
                pt(0.65, -0.55),
            ]);
            line(pt(-0.2, -0.25), pt(-0.2, 0.6));
            line(pt(0.2, -0.25), pt(0.2, 0.6));
        }
        Icon::Apps => {
            block(-0.85, -0.85, -0.15, -0.15);
            block(0.15, -0.85, 0.85, -0.15);
            block(-0.85, 0.15, -0.15, 0.85);
            block(0.15, 0.15, 0.85, 0.85);
        }
        Icon::GridView => {
            boxed(-0.85, -0.85, -0.15, -0.15);
            boxed(0.15, -0.85, 0.85, -0.15);
            boxed(-0.85, 0.15, -0.15, 0.85);
            boxed(0.15, 0.15, 0.85, 0.85);
        }
        Icon::ViewAgenda => {
            block(-0.9, -0.9, 0.9, -0.38);
            block(-0.9, -0.26, 0.9, 0.26);
            block(-0.9, 0.38, 0.9, 0.9);
        }
        Icon::ViewList => {
            for y in [-0.6, 0.0, 0.6] {
                disc(-0.75, y, 0.15);
                line(pt(-0.4, y), pt(0.9, y));
            }
        }
        Icon::Edit => {
            fill(vec![
                pt(-0.9, 0.45),
                pt(0.45, -0.9),
                pt(0.9, -0.45),
                pt(-0.45, 0.9),
            ]);
            fill(vec![pt(-0.9, 0.45), pt(-0.45, 0.9), pt(-0.95, 0.95)]);
            line(pt(-0.55, 0.7), pt(0.75, -0.6));
        }
        Icon::MoveTo => {
            line(pt(-0.9, 0.0), pt(0.45, 0.0));
            head(pt(0.9, 0.0), egui::vec2(1.0, 0.0));
            line(pt(0.85, -0.8), pt(0.85, 0.8));
        }
        Icon::ContentCopy => {
            boxed(-0.9, -0.9, 0.35, 0.35);
            boxed(-0.35, -0.35, 0.9, 0.9);
        }
        Icon::Download => {
            line(pt(0.0, -0.9), pt(0.0, 0.3));
            head(pt(0.0, 0.7), egui::vec2(0.0, 1.0));
            seg(vec![
                pt(-0.9, 0.25),
                pt(-0.9, 0.9),
                pt(0.9, 0.9),
                pt(0.9, 0.25),
            ]);
        }
        Icon::Upload => {
            line(pt(0.0, 0.85), pt(0.0, -0.3));
            head(pt(0.0, -0.75), egui::vec2(0.0, -1.0));
            seg(vec![
                pt(-0.9, 0.25),
                pt(-0.9, 0.9),
                pt(0.9, 0.9),
                pt(0.9, 0.25),
            ]);
        }
        Icon::Stop => block(-0.72, -0.72, 0.72, 0.72),
        Icon::ArrowUp | Icon::ArrowDown => {
            let dir = if matches!(icon, Icon::ArrowUp) { -1.0 } else { 1.0 };
            line(pt(0.0, -dir * 0.85), pt(0.0, dir * 0.85));
            head(pt(0.0, dir * 0.95), egui::vec2(0.0, dir));
        }
        Icon::ArrowBack => {
            line(pt(0.85, 0.0), pt(-0.5, 0.0));
            head(pt(-0.9, 0.0), egui::vec2(-1.0, 0.0));
        }
        Icon::ChevronLeft => seg(vec![pt(0.4, -0.8), pt(-0.5, 0.0), pt(0.4, 0.8)]),
        Icon::ChevronRight => seg(vec![pt(-0.4, -0.8), pt(0.5, 0.0), pt(-0.4, 0.8)]),
        Icon::FitScreen => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                line(pt(sx * 0.9, sy * 0.4), pt(sx * 0.9, sy * 0.9));
                line(pt(sx * 0.4, sy * 0.9), pt(sx * 0.9, sy * 0.9));
            }
        }
        Icon::OneToOne => {
            boxed(-0.9, -0.8, 0.9, 0.8);
            line(pt(0.0, -0.8), pt(0.0, 0.8));
        }
        Icon::Location => {
            // The mock-up's `location_on`: a round head over a point.
            ring(0.0, -0.22, 0.52);
            line(pt(-0.37, 0.15), pt(0.0, 0.92));
            line(pt(0.37, 0.15), pt(0.0, 0.92));
        }
        Icon::OpenInNew => {
            seg(vec![
                pt(-0.9, -0.3),
                pt(-0.9, 0.9),
                pt(0.3, 0.9),
                pt(0.3, 0.4),
            ]);
            line(pt(-0.15, 0.15), pt(0.75, -0.75));
            seg(vec![pt(0.25, -0.9), pt(0.9, -0.9), pt(0.9, -0.25)]);
        }
        Icon::Info => {
            ring(0.0, 0.0, 0.92);
            disc(0.0, -0.42, 0.15);
            line(pt(0.0, -0.05), pt(0.0, 0.55));
        }
        Icon::Slideshow => {
            boxed(-0.95, -0.75, 0.95, 0.75);
            fill(vec![pt(-0.2, -0.35), pt(-0.2, 0.35), pt(0.4, 0.0)]);
        }
        Icon::TextFields => {
            line(pt(-0.7, -0.8), pt(0.7, -0.8));
            line(pt(0.0, -0.8), pt(0.0, 0.85));
            line(pt(-0.35, 0.85), pt(0.35, 0.85));
        }
        Icon::Preview => {
            let eye = |up: f32| {
                let pts: Vec<egui::Pos2> = (0..=20)
                    .map(|i| {
                        let u = i as f32 / 20.0;
                        let x = -0.95 + 1.9 * u;
                        // A quadratic curve through the tips: 0 at the ends, `up` in the middle.
                        let y = up * 4.0 * u * (1.0 - u);
                        pt(x, y)
                    })
                    .collect();
                seg(pts)
            };
            eye(-0.75);
            eye(0.75);
            disc(0.0, 0.0, 0.24);
        }
        Icon::CompareArrows => {
            line(pt(-0.9, -0.38), pt(0.55, -0.38));
            head(pt(0.92, -0.38), egui::vec2(1.0, 0.0));
            line(pt(0.9, 0.38), pt(-0.55, 0.38));
            head(pt(-0.92, 0.38), egui::vec2(-1.0, 0.0));
        }
        Icon::Description => {
            frame(vec![
                pt(-0.7, -0.95),
                pt(0.35, -0.95),
                pt(0.7, -0.55),
                pt(0.7, 0.95),
                pt(-0.7, 0.95),
            ]);
            line(pt(0.35, -0.95), pt(0.35, -0.55));
            line(pt(0.35, -0.55), pt(0.7, -0.55));
            line(pt(-0.4, -0.1), pt(0.4, -0.1));
            line(pt(-0.4, 0.3), pt(0.4, 0.3));
            line(pt(-0.4, 0.7), pt(0.1, 0.7));
        }
        Icon::Psychology => {
            ring(0.0, -0.2, 0.62);
            line(pt(-0.5, 0.15), pt(-0.22, 0.45));
            line(pt(0.5, 0.15), pt(0.22, 0.45));
            line(pt(-0.25, 0.55), pt(0.25, 0.55));
            line(pt(-0.2, 0.8), pt(0.2, 0.8));
        }
        Icon::Label => {
            frame(vec![
                pt(-0.9, -0.7),
                pt(0.1, -0.7),
                pt(0.9, 0.0),
                pt(0.1, 0.7),
                pt(-0.9, 0.7),
            ]);
            disc(-0.45, 0.0, 0.16);
        }
        Icon::Inventory => {
            boxed(-0.9, -0.75, 0.9, 0.9);
            line(pt(-0.9, -0.3), pt(0.9, -0.3));
            line(pt(-0.25, -0.75), pt(-0.25, -0.3));
            line(pt(0.25, -0.75), pt(0.25, -0.3));
        }
        Icon::Terminal => {
            boxed(-0.95, -0.8, 0.95, 0.8);
            seg(vec![pt(-0.55, -0.35), pt(-0.2, 0.0), pt(-0.55, 0.35)]);
            line(pt(0.05, 0.35), pt(0.55, 0.35));
        }
        Icon::AutoAwesome => {
            fill(vec![pt(0.0, -0.95), pt(0.26, 0.0), pt(0.0, 0.95), pt(-0.26, 0.0)]);
            fill(vec![pt(-0.95, 0.0), pt(0.0, 0.26), pt(0.95, 0.0), pt(0.0, -0.26)]);
            disc(0.7, -0.7, 0.16);
        }
        Icon::Calculate => {
            boxed(-0.7, -0.95, 0.7, 0.95);
            line(pt(-0.7, -0.4), pt(0.7, -0.4));
            for y in [0.0, 0.45] {
                for x in [-0.35, 0.0, 0.35] {
                    disc(x, y, 0.13);
                }
            }
        }
        Icon::Cleaning => {
            line(pt(0.9, -0.9), pt(0.15, -0.15));
            fill(vec![
                pt(-0.05, 0.05),
                pt(0.3, 0.4),
                pt(-0.4, 0.95),
                pt(-0.85, 0.5),
            ]);
        }
        Icon::Play => {
            fill(vec![pt(-0.7, -1.0), pt(-0.7, 1.0), pt(1.0, 0.0)]);
        }
        Icon::Pause => {
            block(-0.64, -1.0, -0.2, 1.0);
            block(0.2, -1.0, 0.64, 1.0);
        }
        Icon::Prev | Icon::Next => {
            let dir = if matches!(icon, Icon::Next) { 1.0 } else { -1.0 };
            fill(vec![
                pt(-dir * 0.9, -1.0),
                pt(-dir * 0.9, 1.0),
                pt(dir * 0.4, 0.0),
            ]);
            block(
                if dir > 0.0 { 0.62 } else { -0.98 },
                -1.0,
                if dir > 0.0 { 0.98 } else { -0.62 },
                1.0,
            );
        }
        Icon::Volume => {
            fill(vec![
                pt(-1.0, -0.35),
                pt(-0.5, -0.35),
                pt(-0.05, -1.0),
                pt(-0.05, 1.0),
                pt(-0.5, 0.35),
                pt(-1.0, 0.35),
            ]);
            seg(vec![pt(0.2, -0.5), pt(0.6, 0.0), pt(0.2, 0.5)]);
        }
    }
}

/// The big play button in the centre of a video frame: before the start it is visible that
/// this is a video and not a photo, and one click starts it.
fn paint_play_overlay(ui: &egui::Ui, area: egui::Rect, lang: i18n::Lang) {
    let center = area.center();
    let p = ui.painter();
    let plate = egui::Rect::from_center_size(center, egui::vec2(76.0, 76.0));
    p.rect_filled(plate, 4.0, egui::Color32::from_black_alpha(150));
    p.rect_stroke(
        plate,
        4.0,
        egui::Stroke::new(2.0, egui::Color32::from_gray(235)),
        egui::StrokeKind::Inside,
    );
    paint_icon(
        p,
        center + egui::vec2(2.0, 0.0),
        20.0,
        Icon::Play,
        egui::Color32::from_gray(235),
    );
    // A caption on a backing plate: over a light frame the text is unreadable otherwise.
    let galley = p.layout_no_wrap(
        t(lang, "video · press ▶ to play").into(),
        egui::FontId::proportional(12.0),
        egui::Color32::from_gray(235),
    );
    let pos = egui::pos2(area.left() + 8.0, area.bottom() - 8.0 - galley.size().y);
    let bg = egui::Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(4.0, 2.0));
    p.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(130));
    p.galley(pos, galley, egui::Color32::from_gray(235));
}

/// One way to draw a frame in the full-screen viewer: 100% means fitted, more means
/// zoomed; a zoomed frame is dragged with the mouse (`pan`) and does not leave the edges
/// of the area (the wheel does not page the file).
/// `rot` turns the view by 90° steps, `faces` marks who is in the frame (M22.7) and makes every
/// box a target: a click lists the photos of that face (M22.28).
/// Returns the response of the frame area (a click on a video is play or pause) and the id of the
/// face clicked, if any.
fn draw_viewer_image(
    ui: &mut egui::Ui,
    frame: ViewerFrame<'_>,
    f: &FileRow,
    zoom: f32,
    pan: &mut egui::Vec2,
    size: egui::Vec2,
    lang: i18n::Lang,
) -> (egui::Response, Option<i64>) {
    let tex = frame.textures;
    let rot = frame.rot;
    let (area, resp) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
    let mut resp = if f.kind == "video" {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    };
    ui.painter()
        .rect_filled(area, 4.0, ui.visuals().extreme_bg_color);
    if let Some(t) = tex.video {
        // A playing frame: fitted into the area, without mush and without waiting.
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        p.image(
            t.id(),
            fit_rect(area.shrink(1.0), t.size_vec2()),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        if tex.paused {
            paint_play_overlay(ui, area, lang);
        }
        return (resp, None);
    }
    let mut shown: Option<(egui::Rect, egui::Rect)> = None;
    if let Some((tid, t)) = tex.full
        && *tid == f.id
    {
        let ts = rotated_size(t.size_vec2(), rot);
        let fit = (area.width() / ts.x).min(area.height() / ts.y);
        let size = ts * (fit * zoom);
        // A zoomed frame is dragged with the left button: without this, at a scale above 100%
        // only the centre was visible and a detail could not be examined.
        *pan = clamp_pan(*pan + resp.drag_delta(), size, area);
        if size.x > area.width() + 0.5 || size.y > area.height() + 0.5 {
            if resp.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
            }
            resp = resp.on_hover_cursor(egui::CursorIcon::Grab);
        }
        let disp = egui::Rect::from_center_size(area.center() + *pan, size);
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        paint_frame(&p, t.id(), disp, rot, egui::Color32::WHITE);
        // The box the picture occupies before the turn: the face chips are placed in it.
        shown = Some((
            egui::Rect::from_center_size(disp.center(), rotated_size(disp.size(), rot)),
            disp,
        ));
    } else if let Some(t) = tex.stale {
        // While the next frame is decoding the previous sharp one is held — without mush. The
        // chips stay away: they belong to the file that is opening, not to the frame on screen.
        let disp = fit_rect(area.shrink(1.0), rotated_size(t.size_vec2(), rot));
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        paint_frame(&p, t.id(), disp, rot, egui::Color32::WHITE);
        ui.painter().text(
            egui::pos2(area.left() + 8.0, area.bottom() - 8.0),
            egui::Align2::LEFT_BOTTOM,
            i18n::t(lang, "loading the next frame…"),
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(235),
        );
    } else if let Some(t) = tex.preview {
        // The native 320 px thumbnail in the centre, without stretching it over the whole screen.
        let native = rotated_size(t.size_vec2(), rot);
        let scale = (area.width() / native.x).min(area.height() / native.y).min(1.0);
        let disp = egui::Rect::from_center_size(area.center(), native * scale);
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        paint_frame(&p, t.id(), disp, rot, egui::Color32::from_gray(230));
        shown = Some((
            egui::Rect::from_center_size(disp.center(), rotated_size(disp.size(), rot)),
            disp,
        ));
        // A video never has a "full frame": that is the cover frame, there is nothing to wait for.
        if f.kind != "video" {
            ui.painter().text(
                egui::pos2(area.left() + 8.0, area.bottom() - 8.0),
                egui::Align2::LEFT_BOTTOM,
                i18n::t(lang, "the full frame is loading…"),
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(235),
            );
        }
    } else if f.kind != "video" {
        ui.painter().text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            t(lang, "the full frame is loading…"),
            egui::FontId::proportional(14.0),
            dim(ui),
        );
    }
    let mut face_click = None;
    if let Some((content, disp)) = shown {
        face_click = paint_face_chips(ui, content, disp, rot, f, frame.faces, lang);
    }
    if f.kind == "video" {
        paint_play_overlay(ui, area, lang);
    }
    (resp, face_click)
}

/// The scale that shows one image pixel per screen pixel (M22.7): the frame is fitted into the
/// area at `fit`, so the frame as it is stored needs `1 / fit`. The smallest frame is still shown
/// at half size, the biggest at `MAX_ZOOM`.
fn one_to_one_zoom(frame: egui::Vec2, area: egui::Vec2) -> f32 {
    let fit = (area.x / frame.x.max(1.0))
        .min(area.y / frame.y.max(1.0))
        .max(0.01);
    (1.0 / fit).clamp(0.5, MAX_ZOOM)
}

/// The size of a frame as the view shows it: a 90° turn swaps the sides (M22.7).
fn rotated_size(size: egui::Vec2, rot: u8) -> egui::Vec2 {
    if rot % 2 == 1 {
        egui::vec2(size.y, size.x)
    } else {
        size
    }
}

/// A frame drawn into `disp` and turned by 90° steps about its centre. Only the view turns:
/// the file on disk and the DB are unchanged (M22.7).
fn paint_frame(
    p: &egui::Painter,
    tex: egui::TextureId,
    disp: egui::Rect,
    rot: u8,
    tint: egui::Color32,
) {
    let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
    let rot = rot % 4;
    if rot == 0 {
        p.image(tex, disp, uv, tint);
        return;
    }
    let content = egui::Rect::from_center_size(disp.center(), rotated_size(disp.size(), rot));
    let mut mesh = egui::Mesh::with_texture(tex);
    mesh.add_rect_with_uv(content, uv, tint);
    mesh.rotate(
        egui::emath::Rot2::from_angle(std::f32::consts::FRAC_PI_2 * rot as f32),
        disp.center(),
    );
    p.add(egui::Shape::mesh(mesh));
}

/// A box turned about a centre by 90° steps: the sides stay axis-aligned, so its corners are
/// enough to know the new rectangle (M22.7).
fn rotate_rect(r: egui::Rect, center: egui::Pos2, rot: u8) -> egui::Rect {
    let rot = rot % 4;
    if rot == 0 {
        return r;
    }
    let turn = egui::emath::Rot2::from_angle(std::f32::consts::FRAC_PI_2 * rot as f32);
    egui::Rect::from_points(
        &[r.left_top(), r.right_top(), r.right_bottom(), r.left_bottom()]
            .map(|p| center + turn * (p - center)),
    )
}

/// The face chips over the frame: who is in the picture and with what confidence. Drawn only
/// when the face model has seen this file; the boxes are image pixels of the same oriented frame
/// the viewer shows, so a turn moves them with it (M22.7). Every box is a click target: it returns
/// the id of the face pressed, which opens the photos of that face (M22.28).
fn paint_face_chips(
    ui: &egui::Ui,
    content: egui::Rect,
    disp: egui::Rect,
    rot: u8,
    f: &FileRow,
    faces: &[db::FaceBox],
    lang: i18n::Lang,
) -> Option<i64> {
    if faces.is_empty() || f.width == 0 || f.height == 0 {
        return None;
    }
    let mut clicked = None;
    let p = ui.painter().with_clip_rect(disp);
    for face in faces {
        let r = egui::Rect::from_min_size(
            content.min
                + egui::vec2(
                    face.x as f32 / f.width as f32 * content.width(),
                    face.y as f32 / f.height as f32 * content.height(),
                ),
            egui::vec2(
                face.w as f32 / f.width as f32 * content.width(),
                face.h as f32 / f.height as f32 * content.height(),
            ),
        );
        let r = rotate_rect(r, disp.center(), rot);
        if r.width() < 8.0 || r.height() < 8.0 {
            continue;
        }
        let resp = ui
            .interact(
                r,
                ui.scope_id().with(("face-box", face.id)),
                egui::Sense::click(),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand)
            .on_hover_text(t(lang, "all photos with this face"));
        if resp.clicked() {
            clicked = Some(face.id);
        }
        p.rect_stroke(
            r,
            2.0,
            egui::Stroke::new(1.5, accent(ui)),
            egui::StrokeKind::Outside,
        );
        let name: &str = if face.label.is_empty() {
            i18n::t(lang, "face")
        } else {
            &face.label
        };
        let text = format!("{name} ({:.0}%)", face.score * 100.0);
        let galley = p.layout_no_wrap(
            text,
            egui::FontId::proportional(10.0),
            egui::Color32::from_gray(235),
        );
        // The chip sits above the box and never leaves the frame.
        let pos = egui::pos2(
            r.left(),
            (r.top() - galley.size().y - 6.0).max(disp.top() + 2.0),
        );
        let plate = egui::Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(4.0, 2.0));
        p.rect_filled(plate, 4.0, egui::Color32::from_black_alpha(180));
        p.rect_stroke(
            plate,
            4.0,
            egui::Stroke::new(1.0, accent(ui)),
            egui::StrokeKind::Inside,
        );
        p.galley(pos, galley, egui::Color32::from_gray(235));
    }
    clicked
}

/// The series strip under the frame: the day, the number of its snapshots and the neighbours
/// with their position inside the day. A click opens one; the current one has the accent frame
/// (M22.7).
fn draw_series_strip(
    ui: &mut egui::Ui,
    day: Option<&str>,
    series: &[StripItem],
    cur: usize,
    textures: &HashMap<i64, egui::TextureHandle>,
    lang: i18n::Lang,
) -> Option<usize> {
    let mut open = None;
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        caps_label(ui, t(lang, "Series"));
        ui.label(num(ui, day.unwrap_or_default(), 11.0));
        ui.label(
            egui::RichText::new(format!("· {}", i18n::count(lang, series.len(), "file")))
                .size(11.0)
                .color(dim(ui)),
        );
    });
    ui.add_space(2.0);
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        for item in series {
            let (rect, resp) = ui.allocate_exact_size(
                egui::vec2(SERIES_THUMB_W, SERIES_THUMB_H),
                egui::Sense::click(),
            );
            let picked = item.idx == cur;
            let th = theme(ui);
            ui.painter().rect_filled(rect, 4.0, th.card);
            if let Some(tex) = textures.get(&item.id) {
                ui.painter().with_clip_rect(rect.shrink(1.0)).image(
                    tex.id(),
                    fit_rect(rect.shrink(1.0), tex.size_vec2()),
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            // The position inside the day: the number the owner walks by in the grid.
            let plate = egui::Rect::from_min_size(
                rect.left_bottom() + egui::vec2(3.0, -15.0),
                egui::vec2(20.0, 12.0),
            );
            ui.painter()
                .rect_filled(plate, 2.0, egui::Color32::from_black_alpha(150));
            ui.painter().text(
                plate.center(),
                egui::Align2::CENTER_CENTER,
                item.pos.to_string(),
                egui::FontId::monospace(10.0),
                if picked {
                    accent(ui)
                } else {
                    egui::Color32::from_gray(235)
                },
            );
            if item.video {
                ui.painter().text(
                    rect.right_top() + egui::vec2(-4.0, 3.0),
                    egui::Align2::RIGHT_TOP,
                    "▶",
                    egui::FontId::proportional(10.0),
                    egui::Color32::from_gray(235),
                );
            }
            let stroke = if picked {
                egui::Stroke::new(2.0, accent(ui))
            } else {
                egui::Stroke::new(1.0, th.border)
            };
            ui.painter()
                .rect_stroke(rect, 4.0, stroke, egui::StrokeKind::Inside);
            let resp = resp
                .on_hover_cursor(egui::CursorIcon::PointingHand)
                .on_hover_text(i18n::fill(
                    lang,
                    "photo {} of the day",
                    &[item.pos.to_string()],
                ));
            if resp.clicked() {
                open = Some(item.idx);
            }
        }
    });
    open
}

/// The frame proportion: it chooses the tile width in a row. The sizes live in the DB, so
/// the layout does not wait for textures; without a size (a broken file) — 4:3.
fn frame_aspect(f: &FileRow) -> f32 {
    if f.width > 0 && f.height > 0 {
        (f.width as f32 / f.height as f32).clamp(0.25, 4.0)
    } else {
        4.0 / 3.0
    }
}

/// How many frames fit in a row and how tall the row is. A pure function: the grid can be
/// checked without a window. The height does not exceed the target and does not drop below
/// 70% of it — otherwise one row of two frames would stretch over half the screen.
fn row_fit(aspects: &[f32], avail: f32, target: f32, gap: f32) -> (usize, f32) {
    let mut sum = 0.0f32;
    let mut n = 0usize;
    while n < aspects.len() {
        sum += aspects[n];
        n += 1;
        if sum * target + gap * (n - 1) as f32 >= avail {
            break;
        }
    }
    let gaps = gap * (n.saturating_sub(1)) as f32;
    let h = ((avail - gaps) / sum.max(0.01)).clamp(target * 0.7, target);
    (n.max(1), h)
}

/// One entry point for every file grid: `settings.cell_style` chooses the cell view.
fn draw_files(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, files: &[&FileRow]) {
    match cs.cell_style {
        db::CellStyle::Tiles => draw_tiles(ui, ctx, cs, files),
        db::CellStyle::Rows => draw_flow(ui, ctx, cs, files),
        db::CellStyle::List => draw_list(ui, ctx, cs, files),
    }
}

/// The main view from the mock-up: tiles of equal width, a 4:3 frame, the photo filling the
/// whole frame (the sides that do not fit are cut, never stretched), a caption strip under the
/// frame. The columns split the width of the area, as `grid-cols-6` does in the mock-up.
fn draw_tiles(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, files: &[&FileRow]) {
    if files.is_empty() {
        return;
    }
    let gap = TILE_GAP;
    let target = cs.tile.clamp(100.0, 260.0);
    let avail = ui.available_width().max(64.0);
    let per_row = (((avail + gap) / (target + gap)).floor() as usize).max(1);
    let w = ((avail - gap * (per_row.saturating_sub(1)) as f32) / per_row as f32)
        .floor()
        .max(24.0);
    // The 4:3 frame is the proportion of the mock-up tile; the photo fills it (M22.15).
    let h = (w * 0.75).round();
    for row in files.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for &f in row {
                draw_cell(ui, ctx, cs, f, w, h, ThumbFit::Filling);
            }
        });
        ui.add_space(gap);
    }
}

/// The restore target of the trash: a round 24 px icon target on a tile or a row. The owner
/// asked for an icon instead of the word (M22.27, after the face targets of M22.26); the word
/// lives in the hint, so the action stays findable without a legend.
fn restore_icon(
    ui: &mut egui::Ui,
    f: &FileRow,
    rect: egui::Rect,
    lang: i18n::Lang,
) -> egui::Response {
    tile_icon_target(
        ui,
        egui::Id::unique(("restore", f.id)),
        rect,
        Icon::Restore,
        false,
        t(lang, "Restore: back into the usual views — the file was untouched on disk"),
    )
}

/// The row height from the "Tiles" slider: 24 + 0.16 × the value (40 px at 100, 66 px at 260).
/// One slider value for three cell views (M22.2a).
fn list_row_height(tile: f32) -> f32 {
    24.0 + 0.16 * tile.clamp(100.0, 260.0)
}

/// The font size of the name in a list row: 12 px at 100, 14 px at 260.
fn list_name_px(tile: f32) -> f32 {
    12.0 + 2.0 * ((tile.clamp(100.0, 260.0) - 100.0) / 160.0)
}

/// The font size of the monospaced digits (date, size, duration) in a list row: 10–12 px.
fn list_meta_px(tile: f32) -> f32 {
    10.0 + 2.0 * ((tile.clamp(100.0, 260.0) - 100.0) / 160.0)
}

/// The tooltip of the "Tiles" slider says what it scales in the current view: the width of
/// the tile frame, the row height or the list row.
fn tile_hint(style: db::CellStyle) -> &'static str {
    match style {
        db::CellStyle::Tiles => {
            "tile size (a tile: frame width, rows: row height), 100–260 px"
        }
        db::CellStyle::Rows => {
            "row height: the tile width follows the frame's proportion, 100–260 px"
        }
        db::CellStyle::List => {
            "list row size: height, thumbnail and caption size, 100–260 px"
        }
    }
}

/// The caption date of a file: `YYYY-MM-DD HH:MM`, or only `HH:MM` in a day strip of the
/// calendar, where the date is the same for every row (M22.5). A file without a time keeps its
/// date: an empty caption would say nothing.
fn caption_date(f: &FileRow, time_only: bool, lang: i18n::Lang) -> String {
    match f.taken_at.as_deref() {
        Some(raw) => {
            let text = raw.replace('T', " ");
            match text.get(11..16) {
                Some(time) if time_only => time.to_string(),
                _ => text.chars().take(16).collect(),
            }
        }
        None => t(lang, "no date").to_string(),
    }
}

/// The list: a row of the height from the "Tiles" slider (`list_row_height`) — a 4:3
/// thumbnail as tall as `row − 8`, the name, the date and the size. A dense view of names
/// and dates when the photos are known already; the duplicate and trash decisions use the same buttons.
fn draw_list(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, files: &[&FileRow]) {
    // The list scales with the same slider as the tile and the rows: the row height, the 4:3
    // thumbnail as tall as `row − 8` and the font sizes of the name and the digits grow with it.
    let row_h = list_row_height(cs.tile);
    let name_px = list_name_px(cs.tile);
    let meta_px = list_meta_px(cs.tile);
    let thumb_h = (row_h - 8.0).max(16.0);
    let thumb = egui::vec2((thumb_h * 4.0 / 3.0).round(), thumb_h.round());
    for &f in files {
        // A click on the round restore target must not select the row as well.
        let mut restore = false;
        let (rect, resp) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), row_h),
            egui::Sense::click(),
        );
        if !ui.is_rect_visible(rect) {
            continue;
        }
        let th = theme(ui);
        let chosen = cs.picked.contains(&f.id) || cs.selected == Some(f.id);
        let fill = if chosen {
            th.accent_soft
        } else if resp.hovered() {
            th.card
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_CTRL as u8), fill);
        if chosen {
            let bar = egui::Rect::from_min_size(rect.min, egui::vec2(2.0, rect.height()));
            ui.painter().rect_filled(bar, 0.0, th.accent);
        }
        let tr = egui::Rect::from_min_size(
            rect.left_top() + egui::vec2(6.0, (row_h - thumb.y) / 2.0),
            thumb,
        );
        ui.painter().rect_filled(tr, 2.0, th.canvas);
        if !paint_thumb(ui, ctx, cs, f, tr) {
            ui.painter().text(
                tr.center(),
                egui::Align2::CENTER_CENTER,
                "—",
                egui::FontId::proportional(meta_px),
                th.text_dim,
            );
        }
        let name = file_name(&f.path);
        let date = caption_date(f, cs.time_only, cs.lang);
        let kind = if f.kind == "video" {
            match f.duration_s {
                Some(d) => format!("▶ {}", db::duration(d)),
                None => t(cs.lang, "▶ video").to_string(),
            }
        } else {
            match (f.width, f.height) {
                (w, h) if w > 0 && h > 0 => format!("{w}×{h}"),
                _ => String::new(),
            }
        };
        let meta = format!("{date} · {}", db::human_in(cs.lang, f.size));
        let right = if cs.trash_mode {
            40.0
        } else if cs.show_keep {
            96.0
        } else {
            132.0
        };
        let clip = egui::Rect::from_min_max(
            tr.right_top() + egui::vec2(8.0, 0.0),
            egui::pos2(rect.right() - right, rect.bottom()),
        );
        // Two lines of text sit as a block in the centre of the row: at any font size they do not
        // leave its bounds and are not pressed against the edge.
        let name_top = rect.center().y - name_px - 1.0;
        let meta_top = name_top + name_px + 2.0;
        let painter = ui.painter().with_clip_rect(clip);
        painter.text(
            egui::pos2(clip.left(), name_top),
            egui::Align2::LEFT_TOP,
            &name,
            egui::FontId::proportional(name_px),
            if chosen { th.accent } else { th.text },
        );
        painter.text(
            egui::pos2(clip.left(), meta_top),
            egui::Align2::LEFT_TOP,
            &meta,
            egui::FontId::monospace(meta_px),
            th.text_dim,
        );
        ui.painter().text(
            egui::pos2(rect.right() - 8.0, name_top),
            egui::Align2::RIGHT_TOP,
            &kind,
            egui::FontId::monospace(meta_px),
            th.text_med,
        );
        if cs.show_keep {
            let btn = egui::Rect::from_min_size(
                egui::pos2(rect.right() - 78.0, rect.center().y - 10.0),
                egui::vec2(70.0, 20.0),
            );
            if ui
                .place(btn, egui::Button::new(t(cs.lang, "keep")).small())
                .clicked()
            {
                cs.keep_clicked = Some(f.id);
            }
        }
        if cs.trash_mode {
            let btn = egui::Rect::from_center_size(
                egui::pos2(rect.right() - 20.0, rect.center().y),
                egui::vec2(24.0, 24.0),
            );
            if restore_icon(ui, f, btn, cs.lang).clicked() {
                cs.restore_clicked = Some(f.id);
                restore = true;
            }
        }
        if resp.clicked() && !restore {
            let mods = ui.input(|i| i.modifiers);
            cs.clicked = Some((f.id, pick_kind(&mods)));
        }
        if resp.double_clicked() && !restore {
            cs.open_clicked = Some(f.id);
        }
        resp.on_hover_text(&f.path);
    }
    ui.add_space(4.0);
}

/// One layout for every grid (docs/UI.md § Layout). The "by proportion" rows: the frame is
/// fitted into the tile without margins and without cropping, the row height is set by the
/// "Tiles" slider, the tile width follows the frame proportion, and the row takes the width.
fn draw_flow(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, files: &[&FileRow]) {
    if files.is_empty() {
        return;
    }
    let gap = TILE_GAP;
    let target = cs.tile.clamp(100.0, 260.0);
    let avail = ui.available_width().max(64.0);
    let aspects: Vec<f32> = files.iter().map(|f| frame_aspect(f)).collect();
    let mut i = 0usize;
    while i < aspects.len() {
        let (n, fitted) = row_fit(&aspects[i..], avail, target, gap);
        let n = n.min(aspects.len() - i);
        // An incomplete (last) row keeps the target height: it must not be filled by stretching
        // two or three frames.
        let h = if i + n >= files.len() { target } else { fitted };
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for &f in &files[i..i + n] {
                let w = (h * frame_aspect(f)).round().clamp(24.0, avail);
                draw_cell(ui, ctx, cs, f, w, h, ThumbFit::Whole);
            }
        });
        ui.add_space(gap);
        i += n;
    }
}

/// The file name of a tile caption on one line: the band is 22 px, so a long name is cut with an
/// ellipsis instead of wrapping its first line over the preview (found while checking M22.3).
fn caption_name(
    ui: &egui::Ui,
    name: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob {
        wrap: egui::text::TextWrapping {
            max_width,
            max_rows: 1,
            break_anywhere: true,
            overflow_character: Some('…'),
        },
        ..Default::default()
    };
    job.append(
        name,
        0.0,
        egui::TextFormat {
            font_id: font,
            color,
            ..Default::default()
        },
    );
    ui.painter().layout_job(job)
}

/// A file tile: the card = the frame (w×h) + a 22 px caption strip (mock-up: thumbnail
/// card). The frame owns the whole w×h rect; how the snapshot sits in it is up to `fit`.
/// `fit` says how the preview sits in the frame: the grid tile fills it edge to edge, the
/// proportion rows fit it whole (their tile already has the photo's proportion).
fn draw_cell(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    f: &FileRow,
    w: f32,
    h: f32,
    fit: ThumbFit,
) {
    let th = theme(ui);
    let (rect, resp) = ui.allocate_exact_size(
        egui::vec2(w, h + CELL_CAPTION_H),
        egui::Sense::click(),
    );
    let img_rect = egui::Rect::from_min_max(
        rect.min,
        egui::pos2(rect.right(), rect.top() + h),
    );
    let cap_rect = egui::Rect::from_min_max(
        egui::pos2(rect.left(), img_rect.bottom()),
        rect.right_bottom(),
    );
    // A click on the round restore target must not select the tile as well.
    let mut restore = false;
    if ui.is_rect_visible(rect) {
        // A card radius of 8 px: the top corners on the preview, the bottom ones on the caption strip.
        let top = egui::CornerRadius {
            nw: RADIUS_CARD as u8,
            ne: RADIUS_CARD as u8,
            sw: 0,
            se: 0,
        };
        let bottom = egui::CornerRadius {
            nw: 0,
            ne: 0,
            sw: RADIUS_CARD as u8,
            se: RADIUS_CARD as u8,
        };
        // The card body and the well of the preview: the mock-up's tile is a card tone with the
        // lighter `surface-container-highest` well under the snapshot, so a preview that has not
        // loaded yet and the frame around a video's cover still read as a tile (M22.15).
        ui.painter().rect_filled(rect, RADIUS_CARD, th.card);
        ui.painter().rect_filled(img_rect, top, th.field);
        ui.painter().rect_filled(cap_rect, bottom, th.panel);
        let drawn = match fit {
            ThumbFit::Whole => paint_thumb(ui, ctx, cs, f, img_rect),
            ThumbFit::Filling => paint_thumb_filling(ui, ctx, cs, f, img_rect),
        };
        if drawn {
            round_preview(ui, img_rect, RADIUS_CARD, cs.bg, false);
        }
        // The caption: the file name on the left, the date on the right in monospaced digits (M22).
        // The name is cut at the edge of the date rather than running onto the neighbouring tile.
        let name = file_name(&f.path);
        let date = caption_date(f, cs.time_only, cs.lang);
        let chosen = cs.picked.contains(&f.id) || cs.selected == Some(f.id);
        let name_col = if chosen { th.accent } else { th.text_med };
        let font = egui::FontId::monospace(10.0);
        let date_galley = ui.painter().layout_no_wrap(date.clone(), font.clone(), th.text_dim);
        let cap_pad = 4.0;
        // A narrow tile (100 px) with a long date may leave no room for the name: then the name is
        // not drawn at all rather than overlapping the date.
        let name_max = (cap_rect.width() - date_galley.size().x - 3.0 * cap_pad).max(0.0);
        if name_max > 4.0 {
            let name_galley = caption_name(ui, &name, font, name_col, name_max);
            let name_right =
                (cap_rect.right() - date_galley.size().x - 2.0 * cap_pad).max(cap_rect.left());
            let clip = cap_rect.intersect(egui::Rect::from_min_max(
                cap_rect.min,
                egui::pos2(name_right, cap_rect.bottom()),
            ));
            if clip.is_positive() {
                let painter = ui.painter().with_clip_rect(clip);
                painter.galley(
                    egui::pos2(
                        cap_rect.left() + cap_pad,
                        cap_rect.center().y - name_galley.size().y / 2.0,
                    ),
                    name_galley,
                    name_col,
                );
            }
        }
        let cap_painter = ui.painter().with_clip_rect(cap_rect);
        cap_painter.galley(
            egui::pos2(
                cap_rect.right() - date_galley.size().x - cap_pad,
                cap_rect.center().y - date_galley.size().y / 2.0,
            ),
            date_galley,
            th.text_dim,
        );
        // A 1 px border of the card: on a selected one it is covered by the accent outline below.
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(RADIUS_CARD as u8),
            egui::Stroke::new(1.0, th.border),
            egui::StrokeKind::Inside,
        );
        // A hover response: a thin outline and the cursor — the tile reads as a target (M16).
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            ui.painter().rect_stroke(
                rect,
                egui::CornerRadius::same(RADIUS_CARD as u8),
                egui::Stroke::new(1.0, th.border_strong),
                egui::StrokeKind::Inside,
            );
        }
        if !drawn {
            let lang = cs.lang;
            let txt = match f.kind.as_str() {
                "video" => match f.duration_s {
                    Some(d) => format!("{} · {}", t(lang, "▶ video"), db::duration(d)),
                    None => t(lang, "▶ video").to_string(),
                },
                _ => t(lang, "no preview").to_string(),
            };
            ui.painter().text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                txt,
                egui::FontId::proportional(12.0),
                dim(ui),
            );
        }
        // A video is visible by its cover frame, but the caption with the duration does not let it pass for a photo.
        if drawn && f.kind == "video" {
            let strip = egui::Rect::from_min_max(
                egui::pos2(img_rect.left(), img_rect.bottom() - 16.0),
                img_rect.right_bottom(),
            );
            ui.painter()
                .rect_filled(strip, 0.0, egui::Color32::from_black_alpha(130));
            let txt = match f.duration_s {
                Some(d) => format!("▶ {}", db::duration(d)),
                None => t(cs.lang, "▶ video").to_string(),
            };
            ui.painter().text(
                egui::pos2(img_rect.left() + 4.0, img_rect.bottom() - 8.0),
                egui::Align2::LEFT_CENTER,
                txt,
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(235),
            );
        }
        // The decision on a duplicate is visible on the tile in any view, not only on hover.
        match f.decision.as_deref() {
            Some("drop") => {
                ui.painter().rect_filled(
                    img_rect,
                    egui::CornerRadius::same(RADIUS_CARD as u8),
                    egui::Color32::from_black_alpha(150),
                );
                ui.painter().text(
                    img_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    t(cs.lang, "spare"),
                    egui::FontId::proportional(16.0),
                    egui::Color32::from_gray(210),
                );
            }
            Some("keep") => {
                ui.painter().rect_stroke(
                    img_rect,
                    egui::CornerRadius::same(RADIUS_CARD as u8),
                    egui::Stroke::new(2.0, accent(ui)),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
                    egui::Align2::LEFT_TOP,
                    t(cs.lang, "keep"),
                    egui::FontId::proportional(12.0),
                    accent(ui),
                );
            }
            _ => {}
        }
        if cs.show_keep {
            let btn = egui::Rect::from_min_size(
                egui::pos2(img_rect.right() - 78.0, img_rect.top() + 4.0),
                egui::vec2(74.0, 20.0),
            );
            if ui
                .place(btn, egui::Button::new(t(cs.lang, "keep")).small())
                .clicked()
            {
                cs.keep_clicked = Some(f.id);
            }
        }
        if cs.trash_mode {
            ui.painter().rect_filled(
                img_rect,
                egui::CornerRadius::same(RADIUS_CARD as u8),
                egui::Color32::from_black_alpha(90),
            );
            ui.painter().text(
                egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                t(cs.lang, "in trash"),
                egui::FontId::proportional(12.0),
                egui::Color32::from_gray(235),
            );
            // A narrow row tile keeps the target inside the frame.
            let btn = egui::Rect::from_center_size(
                egui::pos2(
                    (img_rect.right() - 18.0).max(img_rect.left() + 18.0),
                    (img_rect.bottom() - 18.0).max(img_rect.top() + 18.0),
                ),
                egui::vec2(24.0, 24.0),
            );
            if restore_icon(ui, f, btn, cs.lang).clicked() {
                cs.restore_clicked = Some(f.id);
                restore = true;
            }
        }
    }
    if cs.picked.contains(&f.id) {
        // A selected tile is visible without hovering: an outline and a square flag, not colour alone.
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(RADIUS_CARD as u8),
            egui::Stroke::new(2.0, accent(ui)),
            egui::StrokeKind::Inside,
        );
        let badge = egui::Rect::from_min_size(
            egui::pos2(img_rect.right() - 20.0, img_rect.top() + 6.0),
            egui::vec2(14.0, 14.0),
        );
        ui.painter()
            .rect_filled(badge, egui::CornerRadius::same(RADIUS_CTRL as u8), accent(ui));
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            "✓",
            egui::FontId::proportional(11.0),
            theme(ui).on_accent,
        );
    } else if cs.selected == Some(f.id) {
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(RADIUS_CARD as u8),
            egui::Stroke::new(2.0, accent(ui)),
            egui::StrokeKind::Inside,
        );
    }
    if resp.clicked() && !restore {
        let mods = ui.input(|i| i.modifiers);
        cs.clicked = Some((f.id, pick_kind(&mods)));
    }
    if resp.double_clicked() && !restore {
        cs.open_clicked = Some(f.id);
    }
    if resp.hovered()
        && let Some(t) = cs.textures.get(&f.id).cloned()
    {
        resp.on_hover_ui_at_pointer(|ui| {
            // The tooltip does not stretch the frame either: 400 px on the long side, the original proportions.
            let big = egui::load::SizedTexture::new(t.id(), t.size_vec2());
            ui.add(egui::Image::new(big).max_size(egui::vec2(400.0, 400.0)));
            ui.label(&f.path);
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let lang = self.settings.lang;
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // M22: the palette is applied before the first shape of the frame and only on a theme change.
        // "System" is read from the OS, so a theme change in the system is visible without a restart.
        let resolved = match self.settings.theme {
            db::ThemeMode::Dark => egui::Theme::Dark,
            db::ThemeMode::Light => egui::Theme::Light,
            db::ThemeMode::System => ctx.system_theme().unwrap_or(egui::Theme::Dark),
        };
        if self.theme_key != Some(resolved) {
            apply_theme(ctx, self.settings.theme);
            self.theme_key = Some(resolved);
        }
        // M22.2b: scrolling is carried through to every area at once — before the frame draws them.
        // While the touchpad coast runs the frames are requested by themselves; at rest they are not.
        if self.scroll.pump(ctx) {
            ctx.request_repaint();
        }
        // M18: the selection lives inside one set of files; a change of the view, filter, search or
        // order clears it and says why. Esc and Ctrl+A work on the same grid.
        self.sync_pick_scope();
        self.pick_keys(ctx);
        // the end of the background indexing
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.status = match res {
                        Ok(mut s) => {
                            s.lang = self.settings.lang;
                            s.to_string()
                        }
                        Err(e) => i18n::fill(lang, "the indexing failed: {}", &[e.to_string()]),
                    };
                    self.rx = None;
                    self.progress = None;
                    self.reload();
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.rx = None;
                    self.progress = None;
                }
            }
        }

        if let Some(rx) = &self.embed_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.embed_rx = None;
                    self.embed_progress = None;
                    match res {
                        Ok((mut embed, mut tags)) => {
                            self.reload();
                            embed.lang = self.settings.lang;
                            tags.lang = self.settings.lang;
                            self.status = format!("{embed}; {tags}");
                        }
                        Err(e) => self.status = i18n::fill(lang, "CLIP is not computed: {}", &[e.to_string()]),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.embed_rx = None;
                    self.embed_progress = None;
                }
            }
        }

        if let Some(rx) = &self.retag_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.retag_rx = None;
                    self.embed_progress = None;
                    match res {
                        Ok(mut stats) => {
                            self.tag_dict_dirty = false;
                            self.reload();
                            stats.lang = self.settings.lang;
                            self.status = stats.to_string();
                        }
                        Err(e) => self.status = i18n::fill(lang, "the tags are not recomputed: {}", &[e.to_string()]),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.retag_rx = None;
                    self.embed_progress = None;
                }
            }
        }

        if let Some(rx) = &self.search_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.search_rx = None;
                    match res {
                        Ok((model, hits, ms)) => {
                            self.ml_model = Some(model);
                            self.status =
                                i18n::fill(
                                    lang,
                                    "word search: {} files in {} ms",
                                    &[hits.len().to_string(), ms.to_string()],
                                );
                            self.semantic_hits = Some(hits.into_iter().map(|(id, _)| id).collect());
                        }
                        Err(e) => self.status = i18n::fill(lang, "the search failed: {}", &[e.to_string()]),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(100));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.search_rx = None;
                }
            }
        }

        if let Some(rx) = &self.download_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.download_rx = None;
                    self.download_progress = None;
                    match res {
                        Ok(mut report) => {
                            self.settings.model_id = report.model.to_string();
                            self.ml_model = None;
                            self.semantic_hits = None;
                            self.save_settings();
                            self.reload();
                            report.lang = self.settings.lang;
                            self.status = report.to_string();
                        }
                        Err(e) => self.status = i18n::fill(lang, "the model is not downloaded: {}", &[e.to_string()]),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.download_rx = None;
                    self.download_progress = None;
                }
            }
        }

        if let Some(rx) = &self.ocr_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.ocr_rx = None;
                    self.ocr_progress = None;
                    match res {
                        Ok(mut stats) => {
                            self.reload();
                            stats.lang = self.settings.lang;
                            self.status = if stats.cancelled {
                                stats.to_string()
                            } else if stats.candidates == 1 {
                                if stats.chars > 0 {
                                    i18n::fill(lang, "the text is recognized: {} characters", &[stats.chars.to_string()])
                                } else {
                                    t(lang, "no text is found on this photo").into()
                                }
                            } else {
                                stats.to_string()
                            };
                        }
                        Err(e) => self.status = i18n::fill(lang, "the text is not recognized: {}", &[format!("{:#}", e)]),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.ocr_rx = None;
                    self.ocr_progress = None;
                }
            }
        }

        if let Some(rx) = &self.face_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.face_rx = None;
                    self.face_progress = None;
                    match res {
                        FaceReply::Scan(Ok(mut stats)) => {
                            self.reload();
                            stats.lang = self.settings.lang;
                            self.status = stats.to_string();
                        }
                        FaceReply::Scan(Err(e)) => {
                            let lang = self.settings.lang;
                            self.status = format!("{}: {e:#}", t(lang, "no faces found"))
                        }
                        FaceReply::Cluster(Ok(mut stats)) => {
                            self.reload();
                            stats.lang = self.settings.lang;
                            self.status = stats.to_string();
                        }
                        FaceReply::Cluster(Err(e)) => {
                            let lang = self.settings.lang;
                            self.status = format!("{}: {e:#}", t(lang, "groups not built"))
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.face_rx = None;
                    self.face_progress = None;
                }
            }
        }

        if let Some(rx) = &self.trash_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.trash_rx = None;
                    self.trash_progress = None;
                    match res {
                        Ok(report) => {
                            self.reload();
                            self.status = report.to_string();
                        }
                        Err(e) => {
                            let lang = self.settings.lang;
                            self.status =
                                format!("{}: {e:#}", t(lang, "trash not emptied"))
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.trash_rx = None;
                    self.trash_progress = None;
                }
            }
        }

        if let Some(rx) = &self.face_download_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.face_download_rx = None;
                    self.face_download_progress = None;
                    match res {
                        Ok(mut report) => {
                            report.lang = self.settings.lang;
                            self.status = report.to_string();
                            self.reload();
                        }
                        Err(e) => {
                            let lang = self.settings.lang;
                            self.status =
                                format!("{}: {e:#}", t(lang, "face model not downloaded"))
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.face_download_rx = None;
                    self.face_download_progress = None;
                }
            }
        }

        if let Some(rx) = &self.export_rx {
            match rx.try_recv() {
                Ok(mut report) => {
                    self.export_rx = None;
                    self.export_progress = None;
                    report.lang = self.settings.lang;
                    self.status = report.to_string();
                    self.export_report = Some(report);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.export_rx = None;
                    self.export_progress = None;
                    self.status = t(lang, "the export was interrupted without a report").into();
                }
            }
        }

        if let Some(rx) = &self.integrity_rx {
            match rx.try_recv() {
                Ok((job, res)) => {
                    self.integrity_rx = None;
                    self.integrity_progress = None;
                    // The next record of the manifest will ask for a confirmation again.
                    if job == IntegrityJob::Manifest {
                        self.integrity_overwrite = false;
                    }
                    match res {
                        Ok(outcome) => {
                            self.integrity_error.clear();
                            let warn = self.note_integrity(&outcome);
                            let lang = self.settings.lang;
                            let done = outcome.status(lang);
                            self.status = match warn {
                                Some(w) => format!("{done}; {w}"),
                                None => done,
                            };
                            self.integrity_result = Some(outcome);
                        }
                        Err(e) => {
                            // A failed pass does not touch the previous successful report and date: the reminder keeps
                            // counting from it.
                            self.integrity_error = format!("{e:#}");
                            let lang = self.settings.lang;
                            self.status = format!(
                                "{}: {}",
                                job.title(lang),
                                t(lang, "error")
                            );
                        }
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.integrity_rx = None;
                    self.integrity_progress = None;
                    self.status = t(lang, "the check was interrupted without a report").into();
                }
            }
        }

        egui::Panel::top("top")
            .resizable(false)
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .resizable(false)
            .show(ui, |ui| self.status_bar(ui));

        // The details live in the right panel while the grid keeps 4 columns; in a narrow window
        // the card moves to the left one, and the grid does not shrink.
        // There is no card on "Integrity": it checks folders, not files. The "People" screen keeps
        // its group list in the right panel instead of the card (M22.26), and only a narrow window
        // puts that list back into the left panel.
        let wide = ui.available_width() >= DETAILS_MIN_W;
        let faces_panel = self.view == View::Faces && wide;
        let show_faces = self.view == View::Faces && !faces_panel;
        let details = self.details_open
            && self.view != View::Integrity
            && self.view != View::Faces
            && wide;
        let show_card =
            self.details_open && self.view != View::Integrity && self.view != View::Faces && !wide;
        egui::Panel::left("left")
            .exact_size(LEFT_W)
            .resizable(false)
            .show(ui, |ui| self.left_panel(ui, show_card, show_faces));
        if faces_panel {
            egui::Panel::right("faces")
                .exact_size(RIGHT_W)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(theme(ui).panel))
                .show(ui, |ui| self.faces_panel(ui));
        } else if details {
            // The panel carries no margin of its own: the 1 px line stands on its edge and the
            // file card keeps the mock-up's 12 px of air (M22.16).
            egui::Panel::right("details")
                .exact_size(RIGHT_W)
                .resizable(false)
                .frame(egui::Frame::NONE.fill(theme(ui).panel))
                .show(ui, |ui| self.details_panel(ui));
        }

        self.settings_modal(ctx);
        self.roots_modal(ctx);
        self.picker_modal(ctx);
        self.storage_modal(ctx);
        self.album_modal(ctx);
        self.album_pick_modal(ctx);
        self.export_modal(ctx);
        self.trash_modal(ctx);
        self.poll_photo(ctx);
        self.poll_video(ctx);

        // "Integrity" is a form, not a grid: it needs neither the tiles nor their cache nor the order.
        // The screen is drawn before `self.items`/`self.textures` are borrowed and leaves the frame by itself.
        // A form is still a screen of the same window: the canvas tone and the 12/6 margin of the
        // content panel, so the title stands where the title of the grid stands (M22.9).
        if self.view == View::Integrity {
            let canvas = theme(ui).canvas;
            egui::CentralPanel::default().show(ui, |ui| {
                egui::Frame::new()
                    .fill(canvas)
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| self.integrity_panel(ui));
            });
            return;
        }

        let visible = self.filtered();
        // The tile order is computed here and nowhere else (M17): the grid, the full screen and the
        // selection (M18) take the very same list.
        let ordered = self.ordered(&visible);
        // The order for Shift+click — the same ids in the same order as the tiles.
        let pick_order: Vec<i64> = ordered.iter().map(|&i| self.items[i].id).collect();
        // The snapshot of the filters is taken before the grid borrows `&mut self.textures`.
        let chips = self.chips_snapshot();
        let tag_filter_now = self.tag_filters.clone();
        let tag_filter_raw = tag_filter_now
            .iter()
            .map(|t| self.tag_threshold(t))
            .fold(0.0_f32, f32::max);
        let items = std::mem::take(&mut self.items);
        let grid_cell_style = self.grid_cell_style();
        let mut cells = Cells {
            textures: &mut self.textures,
            missing: &mut self.missing,
            thumbs_dir: &self.thumbs_dir,
            selected: self.selected,
            picked: &self.picked,
            clicked: None,
            open_clicked: None,
            show_keep: self.view == View::Dupes,
            keep_clicked: None,
            trash_mode: self.view == View::Trash,
            restore_clicked: None,
            tile: self.tile_size,
            lang: self.settings.lang,
            budget: 8,
            cell_style: grid_cell_style,
            time_only: false,
            // The background a tile lies on: the corner cuts of the preview paint exactly it.
            // A duplicate group is a card, so its tiles lie on the card tone (M22.9).
            bg: if self.view == View::Dupes {
                theme(ui).card
            } else {
                theme(ui).canvas
            },
        };
        let groups = std::mem::take(&mut self.groups);
        let trash_items = std::mem::take(&mut self.trash_items);
        let albums = std::mem::take(&mut self.albums);
        let album_selected = self.album_selected;
        let mut album_expanded = std::mem::take(&mut self.album_expanded);
        let mut album_browse = self.album_browse_all;
        let mut album_action: Option<AlbumAction> = None;
        let mut album_dialog_request: Option<AlbumDialog> = None;
        let mut album_export_request: Option<i64> = None;
        let mut trash_clear = false;
        let mut trash_restore_all = false;
        let face_clusters = std::mem::take(&mut self.face_clusters);
        let face_views = std::mem::take(&mut self.face_views);
        let mut face_picked = self.face_picked.clone();
        let mut clear_picked = false;
        let mut face_group_input = self.face_group_input.clone();
        let mut face_cells = FaceCells {
            textures: std::mem::take(&mut self.face_textures),
            missing: std::mem::take(&mut self.face_missing),
            dir: db::cache_dir().join("faces"),
            selected_file: self.selected,
            split_clicked: None,
            open_clicked: None,
            viewer_clicked: None,
            picked: face_picked.clone(),
            tile: self.tile_size,
        };
        let face_selected = self.face_selected_cluster;
        let face_together = self.face_together;
        let face_picked_clusters = self.face_picked_clusters.clone();
        let mut face_label_input = self.face_label_input.clone();
        let face_total = self.face_faces;
        let face_photos = self.face_photos;
        let mut face_save_label: Option<String> = None;
        let mut face_assign: Option<String> = None;
        let mut reset = false;
        let mut empty_click = false;
        let mut album_pick_request = false;
        let mut view_save = false;
        let mut clear_chips = FilterClear::default();
        let view = self.view;
        let lang = self.settings.lang;
        let files_n = |n: usize| i18n::count(lang, n,"file");
        let mut cal_year = self.cal_year;
        let mut cal_month = self.cal_month;
        let mut cal_day = self.cal_day.clone();
        let mut cal_scale = self.cal_scale;
        let mut cal_week = self.cal_week;

        let (title, count) = match view {
            View::Grid => (
                t(lang, "All files").to_string(),
                if visible.len() == items.len() {
                    files_n(items.len())
                } else {
                    {
                            i18n::fill(lang, "{} of {} files", &[visible.len().to_string(), items.len().to_string()])
                        }
                },
            ),
            // M22.6: the heading names the view, not the open album — the album's own card header
            // (the name at 14 px and its facts) is drawn above its sub-albums, so the name is not
            // printed twice on one screen.
            View::Albums => (
                t(lang, "Albums").to_string(),
                i18n::count(lang, albums.len(), "album"),
            ),
            View::Dupes => {
                let waste: i64 = groups.iter().map(|g| g.waste).sum();
                (
                    t(lang, "Duplicates & series").to_string(),
                    {
                            i18n::fill(lang, "{} groups, {} spare", &[groups.len().to_string(), db::human_in(lang, waste).to_string()])
                        },
                )
            }
            // The facts of the calendar name the layer: a year, a month or a day — the title of the
            // screen stays "Calendar", so the tabs above the grid do not have to be read twice.
            View::Calendar => (
                t(lang, "Calendar").to_string(),
                match cal_scale {
                    CalScale::Year => format!("{cal_year}"),
                    CalScale::Month => {
                        format!("{} {cal_year}", i18n::month_name(lang, cal_month as usize))
                    }
                    CalScale::Week => format!(
                        "{} — {}",
                        cal_week.format("%d.%m"),
                        (cal_week + chrono::Duration::days(6)).format("%d.%m.%Y")
                    ),
                    CalScale::Day => cal_day
                        .as_deref()
                        .map(|d| day_title(lang, d))
                        .unwrap_or_else(|| t(lang, "no day selected").to_string()),
                },
            ),
            View::Faces => (
                t(lang, "People").to_string(),
                {
                        i18n::fill(lang, "{} faces in {} photos", &[face_total.to_string(), face_photos.to_string()])
                    },
            ),
            View::Trash => (
                t(lang, "Trash").to_string(),
                i18n::fill(lang, "{} files, {}", &[self.trash_items.len().to_string(), db::human_in(lang, self.trash_items.iter().map(|r| r.size).sum()).to_string()]),
            ),
            // The "Integrity" screen is drawn before the grid (see `ui`); the branch is needed only
            // for the enum match to be complete.
            View::Integrity => (
                t(lang, "Integrity").to_string(),
                t(lang, "comparing copies and a sha256 manifest").to_string(),
            ),
        };

        egui::CentralPanel::default().show(ui, |ui| {
            // The content panel: the canvas background, the view heading and the row of chips.
            egui::Frame::new()
                .fill(theme(ui).canvas)
                .inner_margin(egui::Margin::symmetric(12, 6))
                .show(ui, |ui| {
            // The content ribbon — one 44 px row, the mock-up's `h-11`: the view name and its
            // facts on the left, the controls of the view on the right (M22.19). Before this the
            // heading, the controls and the chips took three rows of the panel.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), RIBBON_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(egui::RichText::new(title).size(18.0).strong());
                    ui.label(num(ui, count, 12.0).color(dim(ui)));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        // The cell view (M22): the mock-up tile is the main one, the rows and the
                        // list come second. The tile size is the pill of the top bar (M22.19).
                        // On the Albums screen the "Cards / List" switch of the album control row
                        // is the one look switch: it changes the album cards and the snapshots
                        // together, so no second, dead "List" sits in the ribbon (M22.22/M22.23).
                        // The face tiles have one look of their own, so the switch is not drawn
                        // there either (M22.26).
                        if !matches!(view, View::Albums | View::Faces) {
                            let mut style = self.settings.cell_style;
                            for (value, icon, label, hint) in [
                                (
                                    db::CellStyle::Tiles,
                                    Icon::GridView,
                                    t(lang, "Tile"),
                                    t(lang, "as in the mock-up: a 4:3 frame, the snapshot fills it, the sides are cut"),
                                ),
                                (
                                    db::CellStyle::Rows,
                                    Icon::ViewAgenda,
                                    t(lang, "Rows"),
                                    t(lang, "proportion rows: the tile width follows the frame"),
                                ),
                                (
                                    db::CellStyle::List,
                                    Icon::ViewList,
                                    t(lang, "List"),
                                    t(lang, "a list: name, date and size in one row"),
                                ),
                            ] {
                                if ui
                                    .add(WordButton::picked(icon, label, style == value).compact())
                                    .on_hover_text(hint)
                                    .clicked()
                                {
                                    style = value;
                                }
                            }
                            if style != self.settings.cell_style {
                                self.settings.cell_style = style;
                                view_save = true;
                            }
                            ui.add_space(6.0);
                            hairline_vertical(ui);
                            ui.add_space(6.0);
                        }
                        // The order is available in the grid and inside an album (M19), but not
                        // in the tree.
                        let album_scope_now =
                            view == View::Albums && self.album_selected.is_some();
                        if view == View::Grid || album_scope_now {
                            // M17: how to look (months or a run) and in which order.
                            // While a semantic search runs the search sets the order — the switch
                            // is dimmed.
                            let searching = self.semantic_hits.is_some();
                            let mut sort = self.settings.grid_sort;
                            let sort_resp = ui.add_enabled_ui(!searching, |ui| {
                                let resp =
                                    ui.add(WordButton::new(Icon::Sort, t(lang, "Sort")));
                                egui::Popup::menu(&resp).show(|ui| {
                                    if album_scope_now {
                                        ui.selectable_value(
                                            &mut sort,
                                            db::GridSort::Album,
                                            t(lang, "Album order"),
                                        );
                                    }
                                    for (value, label) in GRID_SORTS {
                                        ui.selectable_value(&mut sort, value, t(lang, label));
                                    }
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} {}",
                                            t(lang, "now:"),
                                            sort_label(sort, lang)
                                        ))
                                        .size(11.0)
                                        .color(dim(ui)),
                                    );
                                });
                                resp
                            });
                            sort_resp
                                .inner
                                .on_disabled_hover_text(t(lang, "the search sets the order"))
                                .on_hover_text(t(lang, "tile order: date, name or size"));
                            if sort != self.settings.grid_sort {
                                self.settings.grid_sort = sort;
                                view_save = true;
                            }
                            if view == View::Grid {
                                ui.add_space(6.0);
                                hairline_vertical(ui);
                                ui.add_space(6.0);
                                let mut layout = self.settings.grid_layout;
                                for (value, icon, label) in [
                                    (db::GridLayout::Flat, Icon::ViewAgenda, t(lang, "Flat")),
                                    (
                                        db::GridLayout::Months,
                                        Icon::CalendarMonth,
                                        t(lang, "By months"),
                                    ),
                                ] {
                                    // `add_enabled`, not `add_enabled_ui`: a child ui reports
                                    // its size only after it is drawn, so the row could not wrap
                                    // it and the last button left the panel (M22.8).
                                    let r = ui.add_enabled(
                                        !searching,
                                        WordButton::picked(icon, label, layout == value)
                                            .compact(),
                                    );
                                    if r.clicked() {
                                        layout = value;
                                    }
                                    r.on_disabled_hover_text(t(
                                        lang,
                                        "the search sets the order",
                                    ));
                                }
                                if layout != self.settings.grid_layout {
                                    self.settings.grid_layout = layout;
                                    view_save = true;
                                }
                            }
                        }
                    });
                },
            );
            if matches!(view, View::Grid | View::Calendar | View::Albums) && chips.any() {
                ui.add_space(2.0);
                clear_chips = draw_filter_chips(ui, &chips);
            }
            ui.add_space(2.0);
            ui.separator();
            match view {
            View::Grid => {
                if items.is_empty() {
                    ui.add_space(12.0);
                    ui.label(t(
                        lang,
                        "Add a folder with the “+” button or type a path above, then press “Index”.",
                    ));
                    return;
                }
                if visible.is_empty() {
                    ui.add_space(12.0);
                    match tag_filter_now.len() {
                        0 => {
                            ui.label(t(lang, "nothing found"));
                        }
                        1 => {
                            ui.label(i18n::fill(
                                lang,
                                "no photos with the tag “{}” above the threshold {}% ({}) — lower the threshold slider in the left panel",
                                &[
                                    tag_filter_now[0].clone(),
                                    self.settings.tag_show_pct.to_string(),
                                    format!("{tag_filter_raw:.2}"),
                                ],
                            ));
                        }
                        _ => {
                            ui.label(t(
                                lang,
                                "no photos carry all the picked tags above their thresholds — lower the threshold slider in the left panel",
                            ));
                        }
                    }
                    if word_button(ui, Icon::Close, t(lang, "Reset the filter")).clicked() {
                        reset = true;
                    }
                    return;
                }
                // A click on an empty place clears the selection: the area is registered before the
                // tiles, so a click on a tile stays a click on a tile.
                let bg = ui.interact(
                    ui.available_rect_before_wrap(),
                    egui::Id::unique("grid-empty"),
                    egui::Sense::click(),
                );
                let mut heads: Vec<MonthHead> = Vec::new();
                let grid_out = egui::ScrollArea::vertical()
                    .id_salt("grid")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // The order came from `App::ordered`; only the layout is here.
                        // "Run" — one continuous grid without month headings.
                        // In a semantic search the search sets the order, so the list is flat.
                        if self.semantic_hits.is_some()
                            || self.settings.grid_layout == db::GridLayout::Flat
                        {
                            let row: Vec<&FileRow> = ordered.iter().map(|&i| &items[i]).collect();
                            draw_files(ui, ctx, &mut cells, &row);
                            return;
                        }
                        // "By months": the months in sort order, "No date" as the last group in both
                        // directions; inside a month — the `ordered` order.
                        let ascend = self.settings.grid_sort == db::GridSort::DateAsc;
                        for (key, ids) in month_groups(&ordered, &items, ascend) {
                            let bytes: i64 = ids.iter().map(|&i| items[i].size).sum();
                            let head_top = ui.cursor().top();
                            month_header(ui, &key, ids.len(), bytes, lang);
                            heads.push(MonthHead {
                                top: head_top,
                                height: ui.cursor().top() - head_top,
                                key,
                                files: ids.len(),
                                bytes,
                            });
                            let row: Vec<&FileRow> = ids.iter().map(|&i| &items[i]).collect();
                            draw_files(ui, ctx, &mut cells, &row);
                            ui.add_space(24.0);
                        }
                    });
                // M22.4: the heading of the group under the top edge stays in view.
                draw_sticky_month(ui, grid_out.inner_rect, &heads, lang);
                if bg.clicked() && cells.clicked.is_none() {
                    empty_click = true;
                }
            }
            View::Albums => {
                // The control row of the screen (M22.20, the mock-up's `photocat_3`): the filter of
                // the album names, their order, the cards-or-list look, and the one main action of
                // the view with the accent. All of it is a look: the album order stays the data of
                // the export.
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        if ui
                            .add(WordButton::primary(
                                Icon::CreateNewFolder,
                                t(lang, "+ New album"),
                            ))
                            .on_hover_text(t(
                                lang,
                                "an album is data only: the files on disk do not change",
                            ))
                            .clicked()
                        {
                            album_dialog_request = Some(AlbumDialog::New);
                        }
                        let mut list = self.settings.album_list;
                        for (value, icon, label) in [
                            (false, Icon::GridView, t(lang, "Cards")),
                            (true, Icon::ViewList, t(lang, "List")),
                        ] {
                            if ui
                                .add(WordButton::picked(icon, label, list == value).compact())
                                .on_hover_text(t(
                                    lang,
                                    "the album cards or rows, and its photos as tiles or a list",
                                ))
                                .clicked()
                            {
                                list = value;
                            }
                        }
                        if list != self.settings.album_list {
                            self.settings.album_list = list;
                            view_save = true;
                        }
                        let mut sort = self.settings.album_sort;
                        let resp = ui.add(WordButton::new(Icon::Sort, t(lang, "Sort")));
                        egui::Popup::menu(&resp).show(|ui| {
                            for value in db::AlbumSort::ALL {
                                ui.selectable_value(&mut sort, value, album_sort_label(value, lang));
                            }
                            ui.label(
                                egui::RichText::new(format!(
                                    "{} {}",
                                    t(lang, "now:"),
                                    album_sort_label(sort, lang)
                                ))
                                .size(11.0)
                                .color(dim(ui)),
                            );
                        });
                        if sort != self.settings.album_sort {
                            self.settings.album_sort = sort;
                            view_save = true;
                        }
                        ui.add(
                            egui::TextEdit::singleline(&mut self.album_filter)
                                .hint_text(t(lang, "Filter the albums"))
                                .desired_width(150.0),
                        )
                        .on_hover_text(t(lang, "a part of the album name"));
                    });
                });
                ui.add_space(4.0);
                let chosen: Option<db::AlbumNode> = album_selected
                    .and_then(|id| albums.iter().find(|n| n.id == id).cloned());
                // The breadcrumb: the way up from the open album, one press per level (M22.20).
                if let Some(node) = &chosen {
                    let mut crumbs: Vec<(i64, String)> = Vec::new();
                    let mut at = node.parent_id;
                    for _ in 0..256 {
                        if at == 0 {
                            break;
                        }
                        let Some(up) = albums.iter().find(|n| n.id == at) else {
                            break;
                        };
                        crumbs.push((up.id, up.name.clone()));
                        at = up.parent_id;
                    }
                    crumbs.reverse();
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        if ui
                            .add(WordButton::small(
                                Icon::CollectionsBookmark,
                                t(lang, "All albums"),
                            ))
                            .on_hover_text(t(lang, "the cards of the top level albums"))
                            .clicked()
                        {
                            album_action = Some(AlbumAction::Root);
                        }
                        for (id, name) in &crumbs {
                            ui.label(egui::RichText::new("›").size(11.0).color(dim(ui)));
                            if ui
                                .add(WordButton::small(Icon::Folder, name))
                                .on_hover_text(t(lang, "open the album"))
                                .clicked()
                            {
                                album_action = Some(AlbumAction::Select(*id));
                            }
                        }
                    });
                    ui.add_space(2.0);
                    ui.separator();
                }
                let Some(node) = chosen.clone() else {
                    // The root of the albums: the cards of the top level, as in the mock-up. An
                    // album without parents is what the owner sees first.
                    if albums.is_empty() {
                        ui.add_space(8.0);
                        ui.label(
                            t(
                                lang,
                                "Create the first album with “+ New album”: inside are the sub-albums and the photos in the order they will lie on disk.",
                            ),
                        );
                        ui.label(
                            egui::RichText::new(t(
                                lang,
                                "An album is a set of photos with an order and sub-albums. Data only: the files on disk do not change.",
                            ))
                            .size(11.0)
                            .color(dim(ui)),
                        );
                        return;
                    }
                    let cards = album_cards_of(
                        &albums,
                        0,
                        &self.album_filter,
                        self.settings.album_sort,
                    );
                    if cards.is_empty() {
                        ui.add_space(8.0);
                        ui.label(i18n::fill(
                            lang,
                            "no album with the name “{}”",
                            &[self.album_filter.trim().to_string()],
                        ));
                        if word_button(ui, Icon::Close, t(lang, "Reset the filter")).clicked() {
                            self.album_filter.clear();
                        }
                        return;
                    }
                    let list = self.settings.album_list;
                    egui::ScrollArea::vertical()
                        .id_salt("album-root-cards")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(id) = draw_album_cards(
                                ui,
                                ctx,
                                &mut cells,
                                &cards,
                                &items,
                                self.tile_size,
                                None,
                                list,
                                lang,
                            ) {
                                album_action = Some(AlbumAction::Select(id));
                            }
                        });
                    return;
                };
                // The tree (M19) stays inside an album: there it is the nesting and the order the
                // cards cannot show. The root of the albums is the card grid alone (M22.20).
                ui.add_space(2.0);
                egui::ScrollArea::vertical()
                    .id_salt("album-tree")
                    .max_height(190.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        if let Some(id) =
                            draw_album_tree(ui, &albums, album_selected, &mut album_expanded, lang)
                        {
                            album_action = Some(AlbumAction::Select(id));
                        }
                    });
                ui.separator();
                // M22.6: the open album is a card header — the name at 14 px and the facts of the
                // card in mono 11 — over the action row of the album itself.
                ui.add_space(4.0);
                album_header(ui, &node, lang);
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    if word_button(ui, Icon::Add, t(lang, "+ Sub-album")).clicked() {
                        album_dialog_request = Some(AlbumDialog::Sub);
                    }
                    if word_button(ui, Icon::Edit, t(lang, "Rename")).clicked() {
                        album_dialog_request = Some(AlbumDialog::Rename);
                    }
                    if word_button(ui, Icon::MoveTo, t(lang, "Move to…")).clicked() {
                        album_dialog_request = Some(AlbumDialog::Move);
                    }
                    if ui
                        .add_enabled(
                            node.position > 0,
                            WordButton::new(Icon::ArrowUp, t(lang, "Up")),
                        )
                        .on_hover_text(t(lang, "move the album up among its siblings"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::Up);
                    }
                    if ui
                        .add_enabled(
                            node.position + 1
                                < albums
                                    .iter()
                                    .filter(|n| n.parent_id == node.parent_id)
                                    .count() as i64,
                            WordButton::new(Icon::ArrowDown, t(lang, "Down")),
                        )
                        .on_hover_text(t(lang, "move the album down among its siblings"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::Down);
                    }
                    if ui
                        .add(WordButton::new(Icon::Delete, t(lang, "Delete the album")))
                        .on_hover_text(t(
                            lang,
                            "delete the rows of the album and its sub-albums; the files on disk stay",
                        ))
                        .clicked()
                    {
                        album_dialog_request = Some(AlbumDialog::Delete);
                    }
                });
                let children =
                    album_cards_of(&albums, node.id, &self.album_filter, self.settings.album_sort);
                if !children.is_empty() {
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(t(lang, "Sub-albums"))
                            .size(11.0)
                            .color(dim(ui)),
                    );
                    // The cards scale with the "Tiles" slider, so a row of them can outgrow the
                    // window: one row is always whole, two rows at most, the rest scrolls, and the
                    // grid below keeps 220 px — many sub-albums must not hide the album's files.
                    let card_h = if self.settings.album_list {
                        album_row_height(self.tile_size)
                    } else {
                        album_card_size(self.tile_size).y
                    };
                    let room = (ui.available_height() - 220.0).clamp(card_h, card_h * 2.0 + TILE_GAP);
                    let list = self.settings.album_list;
                    egui::ScrollArea::vertical()
                        .id_salt("album-cards")
                        .max_height(room)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            if let Some(id) = draw_album_cards(
                                ui,
                                ctx,
                                &mut cells,
                                &children,
                                &items,
                                self.tile_size,
                                album_selected,
                                list,
                                lang,
                            ) {
                                album_action = Some(AlbumAction::Select(id));
                            }
                        });
                }
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    // "All files" inside an album — a temporary look to gather photos into an album:
                    // otherwise the selection lives only in that grid (M18).
                    if ui
                        .add(WordButton::picked(
                            Icon::CollectionsBookmark,
                            t(lang, "In the album"),
                            !album_browse,
                        ))
                        .on_hover_text(t(lang, "the photos of this album in its order"))
                        .clicked()
                    {
                        album_browse = false;
                    }
                    if ui
                        .add(WordButton::picked(
                            Icon::PhotoLibrary,
                            t(lang, "All files"),
                            album_browse,
                        ))
                        .on_hover_text(
                            t(
                                lang,
                                "a temporary look at the whole archive: pick photos and press “Add the picked ones”",
                            ),
                        )
                        .clicked()
                    {
                        album_browse = true;
                    }
                    ui.separator();
                    let picked_now = self.picked.len();
                    if ui
                        .add_enabled(
                            picked_now > 0,
                            WordButton::new(
                                Icon::Add,
                                i18n::fill(
                                    lang,
                                    "Add the picked ones ({})",
                                    &[picked_now.to_string()],
                                ),
                            ),
                        )
                        .on_hover_text(t(
                            lang,
                            "add the marked photos to the end of the album; a repeat does not duplicate rows",
                        ))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::AddPicked);
                    }
                    let in_album = self
                        .picked
                        .iter()
                        .filter(|id| self.album_set.contains(id))
                        .count();
                    if ui
                        .add_enabled(
                            in_album > 0,
                            WordButton::new(
                                Icon::Remove,
                                i18n::fill(
                                    lang,
                                    "Remove from the album ({})",
                                    &[in_album.to_string()],
                                ),
                            ),
                        )
                        .on_hover_text(t(
                            lang,
                            "the album rows go away; the files on disk do not change",
                        ))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::RemovePicked);
                    }
                    let file_in_album =
                        self.selected.is_some_and(|id| self.album_set.contains(&id));
                    if ui
                        .add_enabled(
                            file_in_album,
                            WordButton::new(Icon::ArrowUp, t(lang, "File up")),
                        )
                        .on_hover_text(t(lang, "move the selected photo up in the album order"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::FileUp);
                    }
                    if ui
                        .add_enabled(
                            file_in_album,
                            WordButton::new(Icon::ArrowDown, t(lang, "File down")),
                        )
                        .on_hover_text(t(lang, "move the selected photo down in the album order"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::FileDown);
                    }
                    if ui
                        .add(WordButton::new(Icon::Download, t(lang, "Export to disk…")))
                        .on_hover_text(
                            t(
                                lang,
                                "file copies into a folder: the structure and order as in the album; nothing in the target is overwritten",
                            ),
                        )
                        .clicked()
                    {
                        album_export_request = Some(node.id);
                    }
                });
                if album_browse {
                    ui.label(
                        egui::RichText::new(
                            t(
                                lang,
                                "A temporary look: the grid shows the whole archive. Pick photos and press “Add the picked ones”.",
                            ),
                        )
                        .size(11.0)
                        .color(dim(ui)),
                    );
                }
                ui.add_space(4.0);
                if visible.is_empty() {
                    ui.add_space(8.0);
                    ui.label(if album_browse {
                        t(lang, "The archive has no photos under the current filter.")
                    } else if node.files == 0 {
                        t(
                            lang,
                            "The album has no files yet — switch to “All files”, pick photos and press “Add the picked ones”.",
                        )
                    } else {
                        t(lang, "Every file of the album is in the app trash — restore them or remove the rows.")
                    });
                    return;
                }
                let bg = ui.interact(
                    ui.available_rect_before_wrap(),
                    egui::Id::unique("album-grid-empty"),
                    egui::Sense::click(),
                );
                egui::ScrollArea::vertical()
                    .id_salt("album-grid")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let row: Vec<&FileRow> = ordered.iter().map(|&i| &items[i]).collect();
                        draw_files(ui, ctx, &mut cells, &row);
                    });
                if bg.clicked() && cells.clicked.is_none() {
                    empty_click = true;
                }
            }
            View::Dupes => {
                if groups.is_empty() {
                    ui.label(t(lang, "Press “Find groups” on the left."));
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (n, g) in groups.iter().enumerate() {
                            // A group is a card of the palette, the same tone a tile lies on (M22.9).
                            card_block(ui, |ui| {
                                let reason = g.reason.label(lang);
                                let advice_text = g.advice.label(lang);
                                let facts = i18n::fill(
                                    lang,
                                    "{} files · {} spare",
                                    &[
                                        g.members.len().to_string(),
                                        db::human_in(lang, g.waste),
                                    ],
                                );
                                ui.horizontal(|ui| {
                                    ui.label(num(ui, format!("#{}", n + 1), 11.0).color(dim(ui)));
                                    ui.label(egui::RichText::new(reason).size(13.0).strong());
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(num(ui, facts, 11.0).color(dim(ui)));
                                        },
                                    );
                                });
                                let keep = dupes::keep_of(&items, g);
                                let name = Path::new(&items[keep].path)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or(&items[keep].path);
                                let decided = g
                                    .members
                                    .iter()
                                    .any(|&i| items[i].decision.as_deref() == Some("keep"));
                                let advice = i18n::fill(lang, "{}: keep {} — {}", &[if decided { "decision" } else { "advice" }.to_string(), name.to_string(), advice_text.to_string()]);
                                ui.label(egui::RichText::new(advice).size(11.0).color(dim(ui)));
                                ui.add_space(4.0);
                                let row: Vec<&FileRow> =
                                    g.members.iter().map(|&i| &items[i]).collect();
                                draw_files(ui, ctx, &mut cells, &row);
                            });
                        }
                    });
            }
            View::Calendar => {
                egui::ScrollArea::vertical()
                    .id_salt("calendar-scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let mut by_day: std::collections::BTreeMap<String, Vec<usize>> =
                            std::collections::BTreeMap::new();
                        for &i in &visible {
                            let k = items[i]
                                .taken_at
                                .as_deref()
                                .map(|d| d.chars().take(10).collect::<String>())
                                .unwrap_or_else(|| "no date".into());
                            by_day.entry(k).or_default().push(i);
                        }
                        let months: BTreeSet<String> = by_day
                            .keys()
                            .filter(|k| k.as_str() != "no date")
                            .map(|d| d.chars().take(7).collect())
                            .collect();

                        // The calendar layers: year → month → week → day. The tabs go back up, a double click
                        // on a cell goes one layer down.
                        ui.horizontal(|ui| {
                            for (scale, label) in [
                                (CalScale::Year, "Year"),
                                (CalScale::Month, "Month"),
                                (CalScale::Week, "Week"),
                                (CalScale::Day, "Day"),
                            ] {
                                if ui
                                    .selectable_label(cal_scale == scale, t(lang, label))
                                    .clicked()
                                {
                                    // The selected day is the anchor: the month and the week open where it lies.
                                    if let Some(d) = cal_day.as_deref().and_then(parse_date) {
                                        cal_year = d.year();
                                        cal_month = d.month();
                                        cal_week = monday_of(d);
                                    }
                                    // In "Day" without a selected day we show today if it is in the open month, otherwise
                                    // the first day of it.
                                    if scale == CalScale::Day && cal_day.is_none() {
                                        let today = chrono::Local::now().date_naive();
                                        let d = if today.year() == cal_year
                                            && today.month() == cal_month
                                        {
                                            today
                                        } else {
                                            chrono::NaiveDate::from_ymd_opt(
                                                cal_year, cal_month, 1,
                                            )
                                            .unwrap_or(today)
                                        };
                                        cal_day = Some(d.format("%Y-%m-%d").to_string());
                                        cal_week = monday_of(d);
                                    }
                                    cal_scale = scale;
                                }
                            }
                            if word_button(ui, Icon::CalendarToday, t(lang, "Today")).clicked() {
                                let today = chrono::Local::now().date_naive();
                                cal_year = today.year();
                                cal_month = today.month();
                                cal_week = monday_of(today);
                                // On the "Day" layer today is a choice, not a reset.
                                cal_day = if cal_scale == CalScale::Day {
                                    Some(today.format("%Y-%m-%d").to_string())
                                } else {
                                    None
                                };
                                if cal_scale == CalScale::Year {
                                    cal_scale = CalScale::Month;
                                }
                            }
                        });

                        let mut nav = 0i32;
                        ui.horizontal(|ui| {
                            let prev_label = match cal_scale {
                                CalScale::Year => "previous year",
                                CalScale::Month => "previous month with files",
                                CalScale::Week => "previous week",
                                CalScale::Day => "previous day",
                            };
                            let next_label = match cal_scale {
                                CalScale::Year => "next year",
                                CalScale::Month => "next month with files",
                                CalScale::Week => "next week",
                                CalScale::Day => "next day",
                            };
                            let prev_enabled = match cal_scale {
                                CalScale::Year => cal_year > 1900,
                                CalScale::Week | CalScale::Day => true,
                                CalScale::Month => {
                                    shift_to_file_month(&months, cal_year, cal_month, -1).is_some()
                                }
                            };
                            let next_enabled = match cal_scale {
                                CalScale::Year => cal_year < 2199,
                                CalScale::Week | CalScale::Day => true,
                                CalScale::Month => {
                                    shift_to_file_month(&months, cal_year, cal_month, 1).is_some()
                                }
                            };
                            if ui
                                .add_enabled(
                                    prev_enabled,
                                    WordButton::new(Icon::ChevronLeft, ""),
                                )
                                .on_hover_text(t(lang, prev_label))
                                .clicked()
                            {
                                nav = -1;
                            }
                            let title = match cal_scale {
                                CalScale::Year => format!("{cal_year}"),
                                CalScale::Month => format!(
                                    "{} {}",
                                    i18n::month_name(lang, cal_month as usize),
                                    cal_year
                                ),
                                CalScale::Week => {
                                    let end = cal_week + chrono::Duration::days(6);
                                    format!(
                                        "{} — {}",
                                        cal_week.format("%d.%m"),
                                        end.format("%d.%m.%Y")
                                    )
                                }
                                CalScale::Day => cal_day
                                    .as_deref()
                                    .and_then(parse_date)
                                    .map(|d| {
                                        format!(
                                            "{}, {}",
                                            i18n::weekday(
                                                lang,
                                                d.weekday().num_days_from_monday() as usize
                                            ),
                                            d.format("%d.%m.%Y")
                                        )
                                    })
                                    .unwrap_or_else(|| t(lang, "no day selected").to_string()),
                            };
                            ui.heading(title);
                            if ui
                                .add_enabled(
                                    next_enabled,
                                    WordButton::new(Icon::ChevronRight, ""),
                                )
                                .on_hover_text(t(lang, next_label))
                                .clicked()
                            {
                                nav = 1;
                            }
                        });
                        if nav != 0 {
                            match cal_scale {
                                CalScale::Year => {
                                    cal_year = (cal_year + nav).clamp(1900, 2199);
                                    cal_day = None;
                                }
                                CalScale::Month => {
                                    let target = if nav < 0 {
                                        shift_to_file_month(&months, cal_year, cal_month, -1)
                                    } else {
                                        shift_to_file_month(&months, cal_year, cal_month, 1)
                                    };
                                    if let Some((y, m)) = target {
                                        cal_year = y;
                                        cal_month = m;
                                        cal_day = None;
                                    }
                                }
                                CalScale::Week => {
                                    cal_week += chrono::Duration::days(7 * nav as i64);
                                    cal_year = cal_week.year();
                                    cal_month = cal_week.month();
                                    cal_day = None;
                                }
                                CalScale::Day => {
                                    let d = cal_day
                                        .as_deref()
                                        .and_then(parse_date)
                                        .unwrap_or_else(|| chrono::Local::now().date_naive());
                                    let d = d + chrono::Duration::days(nav as i64);
                                    cal_day = Some(d.format("%Y-%m-%d").to_string());
                                    cal_week = monday_of(d);
                                    cal_year = d.year();
                                    cal_month = d.month();
                                }
                            }
                        }

                        match cal_scale {
                            CalScale::Year => {
                                // The month cards of the year: their counters, their volumes and
                                // the first snapshot of each for the cover.
                                let mut per_month = Vec::with_capacity(12);
                                for m in 1..=12u32 {
                                    let key = month_key(cal_year, m);
                                    let mut count = 0usize;
                                    let mut bytes = 0i64;
                                    let mut first: Option<usize> = None;
                                    for (day, ids) in by_day.range(key.clone()..) {
                                        if !day.starts_with(&key) {
                                            break;
                                        }
                                        count += ids.len();
                                        bytes += ids.iter().map(|&i| items[i].size).sum::<i64>();
                                        if first.is_none() {
                                            first = ids.first().copied();
                                        }
                                    }
                                    per_month.push((count, bytes, first));
                                }
                                let year_n: usize = per_month.iter().map(|(n, _, _)| *n).sum();
                                let year_bytes: i64 = per_month.iter().map(|(_, b, _)| *b).sum();
                                let busiest = per_month.iter().map(|(n, _, _)| *n).max().unwrap_or(0);
                                ui.horizontal(|ui| {
                                    caps_label(
                                        ui,
                                        &i18n::fill(
                                            lang,
                                            "12 months of {}",
                                            &[cal_year.to_string()],
                                        ),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            let facts = i18n::fill(
                                                lang,
                                                "{} in the year · {}",
                                                &[
                                                    i18n::count(lang, year_n, "file"),
                                                    db::human_in(lang, year_bytes),
                                                ],
                                            );
                                            ui.label(num(ui, facts, 10.0).color(dim(ui)));
                                        },
                                    );
                                });
                                ui.add_space(6.0);
                                let cell_w =
                                    ((ui.available_width() - 3.0 * CAL_GAP) / 4.0).max(40.0);
                                for row in 0..3u32 {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = CAL_GAP;
                                        for col in 0..4u32 {
                                            let m = row * 4 + col + 1;
                                            let (count, bytes, first) = per_month[(m - 1) as usize];
                                            let (rect, resp) = ui.allocate_exact_size(
                                                egui::vec2(cell_w, CAL_CARD_H),
                                                egui::Sense::click(),
                                            );
                                            let resp = resp.on_hover_text(t(
                                                lang,
                                                "double-click to open the month",
                                            ));
                                            if ui.is_rect_visible(rect) {
                                                let selected = cal_month == m;
                                                let r = rect.shrink(1.0);
                                                let th = cal_cell(
                                                    ui,
                                                    r,
                                                    RADIUS_CARD,
                                                    count > 0,
                                                    selected,
                                                );
                                                let pad = 8.0;
                                                // "01 January" — the mono number and the month's
                                                // name, the counter at the right edge.
                                                ui.painter().text(
                                                    egui::pos2(r.left() + pad, r.top() + 6.0),
                                                    egui::Align2::LEFT_TOP,
                                                    format!("{m:02}"),
                                                    egui::FontId::monospace(10.0),
                                                    th.text_dim,
                                                );
                                                ui.painter().text(
                                                    egui::pos2(r.left() + pad + 18.0, r.top() + 4.0),
                                                    egui::Align2::LEFT_TOP,
                                                    i18n::month_name(lang, m as usize),
                                                    egui::FontId::proportional(12.0),
                                                    if selected {
                                                        th.accent
                                                    } else if count > 0 {
                                                        th.text
                                                    } else {
                                                        th.text_dim
                                                    },
                                                );
                                                ui.painter().text(
                                                    egui::pos2(r.right() - pad, r.top() + 5.0),
                                                    egui::Align2::RIGHT_TOP,
                                                    format!("{count}"),
                                                    egui::FontId::monospace(10.0),
                                                    if selected { th.accent } else { th.text_med },
                                                );
                                                // The cover: the first snapshot of the month,
                                                // fitted whole — a preview is never cropped.
                                                let cover = egui::Rect::from_min_max(
                                                    egui::pos2(
                                                        r.left() + pad,
                                                        r.top() + 23.0,
                                                    ),
                                                    egui::pos2(
                                                        r.right() - pad,
                                                        r.bottom() - 17.0,
                                                    ),
                                                );
                                                if cover.height() > 8.0 {
                                                    ui.painter().rect_filled(
                                                        cover,
                                                        egui::CornerRadius::same(
                                                            RADIUS_CTRL as u8,
                                                        ),
                                                        th.canvas,
                                                    );
                                                    if let Some(i) = first {
                                                        paint_thumb(
                                                            ui, ctx, &mut cells, &items[i], cover,
                                                        );
                                                    }
                                                }
                                                // The footer: the volume at the left edge and
                                                // the size of the month at the right one.
                                                let bar = egui::Rect::from_min_size(
                                                    egui::pos2(r.left() + pad, r.bottom() - 9.0),
                                                    egui::vec2(44.0, 4.0),
                                                );
                                                if count > 0 && busiest > 0 {
                                                    cal_bar(
                                                        ui,
                                                        bar,
                                                        count as f32 / busiest as f32,
                                                    );
                                                    ui.painter().text(
                                                        egui::pos2(r.right() - pad, r.bottom() - 5.0),
                                                        egui::Align2::RIGHT_BOTTOM,
                                                        db::human_in(lang, bytes),
                                                        egui::FontId::monospace(10.0),
                                                        th.text_dim,
                                                    );
                                                }
                                            }
                                            if resp.double_clicked() {
                                                cal_month = m;
                                                cal_day = None;
                                                if let Some(next) = cal_scale.deeper() {
                                                    cal_scale = next;
                                                }
                                            } else if resp.clicked() {
                                                cal_month = m;
                                                cal_day = None;
                                            }
                                        }
                                    });
                                    ui.add_space(CAL_GAP);
                                }
                            }
                            CalScale::Month => {
                                let key = month_key(cal_year, cal_month);
                                let month_n: usize = by_day
                                    .iter()
                                    .filter(|(d, _)| d.starts_with(&key))
                                    .map(|(_, v)| v.len())
                                    .sum();
                                let month_bytes: i64 = by_day
                                    .iter()
                                    .filter(|(d, _)| d.starts_with(&key))
                                    .flat_map(|(_, v)| v.iter())
                                    .map(|&i| items[i].size)
                                    .sum();
                                // The busiest day of the month: the day counters are drawn as the
                                // mock-up's bar, so the shape of the month is visible at a glance.
                                let busiest = by_day
                                    .iter()
                                    .filter(|(d, _)| d.starts_with(&key))
                                    .map(|(_, v)| v.len())
                                    .max()
                                    .unwrap_or(0);
                                let facts = i18n::fill(
                                    lang,
                                    "{} this month, {}",
                                    &[
                                        i18n::count(lang, month_n, "file"),
                                        db::human_in(lang, month_bytes),
                                    ],
                                );
                                ui.label(num(ui, facts, 11.0).color(dim(ui)));
                                if month_n == 0 {
                                    ui.add_space(8.0);
                                    ui.label(t(lang, "No files in this month."));
                                    // The nearest month with files, so an empty month is a
                                    // stop on the way, not a dead end.
                                    if let Some((y, m)) =
                                        nearest_file_month(&months, cal_year, cal_month)
                                    {
                                        ui.add_space(4.0);
                                        if ui
                                            .add(WordButton::new(
                                                Icon::CalendarToday,
                                                format!(
                                                    "{} {} {}",
                                                    t(lang, "Go to"),
                                                    i18n::month_name(lang, m as usize),
                                                    y
                                                ),
                                            ))
                                            .clicked()
                                        {
                                            cal_year = y;
                                            cal_month = m;
                                            cal_day = None;
                                        }
                                    }
                                    return;
                                }

                                ui.add_space(4.0);
                                // The weekday row of the mock-up: caps labels over the columns.
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = CAL_GAP;
                                    let wd_w =
                                        (ui.available_width() - 6.0 * CAL_GAP) / 7.0;
                                    for wd in 0..7u32 {
                                        ui.allocate_ui(egui::vec2(wd_w, 16.0), |ui| {
                                            ui.label(
                                                egui::RichText::new(
                                                    i18n::weekday(lang, wd as usize)
                                                        .to_uppercase(),
                                                )
                                                .size(10.0)
                                                .color(dim(ui)),
                                            );
                                        });
                                    }
                                });
                                let cell_w = ((ui.available_width() - 6.0 * CAL_GAP) / 7.0)
                                    .max(32.0);
                                let cell_h = CAL_DAY_H;
                                let first = first_weekday_monday(cal_year, cal_month);
                                let n_days = days_in_month(cal_year, cal_month);
                                let mut day = 1u32;
                                for _week in 0..6 {
                                    if day > n_days {
                                        break;
                                    }
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = CAL_GAP;
                                        for wd in 0..7u32 {
                                            if day > n_days {
                                                ui.allocate_space(egui::vec2(cell_w, cell_h));
                                                continue;
                                            }
                                            if wd < first && day == 1 {
                                                ui.allocate_space(egui::vec2(cell_w, cell_h));
                                                continue;
                                            }
                                            let date =
                                                format!("{cal_year:04}-{cal_month:02}-{day:02}");
                                            let ids: &[usize] = by_day
                                                .get(&date)
                                                .map_or(&[], |v| v.as_slice());
                                            let (rect, resp) = ui.allocate_exact_size(
                                                egui::vec2(cell_w, cell_h),
                                                egui::Sense::click(),
                                            );
                                            let resp = resp.on_hover_text(t(
                                                lang,
                                                "double-click to open the week",
                                            ));
                                            if ui.is_rect_visible(rect) {
                                                let selected = cal_day.as_deref()
                                                    == Some(date.as_str());
                                                let n = ids.len();
                                                let th = cal_cell(
                                                    ui,
                                                    rect.shrink(1.0),
                                                    RADIUS_CTRL,
                                                    n > 0,
                                                    selected,
                                                );
                                                let r = rect.shrink(1.0);
                                                let pad = 5.0;
                                                ui.painter().text(
                                                    egui::pos2(r.left() + pad, r.top() + 4.0),
                                                    egui::Align2::LEFT_TOP,
                                                    day.to_string(),
                                                    egui::FontId::monospace(11.0),
                                                    if selected {
                                                        th.accent
                                                    } else if n > 0 {
                                                        th.text
                                                    } else {
                                                        th.text_dim
                                                    },
                                                );
                                                if n > 0 {
                                                    // The counter as a badge, as on the mock-up's
                                                    // day cell: digit on a soft accent pill.
                                                    let label = n.to_string();
                                                    let badge = egui::Rect::from_min_size(
                                                        egui::pos2(
                                                            r.right() - pad - 6.0 * label.len() as f32 - 4.0,
                                                            r.top() + 3.0,
                                                        ),
                                                        egui::vec2(
                                                            6.0 * label.len() as f32 + 8.0,
                                                            13.0,
                                                        ),
                                                    );
                                                    ui.painter().rect_filled(
                                                        badge,
                                                        egui::CornerRadius::same(
                                                            RADIUS_CTRL as u8,
                                                        ),
                                                        th.accent_soft,
                                                    );
                                                    ui.painter().text(
                                                        badge.center(),
                                                        egui::Align2::CENTER_CENTER,
                                                        label,
                                                        egui::FontId::monospace(10.0),
                                                        th.accent,
                                                    );
                                                    let area = egui::Rect::from_min_max(
                                                        egui::pos2(r.left() + 4.0, r.top() + 22.0),
                                                        egui::pos2(
                                                            r.right() - 4.0,
                                                            r.bottom() - 10.0,
                                                        ),
                                                    );
                                                    let shown = n.min(3);
                                                    let tw = ((area.width()
                                                        - (shown as f32 - 1.0) * 2.0)
                                                        / shown as f32)
                                                        .max(8.0);
                                                    for (k, &i) in
                                                        ids.iter().take(shown).enumerate()
                                                    {
                                                        let tr = egui::Rect::from_min_size(
                                                            egui::pos2(
                                                                area.left()
                                                                    + k as f32 * (tw + 2.0),
                                                                area.top(),
                                                            ),
                                                            egui::vec2(tw, area.height()),
                                                        );
                                                        ui.painter().rect_filled(
                                                            tr,
                                                            2.0,
                                                            th.canvas,
                                                        );
                                                        paint_thumb(
                                                            ui, ctx, &mut cells, &items[i], tr,
                                                        );
                                                    }
                                                    // The volume of the day against the busiest
                                                    // day of the month.
                                                    let bar = egui::Rect::from_min_size(
                                                        egui::pos2(r.left() + 4.0, r.bottom() - 7.0),
                                                        egui::vec2((r.width() - 8.0).max(4.0), 3.0),
                                                    );
                                                    let share = if busiest > 0 {
                                                        n as f32 / busiest as f32
                                                    } else {
                                                        0.0
                                                    };
                                                    cal_bar(ui, bar, share);
                                                }
                                            }
                                            if resp.double_clicked() {
                                                cal_day = Some(date.clone());
                                                if let Some(d) = parse_date(&date) {
                                                    cal_week = monday_of(d);
                                                    cal_year = d.year();
                                                    cal_month = d.month();
                                                }
                                                if let Some(next) = cal_scale.deeper() {
                                                    cal_scale = next;
                                                }
                                            } else if resp.clicked() {
                                                cal_day = Some(date.clone());
                                                if let Some(d) = parse_date(&date) {
                                                    cal_week = monday_of(d);
                                                }
                                            }
                                            day += 1;
                                        }
                                    });
                                    ui.add_space(CAL_GAP);
                                }
                            }
                            CalScale::Week => {
                                let cell_w = ((ui.available_width() - 6.0 * CAL_GAP) / 7.0)
                                    .max(32.0);
                                let cell_h = 190.0;
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = CAL_GAP;
                                    for d in 0..7i64 {
                                        let date = cal_week + chrono::Duration::days(d);
                                        let key = date.format("%Y-%m-%d").to_string();
                                        let idxs = by_day.get(&key);
                                        let (rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(cell_w, cell_h),
                                            egui::Sense::click(),
                                        );
                                        let resp = resp.on_hover_text(t(
                                            lang,
                                            "double-click to open the day",
                                        ));
                                        if ui.is_rect_visible(rect) {
                                            let selected = cal_day.as_deref()
                                                == Some(key.as_str());
                                            let n = idxs.map_or(0, |ids| ids.len());
                                            let th = cal_cell(
                                                ui,
                                                rect.shrink(1.0),
                                                RADIUS_CTRL,
                                                n > 0,
                                                selected,
                                            );
                                            let r = rect.shrink(1.0);
                                            let pad = 5.0;
                                            ui.painter().text(
                                                egui::pos2(r.left() + pad, r.top() + 4.0),
                                                egui::Align2::LEFT_TOP,
                                                i18n::weekday(lang, d as usize),
                                                egui::FontId::proportional(11.0),
                                                if selected { th.accent } else { th.text_med },
                                            );
                                            ui.painter().text(
                                                egui::pos2(
                                                    r.left() + pad + 26.0,
                                                    r.top() + 5.0,
                                                ),
                                                egui::Align2::LEFT_TOP,
                                                date.format("%d.%m").to_string(),
                                                egui::FontId::monospace(10.0),
                                                th.text_dim,
                                            );
                                            ui.painter().text(
                                                egui::pos2(r.right() - pad, r.top() + 5.0),
                                                egui::Align2::RIGHT_TOP,
                                                n.to_string(),
                                                egui::FontId::monospace(10.0),
                                                if selected { th.accent } else { th.text_med },
                                            );
                                            if let Some(ids) = idxs {
                                                let area = egui::Rect::from_min_max(
                                                    egui::pos2(r.left() + 4.0, r.top() + 22.0),
                                                    egui::pos2(r.right() - 4.0, r.bottom() - 4.0),
                                                );
                                                let rows = 3usize;
                                                let cols = 2usize;
                                                let tw = ((area.width()
                                                    - (cols as f32 - 1.0) * 2.0)
                                                    / cols as f32)
                                                    .max(8.0);
                                                let th_h = ((area.height()
                                                    - (rows as f32 - 1.0) * 2.0)
                                                    / rows as f32)
                                                    .max(8.0);
                                                for (k, &i) in
                                                    ids.iter().take(rows * cols).enumerate()
                                                {
                                                    let tr = egui::Rect::from_min_size(
                                                        egui::pos2(
                                                            area.left()
                                                                + (k % cols) as f32
                                                                    * (tw + 2.0),
                                                            area.top()
                                                                + (k / cols) as f32
                                                                    * (th_h + 2.0),
                                                        ),
                                                        egui::vec2(tw, th_h),
                                                    );
                                                    paint_thumb(
                                                        ui, ctx, &mut cells, &items[i], tr,
                                                    );
                                                }
                                            }
                                        }
                                        if resp.double_clicked() {
                                            cal_day = Some(key.clone());
                                            cal_year = date.year();
                                            cal_month = date.month();
                                            cal_week = monday_of(date);
                                            if let Some(next) = cal_scale.deeper() {
                                                cal_scale = next;
                                            }
                                        } else if resp.clicked() {
                                            cal_day = Some(key.clone());
                                            cal_year = date.year();
                                            cal_month = date.month();
                                        }
                                    }
                                });
                            }
                            CalScale::Day => {
                                ui.add_space(4.0);
                                let day = cal_day.clone().unwrap_or_else(|| {
                                    chrono::Local::now()
                                        .date_naive()
                                        .format("%Y-%m-%d")
                                        .to_string()
                                });
                                match by_day.get(&day) {
                                    Some(idxs) if !idxs.is_empty() => {
                                        let bytes: i64 =
                                            idxs.iter().map(|&i| items[i].size).sum();
                                        day_heading(
                                            ui,
                                            &day_title(lang, &day),
                                            idxs.len(),
                                            bytes,
                                            lang,
                                        );
                                        // The day layer keeps the owner's cell view (`draw_cell`):
                                        // only the caption shows the time, the date is the same for
                                        // every tile of the day.
                                        cells.time_only = true;
                                        let row: Vec<&FileRow> =
                                            idxs.iter().map(|&i| &items[i]).collect();
                                        draw_files(ui, ctx, &mut cells, &row);
                                        cells.time_only = false;
                                    }
                                    _ => {
                                        ui.label(t(lang, "No files on this day."));
                                    }
                                }
                            }
                        }
                        if cal_scale != CalScale::Day {
                            hairline(ui);
                            ui.add_space(8.0);
                            match cal_day.as_deref().map(|d| (d, by_day.get(d))) {
                                Some((day, Some(idxs))) if !idxs.is_empty() => {
                                    day_strip(ui, ctx, &mut cells, day, idxs, &items, lang);
                                }
                                Some(_) => {
                                    ui.label(t(lang, "No files on this day."));
                                }
                                None => {
                                    ui.label(t(
                                        lang,
                                        "Pick a day in the calendar — its photos appear below.",
                                    ));
                                }
                            }
                        }
                        if let Some(idxs) = by_day.get("no date") {
                            hairline(ui);
                            ui.add_space(8.0);
                            egui::CollapsingHeader::new(i18n::fill(lang, "No date — {} files", &[idxs.len().to_string()]))
                            .id_salt("no-date")
                            .show(ui, |ui| {
                                let row: Vec<&FileRow> =
                                    idxs.iter().map(|&i| &items[i]).collect();
                                draw_files(ui, ctx, &mut cells, &row);
                            });
                        }
                    });
            }
            View::Faces => {
                let faces_n = |n: usize| {
                    i18n::count(lang, n,"person")
                };
                let photos_n = |n: usize| {
                    i18n::count(lang, n,"photo")
                };
                if face_together && face_picked_clusters.len() >= 2 {
                    let mut per_file: HashMap<i64, HashSet<i64>> = HashMap::new();
                    for f in &face_views {
                        if let Some(cid) = f.cluster_id
                            && face_picked_clusters.contains(&cid)
                        {
                            per_file.entry(f.file_id).or_default().insert(cid);
                        }
                    }
                    let ids: HashSet<i64> = per_file
                        .into_iter()
                        .filter(|(_, set)| face_picked_clusters.iter().all(|c| set.contains(c)))
                        .map(|(id, _)| id)
                        .collect();
                    let names: Vec<String> = face_clusters
                        .iter()
                        .filter(|c| face_picked_clusters.contains(&c.id))
                        .map(|c| {
                            if c.label.is_empty() {
                                t(lang, "no name").to_string()
                            } else {
                                c.label.clone()
                            }
                        })
                        .collect();
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(t(lang, "Photos with everyone together"))
                                .size(14.0)
                                .strong(),
                        );
                        ui.label(
                            egui::RichText::new(names.join(" + "))
                                .size(12.0)
                                .color(dim(ui)),
                        );
                    });
                    let found = i18n::fill(lang, "photos: {}", &[ids.len().to_string()]);
                    ui.label(num(ui, found, 11.0).color(dim(ui)));
                    if ids.is_empty() {
                        ui.label(t(
                            lang,
                            "No photo has faces from all the marked groups.",
                        ));
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let row: Vec<&FileRow> =
                                items.iter().filter(|f| ids.contains(&f.id)).collect();
                            draw_files(ui, ctx, &mut cells, &row);
                        });
                } else {
                let current = face_clusters
                    .iter()
                    .find(|c| Some(c.id) == face_selected)
                    .cloned();
                if let Some(c) = &current {
                    // The name of the group is a subheading of the view: 14 px, the facts in mono 11
                    // (M22.9) — the same rhythm as the album's own card header.
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(if c.label.is_empty() {
                                t(lang, "no name").to_string()
                            } else {
                                c.label.clone()
                            })
                            .size(14.0)
                            .strong(),
                        );
                        let facts = format!(
                            "{} · {}",
                            faces_n(c.faces as usize),
                            photos_n(c.photos as usize)
                        );
                        ui.label(num(ui, facts, 11.0).color(dim(ui)));
                    });
                    ui.horizontal(|ui| {
                        ui.label(t(lang, "Person's name:"));
                        ui.add(
                            egui::TextEdit::singleline(&mut face_label_input)
                                .hint_text(t(lang, "e.g. Lisa"))
                                .desired_width(220.0),
                        );
                        if word_button(ui, Icon::Check, t(lang, "Save the name")).clicked() {
                            face_save_label = Some(face_label_input.trim().to_string());
                        }
                    });
                    ui.label(t(
                        lang,
                        "A name pins the group: rebuilding by threshold will not shuffle it. A wrong merge is split by the crossed-head button on the tile.",
                    ));
                } else {
                    ui.label(
                        egui::RichText::new(t(
                            lang,
                            "Pick a group in the list to name it, or split the tiles.",
                        ))
                        .size(12.0)
                        .color(dim(ui)),
                    );
                }
                let shown: Vec<&db::FaceView> = face_views
                    .iter()
                    .filter(|f| face_selected.is_none() || f.cluster_id == face_selected)
                    .collect();
                // A batch naming: mark faces with the tick circle and give them one name.
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!(
                        "{}: {}",
                        t(lang, "People picked"),
                        face_picked.len()
                    ));
                    ui.add(
                        egui::TextEdit::singleline(&mut face_group_input)
                            .hint_text(t(lang, "name for the picked people"))
                            .desired_width(220.0),
                    );
                    if ui
                        .add_enabled(
                            !face_picked.is_empty(),
                            WordButton::new(Icon::Label, t(lang, "Name the picked people")),
                        )
                        .on_hover_text(t(
                            lang,
                            "the picked faces move into one group; an empty name creates an unnamed group",
                        ))
                        .clicked()
                    {
                        face_assign = Some(face_group_input.trim().to_string());
                    }
                    if ui
                        .add_enabled(
                            !face_picked.is_empty(),
                            WordButton::new(Icon::Remove, t(lang, "Unpick")),
                        )
                        .clicked()
                    {
                        face_picked.clear();
                        clear_picked = true;
                    }
                });
                if face_total == 0 {
                    ui.add_space(12.0);
                    ui.label(t(
                        lang,
                        "No faces yet: press “Find faces” on the left (the face model is required).",
                    ));
                } else if shown.is_empty() {
                    ui.label(t(lang, "No faces in this group."));
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for f in &shown {
                                draw_face_cell(
                                    ui,
                                    ctx,
                                    &mut face_cells,
                                    f,
                                    lang,
                                    face_selected.is_some(),
                                );
                            }
                        });
                    });
                }
            }
            View::Trash => {
                ui.label(t(
                    lang,
                    "Files in the app trash: untouched on disk. “Restore” brings them back to the archive, “Empty the trash” moves them to the OS trash after a confirmation.",
                ));
                if trash_items.is_empty() {
                    ui.add_space(12.0);
                    ui.label(t(lang, "The trash is empty."));
                } else {
                    ui.horizontal(|ui| {
                        let bytes: i64 = trash_items.iter().map(|r| r.size).sum();
                        let facts = format!(
                            "{} · {}",
                            files_n(trash_items.len()),
                            db::human_in(lang, bytes)
                        );
                        ui.label(num(ui, facts, 11.0).color(dim(ui)));
                        if word_button(ui, Icon::Restore, t(lang, "Restore all")).clicked() {
                            trash_restore_all = true;
                        }
                        // The only red word in the window: this is the one path that changes the
                        // disk, and it asks for a confirmation (M22.9).
                        let b = WordButton::danger(
                            Icon::Delete,
                            format!(
                                "{} ({})",
                                t(lang, "Empty the trash"),
                                trash_items.len()
                            ),
                        );
                        if ui
                            .add(b)
                            .on_hover_text(t(
                                lang,
                                "moved to the OS trash; nothing is deleted for good",
                            ))
                            .clicked()
                        {
                            trash_clear = true;
                        }
                    });
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            let row: Vec<&FileRow> = trash_items.iter().collect();
                            draw_files(ui, ctx, &mut cells, &row);
                        });
                }
            }
            // Unreachable: the screen is drawn before the grid (see `ui`), but the enum must be
            // matched completely.
            View::Integrity => {}
            }
            // The selection bar of the mock-up: a floating pill at the bottom edge of the grid,
            // where the eye is after picking a tile (M22.19). The count and the volume are its
            // left part; "Select all" and "Clear selection" are the actions the grid has.
            if chips.pickable && chips.picked > 0 {
                let picked_text = i18n::fill(
                    lang,
                    "{} selected · {}",
                    &[
                        chips.picked.to_string(),
                        db::human_in(lang, chips.picked_bytes).to_string(),
                    ],
                );
                let all = WordButton::small(Icon::Check, i18n::select_all(lang, visible.len()));
                let clear = WordButton::small(Icon::Close, t(lang, "Clear selection"));
                // M22.20: with albums around, the bar also carries the way into them — the one
                // action of the bar that changes data, and it changes rows only.
                let add = WordButton::small(
                    Icon::Add,
                    i18n::fill(
                        lang,
                        "Add to an album… ({})",
                        &[chips.picked.to_string()],
                    ),
                );
                let add_w = if albums.is_empty() {
                    0.0
                } else {
                    add.size(ui).0 + 6.0
                };
                let w = 12.0 + 6.0 + 6.0 + mono_width(ui, &picked_text, 11.0) + 12.0 + 1.0 + 12.0
                    + add_w
                    + all.size(ui).0
                    + 6.0
                    + clear.size(ui).0
                    + 12.0;
                let view_rect = ui.max_rect();
                let pos = egui::pos2(
                    view_rect.center().x - w * 0.5,
                    view_rect.bottom() - 12.0 - 32.0,
                );
                let bar = egui::Rect::from_min_size(pos, egui::vec2(w, 32.0));
                ui.scope_builder(egui::UiBuilder::new().max_rect(bar), |ui| {
                        egui::Frame::new()
                            .fill(theme(ui).card)
                            .stroke(egui::Stroke::new(1.0, theme(ui).border_strong))
                            .corner_radius(egui::CornerRadius::same(RADIUS_CARD as u8))
                            .inner_margin(egui::Margin::symmetric(12, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 6.0;
                                    let (dot, _) = ui.allocate_exact_size(
                                        egui::vec2(6.0, 6.0),
                                        egui::Sense::hover(),
                                    );
                                    ui.painter()
                                        .circle_filled(dot.center(), 3.0, theme(ui).accent);
                                    ui.label(num(ui, picked_text, 11.0).color(theme(ui).accent));
                                    ui.add_space(6.0);
                                    hairline_vertical(ui);
                                    ui.add_space(6.0);
                                    if !albums.is_empty()
                                        && ui
                                            .add(add)
                                            .on_hover_text(t(
                                                lang,
                                                "put the marked photos into an album; the files on disk stay untouched",
                                            ))
                                            .clicked()
                                    {
                                        album_pick_request = true;
                                    }
                                    if ui
                                        .add(all)
                                        .on_hover_text(t(lang, "marks everything visible now; Ctrl+A does the same"))
                                        .clicked()
                                    {
                                        clear_chips.pick_all = true;
                                    }
                                    if ui
                                        .add(clear)
                                        .on_hover_text(t(lang, "clears the selection in this grid; Esc does the same"))
                                        .clicked()
                                    {
                                        clear_chips.clear_pick = true;
                                    }
                                });
                            });
                });
            }
                });
        });

        self.items = items;
        self.groups = groups;
        self.trash_items = trash_items;
        self.albums = albums;
        self.album_expanded = album_expanded;
        self.album_browse_all = album_browse;
        self.face_clusters = face_clusters;
        self.face_views = face_views;
        let face_split = face_cells.split_clicked;
        let face_open = face_cells.open_clicked;
        let face_viewer = face_cells.viewer_clicked;
        self.face_textures = std::mem::take(&mut face_cells.textures);
        self.face_missing = std::mem::take(&mut face_cells.missing);
        self.face_label_input = face_label_input;
        self.face_picked = if clear_picked {
            HashSet::new()
        } else {
            face_cells.picked.clone()
        };
        self.face_group_input = face_group_input;
        self.cal_year = cal_year;
        self.cal_month = cal_month;
        self.cal_day = cal_day;
        self.cal_scale = cal_scale;
        self.cal_week = cal_week;
        let cell_open = cells.open_clicked;
        let cell_click = cells.clicked;
        let keep_click = cells.keep_clicked;
        let restore_click = cells.restore_clicked;
        // Release the borrows of self.textures/missing before open_viewer/reload.
        #[allow(clippy::drop_non_drop)]
        drop(cells);
        if view_save {
            self.save_settings();
        }
        // The album dialog opens after the grid borrows are released (M19).
        if let Some(dialog) = album_dialog_request {
            self.album_open_dialog(dialog);
        }
        if let Some(action) = album_action {
            self.album_apply(action);
        }
        if let Some(id) = album_export_request {
            self.export_dialog_open(id);
        }
        // The "add to an album" dialog takes the selection as it was at the click (M22.20).
        if album_pick_request {
            let files: Vec<i64> = self.picked.iter().copied().collect();
            self.album_pick_open(files);
        }
        // A chip removes exactly its own filter; the grid is recomputed on the next frame.
        if clear_chips.query {
            self.query.clear();
        }
        if let Some(i) = clear_chips.tag_at
            && i < self.tag_filters.len()
        {
            self.tag_filters.remove(i);
        }
        if clear_chips.tags {
            self.tag_filters.clear();
        }
        if clear_chips.year {
            self.year_filter = None;
        }
        if clear_chips.no_date {
            self.only_no_date = false;
        }
        if clear_chips.no_preview {
            self.only_no_preview = false;
        }
        if clear_chips.kind {
            self.kind_filter = KindFilter::All;
        }
        if clear_chips.hits {
            self.semantic_hits = None;
            self.ocr_snippets.clear();
        }
        if clear_chips.album {
            self.album_close();
        }
        if clear_chips.pick_all {
            self.pick_all();
        } else if clear_chips.clear_pick {
            self.clear_picked(t(lang, "the selection is cleared"));
        }
        if let Some(id) = cell_open {
            self.open_viewer(id);
        } else if let Some((id, kind)) = cell_click {
            self.pick_clicked(id, kind, &pick_order);
            self.prefetch_photo(id);
        }
        if empty_click && cell_click.is_none() {
            self.clear_picked(t(lang, "the selection is cleared"));
        }
        if let Some(file_id) = face_viewer {
            self.open_viewer(file_id);
        }
        if trash_restore_all {
            let mut n = 0usize;
            for r in &self.trash_items {
                if db::untrash(&self.db_path, r.id).is_ok() {
                    n += 1;
                }
            }
            self.reload();
            let lang = self.settings.lang;
            self.status = format!(
                "{}: {n}",
                t(lang, "restored from the trash")
            );
        }
        if let Some(id) = restore_click {
            let lang = self.settings.lang;
            match db::untrash(&self.db_path, id) {
                Ok(()) => {
                    self.reload();
                    self.status = t(lang, "file restored from the trash").into();
                }
                Err(e) => self.status = format!("{}: {e}", t(lang, "file not restored")),
            }
        }
        if trash_clear {
            self.trash_confirm = true;
        }
        if let Some(keep_id) = keep_click {
            self.select_one(keep_id);
            if let Some(gi) = self
                .groups
                .iter()
                .position(|g| g.members.iter().any(|&i| self.items[i].id == keep_id))
            {
                let members = self.groups[gi].members.clone();
                let mut drops = Vec::new();
                for &i in &members {
                    if self.items[i].id == keep_id {
                        self.items[i].decision = Some("keep".into());
                    } else {
                        self.items[i].decision = Some("drop".into());
                        drops.push(self.items[i].id);
                    }
                }
                let name = self
                    .items
                    .iter()
                    .find(|r| r.id == keep_id)
                    .and_then(|r| Path::new(&r.path).file_name().and_then(|s| s.to_str()))
                    .unwrap_or(t(lang, "file"))
                    .to_string();
                match db::mark_keep(&self.db_path, keep_id, &drops) {
                    Ok(()) => {
                        self.status = match self.settings.lang {
                            i18n::Lang::En => format!("kept {name}, {} marked spare", drops.len()),
                            i18n::Lang::Ru => {
                                i18n::fill(lang, "kept {}, {} marked spare", &[name.to_string(), drops.len().to_string()])
                            }
                        }
                    }
                    Err(e) => {
                        self.status =
                            format!("{}: {e}", t(self.settings.lang, "decision not saved"))
                    }
                }
            }
        }
        if let Some(label) = face_save_label
            && let Some(id) = face_selected
        {
            let lang = self.settings.lang;
            match faces::label(&self.db_path, id, &label) {
                Ok(()) => {
                    self.status = if label.is_empty() {
                        t(lang, "group name removed").into()
                    } else {
                        i18n::fill(lang, "the group is named: “{}”", std::slice::from_ref(&label))
                    };
                    self.reload();
                }
                Err(e) => self.status = format!("{}: {e:#}", t(lang, "name not saved")),
            }
        }
        if let Some(face_id) = face_split {
            let lang = self.settings.lang;
            match faces::split(&self.db_path, face_id) {
                Ok(cid) => {
                    self.face_selected_cluster = Some(cid);
                    self.reload();
                    self.status =
                        t(lang, "face moved into a new group; give it a name").into();
                }
                Err(e) => self.status = format!("{}: {e:#}", t(lang, "face not split off")),
            }
        }
        if face_split.is_none()
            && face_viewer.is_none()
            && let Some(file_id) = face_open
        {
            self.open_from_faces(file_id);
        }
        if let Some(label) = face_assign {
            let ids: Vec<i64> = self.face_picked.iter().copied().collect();
            if !ids.is_empty() {
                let lang = self.settings.lang;
                match faces::group_picked(&self.db_path, &ids, &label) {
                    Ok(cid) => {
                        self.face_selected_cluster = Some(cid);
                        self.face_picked.clear();
                        self.reload();
                        self.status = if label.trim().is_empty() {
                            {
                                    i18n::fill(lang, "{} faces are gathered into a new group; give it a name", &[ids.len().to_string()])
                                }
                        } else {
                            {
                                    i18n::fill(lang, "{} faces are assigned to the group “{}”", &[ids.len().to_string(), label.trim().to_string()])
                                }
                        };
                    }
                    Err(e) => self.status = format!("{}: {e:#}", t(lang, "faces not assigned")),
                }
            }
        }
        if reset {
            self.reset_filters();
        }
        self.viewer_modal(ctx);
        if self.photo_rx.is_some() || !self.photo_jobs.is_empty() {
            ctx.request_repaint_after(std::time::Duration::from_millis(30));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AlbumDialog, App, COAST_DECAY_S, CalScale, IntegrityJob, IntegrityOutcome, IntegrityPlan,
        KindFilter, PickClick, Scroll, SettingsTab, StorageAction, View, Viewer, WHEEL_NOTCH_PTS,
        clamp_pan, album_card_size, cover_rect, days_in_month, db, first_weekday_monday, fit_rect,
        list_meta_px,
        list_name_px,
        list_row_height, monday_of, month_groups, nearest_file_month, one_to_one_zoom, parse_date,
        pick_kind, rotate_rect, rotated_size, score_percentile, shift_month, shift_to_file_month,
        MAX_ZOOM, PANEL_HEAD_H, PANEL_PAD, RIGHT_W,
    };
    use crate::embed;
    use crate::i18n::{self, t};
    use std::collections::BTreeSet;

    fn file_row(id: i64, kind: &str, name: &str) -> db::FileRow {
        db::FileRow {
            id,
            path: format!("/tmp/{name}"),
            size: 1,
            mtime: 0,
            sha256: String::new(),
            kind: kind.into(),
            width: 320,
            height: 240,
            taken_at: Some("2024-05-01T10:00".into()),
            taken_src: "exif".into(),
            lat: None,
            lon: None,
            camera: None,
            dhash: None,
            sharpness: None,
            duration_s: (kind == "video").then_some(22.0),
            thumb: None,
            decision: None,
            trashed_at: None,
        }
    }

    fn collect_text(shape: &egui::Shape, out: &mut Vec<String>) {
        match shape {
            egui::Shape::Text(t) => out.push(t.galley.text().to_owned()),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_text(s, out);
                }
            }
            _ => {}
        }
    }

    /// The boxes of the drawn pictures: a preview is a mesh on the texture, and its box says
    /// whether it was fitted into the frame or covers it (M22.15).
    fn collect_meshes(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Mesh(m) => {
                let mut r = egui::Rect::NOTHING;
                for v in &m.vertices {
                    r.extend_with(v.pos);
                }
                out.push(r);
            }
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_meshes(s, out);
                }
            }
            _ => {}
        }
    }

    /// The drawn text with where it landed: a panel budget is checked by places, not by words.
    fn collect_text_pos(shape: &egui::Shape, out: &mut Vec<(String, egui::Pos2)>) {
        match shape {
            egui::Shape::Text(t) => out.push((t.galley.text().to_owned(), t.pos)),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_text_pos(s, out);
                }
            }
            _ => {}
        }
    }

    /// The drawn text with its rectangle: a row wider than its panel pushes the panel out of the
    /// window, and the words are the first thing to tell (M22.12).
    fn collect_text_rects(shape: &egui::Shape, out: &mut Vec<(String, egui::Rect)>) {
        match shape {
            egui::Shape::Text(t) => {
                // A galley may be anchored at its left, its right or its centre: a right-to-left
                // row places the words by their right edge, and `pos` is that edge, not the left
                // one.
                let size = t.galley.size();
                let rect = match t.galley.job.halign {
                    egui::Align::Min => egui::Rect::from_min_size(t.pos, size),
                    egui::Align::Max => egui::Rect::from_min_size(t.pos - egui::vec2(size.x, 0.0), size),
                    egui::Align::Center => egui::Rect::from_center_size(
                        t.pos + egui::vec2(0.0, size.y * 0.5),
                        size,
                    ),
                };
                out.push((t.galley.text().to_owned(), rect));
            }
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_text_rects(s, out);
                }
            }
            _ => {}
        }
    }

    /// Every drawn text section with its font and colour: the screens without a mock-up must put
    /// their numbers in the numeric family and take their colours from the palette (M22.9).
    fn collect_text_formats(
        shape: &egui::Shape,
        out: &mut Vec<(String, egui::FontId, egui::Color32)>,
    ) {
        match shape {
            egui::Shape::Text(t) => {
                let job = &t.galley.job;
                for s in &job.sections {
                    let text = job
                        .text
                        .get(usize::from(s.byte_range.start)..usize::from(s.byte_range.end))
                        .unwrap_or_default()
                        .trim()
                        .to_owned();
                    if !text.is_empty() {
                        // A galley may leave the colour unset, so the frame's fallback and an
                        // override decide what is painted; both are the palette's doing.
                        let color = if s.format.color == egui::Color32::PLACEHOLDER {
                            t.fallback_color
                        } else {
                            s.format.color
                        };
                        out.push((
                            text,
                            s.format.font_id.clone(),
                            t.override_text_color.unwrap_or(color),
                        ));
                    }
                }
            }
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_text_formats(s, out);
                }
            }
            _ => {}
        }
    }

    /// The bounding box of every painted non-text shape: an icon is geometry, so "is there an
    /// icon and where does it sit" is answered by the shapes and not by pixels (M22.8).
    fn collect_bounds(shape: &egui::Shape, out: &mut Option<egui::Rect>) {
        let mut add = |r: egui::Rect| {
            *out = Some(out.map_or(r, |b: egui::Rect| b.union(r)));
        };
        match shape {
            egui::Shape::Rect(r) => add(r.rect),
            egui::Shape::Circle(c) => add(egui::Rect::from_center_size(
                c.center,
                egui::vec2(c.radius * 2.0, c.radius * 2.0),
            )),
            egui::Shape::Path(p) => {
                for pt in &p.points {
                    add(egui::Rect::from_min_max(*pt, *pt));
                }
            }
            egui::Shape::LineSegment { points, .. } => {
                for pt in points {
                    add(egui::Rect::from_min_max(*pt, *pt));
                }
            }
            egui::Shape::QuadraticBezier(q) => {
                for pt in [q.points[0], q.points[1], q.points[2]] {
                    add(egui::Rect::from_min_max(pt, pt));
                }
            }
            egui::Shape::CubicBezier(c) => {
                for pt in c.points {
                    add(egui::Rect::from_min_max(pt, pt));
                }
            }
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_bounds(s, out);
                }
            }
            // Text is not an icon; a mesh or a callback is not used by `paint_icon`.
            _ => {}
        }
    }

    /// Every filled rectangle of a frame with its colour: the 2 px accent edge of the active
    /// settings section is a fill, not a stroke (M22.8).
    fn collect_fills(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Color32)>) {
        match shape {
            egui::Shape::Rect(r) if r.fill != egui::Color32::TRANSPARENT => {
                out.push((r.rect, r.fill));
            }
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_fills(s, out);
                }
            }
            _ => {}
        }
    }

    /// Every outlined rectangle of a frame with its stroke: the calendar check needs to know which
    /// cell carries the accent frame, and that nothing else does (M22.5).
    fn collect_strokes(shape: &egui::Shape, out: &mut Vec<(egui::Rect, egui::Stroke)>) {
        match shape {
            egui::Shape::Rect(r) if r.stroke != egui::Stroke::NONE => {
                out.push((r.rect, r.stroke));
            }
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_strokes(s, out);
                }
            }
            _ => {}
        }
    }

    /// Every rectangle of a frame with its fill, its line and its rounding: a dialog is a card,
    /// so its frame is recognised by the tone, the 1 px line and the 8 px radius together (M22.10).
    fn collect_rect_styles(
        shape: &egui::Shape,
        out: &mut Vec<(egui::Rect, egui::Color32, egui::Stroke, egui::CornerRadius)>,
    ) {
        match shape {
            egui::Shape::Rect(r) => out.push((r.rect, r.fill, r.stroke, r.corner_radius)),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_rect_styles(s, out);
                }
            }
            _ => {}
        }
    }

    /// One frame of the whole window with every outlined rectangle: the accent frame of a picked
    /// calendar cell is a 1 px stroke of the palette accent, so its colour and width are checkable.
    fn window_strokes(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> Vec<(egui::Rect, egui::Stroke)> {
        super::style(ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
        let mut rects = Vec::new();
        for c in &out.shapes {
            collect_strokes(&c.shape, &mut rects);
        }
        out.drop_without_applying_deltas();
        rects
    }

    /// One frame of the whole window with every filled rectangle: the accent edge of the active
    /// section of the settings dialog is a 2 px fill, and a fill is not a stroke (M22.8).
    fn window_fills(app: &mut App, ctx: &egui::Context) -> Vec<(egui::Rect, egui::Color32)> {
        super::style(ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 840.0),
            )),
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
        let mut fills = Vec::new();
        for c in &out.shapes {
            collect_fills(&c.shape, &mut fills);
        }
        out.drop_without_applying_deltas();
        fills
    }

    /// One frame of the whole window at a given size with the position of every word: that is how
    /// the panel budget (M22.3) is checked without a window.
    fn window_layout(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> Vec<(String, egui::Pos2)> {
        super::style(ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text_pos(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// The tag chips of the left panel drawn in a frame without a window: the size of the row and
    /// the place of every chip. Two runs compare the row before and after a click.
    fn chips_layout(app: &mut App, width: f32) -> (egui::Vec2, Vec<(String, egui::Pos2)>) {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 400.0),
            )),
            ..Default::default()
        };
        let mut used = egui::Vec2::ZERO;
        let out = ctx.run_ui(raw, |ui| {
            app.tag_chips(ui);
            used = ui.min_rect().size();
        });
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text_pos(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        (used, texts)
    }

    /// A click on a chip of the cloud drawn alone: the press and the release in two frames of
    /// the same context, as `click_at` does for the whole window.
    fn chips_click(app: &mut App, width: f32, at: egui::Pos2) {
        let ctx = egui::Context::default();
        super::style(&ctx);
        // Frames in order: the chips under the pointer, the press, the release.
        let frames: Vec<Vec<egui::Event>> = vec![
            vec![egui::Event::PointerMoved(at)],
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed: false,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        ];
        for events in frames {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width, 400.0),
                )),
                events,
                ..Default::default()
            };
            let out = ctx.run_ui(raw, |ui| {
                app.tag_chips(ui);
            });
            out.drop_without_applying_deltas();
        }
    }

    /// The "Tags" heading drawn alone: every filled rectangle of the row, so a test can see
    /// whether the 20 px reset target is there or not.
    fn tags_heading_frame(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<egui::Rect> {
        super::style(ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(236.0, 100.0),
            )),
            events,
            ..Default::default()
        };
        let out = ctx.run_ui(raw, |ui| {
            ui.set_width(236.0);
            app.tags_heading(ui);
        });
        let mut fills = Vec::new();
        for c in &out.shapes {
            collect_fills(&c.shape, &mut fills);
        }
        out.drop_without_applying_deltas();
        fills.into_iter().map(|(r, _)| r).collect()
    }

    /// One frame of the full-screen viewer without a window: returns all the drawn text.
    /// The only way to check that "paging shows the next file" without a GUI.
    fn viewer_frame(
        app: &mut App,
        ctx: &egui::Context,
        key: Option<egui::Key>,
    ) -> Vec<String> {
        let raw = |key: Option<egui::Key>| {
            let mut raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1240.0, 840.0),
                )),
                ..Default::default()
            };
            if let Some(key) = key {
                raw.events.push(egui::Event::Key {
                    key,
                    physical_key: None,
                    pressed: true,
                    repeat: false,
                    modifiers: Default::default(),
                });
            }
            raw
        };
        // A modal window (an anchored area) does not know its size on the first frame and draws
        // only the background. So the frame is run twice: the event in the first one, the ready
        // shapes from the second.
        let first = ctx.run_ui(raw(key), |ui| {
            let ctx = ui.ctx().clone();
            app.viewer_modal(&ctx);
        });
        first.drop_without_applying_deltas();
        let out = ctx.run_ui(raw(None), |ui| {
            let ctx = ui.ctx().clone();
            app.viewer_modal(&ctx);
        });
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// The text of a full-screen frame with where it landed: the strip budget is checked by places,
    /// not by words (M22.7).
    fn viewer_frame_positions(app: &mut App, ctx: &egui::Context) -> Vec<(String, egui::Pos2)> {
        viewer_frame_positions_at(app, ctx, egui::vec2(1240.0, 840.0))
    }

    /// The same at any window size: the tools row wraps on a narrow window, the body must follow.
    fn viewer_frame_positions_at(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> Vec<(String, egui::Pos2)> {
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let first = ctx.run_ui(raw(), |ui| {
            let ctx = ui.ctx().clone();
            app.viewer_modal(&ctx);
        });
        first.drop_without_applying_deltas();
        let out = ctx.run_ui(raw(), |ui| {
            let ctx = ui.ctx().clone();
            app.viewer_modal(&ctx);
        });
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text_pos(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// One frame of the file card without a window: it checks the "not sure" text and the top 3.
    fn card_frame(app: &mut App, ctx: &egui::Context) -> Vec<String> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(320.0, 840.0),
            )),
            ..Default::default()
        };
        let out = ctx.run_ui(raw, |ui| app.details_panel(ui));
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// One frame of a "Settings" section without a window: it checks the captions of the section.
    /// The modal window is not drawn: the section is the same function called inside it.
    fn settings_frame(app: &mut App, ctx: &egui::Context, tab: SettingsTab) -> Vec<String> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(560.0, 1400.0),
            )),
            ..Default::default()
        };
        let out = ctx.run_ui(raw, |ui| app.settings_section(ui, tab));
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// One frame of a modal dialog without a window: `picker_modal`, `storage_modal`, `roots_modal`.
    /// Like `viewer_frame`, the frame is run twice: a modal window on the first frame does not
    /// know its size yet and draws only the background.
    fn modal_frame(
        app: &mut App,
        ctx: &egui::Context,
        draw: fn(&mut App, &egui::Context),
    ) -> Vec<String> {
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 840.0),
            )),
            ..Default::default()
        };
        let first = ctx.run_ui(raw(), |ui| {
            let ctx = ui.ctx().clone();
            draw(app, &ctx);
        });
        first.drop_without_applying_deltas();
        let out = ctx.run_ui(raw(), |ui| {
            let ctx = ui.ctx().clone();
            draw(app, &ctx);
        });
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// One dialog frame with the words and their fonts, the fills and every rectangle (fill, line,
    /// rounding): a dialog is a card with one accent button and mono numbers, and all of that is
    /// checkable without a window (M22.10).
    #[derive(Default)]
    struct DialogParts {
        texts: Vec<(String, egui::FontId, egui::Color32)>,
        fills: Vec<(egui::Rect, egui::Color32)>,
        rects: Vec<(egui::Rect, egui::Color32, egui::Stroke, egui::CornerRadius)>,
    }

    /// The dialogs the window has: the folder list, the picker, the three storage confirmations,
    /// the album dialog, the export dialog, the trash confirmation and the settings dialog.
    #[derive(Clone, Copy, PartialEq, Debug)]
    enum Dialog {
        Roots,
        Picker,
        Storage,
        Album,
        Export,
        Trash,
        Settings,
    }

    /// The state a dialog needs and the function that draws it: one place for both, so a test
    /// walks over every dialog of the window.
    fn open_dialog(app: &mut App, which: Dialog) -> fn(&mut App, &egui::Context) {
        match which {
            Dialog::Roots => {
                app.roots_open = true;
                App::roots_modal
            }
            Dialog::Picker => {
                app.picker_open = true;
                app.refresh_picker();
                App::picker_modal
            }
            Dialog::Storage => {
                app.storage_confirm = Some(StorageAction::Thumbs);
                App::storage_modal
            }
            Dialog::Album => {
                app.album_selected = app.albums.first().map(|n| n.id);
                app.album_open_dialog(AlbumDialog::Delete);
                App::album_modal
            }
            Dialog::Export => {
                if let Some(id) = app.albums.first().map(|n| n.id) {
                    app.export_dialog_open(id);
                }
                App::export_modal
            }
            Dialog::Trash => {
                let mut row = file_row(1, "image", "trashed.jpg");
                row.trashed_at = Some(1_700_000_000);
                app.trash_items = vec![row];
                app.trash_confirm = true;
                App::trash_modal
            }
            Dialog::Settings => {
                app.settings_open = true;
                App::settings_modal
            }
        }
    }

    fn dialog_parts(
        app: &mut App,
        ctx: &egui::Context,
        draw: fn(&mut App, &egui::Context),
    ) -> DialogParts {
        // The palette of the window: a modal inherits the style of the context it is shown in, so
        // the dialog is looked at in the theme the app carries (M22.11).
        super::apply_theme(ctx, app.settings.theme);
        let raw = |t: f64| egui::RawInput {
            time: Some(t),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1240.0, 840.0),
            )),
            ..Default::default()
        };
        // A modal does not know its size on the first frame and fades in over a fifth of a second:
        // the colours of the last frame are the palette's only when the clock has run.
        let mut t = 0.0_f64;
        for _ in 0..20 {
            let out = ctx.run_ui(raw(t), |ui| {
                let ctx = ui.ctx().clone();
                draw(app, &ctx);
            });
            out.drop_without_applying_deltas();
            t += 1.0 / 60.0;
        }
        let out = ctx.run_ui(raw(t), |ui| {
            let ctx = ui.ctx().clone();
            draw(app, &ctx);
        });
        let mut parts = DialogParts::default();
        for c in &out.shapes {
            collect_text_formats(&c.shape, &mut parts.texts);
            collect_fills(&c.shape, &mut parts.fills);
            collect_rect_styles(&c.shape, &mut parts.rects);
        }
        out.drop_without_applying_deltas();
        parts
    }

    /// One frame of the whole window without a window: it checks the month groups and their order.
    /// `eframe::Frame::_new_kittest` is the standard test frame of eframe, no window is opened.
    fn window_frame(app: &mut App, ctx: &egui::Context) -> Vec<String> {
        window_frame_with(app, ctx, Vec::new())
    }

    /// The same frame with keyboard events: that is how Esc and Ctrl+A are checked without a GUI.
    fn window_frame_with(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<String> {
        window_frame_at(app, ctx, events, egui::vec2(1240.0, 840.0))
    }

    /// A frame at a given window size: a tall window is needed so that the blocks of the left
    /// panel (the list of face groups) are not cut off and land in the drawn text.
    fn window_frame_at(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
        size: egui::Vec2,
    ) -> Vec<String> {
        super::style(ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// One frame of the whole window with every drawn word, its font and its colour: the screens
    /// without a mock-up are checked for the numeric family and for the palette (M22.9).
    fn window_text_formats(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> Vec<(String, egui::FontId, egui::Color32)> {
        super::style(ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text_formats(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// The rectangles of the frame (fills and strokes): a row that does not fit its panel paints
    /// past the window, and a word alone can hide inside the padding of a button (M22.12).
    fn collect_rect_bounds(shape: &egui::Shape, out: &mut Vec<egui::Rect>) {
        match shape {
            egui::Shape::Rect(r) => out.push(r.rect),
            egui::Shape::Vec(v) => {
                for s in v {
                    collect_rect_bounds(s, out);
                }
            }
            _ => {}
        }
    }

    /// One frame of the whole window with every rectangle it painted.
    fn window_rects_at(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> Vec<egui::Rect> {
        super::style(ctx);
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let first = ctx.run_ui(raw(), |ui| eframe::App::ui(app, ui, &mut frame));
        first.drop_without_applying_deltas();
        let out = ctx.run_ui(raw(), |ui| eframe::App::ui(app, ui, &mut frame));
        let mut rects = Vec::new();
        for c in &out.shapes {
            collect_rect_bounds(&c.shape, &mut rects);
        }
        out.drop_without_applying_deltas();
        rects
    }

    /// One frame of the whole window with every word and where it landed, at any window size.
    fn window_word_rects_at(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
    ) -> Vec<(String, egui::Rect)> {
        super::style(ctx);
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut frame = eframe::Frame::_new_kittest();
        let first = ctx.run_ui(raw(), |ui| eframe::App::ui(app, ui, &mut frame));
        first.drop_without_applying_deltas();
        let out = ctx.run_ui(raw(), |ui| eframe::App::ui(app, ui, &mut frame));
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text_rects(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    /// The whole window with the wheel turned over the grid: egui smooths a notch over ~10 frames,
    /// so the clock has to run and several frames are needed for the grid to move (M22.4). The
    /// clock is shared between the calls: egui reads a step backwards as no time at all.
    fn scrolled_grid(
        app: &mut App,
        ctx: &egui::Context,
        clock: &mut f64,
        notches: f32,
    ) -> Vec<(String, egui::Pos2)> {
        super::style(ctx);
        let size = egui::vec2(1240.0, 840.0);
        let mut drawn = Vec::new();
        for pass in 0..24 {
            let mut events = Vec::new();
            if pass == 0 && notches != 0.0 {
                // The wheel moves the area under the pointer, so it has to lie over the grid.
                events.push(egui::Event::PointerMoved(egui::pos2(700.0, 400.0)));
                events.push(wheel_event(-notches));
            }
            let raw = egui::RawInput {
                time: Some(*clock),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            };
            let mut window = eframe::Frame::_new_kittest();
            let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut window));
            drawn.clear();
            for c in &out.shapes {
                collect_text_pos(&c.shape, &mut drawn);
            }
            out.drop_without_applying_deltas();
            *clock += 1.0 / 60.0;
        }
        drawn
    }

    /// A key press without modifiers — for the headless check of Esc and Ctrl+A.
    fn key_event(key: egui::Key) -> egui::Event {
        egui::Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        }
    }

    // ---- M22.2b: scrolling with the wheel and the touchpad ---------------------------------

    /// A touchpad event: on Linux and Windows winit gives exact deltas without a source.
    fn trackpad_event(dy: f32) -> egui::Event {
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Default::default(),
        }
    }

    /// A touchpad event with a modifier (Ctrl/Cmd is a scale gesture, not a scroll).
    fn trackpad_event_mod(dy: f32, modifiers: egui::Modifiers) -> egui::Event {
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers,
        }
    }

    /// A wheel event: one "notch" is one line (winit), egui converts it into points itself.
    fn wheel_event(dy: f32) -> egui::Event {
        egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Line,
            delta: egui::vec2(0.0, dy),
            phase: egui::TouchPhase::Move,
            source: egui::MouseWheelSource::Unknown,
            modifiers: Default::default(),
        }
    }

    /// One frame of the scrolling profile: feed the events, run `Scroll::pump` and return how
    /// many points it added to the frame scroll and whether it asks for the next frame.
    fn scroll_frame(
        scroll: &mut Scroll,
        ctx: &egui::Context,
        t: f64,
        events: Vec<egui::Event>,
    ) -> (bool, egui::Vec2) {
        scroll_frame_mod(scroll, ctx, t, events, Default::default())
    }

    /// The same with the modifier held: that is how "Ctrl + scroll is a zoom" is checked.
    fn scroll_frame_mod(
        scroll: &mut Scroll,
        ctx: &egui::Context,
        t: f64,
        events: Vec<egui::Event>,
        modifiers: egui::Modifiers,
    ) -> (bool, egui::Vec2) {
        let mut events = events;
        if modifiers != egui::Modifiers::NONE {
            events.insert(0, egui::Event::ModifiersChanged(modifiers));
        }
        let raw = egui::RawInput {
            time: Some(t),
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(800.0, 600.0),
            )),
            events,
            ..Default::default()
        };
        let mut wants = false;
        let mut added = egui::Vec2::ZERO;
        let out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            let before = ctx.input(|i| i.smooth_scroll_delta);
            wants = scroll.pump(&ctx);
            added = ctx.input(|i| i.smooth_scroll_delta) - before;
        });
        out.drop_without_applying_deltas();
        (wants, added)
    }

    /// A six-frame touchpad gesture and the coast to a stop: returns the distance travelled and
    /// how many frames it took.
    fn swipe_and_coast(scroll: &mut Scroll, ctx: &egui::Context) -> (f32, usize) {
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        // The gesture: 6 frames of 20 px — 1200 px/s, a usual short fling speed.
        for _ in 0..6 {
            let (wants, _) = scroll_frame(scroll, ctx, t, vec![trackpad_event(-20.0)]);
            assert!(!wants, "coasting does not start during a gesture");
            t += dt;
        }
        // The pause between the fingers lifting and the coast starting is not a scroll.
        let mut waited = 0;
        while waited < 20 {
            let (wants, _) = scroll_frame(scroll, ctx, t, Vec::new());
            t += dt;
            waited += 1;
            if wants {
                break;
            }
        }
        assert!(waited < 20, "coasting did not start after the fingers lift");
        let mut coasted = 0.0;
        let mut frames = 0;
        loop {
            let (wants, added) = scroll_frame(scroll, ctx, t, Vec::new());
            t += dt;
            frames += 1;
            coasted -= added.y;
            if !wants {
                break;
            }
            assert!(frames < 600, "coasting never ends");
        }
        (coasted, frames)
    }

    /// A wheel notch used to be 40 px on a grid of 2000 rows — 50 notches to a month. Now one
    /// notch is a third of the default window, and the profile is one for both themes.
    #[test]
    fn wheel_scrolls_a_third_of_the_screen() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let fallback_h = 840.0;
        let speed = ctx.options(|o| o.input_options.line_scroll_speed);
        assert!((speed - WHEEL_NOTCH_PTS).abs() < 0.01, "the notch is not from the profile: {speed}");
        assert!(
            (speed - fallback_h / 3.0).abs() < 0.01,
            "the notch {speed} is not a third of the window {fallback_h}"
        );
        super::apply_theme(&ctx, db::ThemeMode::Light);
        assert_eq!(ctx.options(|o| o.input_options.line_scroll_speed), WHEEL_NOTCH_PTS);
        let decay = ctx
            .style_of(egui::Theme::Light)
            .spacing
            .scroll
            .kinetic
            .decay_time;
        assert!((decay - COAST_DECAY_S).abs() < 1e-6, "the decay is not from the profile: {decay}");
    }

    /// The touchpad: after the fingers lift the gesture continues as a coast and stops by itself.
    /// A short gesture — 200–300 px, not "half a row" and not half a screen.
    #[test]
    fn trackpad_coasts_after_the_fingers_lift() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut scroll = Scroll::default();
        let (coasted, frames) = swipe_and_coast(&mut scroll, &ctx);
        assert!(
            (200.0..=340.0).contains(&coasted),
            "a short gesture coasts {coasted} px — not 200–300"
        );
        assert!(frames > 5, "coasting broke off after {frames} frames");
    }

    /// An end-to-end check on a real window frame: the wheel and the touchpad move the grid itself.
    /// The scrolling profile was checked by numbers above; what matters here is that the addition
    /// does not get lost on the way to the `ScrollArea`, and that the coast runs without touchpad events and fades out by itself.
    #[test]
    fn wheel_and_trackpad_move_the_grid() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-scroll.db"));
        app.settings.grid_layout = db::GridLayout::Flat;
        app.settings.lang = i18n::Lang::En;
        app.items = (0..60)
            .map(|i| file_row(i as i64 + 1, "image", &format!("f{i}.jpg")))
            .collect();
        let ctx = egui::Context::default();
        // The cursor in the centre of the grid: without it the `ScrollArea` does not take the scroll.
        let at_grid = vec![egui::Event::PointerMoved(egui::pos2(700.0, 400.0))];
        // The first visible tile: the smaller name on the screen.
        let first_shown = |texts: &[String]| {
            texts
                .iter()
                .filter_map(|t| t.strip_prefix('f')?.strip_suffix(".jpg")?.parse::<usize>().ok())
                .min()
                .unwrap_or(usize::MAX)
        };
        window_frame_with(&mut app, &ctx, at_grid.clone());
        let top = window_frame_with(&mut app, &ctx, at_grid.clone());
        assert_eq!(first_shown(&top), 0, "the grid does not start at the top: {:?}", top.len());

        // The wheel: two notches (a third of the screen each) — the first tiles go up.
        let mut after_wheel = top.clone();
        for _ in 0..2 {
            after_wheel = window_frame_with(&mut app, &ctx, vec![wheel_event(-1.0)]);
        }
        for _ in 0..20 {
            after_wheel = window_frame_with(&mut app, &ctx, Vec::new());
        }
        let wheel_top = first_shown(&after_wheel);
        assert!(wheel_top > 0, "the wheel did not move the grid");

        // The touchpad: a gesture, and after the fingers lift — frames without events.
        for _ in 0..6 {
            let mut events = vec![trackpad_event(-20.0)];
            events.extend(at_grid.clone());
            window_frame_with(&mut app, &ctx, events);
        }
        // Wait for the coast to start (the state of the profile itself, not "the frame moved":
        // the smoothing of the last deltas moves the grid without inertia too).
        let mut coast_from = None;
        let mut settled_at = None;
        for _ in 0..300 {
            let texts = window_frame_with(&mut app, &ctx, Vec::new());
            if coast_from.is_none() && app.scroll.coasting {
                coast_from = Some(first_shown(&texts));
            } else if coast_from.is_some() && !app.scroll.coasting {
                settled_at = Some(first_shown(&texts));
                break;
            }
        }
        let coast_from = coast_from.expect("coasting did not start after the fingers lift");
        let settled_at = settled_at.expect("coasting did not settle within 300 frames");
        assert!(
            settled_at > coast_from,
            "coasting does not move the grid: was {coast_from}, became {settled_at}"
        );
        // At rest the profile is silent and the grid stands.
        let mut settled = Vec::new();
        for _ in 0..40 {
            settled = window_frame_with(&mut app, &ctx, Vec::new());
        }
        assert_eq!(
            first_shown(&settled),
            settled_at,
            "the grid keeps moving after the coast"
        );
    }

    /// A change of direction gives no jump: a new touch cancels the coast in the same frame.
    #[test]
    fn a_new_gesture_cancels_the_coast() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut scroll = Scroll::default();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        for _ in 0..4 {
            scroll_frame(&mut scroll, &ctx, t, vec![trackpad_event(-12.0)]);
            t += dt;
        }
        // The first frame after the pause starts the coast.
        let mut started = false;
        for _ in 0..10 {
            let (wants, _) = scroll_frame(&mut scroll, &ctx, t, Vec::new());
            t += dt;
            if wants {
                started = true;
                break;
            }
        }
        assert!(started, "coasting never started");
        // The reverse gesture: in this frame the profile adds nothing — no jump; after that egui
        // itself drives the wheel.
        let (wants, added) = scroll_frame(&mut scroll, &ctx, t, vec![trackpad_event(30.0)]);
        assert!(!wants, "a new gesture did not interrupt the coast");
        assert_eq!(added, egui::Vec2::ZERO, "a jump on a change of direction: {added:?}");
    }

    /// The wheel and any other way of scrolling break the touchpad coast too.
    #[test]
    fn the_wheel_cancels_the_trackpad_coast() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut scroll = Scroll::default();
        let (coasted, _) = swipe_and_coast(&mut scroll, &ctx);
        assert!(coasted > 0.0);
        let (wants, added) = scroll_frame(&mut scroll, &ctx, 10.0, vec![wheel_event(-1.0)]);
        assert!(!wants, "the wheel did not cancel the coast");
        assert_eq!(added, egui::Vec2::ZERO);
    }

    /// Ctrl/Cmd + gesture is a zoom, not a scroll: after it the grid must not roll.
    #[test]
    fn a_ctrl_gesture_does_not_coast() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut scroll = Scroll::default();
        let dt = 1.0 / 60.0;
        let mut t = 0.0;
        for _ in 0..6 {
            let (wants, added) = scroll_frame_mod(
                &mut scroll,
                &ctx,
                t,
                vec![trackpad_event_mod(-20.0, egui::Modifiers::COMMAND)],
                egui::Modifiers::COMMAND,
            );
            assert!(!wants);
            assert_eq!(added, egui::Vec2::ZERO, "a zoom gesture moved the grid");
            t += dt;
        }
        for i in 0..30 {
            let (wants, added) = scroll_frame_mod(
                &mut scroll,
                &ctx,
                t + i as f64 * dt,
                Vec::new(),
                egui::Modifiers::NONE,
            );
            assert!(!wants, "coasting started after a zoom gesture");
            assert_eq!(added, egui::Vec2::ZERO);
        }
    }

    /// At rest the profile is silent: no idle frames are requested, the window is not repainted in vain.
    #[test]
    fn frames_are_not_requested_at_rest() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut scroll = Scroll::default();
        for i in 0..30 {
            let (wants, added) = scroll_frame(&mut scroll, &ctx, i as f64 / 60.0, Vec::new());
            assert!(!wants, "a frame was requested without scrolling");
            assert_eq!(added, egui::Vec2::ZERO);
        }
    }

    /// The rule "one profile for every area": no `ScrollArea` sets wheel multipliers of its own
    /// and does not switch the scroll sources on or off.
    #[test]
    fn scroll_areas_do_not_set_their_own_multipliers() {
        let src = include_str!("app.rs");
        for needle in [concat!("wheel_scroll", "_multiplier"), concat!(".scroll_", "source(")] {
            assert!(!src.contains(needle), "a hand-rolled scroll rule was found: {needle}");
        }
        assert!(src.contains("ScrollArea"), "the file holds no scroll area at all");
    }

    /// The M22.2b measurement on the real corpus: grid frames while scrolling with the wheel and the touchpad.
    /// Not in the ordinary run: `PHOTOCAT_BENCH_DB=~/.cache/photocat/photocat.db \
    /// cargo test --release -- --ignored scroll_bench --nocapture`.
    #[test]
    #[ignore]
    fn grid_scroll_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut frame = eframe::Frame::_new_kittest();
        let size = egui::vec2(1240.0, 840.0);
        let mut drive = |events: Vec<egui::Event>, t: f64| {
            let raw = egui::RawInput {
                time: Some(t),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                events,
                ..Default::default()
            };
            let out = ctx.run_ui(raw, |ui| eframe::App::ui(&mut app, ui, &mut frame));
            out.drop_without_applying_deltas();
        };
        let dt = 1.0 / 240.0;
        let mut t: f64 = 0.0;
        let run = |drive: &mut dyn FnMut(Vec<egui::Event>, f64),
                       t: &mut f64,
                       events: Vec<egui::Event>| {
            let start = Instant::now();
            drive(events, *t);
            *t += dt;
            start.elapsed().as_secs_f32() * 1000.0
        };
        let mut times = Vec::new();
        for _ in 0..20 {
            run(&mut drive, &mut t, Vec::new());
        }
        // Wheel notches down — the grid goes from the fresh months to the old ones: one frame per
        // notch, 240 "notches per second", together with loading the previews.
        for _ in 0..80 {
            times.push(run(&mut drive, &mut t, vec![wheel_event(-1.0)]));
        }
        // The touchpad: a continuous gesture with a coast (the profile adds the inertia itself).
        for _ in 0..80 {
            times.push(run(&mut drive, &mut t, vec![trackpad_event(-14.0)]));
        }
        times.sort_by(|a, b| a.total_cmp(b));
        let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
        println!(
            "corpus: {n} files; frames: {}; p50 {:.2} ms ({:.0} fps), p95 {:.2} ms ({:.0} fps), p99 {:.2} ms",
            times.len(),
            p(0.50),
            1000.0 / p(0.50),
            p(0.95),
            1000.0 / p(0.95),
            p(0.99)
        );
    }

    /// The M25.2 panel budget on the real corpus: where the scrolling part, the year cloud and the
    /// pinned summary stand in a 1240×840 window, and that the "…" is among the drawn chips.
    /// `PHOTOCAT_BENCH_DB=~/.cache/photocat/photocat.db \
    /// cargo test --release -- --ignored panel_years_measurement --nocapture`.
    #[test]
    #[ignore]
    fn panel_years_measurement() {
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        app.settings.lang = i18n::Lang::Ru;
        let years = app.year_counts();
        let cloud: Vec<&str> = years.iter().take(5).map(|(y, _)| y.as_str()).collect();
        assert_eq!(cloud.len(), 5, "the corpus needs at least five years");
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_layout(&mut app, &ctx, size);
        let drawn = window_layout(&mut app, &ctx, size);
        let at = |needle: &str| {
            drawn
                .iter()
                .rfind(|(text, pos)| text == needle && pos.x < super::LEFT_W)
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("“{needle}” is not drawn"))
        };
        let heading = at("ГОДЫ");
        let first = at(cloud[0]);
        let ellipsis = at("…");
        let summary = at("Сводка");
        let last_old = years.last().map(|(y, _)| y.as_str()).unwrap_or("");
        println!(
            "corpus: {n} files, {} years ({last_old}–{}); the years heading at y {:.1}, the first \
             chip at y {:.1}, the “…” chip at y {:.1} (h {:.1}), the cloud ends at {:.1}, the pinned \
             summary at y {:.1}",
            years.len(),
            cloud[0],
            heading.y,
            first.y,
            ellipsis.y,
            super::YEARS_CHIP_H,
            ellipsis.y + super::YEARS_CHIP_H,
            summary.y
        );
        assert!(
            ellipsis.y + super::YEARS_CHIP_H <= summary.y,
            "the cloud is cut off by the pinned summary"
        );
    }

    /// A month heading as `month_header` draws it: a month name, a space, a four-digit year.
    fn is_month_heading(text: &str, lang: i18n::Lang) -> bool {
        let year = text.split_whitespace().last().unwrap_or("");
        year.len() == 4
            && year.chars().all(|c| c.is_ascii_digit())
            && (1..=12).any(|m| text.starts_with(&format!("{} ", i18n::month_name(lang, m))))
    }

    /// The M22.4 pinning on the real corpus: after a wheel notch the month heading of the group
    /// under the top edge is still drawn in the same place. `PHOTOCAT_BENCH_DB=~/.cache/photocat/photocat.db \
    /// cargo test --release -- --ignored sticky_heading_measurement --nocapture`.
    #[test]
    #[ignore]
    fn sticky_heading_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        let lang = app.settings.lang;
        let ctx = egui::Context::default();
        let mut clock = 0.0_f64;
        // The owner may keep "Flat": the heading exists only in the grouped layout.
        app.settings.grid_layout = db::GridLayout::Months;
        let start = scrolled_grid(&mut app, &ctx, &mut clock, 0.0);
        let (first, top) = start
            .iter()
            .find(|(text, _)| is_month_heading(text, lang))
            .cloned()
            .expect("no month heading on the first screen");
        let began = Instant::now();
        let scrolled = scrolled_grid(&mut app, &ctx, &mut clock, 1.0);
        let frames = began.elapsed();
        let (pinned, placed) = scrolled
            .iter()
            .find(|(text, pos)| is_month_heading(text, lang) && (pos.y - top.y).abs() < 1.5)
            .cloned()
            .unwrap_or_else(|| panic!("no heading is pinned after a notch: {scrolled:?}"));
        println!(
            "corpus: {n} files; the first heading “{first}” at y {:.1}; after a wheel notch “{pinned}” \
             is drawn at y {:.1} ({}); 24 frames in {:.1} ms ({:.1} ms/frame)",
            top.y,
            placed.y,
            if pinned == first { "the same group" } else { "the next group" },
            frames.as_secs_f32() * 1000.0,
            frames.as_secs_f32() * 1000.0 / 24.0
        );
    }

    /// The M22.5 measurement on the real corpus: how much a calendar frame costs in the year and the
    /// month layers, and that the year layer fits a 1240×840 window. `PHOTOCAT_BENCH_DB=~/.cache/photocat/photocat.db \
    /// cargo test --release -- --ignored calendar_measurement --nocapture`.
    #[test]
    #[ignore]
    fn calendar_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut frame = eframe::Frame::_new_kittest();
        let size = egui::vec2(1240.0, 840.0);
        app.view = View::Calendar;
        fn drive(
            app: &mut App,
            ctx: &egui::Context,
            frame: &mut eframe::Frame,
            size: egui::Vec2,
            t: f64,
        ) -> f32 {
            let raw = egui::RawInput {
                time: Some(t),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let start = Instant::now();
            let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, frame));
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            out.drop_without_applying_deltas();
            ms
        }
        let mut t = 0.0_f64;
        for (scale, word) in [(CalScale::Year, "year"), (CalScale::Month, "month")] {
            app.cal_scale = scale;
            for _ in 0..4 {
                drive(&mut app, &ctx, &mut frame, size, t);
                t += 1.0 / 60.0;
            }
            let mut times = Vec::new();
            for _ in 0..16 {
                times.push(drive(&mut app, &ctx, &mut frame, size, t));
                t += 1.0 / 60.0;
            }
            times.sort_by(|a, b| a.total_cmp(b));
            let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
            println!(
                "corpus {n} files, {word} layer: p50 {:.2} ms ({:.0} fps), p95 {:.2} ms, p99 {:.2} ms",
                p(0.50),
                1000.0 / p(0.50),
                p(0.95),
                p(0.99)
            );
        }
        // The year layer fits: the last card of December and the day strip below are on the screen.
        app.cal_scale = CalScale::Year;
        let drawn = window_layout(&mut app, &ctx, size);
        let last_month = i18n::month_name(app.settings.lang, 12);
        let december = drawn
            .iter()
            .find(|(text, _)| text == last_month)
            .map(|(_, pos)| *pos);
        println!(
            "{} files, year layer at 1240×840: December at {december:?}",
            app.items.len()
        );
    }

    /// The albums screen on the real DB: a root album with eight sub-albums whose covers are real
    /// previews, sixteen 1240×840 frames. The numbers go to `docs/ARCHITECTURE.md` § Measurements.
    /// `PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored albums_measurement --nocapture`.
    #[test]
    #[ignore]
    fn albums_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        let ids: Vec<i64> = app.items.iter().map(|f| f.id).collect();
        let covers: Vec<String> = app
            .items
            .iter()
            .filter_map(|f| f.thumb.clone())
            .take(8)
            .collect();
        let mut albums = vec![db::AlbumNode {
            id: 1,
            parent_id: 0,
            position: 0,
            name: "Summer 2024".into(),
            note: String::new(),
            updated_at: "1714557600".into(),
            depth: 0,
            files: ids.len() as i64,
            trashed: 0,
            subalbums: covers.len() as i64,
            bytes: app.items.iter().map(|f| f.size).sum(),
            covers: covers.iter().take(4).cloned().collect(),
        }];
        for i in 0..covers.len() {
            albums.push(db::AlbumNode {
                id: i as i64 + 2,
                parent_id: 1,
                position: i as i64,
                name: format!("Month {}", i + 1),
                note: String::new(),
                updated_at: "1714557600".into(),
                depth: 1,
                files: 12,
                trashed: 0,
                subalbums: 0,
                bytes: 12 * 1024 * 1024,
                // Every sub-album card carries the full mosaic of four previews (M22.21).
                covers: covers.iter().take(4).cloned().collect(),
            });
        }
        app.albums = albums;
        app.album_selected = Some(1);
        app.album_set = ids.into_iter().collect();
        app.album_browse_all = false;
        app.view = View::Albums;

        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut frame = eframe::Frame::_new_kittest();
        let size = egui::vec2(1240.0, 840.0);
        let mut drive = |app: &mut App, t: f64| -> f32 {
            let raw = egui::RawInput {
                time: Some(t),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let start = Instant::now();
            let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            out.drop_without_applying_deltas();
            ms
        };
        let mut t = 0.0_f64;
        for _ in 0..4 {
            drive(&mut app, t);
            t += 1.0 / 60.0;
        }
        let mut times = Vec::new();
        for _ in 0..16 {
            times.push(drive(&mut app, t));
            t += 1.0 / 60.0;
        }
        times.sort_by(|a, b| a.total_cmp(b));
        let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
        println!(
            "corpus {n} files, albums: p50 {:.2} ms ({:.0} fps), p95 {:.2} ms, p99 {:.2} ms",
            p(0.50),
            1000.0 / p(0.50),
            p(0.95),
            p(0.99)
        );
        let drawn = window_layout(&mut app, &ctx, size);
        let card = drawn
            .iter()
            .find(|(text, _)| text == "Month 1")
            .map(|(_, pos)| *pos);
        // The grid stays on screen under the cards: the album's snapshots are drawn, not pushed out.
        let snapshots = drawn
            .iter()
            .filter(|(text, _)| {
                let low = text.to_lowercase();
                low.ends_with(".jpg") || low.ends_with(".jpeg") || low.ends_with(".png") || low.ends_with(".mp4")
            })
            .count();
        println!(
            "{} files, albums at 1240×840: the first sub-album card at {card:?}, {snapshots} snapshot tiles on screen",
            app.items.len()
        );
    }

    /// The full screen on the real DB: the frame, the face chips, the series strip and the
    /// inspector, sixteen 1240×840 frames. The numbers go to `docs/ARCHITECTURE.md`
    /// § Measurements.
    /// `PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored viewer_measurement --nocapture`.
    #[test]
    #[ignore]
    fn viewer_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        // The day with the most snapshots: the series strip is part of the measurement.
        let mut best = (0usize, 0i64);
        for r in app.items.iter().filter(|r| r.kind == "image" && r.taken_at.is_some()) {
            let day = r.taken_at.as_deref().map(|d| &d[..10.min(d.len())]);
            let same = app
                .items
                .iter()
                .filter(|x| x.taken_at.as_deref().map(|d| &d[..10.min(d.len())]) == day)
                .count();
            if same > best.0 {
                best = (same, r.id);
            }
        }
        assert!(best.1 != 0, "a photo with a date in the corpus");
        app.open_viewer(best.1);
        app.viewer.as_mut().expect("the viewer is open").inspector = true;
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut frame = eframe::Frame::_new_kittest();
        let size = egui::vec2(1240.0, 840.0);
        let mut drive = |app: &mut App, t: f64| -> f32 {
            let raw = egui::RawInput {
                time: Some(t),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let start = Instant::now();
            let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            out.drop_without_applying_deltas();
            ms
        };
        let mut t = 0.0_f64;
        for (inspector, word) in [
            (false, "frame + chips + strip"),
            (true, "frame + chips + strip + inspector"),
        ] {
            app.viewer.as_mut().expect("the viewer is open").inspector = inspector;
            for _ in 0..4 {
                drive(&mut app, t);
                t += 1.0 / 60.0;
            }
            let mut times = Vec::new();
            for _ in 0..16 {
                times.push(drive(&mut app, t));
                t += 1.0 / 60.0;
            }
            times.sort_by(|a, b| a.total_cmp(b));
            let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
            println!(
                "corpus {n} files, full screen ({word}): p50 {:.2} ms ({:.0} fps), p95 {:.2} ms, p99 {:.2} ms",
                p(0.50),
                1000.0 / p(0.50),
                p(0.95),
                p(0.99)
            );
        }
        let faces = app.viewer_faces.as_ref().map(|(_, f)| f.len()).unwrap_or(0);
        let (day, series) = app
            .viewer
            .as_ref()
            .map(|v| (v.day.clone(), v.series.len()))
            .unwrap_or_default();
        println!(
            "the series of {day:?} — {series} snapshots, {faces} faces on the frame"
        );
    }

    fn viewer_with(ids: Vec<i64>, idx: usize) -> Viewer {
        Viewer {
            ids,
            idx,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            rotation: 0,
            inspector: false,
            show_faces: false,
            face_focus: None,
            day: None,
            series: Vec::new(),
            show_text: false,
            slideshow: false,
            interval_s: 3,
            last_advance: std::time::Instant::now(),
        }
    }

    /// Paging in the full screen must change the file within the same second, and a video must
    /// stop. Otherwise the frame of the old video is drawn over the new file.
    /// The owner: "a document on top, a screenshot below, and the furniture later". With close
    /// weights the card must say "not sure", show the top 3 and highlight nothing.
    #[test]
    fn card_writes_not_sure_when_tags_are_close() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![file_row(1, "image", "8.jpg")];
        app.selected = Some(1);
        app.tags.insert(
            1,
            vec![
                ("документ".to_string(), 0.276),
                ("интерьер".to_string(), 0.271),
                ("мебель".to_string(), 0.255),
                ("дети".to_string(), 0.080),
            ],
        );
        app.tag_dists = embed::tag_distributions(&app.tags);
        // The Russian text is checked explicitly: the language is a settings field, not a global variable.
        app.settings.lang = i18n::Lang::Ru;
        let texts = card_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t.contains("тег: не уверен")),
            "no honest “not sure”: {texts:?}"
        );
        assert!(texts.iter().any(|t| t.starts_with("документ ")));
        assert!(texts.iter().any(|t| t.starts_with("мебель ")));
        assert!(
            !texts.iter().any(|t| t.starts_with("дети ")),
            "the fourth tag is hidden under “all tags”: {texts:?}"
        );
    }

    #[test]
    fn card_highlights_a_confident_tag() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.selected = Some(1);
        app.tags.insert(
            1,
            vec![
                ("мебель".to_string(), 0.30),
                ("интерьер".to_string(), 0.25),
            ],
        );
        app.tag_dists = embed::tag_distributions(&app.tags);
        app.settings.lang = i18n::Lang::Ru;
        let texts = card_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "тег: мебель"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("не уверен")), "{texts:?}");
    }

    /// M21.3: the card and the inspector are translated completely — the heading, the captions,
    /// the date source, the tags, the own tags and the recognition block. The data (the file name, the tags themselves) is not touched.
    #[test]
    fn english_translates_the_file_card() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-card-i18n.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.items[0].camera = Some("Canon".into());
        app.items[0].taken_src = "filename".into();
        app.selected = Some(1);
        app.tags.insert(
            1,
            vec![
                ("furniture".to_string(), 0.30),
                ("interior".to_string(), 0.25),
            ],
        );
        app.tag_dists = embed::tag_distributions(&app.tags);
        app.settings.lang = i18n::Lang::En;
        let texts = card_frame(&mut app, &egui::Context::default());
        for needle in [
            "File details",
            "SHOOTING PARAMS",
            "DATE AND TIME",
            "RESOLUTION",
            "AI TAGS",
            "MY TAGS",
            "TEXT ON THE PHOTO",
            "Open full screen",
            "To trash",
        ] {
            assert!(
                texts.iter().any(|t| t == needle),
                "missing “{needle}”: {texts:?}"
            );
        }
        assert!(
            texts.iter().any(|t| t.starts_with("tag: ")),
            "a tag without an English label: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("file name")),
            "the date source is not translated: {texts:?}"
        );
        for ru in [
            "Сведения о файле",
            "ТЕГИ ИИ",
            "СВОИ ТЕГИ",
            "ТЕКСТ НА ФОТО",
            "Открыть полный экран",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
    }

    /// M21.4: the "Duplicates and series" screen is translated completely — the control panel,
    /// the tile captions and the group row; the names of the duplicate formats (`reason`, `advice`)
    /// are data of the `dupes` module, M21.6 translates them together with the CLI reports.
    #[test]
    fn english_translates_the_duplicates_screen() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-dupes-i18n.db"));
        let mut a = file_row(1, "image", "a.jpg");
        let mut b = file_row(2, "image", "b.jpg");
        a.sha256 = "same".into();
        b.sha256 = "same".into();
        app.items = vec![a, b];
        app.groups = crate::dupes::find_groups(&app.items, 6);
        assert_eq!(app.groups.len(), 1, "one group of exact copies");
        app.settings.lang = i18n::Lang::En;
        app.view = View::Dupes;
        // A taller frame: the two "missing" filter chips (M25.4) start the scrolling part 28 px
        // lower, so the last control of the duplicates block needs the room the test gives it.
        let texts = window_frame_at(
            &mut app,
            &egui::Context::default(),
            Vec::new(),
            egui::vec2(1240.0, 1000.0),
        );
        for needle in [
            "Duplicates & series",
            "Find groups",
            "Export the plan",
            "Copy the list",
        ] {
            assert!(
                texts.iter().any(|t| t == needle),
                "missing “{needle}”: {texts:?}"
            );
        }
        // The group is a card: "#1", the reason and the facts in mono (M22.9).
        assert!(
            texts.iter().any(|t| t == "#1")
                && texts.iter().any(|t| t.contains("2 files") && t.contains("spare")),
            "the group line is not translated: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("Similarity threshold: 6 bits")),
            "the threshold is not translated: {texts:?}"
        );
        for ru in [
            "Найти группы",
            "Экспорт плана",
            "Скопировать список",
            "оставить",
            "без даты",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
    }

    /// M21.4b: the calendar is translated completely — the layers, the months, the weekdays, the
    /// navigation and the captions of empty days. The month and weekday names live in `i18n`, not in `app`.
    #[test]
    fn english_translates_the_calendar() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-calendar-i18n.db"));
        app.items = vec![file_row(1, "image", "may.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.view = View::Calendar;
        app.cal_year = 2024;
        app.cal_month = 5;
        let texts = window_frame(&mut app, &egui::Context::default());
        for needle in ["Year", "Month", "Week", "Day", "Today", "May 2024"] {
            assert!(
                texts.iter().any(|t| t == needle),
                "missing “{needle}”: {texts:?}"
            );
        }
        // The weekday row of the month layer is a row of caps labels (M22.5), so it is upper case.
        for needle in ["MON", "SUN"] {
            assert!(
                texts.iter().any(|t| t == needle),
                "no weekday “{needle}”: {texts:?}"
            );
        }
        assert!(
            texts.iter().any(|t| t.contains("1 file this month")),
            "the month summary is not translated: {texts:?}"
        );
        for ru in [
            "Год", "Месяц", "Неделя", "День", "Сегодня", "Пн", "Вс", "мая", "ПН", "ВС",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
    }

    /// M21.4c: the "Faces" screen is translated completely — the threshold panel, the group list,
    /// the face tiles and the name fields. The names of people and the thresholds are data and are not translated.
    #[test]
    fn english_translates_the_faces_screen() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-faces-i18n.db"));
        app.face_faces = 2;
        app.face_photos = 1;
        app.face_images = 3;
        app.face_todo = 2;
        app.face_clusters = vec![db::FaceCluster {
            id: 7,
            label: String::new(),
            faces: 2,
            photos: 1,
        }];
        app.face_views = vec![db::FaceView {
            id: 1,
            file_id: 1,
            path: "/tmp/a.jpg".into(),
            taken_at: None,
            thumb: None,
            score: 0.62,
            cluster_id: Some(7),
        }];
        app.face_selected_cluster = Some(7);
        app.settings.lang = i18n::Lang::En;
        app.view = View::Faces;
        // A tall window: the group list in the right panel must not be cut off.
        let texts = window_frame_at(
            &mut app,
            &egui::Context::default(),
            Vec::new(),
            egui::vec2(1240.0, 1400.0),
        );
        for needle in [
            "People",
            "Resume (2)",
            "Rebuild groups",
            "Groups",
            "All people · 2",
            "no name · 1 photo",
            "Person's name:",
            "Save the name",
            "Name the picked people",
            "Unpick",
            "no frame",
            "no date",
        ] {
            assert!(
                texts.iter().any(|t| t == needle),
                "missing “{needle}”: {texts:?}"
            );
        }
        for ru in [
            "Лица",
            "Найти лица (2)",
            "Продолжить (2)",
            "Пересобрать группы",
            "Группы",
            "без имени",
            "не он",
            "выбрать",
            "нет кадра",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
    }

    /// M22.26: the group list is the right panel of the "People" screen, and the ribbon carries
    /// no dead cell-view switch there: the face tiles have one look of their own.
    #[test]
    fn the_faces_screen_keeps_its_list_in_the_right_panel() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-faces-panel.db"));
        app.face_faces = 2;
        app.face_photos = 1;
        app.face_clusters = vec![db::FaceCluster {
            id: 7,
            label: "Lisa".into(),
            faces: 2,
            photos: 1,
        }];
        app.face_views = vec![db::FaceView {
            id: 1,
            file_id: 1,
            path: "/tmp/a.jpg".into(),
            taken_at: None,
            thumb: None,
            score: 0.62,
            cluster_id: Some(7),
        }];
        app.settings.lang = i18n::Lang::En;
        app.view = View::Faces;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 1400.0);
        let layout = window_layout(&mut app, &ctx, size);
        let pos = |needle: &str| {
            layout
                .iter()
                .find(|(t, _)| t == needle)
                .map(|(_, p)| *p)
                .unwrap_or_else(|| panic!("no “{needle}”: {layout:?}"))
        };
        // The list lives on the right, after the 280 px panel edge, not in the left panel.
        assert!(
            pos("Groups").x >= size.x - super::RIGHT_W - 1.0,
            "the group heading is not in the right panel: {:?}",
            pos("Groups")
        );
        assert!(
            pos("All people · 2").x >= size.x - super::RIGHT_W,
            "the group rows are not in the right panel: {:?}",
            pos("All people · 2")
        );
        // The dead look switch and the card's own toggle are gone from this screen.
        for word in ["Tile", "Rows", "List", "Details"] {
            assert!(
                !layout.iter().any(|(t, _)| t == word),
                "the dead control “{word}” is drawn on the People screen: {layout:?}"
            );
        }
    }

    /// M21.4d: the trash is translated completely — the screen, the tile and the confirmation dialog.
    /// A modal window does not know its size on the first frame, so there are two frames.
    #[test]
    fn english_translates_the_trash() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-trash-i18n.db"));
        let mut row = file_row(1, "image", "trashed.jpg");
        row.trashed_at = Some(1_700_000_000);
        app.trash_items = vec![row];
        app.settings.lang = i18n::Lang::En;
        app.view = View::Trash;
        app.trash_confirm = true;
        let ctx = egui::Context::default();
        let _ = window_frame(&mut app, &ctx);
        let texts = window_frame(&mut app, &ctx);
        for needle in [
            "Restore all",
            "Empty the trash (1)",
            "in trash",
            "Empty the trash?",
            "Move to the OS trash",
            "Cancel",
        ] {
            assert!(
                texts.iter().any(|t| t == needle),
                "missing “{needle}”: {texts:?}"
            );
        }
        assert!(
            texts.iter().any(|t| t.contains("1 file · ")),
            "the trash counter is not translated: {texts:?}"
        );
        for ru in [
            "Корзина пуста.",
            "Вернуть всё",
            "Очистить корзину (1)",
            "в корзине",
            "вернуть",
            "Очистить корзину?",
            "Переместить в системную корзину",
            "Отмена",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
    }

    /// M21.4e: the "Integrity" screen is translated completely — the fields, the three buttons,
    /// the reminder and the verdict. The report text is the output of the `verify` module (as in the CLI), M21.6 translates it.
    #[test]
    fn english_translates_the_integrity_screen() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity-i18n.db"));
        app.settings.lang = i18n::Lang::En;
        app.view = View::Integrity;
        app.integrity_result = Some(IntegrityOutcome::Verify {
            a: std::path::PathBuf::from("/tmp/a"),
            b: std::path::PathBuf::from("/tmp/b"),
            report: crate::verify::Report {
                files_a: 2,
                files_b: 2,
                only_a: Vec::new(),
                only_b: Vec::new(),
                size_diff: Vec::new(),
                content_diff: Vec::new(),
                read_errors: Vec::new(),
                same: 2,
                hashed: 2,
                hashed_bytes: 20,
                unchecked: 0,
                scan_errors: 0,
                skipped_a: 0,
                skipped_b: 0,
                cancelled: false,
                seconds: 1.0,
                lang: i18n::Lang::En,
            },
        });
        let texts = window_frame(&mut app, &egui::Context::default());
        for want in [
            "Integrity",
            "comparing copies and a sha256 manifest",
            "Folder A (archive):",
            "Folder B (copy):",
            "Manifest file:",
            "Show all paths",
            "Compare the copies",
            "Write the manifest",
            "Check against the manifest",
            "Remind me to check every",
            "0 — do not remind",
            "no manifest set — the reminder stays quiet",
            "the copies match",
            "Copy the report",
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "missing “{want}”: {texts:?}"
            );
        }
        for ru in [
            "Сверить копии",
            "Записать слепок",
            "Проверить по слепку",
            "Показать все пути",
            "Каталог A (архив):",
            "Напоминать о проверке каждые",
            "Скопировать отчёт",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
        // Since M21.6 the report text is the `verify` output in the same language: the report and
        // the screen no longer differ in words or units.
        assert!(
            texts.iter().any(|t| t.contains("verdict: the copies match")),
            "the report is drawn in the report's language: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("вердикт: копии совпадают")),
            "a Russian report is left in the English window: {texts:?}"
        );
    }

    /// The colours a screen may use: the tokens of one palette plus white (an image tint); a
    /// colour with transparency is a tone of the palette over something else and is allowed too.
    fn palette(p: super::Theme) -> Vec<egui::Color32> {
        vec![
            p.canvas,
            p.panel,
            p.card,
            p.field,
            p.border,
            p.border_strong,
            p.text,
            p.text_med,
            p.text_dim,
            p.accent,
            p.accent_hover,
            p.on_accent,
            p.accent_soft,
            p.danger,
            egui::Color32::WHITE,
        ]
    }

    /// The WCAG 2.1 contrast ratio of two opaque tones: 1.0 is the same colour, 21.0 is black on
    /// white. M22.11 checks the palette with it instead of judging by eye.
    fn contrast(a: egui::Color32, b: egui::Color32) -> f32 {
        let luminance = |c: egui::Color32| {
            let channel = |v: u8| {
                let v = v as f32 / 255.0;
                if v <= 0.039_28 {
                    v / 12.92
                } else {
                    ((v + 0.055) / 1.055).powf(2.4)
                }
            };
            let [r, g, b, _] = c.to_srgba_unmultiplied();
            0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
        };
        let (a, b) = (luminance(a), luminance(b));
        let (hi, lo) = if a > b { (a, b) } else { (b, a) };
        (hi + 0.05) / (lo + 0.05)
    }

    /// What the eye sees when a translucent token is laid over a ground. egui keeps colours
    /// premultiplied, so the fill is already weighted by its alpha and is added to the ground.
    fn over(fill: egui::Color32, ground: egui::Color32) -> egui::Color32 {
        let [fr, fg, fb, fa] = fill.to_array();
        let [br, bg, bb, _] = ground.to_array();
        let k = 1.0 - fa as f32 / 255.0;
        let mix = |f: u8, b: u8| (f as f32 + b as f32 * k).round().min(255.0) as u8;
        egui::Color32::from_rgb(mix(fr, br), mix(fg, bg), mix(fb, bb))
    }

    /// One frame of the whole window in a chosen palette with every word, fill and line. The theme
    /// is set as a restart would set it: `settings.theme`, and the window applies it itself. Two
    /// frames, because a modal does not know its size on the first one.
    fn window_parts_in(
        app: &mut App,
        ctx: &egui::Context,
        size: egui::Vec2,
        mode: db::ThemeMode,
    ) -> DialogParts {
        app.settings.theme = mode;
        app.theme_key = None;
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            ..Default::default()
        };
        let mut window = eframe::Frame::_new_kittest();
        let first = ctx.run_ui(raw(), |ui| eframe::App::ui(app, ui, &mut window));
        first.drop_without_applying_deltas();
        let out = ctx.run_ui(raw(), |ui| eframe::App::ui(app, ui, &mut window));
        let mut parts = DialogParts::default();
        for c in &out.shapes {
            collect_text_formats(&c.shape, &mut parts.texts);
            collect_fills(&c.shape, &mut parts.fills);
            collect_rect_styles(&c.shape, &mut parts.rects);
        }
        out.drop_without_applying_deltas();
        parts
    }

    /// M22.9, M22.11: a screen writes no colour of its own — every drawn word, fill and stroke is a
    /// token of the palette of the theme in use, in both themes. (Text over a darkened photograph
    /// is the documented exception; it is the grey of a mark on a tile, never a theme tone.)
    fn assert_palette(app: &mut App, ctx: &egui::Context, screen: &str, p: super::Theme) {
        let size = egui::vec2(1240.0, 1400.0);
        let mode = if p.dark {
            db::ThemeMode::Dark
        } else {
            db::ThemeMode::Light
        };
        let allowed = palette(p);
        let theme = if p.dark { "dark" } else { "light" };
        let ok = |c: &egui::Color32, what: &str| {
            assert!(
                allowed.iter().any(|q| q == c) || c.a() < 255,
                "the {screen} screen paints {what} off the {theme} palette: {c:?}"
            );
        };
        let parts = window_parts_in(app, ctx, size, mode);
        for (text, _, color) in &parts.texts {
            // Text on a dimmed frame is the documented exception: the background there is a
            // photograph, not the theme. The marks on a tile keep gray(235)/gray(210).
            if *color == egui::Color32::from_gray(235) || *color == egui::Color32::from_gray(210) {
                continue;
            }
            ok(color, &format!("“{text}”"));
        }
        for (_, color) in &parts.fills {
            ok(color, "a fill");
        }
        for (_, _, stroke, _) in &parts.rects {
            if *stroke != egui::Stroke::NONE {
                ok(&stroke.color, "a stroke");
            }
        }
    }

    /// M22.9: the duplicates screen has no mock-up, so it is built from the parts of the main one —
    /// a group is a card of the card tone, "#1" and the facts in mono, the words "keep" and "spare"
    /// stay words, and nothing here is red: the screen touches no file.
    #[test]
    fn the_duplicates_screen_borrows_the_parts_of_the_main_one() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-dupes-card.db"));
        let mut a = file_row(1, "image", "a.jpg");
        let mut b = file_row(2, "image", "b.jpg");
        a.sha256 = "same".into();
        b.sha256 = "same".into();
        app.items = vec![a, b];
        app.groups = crate::dupes::find_groups(&app.items, 6);
        app.settings.lang = i18n::Lang::En;
        app.view = View::Dupes;
        let ctx = egui::Context::default();
        let formats = window_text_formats(&mut app, &ctx, egui::vec2(1240.0, 1400.0));
        let find = |needle: &str| {
            formats
                .iter()
                .find(|(t, _, _)| t == needle)
                .cloned()
                .unwrap_or_else(|| panic!("no “{needle}”: {formats:?}"))
        };
        // The labels are the ones the screen had: the number, the reason, the facts, the buttons.
        let (_, number_font, _) = find("#1");
        assert_eq!(number_font, egui::FontId::monospace(11.0), "the group number is mono");
        find("exact copy");
        let (_, facts_font, _) = find("2 files · 1 B spare");
        assert_eq!(facts_font, egui::FontId::monospace(11.0), "the facts are mono");
        find("Find groups");
        find("keep");
        assert!(
            formats.iter().any(|(t, _, _)| t.contains("keep b.jpg")),
            "the advice keeps its words: {formats:?}"
        );
        // The group is a card: a fill of the card tone across the content panel.
        let content = egui::Rect::from_min_max(
            egui::pos2(super::LEFT_W, 100.0),
            egui::pos2(1240.0, 800.0),
        );
        let fills = window_fills(&mut app, &ctx);
        assert!(
            fills.iter().any(|(r, c)| *c == super::DARK.card
                && content.contains_rect(*r)
                && r.width() > 400.0),
            "the group is not a card: {fills:?}"
        );
        assert!(
            !formats.iter().any(|(_, _, c)| *c == super::DARK.danger),
            "the duplicates screen has no red: {formats:?}"
        );
        assert_palette(&mut app, &ctx, "duplicates", super::DARK);
        assert_palette(&mut app, &ctx, "duplicates", super::LIGHT);
    }

    /// M22.9: the faces screen keeps the metrics of a face tile (150×150 + 26) and takes the look
    /// from the palette: the date and the score in mono 10, a picked face in the accent.
    #[test]
    fn the_faces_screen_takes_its_caption_from_the_palette() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-faces-card.db"));
        app.face_faces = 1;
        app.face_photos = 1;
        app.face_images = 1;
        app.face_todo = 0;
        app.face_clusters = vec![db::FaceCluster {
            id: 7,
            label: "Lisa".into(),
            faces: 1,
            photos: 1,
        }];
        app.face_views = vec![db::FaceView {
            id: 1,
            file_id: 1,
            path: "/tmp/a.jpg".into(),
            taken_at: Some("2024-05-01 10:00".into()),
            thumb: None,
            score: 0.62,
            cluster_id: Some(7),
        }];
        app.face_selected_cluster = Some(7);
        app.face_picked.insert(1);
        app.settings.lang = i18n::Lang::En;
        app.view = View::Faces;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 1400.0);
        let formats = window_text_formats(&mut app, &ctx, size);
        let find = |needle: &str| {
            formats
                .iter()
                .find(|(t, _, _)| t == needle)
                .cloned()
                .unwrap_or_else(|| panic!("no “{needle}”: {formats:?}"))
        };
        // The tile buttons are icons now: no word "not him", no word "pick" (M22.26).
        assert!(
            !formats
                .iter()
                .any(|(t, _, _)| t == "not him" || t == "pick" || t == "picked"),
            "a face tile still carries a worded button: {formats:?}"
        );
        find("Save the name");
        let (_, title_font, _) = find("Lisa");
        assert_eq!(title_font.size, 14.0, "the group's name is a 14 px subheading");
        let (_, facts_font, _) = find("1 person · 1 photo");
        assert_eq!(facts_font, egui::FontId::monospace(11.0), "the facts are mono");
        let (_, date_font, date_color) = find("2024-05-01");
        assert_eq!(date_font, egui::FontId::monospace(10.0), "the date is mono 10");
        assert_eq!(date_color, super::DARK.text_med);
        let (_, score_font, score_color) = find("0.62");
        assert_eq!(score_font, egui::FontId::monospace(10.0), "the score is mono 10");
        assert_eq!(score_color, super::DARK.text_dim);
        // A picked face carries a 2 px accent frame — the same mark as a picked tile.
        let strokes = window_strokes(&mut app, &ctx, size);
        assert!(
            strokes
                .iter()
                .any(|(_, s)| s.width == 2.0 && s.color == super::DARK.accent),
            "the picked face is not marked in the accent: {strokes:?}"
        );
        assert_palette(&mut app, &ctx, "faces", super::DARK);
        assert_palette(&mut app, &ctx, "faces", super::LIGHT);
    }

    /// M22.9: the trash counts in mono and has exactly one red word — its one action that changes
    /// the disk.
    #[test]
    fn the_trash_paints_only_its_emptying_in_danger() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-trash-card.db"));
        let mut row = file_row(1, "image", "trashed.jpg");
        row.trashed_at = Some(1_700_000_000);
        app.trash_items = vec![row];
        app.settings.lang = i18n::Lang::En;
        app.view = View::Trash;
        let ctx = egui::Context::default();
        let formats = window_text_formats(&mut app, &ctx, egui::vec2(1240.0, 1400.0));
        let find = |needle: &str| {
            formats
                .iter()
                .find(|(t, _, _)| t == needle)
                .cloned()
                .unwrap_or_else(|| panic!("no “{needle}”: {formats:?}"))
        };
        find("Restore all");
        find("in trash");
        // M22.27: the per-tile restore is a round icon target, not a worded button.
        assert!(
            !formats.iter().any(|(t, _, _)| t == "restore"),
            "a worded restore is still on the tile: {formats:?}"
        );
        let (_, empty_font, empty_color) = find("Empty the trash (1)");
        assert_eq!(empty_color, super::DARK.danger, "the emptying is the red word");
        assert_eq!(empty_font.size, 13.0);
        let restore_color = find("Restore all").2;
        assert_ne!(restore_color, super::DARK.danger, "the restore stays neutral");
        let (_, count_font, count_color) = find("1 file · 1 B");
        assert_eq!(count_font, egui::FontId::monospace(11.0), "the counter is mono");
        assert_eq!(count_color, super::DARK.text_med);
        assert_palette(&mut app, &ctx, "trash", super::DARK);
        assert_palette(&mut app, &ctx, "trash", super::LIGHT);
    }

    /// M22.9: the integrity screen is a form of the same window — the title where the grid's title
    /// stands, the path fields in the numeric family, one accent button and no other colour.
    #[test]
    fn the_integrity_screen_takes_its_fields_and_its_one_accent() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity-card.db"));
        app.integrity_a = "/tmp/archive".into();
        app.settings.lang = i18n::Lang::En;
        app.view = View::Integrity;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 1400.0);
        let formats = window_text_formats(&mut app, &ctx, size);
        let find = |needle: &str| {
            formats
                .iter()
                .find(|(t, _, _)| t == needle)
                .cloned()
                .unwrap_or_else(|| panic!("no “{needle}”: {formats:?}"))
        };
        // "Integrity" is also the word of the navigation row (13 px): the view title is the 18 px one.
        let title_font = formats
            .iter()
            .find(|(t, f, _)| t == "Integrity" && f.size == 18.0)
            .map(|(_, f, _)| f.clone())
            .expect("the title of the form");
        assert_eq!(title_font.size, 18.0, "the title is the view title");
        let (_, facts_font, _) = find("comparing copies and a sha256 manifest");
        assert_eq!(facts_font, egui::FontId::monospace(12.0), "the facts are mono 12");
        let (_, path_font, _) = find("/tmp/archive");
        assert_eq!(
            path_font.family,
            egui::FontFamily::Monospace,
            "a path is not proportional text"
        );
        // The title stands at the same place as the title of the grid: the content panel margin
        // of 12 px, not the raw edge of the central panel.
        let layout = window_layout(&mut app, &ctx, size);
        let (_, title_pos) = layout
            .iter()
            .find(|(t, pos)| t == "Integrity" && pos.x > super::LEFT_W)
            .expect("the title of the form");
        assert!(
            title_pos.x >= super::LEFT_W + 12.0,
            "the form starts at the edge of the panel: {title_pos:?}"
        );
        // One accent button: the main action of the screen.
        let content = egui::Rect::from_min_max(
            egui::pos2(super::LEFT_W, 100.0),
            egui::pos2(1240.0, 1000.0),
        );
        let accent_fills = window_fills(&mut app, &ctx)
            .into_iter()
            .filter(|(r, c)| *c == super::DARK.accent && content.contains_rect(*r))
            .count();
        assert_eq!(accent_fills, 1, "the form has exactly one accent fill");
        assert_palette(&mut app, &ctx, "integrity", super::DARK);
        assert_palette(&mut app, &ctx, "integrity", super::LIGHT);
    }

    /// The M22.9 measurement on the real DB: a frame of each screen without a mock-up.
    /// Not in the ordinary run: `PHOTOCAT_BENCH_DB=~/.cache/photocat/photocat.db \
    /// cargo test --release -- --ignored screens_without_a_mock_up_measurement --nocapture`.
    #[test]
    #[ignore]
    fn screens_without_a_mock_up_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        // The groups of the left panel's button: the screen is measured with its content, not empty.
        app.groups = crate::dupes::find_groups(&app.items, app.max_dist);
        let faces = app.face_views.len();
        if let Some(c) = app.face_clusters.first() {
            app.face_selected_cluster = Some(c.id);
        }
        let ctx = egui::Context::default();
        super::style(&ctx);
        let mut frame = eframe::Frame::_new_kittest();
        let size = egui::vec2(1240.0, 840.0);
        let mut t = 0.0_f64;
        let mut drive = |app: &mut App, t: &mut f64| {
            let raw = egui::RawInput {
                time: Some(*t),
                screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                ..Default::default()
            };
            let start = Instant::now();
            let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
            let ms = start.elapsed().as_secs_f32() * 1000.0;
            out.drop_without_applying_deltas();
            *t += 1.0 / 60.0;
            ms
        };
        for (view, screen) in [
            (View::Dupes, "duplicates"),
            (View::Faces, "faces"),
            (View::Trash, "trash"),
            (View::Integrity, "integrity"),
        ] {
            app.view = view;
            // Both palettes (M22.11): the light theme must cost what the dark one costs, and on the
            // real archive no word, fill or line may fall off the palette of the theme in use.
            for (mode, name, theme) in [
                (db::ThemeMode::Dark, "dark", super::DARK),
                (db::ThemeMode::Light, "light", super::LIGHT),
            ] {
                app.settings.theme = mode;
                let mut times = Vec::new();
                for _ in 0..40 {
                    times.push(drive(&mut app, &mut t));
                }
                times.sort_by(|a, b| a.total_cmp(b));
                let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
                println!(
                    "{screen} ({name}): {} files, {} groups, {faces} faces; p50 {:.2} ms ({:.0} fps), p95 {:.2} ms",
                    n,
                    app.groups.len(),
                    p(0.50),
                    1000.0 / p(0.50),
                    p(0.95)
                );
                assert_palette(&mut app, &ctx, screen, theme);
            }
        }
    }

    /// M21.4f: the left panel is translated completely — the model block, the text block and the summary.
    /// The model titles, the tags and the paths are data and stay as they are.
    #[test]
    fn english_translates_the_left_panel() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-left-i18n.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        // An empty models folder: the block says "not downloaded" in both languages.
        app.settings.models_dir =
            std::env::temp_dir().join("photocat-test-models-never-downloaded");
        app.settings.lang = i18n::Lang::En;
        // Two frames: the height of the bottom summary panel comes from the previous frame, so on
        // the first one it is cropped and the checked lines appear on the second.
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 1100.0);
        window_frame_at(&mut app, &ctx, Vec::new(), size);
        let texts = window_frame_at(&mut app, &ctx, Vec::new(), size);
        for want in [
            "AI model",
            "model not downloaded",
            "Model settings",
            "Text on the photo",
            "0 candidates, 0 left",
            "Summary",
            "Files:",
            "Size:",
            // The two missing numbers are the counts of the filter chips (M25.4), not summary rows.
            "No date · 0",
            "No preview · 1",
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "missing “{want}”: {texts:?}"
            );
        }
        for ru in [
            "Модель ИИ",
            "модель не скачана",
            "Настройки модели",
            "Текст на фото",
            "Сводка",
            "Файлов:",
            "Объём:",
            "Без даты ·",
        ] {
            assert!(
                !texts.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left: {texts:?}"
            );
        }
    }

    /// M21.4f: the sections of the "Settings" dialog are translated completely: the model, the
    /// indexing, the tags with the dictionary, the faces, the storage and the external programs.
    #[test]
    fn english_translates_the_settings_panes() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-settings-i18n.db"));
        app.settings.models_dir =
            std::env::temp_dir().join("photocat-test-models-never-downloaded");
        app.settings.lang = i18n::Lang::En;
        app.refresh_tool_states();
        let ctx = egui::Context::default();
        let model = settings_frame(&mut app, &ctx, SettingsTab::Model);
        for want in [
            "Model",
            "Model state",
            "Not downloaded",
            "Check the model",
            "Model folder",
            "Apply the path",
            "Download limit",
            "Network consent",
            "Download the model",
            "Compute tags",
        ] {
            assert!(
                model.iter().any(|t| t == want),
                "missing “{want}” in the model section: {model:?}"
            );
        }
        assert!(
            model.iter().any(|t| t.contains("dims")),
            "the size units are not translated: {model:?}"
        );
        let index = settings_frame(&mut app, &ctx, SettingsTab::Index);
        for want in ["Indexing threads", "Duplicate threshold", "bits out of 64"] {
            assert!(
                index.iter().any(|t| t == want),
                "missing “{want}” in the indexing section: {index:?}"
            );
        }
        // An empty dictionary: otherwise the row of buttons under 14 labels goes beyond the tall frame.
        app.tag_defs.clear();
        let tags = settings_frame(&mut app, &ctx, SettingsTab::Tags);
        for want in [
            "Tag filter threshold",
            "Confident tag threshold (cosine)",
            "Margin over the second tag",
            "Tag dictionary",
            "the dictionary is not seeded yet — it appears after the first “Compute tags”",
            "Recompute tags",
            "Add to the dictionary",
            "Export JSON",
            "Import JSON",
            "Restore the defaults",
        ] {
            assert!(
                tags.iter().any(|t| t == want),
                "missing “{want}” in the tags section: {tags:?}"
            );
        }
        let faces = settings_frame(&mut app, &ctx, SettingsTab::Faces);
        for want in [
            "Face merge threshold (cosine)",
            "Download the face model",
            "Check the face model",
        ] {
            assert!(
                faces.iter().any(|t| t == want),
                "missing “{want}” in the faces section: {faces:?}"
            );
        }
        assert!(
            faces.iter().any(|t| t.contains("not downloaded")),
            "the face model state is not translated: {faces:?}"
        );
        let storage = settings_frame(&mut app, &ctx, SettingsTab::Storage);
        for want in [
            "Index",
            "DB",
            "Cache",
            "Models",
            "People",
            "Compute the used space",
            "Cleaning the cache and the index",
            "Clear the previews…",
        ] {
            assert!(
                storage.iter().any(|t| t == want),
                "missing “{want}” in the storage section: {storage:?}"
            );
        }
        let tools = settings_frame(&mut app, &ctx, SettingsTab::Tools);
        assert!(
            tools.iter().any(|t| t == "Check and save the programs"),
            "the save-programs button is missing: {tools:?}"
        );
        assert!(
            tools.iter().any(|t| t.starts_with("without it: ")),
            "the “without it” label is not translated: {tools:?}"
        );
        assert!(
            tools
                .iter()
                .any(|t| t.contains("videos without duration or resolution")),
            "the ffprobe description is not translated: {tools:?}"
        );
        for (name, texts) in [
            ("model", &model),
            ("index", &index),
            ("tags", &tags),
            ("faces", &faces),
            ("storage", &storage),
            ("tools", &tools),
        ] {
            for ru in [
                "Модель",
                "Состояние модели",
                "Порог дублей",
                "Словарь тегов",
                "Хранилище",
            ] {
                assert!(
                    !texts.iter().any(|t| t == ru),
                    "the Russian label “{ru}” is left in the {name} section: {texts:?}"
                );
            }
        }
    }

    /// M21.5: the full screen and the player controls are translated completely: the heading,
    /// paging, the slideshow, the scale, the text on a photo and the caption over a video frame.
    #[test]
    fn english_translates_the_full_screen() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-viewer-i18n.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.open_viewer(1);
        let ctx = egui::Context::default();
        let texts = viewer_frame(&mut app, &ctx, None);
        for want in [
            "Back to the grid",
            "Slideshow",
            "Fit",
            "1:1",
            "Rotate",
            "Inspector",
            "Zoom out",
            "Zoom in",
            "interval:",
            "3 s",
            "Text on the photo",
            "Show faces",
            "In the system viewer",
            "1 of 1",
            "the full frame is loading…",
        ] {
            assert!(
                texts.iter().any(|t| t == want || t.contains(want)),
                "missing “{want}” on the full screen: {texts:?}"
            );
        }
        for ru in [
            "К сетке",
            "Вписать",
            "Уменьшить",
            "Повернуть",
            "Инспектор",
            "Слайд-шоу",
            "интервал:",
            "Текст на фото",
            "Показать лица",
            "В системном просмотрщике",
            "масштаб",
            "полный кадр загружается",
            "из",
        ] {
            assert!(
                !texts.iter().any(|t| t.contains(ru)),
                "the Russian label “{ru}” is left on the full screen: {texts:?}"
            );
        }
        // Video: the caption on the big ▶ button is translated too.
        app.items = vec![file_row(2, "video", "b.mp4")];
        app.open_viewer(2);
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t.contains("press ▶ to play")),
            "the video label is not translated: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("нажмите ▶")),
            "a Russian label is left on the video: {texts:?}"
        );
    }

    /// M21.5: the archive folders and the built-in folder picker are translated together:
    /// opening a folder without translating the parent dialog is a half translation.
    #[test]
    fn english_translates_the_folders_and_the_folder_picker() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-picker-i18n.db"));
        app.settings.lang = i18n::Lang::En;
        let ctx = egui::Context::default();
        app.roots_open = true;
        let roots = modal_frame(&mut app, &ctx, App::roots_modal);
        for want in [
            "Archive folders",
            "No folders yet — add one with the button below.",
            "Add a folder…",
            "Index all",
            "Close",
        ] {
            assert!(
                roots.iter().any(|t| t == want),
                "missing “{want}” in the folder list: {roots:?}"
            );
        }
        for ru in ["Каталоги архива", "Добавить каталог", "Индексировать все", "Закрыть"] {
            assert!(
                !roots.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left in the folders: {roots:?}"
            );
        }
        // An empty folder: a hint is visible instead of the list of subfolders.
        let empty = std::env::temp_dir().join("photocat-test-picker-empty");
        std::fs::create_dir_all(&empty).expect("a temporary folder");
        app.roots_open = false;
        app.picker_open = true;
        app.picker_current = empty;
        app.refresh_picker();
        let picker = modal_frame(&mut app, &ctx, App::picker_modal);
        for want in [
            "Add a folder",
            "Up",
            "Add and index",
            "Add without indexing",
            "Cancel",
            "No subfolders.",
        ] {
            assert!(
                picker.iter().any(|t| t == want),
                "missing “{want}” in the folder picker: {picker:?}"
            );
        }
        for ru in ["Добавить каталог", "Вверх", "Отмена", "Подкаталогов нет."] {
            assert!(
                !picker.iter().any(|t| t == ru),
                "the Russian label “{ru}” is left in the folder picker: {picker:?}"
            );
        }
        app.picker_open = false;
    }

    /// M21.5: the confirmation of cleaning the cache and the index is translated — all three variants.
    #[test]
    fn english_translates_the_emptying_confirmation() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-storage-i18n.db"));
        app.settings.lang = i18n::Lang::En;
        let ctx = egui::Context::default();
        for (action, title, button) in [
            (StorageAction::Thumbs, "Clear the previews?", "Clear the previews"),
            (StorageAction::Faces, "Clear the face crops?", "Clear the faces"),
            (StorageAction::Index, "Reset the index?", "Reset the index"),
        ] {
            app.storage_confirm = Some(action);
            let texts = modal_frame(&mut app, &ctx, App::storage_modal);
            for want in [title, button, "Cancel"] {
                assert!(
                    texts.iter().any(|t| t == want),
                    "missing “{want}” in the cleanup confirmation: {texts:?}"
                );
            }
            for ru in ["Очистить превью?", "Очистить кропы лиц?", "Сбросить индекс?", "Отмена"] {
                assert!(
                    !texts.iter().any(|t| t == ru),
                    "the Russian label “{ru}” is left in the confirmation: {texts:?}"
                );
            }
            // The frame is read only: the confirmation is cleared, nothing is deleted.
            app.storage_confirm = None;
        }
    }

    #[test]
    fn paging_wraps_around_and_resets_the_slideshow_timer() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        let mut v = viewer_with(vec![7, 8, 9], 0);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 1);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 2);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 0, "the last file gives way to the first, as in a slideshow");
        app.viewer_step(&mut v, -1);
        assert_eq!(v.idx, 2);
        assert!(v.last_advance.elapsed() < std::time::Duration::from_millis(100));
    }

    /// Changing the file stops the player: without it `video_id` stays from the old frame and
    /// `poll_video` keeps drawing it over the new photo.
    #[test]
    fn changing_the_file_stops_the_video() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.video_id = Some(7);
        let mut v = viewer_with(vec![7, 8], 0);
        app.viewer_step(&mut v, 1);
        assert_eq!(app.video_id, None);
        assert_eq!(v.idx, 1);
    }

    /// The owner: "paging back and forth shows nothing". A video must be shown with its cover
    /// frame and a ▶ button, and the arrow must open the next file.
    #[test]
    fn the_video_view_draws_the_button_and_pages_with_arrows() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![
            file_row(1, "image", "a.jpg"),
            file_row(2, "video", "b.mp4"),
            file_row(3, "image", "c.jpg"),
        ];
        // The behaviour is checked, not the language: the captions on the frame are Russian.
        app.settings.lang = i18n::Lang::Ru;
        app.open_viewer(2);
        let ctx = egui::Context::default();
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t.contains("нажмите ▶")),
            "a big ▶ button must sit on the video frame: {texts:?}"
        );
        let texts = viewer_frame(&mut app, &ctx, Some(egui::Key::ArrowRight));
        assert_eq!(app.viewer.as_ref().expect("the viewer is open").idx, 2);
        assert!(
            texts.iter().any(|t| t == "c.jpg"),
            "the title must show the next file: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("нажмите ▶")),
            "the photo has no ▶ button: {texts:?}"
        );
    }

    /// The previous photo is held on the screen while the next frame loads. A video has no
    /// "next frame" — otherwise after paging an old photo stayed over the video and the owner
    /// saw "nothing is shown".
    #[test]
    fn switching_to_a_video_keeps_no_previous_photo() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![file_row(1, "image", "a.jpg"), file_row(2, "video", "b.mp4")];
        app.settings.lang = i18n::Lang::Ru;
        app.open_viewer(1);
        let ctx = egui::Context::default();
        let prev = ctx.load_texture(
            "prev",
            egui::ColorImage::from_rgba_unmultiplied([2, 2], &[200u8; 16]),
            egui::TextureOptions::LINEAR,
        );
        app.viewer_tex = Some((1, prev));
        let texts = viewer_frame(&mut app, &ctx, Some(egui::Key::ArrowRight));
        assert!(
            texts.iter().any(|t| t == "b.mp4"),
            "the next file must open: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("загрузка следующего кадра")),
            "the previous photo's frame must not stay on the video: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("нажмите ▶")),
            "the video must hold a ▶ button: {texts:?}"
        );
    }

    #[test]
    fn paging_a_single_file_does_nothing() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        let mut v = viewer_with(vec![7], 0);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 0);
    }

    /// M13d: at a scale above 100% the frame is dragged with the mouse, and at "fit" the offset
    /// is reset — otherwise a new photo would open cropped.
    #[test]
    fn changing_the_file_centres_the_frame() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        let mut v = viewer_with(vec![7, 8], 0);
        v.pan = egui::vec2(40.0, -10.0);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.pan, egui::Vec2::ZERO);
    }

    /// The offset does not tear the frame away from the edges of the area; the side that is
    /// fitted stands in the centre.
    #[test]
    fn panning_keeps_the_frame_off_the_edges() {
        let area = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(400.0, 300.0));
        // A fitted frame: there is nothing to drag.
        assert_eq!(
            clamp_pan(egui::vec2(50.0, -20.0), egui::vec2(400.0, 300.0), area),
            egui::Vec2::ZERO
        );
        // Twice the area: 200 px to each side and 150 px up and down.
        assert_eq!(
            clamp_pan(egui::vec2(999.0, -999.0), egui::vec2(800.0, 600.0), area),
            egui::vec2(200.0, -150.0)
        );
        // Wider than the area but lower than it: it moves horizontally only.
        assert_eq!(
            clamp_pan(egui::vec2(0.0, 80.0), egui::vec2(1000.0, 200.0), area),
            egui::Vec2::ZERO
        );
    }

    /// The drag hint is visible only when there is something to drag: at "fit" it would be a
    /// promise the code does not keep.
    #[test]
    fn the_pan_hint_shows_only_when_zoomed() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.settings.lang = i18n::Lang::Ru;
        app.open_viewer(1);
        let ctx = egui::Context::default();
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            !texts.iter().any(|t| t.contains("перетаскивание")),
            "at 100 % zoom there is no panning hint: {texts:?}"
        );
        app.viewer.as_mut().expect("the viewer is open").zoom = 2.0;
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t.contains("перетаскивание — сдвиг кадра")),
            "at 200 % zoom a panning hint is needed: {texts:?}"
        );
    }

    fn bounds() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(150.0, 150.0))
    }

    /// M22.7: the series is the day of the current file — the neighbours in viewer order with
    /// their position inside the day. A file of another date is not in it, and a file without
    /// a date has no series at all.
    #[test]
    fn the_series_is_the_day_of_the_current_file() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-series.db"));
        let mut other = file_row(2, "image", "b.jpg");
        other.taken_at = Some("2024-05-02T09:00".into());
        let mut late = file_row(3, "image", "c.jpg");
        late.taken_at = Some("2024-05-01T23:00".into());
        app.items = vec![file_row(1, "image", "a.jpg"), other, late];
        let v = viewer_with(vec![1, 2, 3], 0);
        let (day, series) = app.viewer_series(&v);
        assert_eq!(day.as_deref(), Some("2024-05-01"));
        assert_eq!(
            series.iter().map(|s| s.idx).collect::<Vec<_>>(),
            vec![0, 2],
            "only the two snapshots of that day"
        );
        assert_eq!(series.iter().map(|s| s.pos).collect::<Vec<_>>(), vec![1, 2]);

        app.items[0].taken_at = None;
        let (day, series) = app.viewer_series(&viewer_with(vec![1, 2], 0));
        assert!(day.is_none(), "a file without a date has no series");
        assert!(series.is_empty());
    }

    /// M22.7: a turn by 90° swaps the sides of the frame, and the box of a face turns with it.
    #[test]
    fn a_turn_by_ninety_degrees_swaps_the_sides_of_the_frame() {
        let portrait = egui::vec2(240.0, 320.0);
        assert_eq!(rotated_size(egui::vec2(320.0, 240.0), 0), egui::vec2(320.0, 240.0));
        assert_eq!(rotated_size(egui::vec2(320.0, 240.0), 1), portrait);
        assert_eq!(rotated_size(egui::vec2(320.0, 240.0), 2), egui::vec2(320.0, 240.0));
        assert_eq!(rotated_size(egui::vec2(320.0, 240.0), 3), portrait);

        let center = egui::pos2(100.0, 100.0);
        let r = egui::Rect::from_min_size(egui::pos2(80.0, 90.0), egui::vec2(20.0, 10.0));
        assert_eq!(rotate_rect(r, center, 0), r);
        let turned = rotate_rect(r, center, 1);
        assert_eq!(turned, egui::Rect::from_min_size(egui::pos2(100.0, 80.0), egui::vec2(10.0, 20.0)));
        assert_eq!(rotate_rect(r, center, 2), egui::Rect::from_min_size(egui::pos2(100.0, 100.0), egui::vec2(20.0, 10.0)));
    }

    /// M22.7: "1:1" is the scale where one image pixel takes one screen pixel, not "100 %".
    #[test]
    fn one_to_one_shows_the_frame_as_it_is_stored() {
        let area = egui::vec2(896.0, 620.0);
        // A 1920×1080 frame is fitted at 0.4667 and needs 2.14× to be seen pixel for pixel.
        let z = one_to_one_zoom(egui::vec2(1920.0, 1080.0), area);
        assert!((z - 2.1429).abs() < 0.001, "got {z}");
        // A small frame is not blown up: the fit is 100 % already, the floor keeps it sane.
        assert_eq!(one_to_one_zoom(egui::vec2(320.0, 240.0), area), 0.5);
        // A huge frame is capped by MAX_ZOOM rather than by an endless scale.
        assert_eq!(one_to_one_zoom(egui::vec2(20000.0, 15000.0), area), MAX_ZOOM);
    }

    /// M22.7: a turn is about one file; the next one is shown the way it is stored.
    #[test]
    fn turning_the_view_does_not_follow_the_next_file() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-turn.db"));
        let mut v = viewer_with(vec![7, 8], 0);
        v.rotation = 1;
        app.viewer_step(&mut v, 1);
        assert_eq!(v.rotation, 0);
    }

    /// M22.7: the full screen shows the series of the day under the frame.
    #[test]
    fn the_full_screen_draws_the_series_of_the_day() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-series-frame.db"));
        app.items = vec![file_row(1, "image", "a.jpg"), file_row(2, "image", "b.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.open_viewer(1);
        let ctx = egui::Context::default();
        let positions = viewer_frame_positions(&mut app, &ctx);
        let texts: Vec<String> = positions.iter().map(|(t, _)| t.clone()).collect();
        assert!(
            texts.iter().any(|t| t == "SERIES"),
            "the strip has a label: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "2024-05-01"),
            "the strip names the day: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "· 2 files"),
            "the strip counts the snapshots of the day: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "1") && texts.iter().any(|t| t == "2"),
            "every neighbour carries its position in the day: {texts:?}"
        );
        // Neither the tools row nor the strip is cut off by the modal: the window is 840 px, the
        // modal keeps a 24 px margin, a row of buttons is 26 px and a 64×48 thumbnail ends 14 px
        // below the top of its number.
        let bottom = positions
            .iter()
            .filter(|(t, _)| t == "1" || t == "2")
            .map(|(_, p)| p.y)
            .fold(0.0_f32, f32::max);
        assert!(
            bottom + 14.0 <= 816.0,
            "the series strip must fit the modal: the number at {bottom} with a 48 px thumbnail"
        );
        let tools = positions
            .iter()
            .find(|(t, _)| t == "Zoom out")
            .map(|(_, p)| p.y)
            .expect("the tools row is drawn");
        assert!(
            tools + 20.0 <= positions
                .iter()
                .find(|(t, _)| t == "SERIES")
                .map(|(_, p)| p.y)
                .expect("the strip is below the frame"),
            "the tools row comes before the strip"
        );
        // A narrow window wraps the tools row instead of clipping it, and the body follows: the
        // strip still ends above the bottom edge of the modal.
        let narrow = viewer_frame_positions_at(&mut app, &ctx, egui::vec2(760.0, 620.0));
        let bottom = narrow
            .iter()
            .filter(|(t, _)| t == "1" || t == "2")
            .map(|(_, p)| p.y)
            .fold(0.0_f32, f32::max);
        assert!(
            bottom > 0.0 && bottom + 14.0 <= 596.0,
            "at 760×620 the strip must fit the modal: the number at {bottom}"
        );
    }

    /// M22.7, M22.28: the boxes of the found faces are a tool, off by default; the "Show faces"
    /// word turns them on, and then they are marked with the name and the score.
    #[test]
    fn the_full_screen_hides_the_face_boxes_until_asked() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-viewer-faces.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.viewer_faces = Some((
            1,
            vec![db::FaceBox {
                id: 10,
                x: 10,
                y: 20,
                w: 30,
                h: 40,
                score: 0.92,
                cluster_id: Some(7),
                label: "Lisa".into(),
            }],
        ));
        app.open_viewer(1);
        let ctx = egui::Context::default();
        let tex = ctx.load_texture(
            "frame",
            egui::ColorImage::from_rgba_unmultiplied([4, 3], &[200u8; 48]),
            egui::TextureOptions::LINEAR,
        );
        app.viewer_tex = Some((1, tex));
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            !texts.iter().any(|t| t.contains("(92%)")),
            "the boxes are hidden by default: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Show faces"),
            "the tool is on the row: {texts:?}"
        );
        // The word turns the boxes on: the face is named over the frame.
        app.viewer.as_mut().expect("the viewer is open").show_faces = true;
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t == "Lisa (92%)"),
            "the face is named over the frame: {texts:?}"
        );
        // Without faces nothing is marked: an empty list draws no chips.
        app.viewer_faces = Some((1, Vec::new()));
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            !texts.iter().any(|t| t.contains("(92%)")),
            "no faces, no chips: {texts:?}"
        );
        // A turned frame is drawn through the mesh and keeps the chips over it.
        app.viewer_faces = Some((
            1,
            vec![db::FaceBox {
                id: 10,
                x: 10,
                y: 20,
                w: 30,
                h: 40,
                score: 0.92,
                cluster_id: Some(7),
                label: "Lisa".into(),
            }],
        ));
        app.viewer.as_mut().expect("the viewer is open").rotation = 1;
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t == "Lisa (92%)"),
            "the chip turns with the frame: {texts:?}"
        );
    }

    /// M22.28: a click on a face opens the right panel with every photo of that face, oldest
    /// first; an ungrouped face lists only the frame on screen.
    #[test]
    fn a_picked_face_lists_its_photos_in_the_right_panel() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-face-panel.db"));
        app.items = vec![
            file_row(1, "image", "a.jpg"),
            file_row(2, "image", "b.jpg"),
        ];
        app.items[1].taken_at = Some("2024-04-01T10:00".into());
        app.face_views = vec![
            db::FaceView {
                id: 10,
                file_id: 1,
                path: "/tmp/a.jpg".into(),
                taken_at: Some("2024-05-01T10:00".into()),
                thumb: None,
                score: 0.9,
                cluster_id: Some(7),
            },
            db::FaceView {
                id: 11,
                file_id: 2,
                path: "/tmp/b.jpg".into(),
                taken_at: Some("2024-04-01T10:00".into()),
                thumb: None,
                score: 0.8,
                cluster_id: Some(7),
            },
        ];
        app.settings.lang = i18n::Lang::En;
        let face = db::FaceBox {
            id: 10,
            x: 10,
            y: 20,
            w: 30,
            h: 40,
            score: 0.92,
            cluster_id: Some(7),
            label: "Lisa".into(),
        };
        let focus = app.face_focus_for(&face, 1);
        assert_eq!(
            focus.file_ids,
            vec![2, 1],
            "every photo of the group, oldest first"
        );
        app.open_viewer(1);
        let viewer = app.viewer.as_mut().expect("the viewer is open");
        viewer.show_faces = true;
        viewer.face_focus = Some(focus);
        let ctx = egui::Context::default();
        let texts = viewer_frame(&mut app, &ctx, None);
        for needle in ["Lisa", "2 photos", "a.jpg", "b.jpg"] {
            assert!(
                texts.iter().any(|t| t == needle),
                "the panel lists “{needle}”: {texts:?}"
            );
        }
        // A face that is not grouped yet: the panel says so and lists only this frame.
        let lone = db::FaceBox {
            id: 12,
            x: 10,
            y: 20,
            w: 30,
            h: 40,
            score: 0.7,
            cluster_id: None,
            label: String::new(),
        };
        let focus = app.face_focus_for(&lone, 1);
        assert_eq!(focus.file_ids, vec![1]);
        app.viewer.as_mut().expect("the viewer is open").face_focus = Some(focus);
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts
                .iter()
                .any(|t| t == "This face is not in a group yet."),
            "an ungrouped face is explained: {texts:?}"
        );
    }

    /// M22.7: the inspector shows the file card of the frame inside the viewer.
    #[test]
    fn the_inspector_shows_the_card_of_the_frame() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-viewer-card.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.open_viewer(1);
        app.viewer.as_mut().expect("the viewer is open").inspector = true;
        let ctx = egui::Context::default();
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t == "AI TAGS"),
            "the card of the frame is next to it: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "a.jpg"),
            "the header keeps the name: {texts:?}"
        );
    }

    /// The row of an own tag has to fit the 280 px panel: the field takes what the button leaves.
    /// A fixed 150 px field plus the word button were wider than the card, egui widened the panel
    /// past the window edge, the panel was shifted right and the card was clipped on both sides
    /// (M22.12).
    #[test]
    fn the_file_card_fits_the_right_panel() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-card-fit.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        // The own-tag row is the widest row of the card and the one that used to widen the panel
        // (M22.12). Since M22.18 its button is an icon-only "+", so the hint of the field is the
        // widest word of the row; the box of the button is covered by
        // `the_file_card_keeps_the_mock_ups_band_and_air`.
        app.settings.lang = i18n::Lang::Ru;
        app.selected = Some(1);
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let words = window_word_rects_at(&mut app, &ctx, size);
        let panel_left = size.x - 280.0;
        let field = words
            .iter()
            .find(|(w, _)| w == "свой тег, например мебель")
            .unwrap_or_else(|| panic!("the own-tag row is on screen: {words:?}"));
        assert!(
            field.1.left() >= panel_left && field.1.right() <= size.x,
            "the own-tag row stays in the panel: {:?} in [{panel_left}, {}]",
            field.1,
            size.x
        );
        let title = words
            .iter()
            .find(|(w, _)| w == "Сведения о файле")
            .expect("the panel title is on screen");
        // The panel keeps its 280 px: a row wider than the card widens it past the window and
        // shifts the whole card right, so the title leaves its edge. The info icon and the gap
        // before the word take 20 px of the 12 px inner margin.
        assert!(
            title.1.left() >= panel_left && title.1.left() <= panel_left + 40.0,
            "the card is not shifted right: the title at {:?}, the panel at {panel_left}",
            title.1
        );
        // Nothing is painted past the window edge: the frame of the own-tag button used to be
        // 13 px wider than the card and the whole panel was pushed out (M22.12).
        for rect in window_rects_at(&mut app, &ctx, size) {
            assert!(
                rect.left() >= -0.5 && rect.right() <= size.x + 0.5,
                "a frame leaves the window: {rect:?} in {size:?}"
            );
        }
    }

    /// M22.25: a long file name is cut to the room the size leaves. It used to widen the card and
    /// with it the whole right panel (306 px instead of 280), the panel ran past the window and the
    /// grid changed its tile width as soon as a photo was selected.
    #[test]
    fn a_long_file_name_does_not_widen_the_right_panel() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-long-name.db"));
        let long = "66248-chelovek_logotip_chernyj_fon_2026-10-16_18-38-42_very_long_name.jpg";
        app.items = vec![file_row(1, "image", long)];
        app.settings.lang = i18n::Lang::Ru;
        app.selected = None;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let central = |rects: &[egui::Rect]| {
            let mut widths: Vec<f32> = rects
                .iter()
                .filter(|r| r.left() >= super::LEFT_W && r.right() <= size.x - super::RIGHT_W)
                .filter(|r| r.width() > 100.0 && r.width() < 300.0 && r.height() > 100.0)
                .map(|r| r.width())
                .collect();
            widths.sort_by(|a, b| a.total_cmp(b));
            widths.dedup_by(|a, b| (*a - *b).abs() < 0.5);
            widths
        };
        let before = window_rects_at(&mut app, &ctx, size);
        app.selected = Some(1);
        let after = window_rects_at(&mut app, &ctx, size);
        assert!(
            after
                .iter()
                .any(|r| (r.width() - super::RIGHT_W).abs() <= 1.5
                    && (r.right() - size.x).abs() <= 1.5),
            "the right panel keeps its 280 px at the window edge: {:?}",
            after
                .iter()
                .filter(|r| r.right() > size.x - 40.0)
                .collect::<Vec<_>>()
        );
        for rect in &after {
            assert!(
                rect.left() >= -0.5 && rect.right() <= size.x + 0.5,
                "a frame leaves the window: {rect:?} in {size:?}"
            );
        }
        let panel_left = size.x - super::RIGHT_W;
        let words = window_word_rects_at(&mut app, &ctx, size);
        for (word, r) in &words {
            assert!(
                r.right() <= size.x + 0.5,
                "the word “{word}” leaves the window: {r:?}"
            );
        }
        assert!(
            !words
                .iter()
                .any(|(w, r)| w == long && r.left() >= panel_left),
            "the long name in the panel is cut, not printed whole: {words:?}"
        );
        assert_eq!(
            central(&before),
            central(&after),
            "selecting a photo must not change the tile width"
        );
    }

    /// M22.18: the own-tag row keeps a wide field and a bare "+" — a word button took 140 px of
    /// the 255 px card and left the field short.
    #[test]
    fn the_own_tag_row_keeps_a_wide_field_and_a_bare_plus() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-add-tag.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.selected = Some(1);
        // The Russian hint of the field is the widest text of the row.
        app.settings.lang = i18n::Lang::Ru;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let words = window_word_rects_at(&mut app, &ctx, size);
        assert!(
            !words.iter().any(|(w, _)| w == "Добавить тег"),
            "the button keeps only the +: {words:?}"
        );
        let row = words
            .iter()
            .find(|(w, _)| w == "СВОИ ТЕГИ")
            .expect("the own-tag block is on screen")
            .1;
        let hint = words
            .iter()
            .find(|(w, _)| w == "свой тег, например мебель")
            .expect("the field of the own tag is on screen")
            .1;
        let plus = window_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|r| {
                (r.width() - 22.0).abs() < 0.5
                    && (r.height() - 22.0).abs() < 0.5
                    && r.center().y > row.bottom()
                    && r.left() > size.x - RIGHT_W
            })
            .expect("the + of the own-tag row is drawn");
        assert!(
            plus.left() - hint.left() >= 150.0,
            "the field stays wide: the hint at {hint:?}, the + at {plus:?}"
        );
        assert!(plus.right() <= size.x, "the + stays in the window: {plus:?}");
        // The row is the mock-up's 28 px, so the field is neither tight nor loose.
        assert!(
            window_rects_at(&mut app, &ctx, size).iter().any(|r| {
                (r.height() - 28.0).abs() < 0.5 && r.width() > 200.0 && r.contains(plus.center())
            }),
            "the own-tag row is 28 px high: {plus:?}"
        );
        // M22.18: the text edit used to be a 26 px button-like box with its text at the top, so
        // the hint stuck to the upper edge of the row and the field looked short. The "#", the tag
        // and the "+" share one line now.
        let hash = words
            .iter()
            .find(|(w, _)| w == "#")
            .expect("the # of the own-tag row is on screen")
            .1;
        for (name, r) in [("#", hash), ("the hint", hint)] {
            assert!(
                (r.center().y - plus.center().y).abs() <= 1.0,
                "the {name} sits on the line of the +: {r:?} against {plus:?}"
            );
        }
    }

    /// A press and a release at one point: egui registers a click on the release, so the two
    /// frames are needed (M22.17).
    fn click_at(app: &mut App, ctx: &egui::Context, at: egui::Pos2) {
        for pressed in [true, false] {
            window_frame_with(
                app,
                ctx,
                vec![
                    egui::Event::PointerMoved(at),
                    egui::Event::PointerButton {
                        pos: at,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ],
            );
        }
    }

    /// M22.17: the word of a card chip acts on the tag, the "×" on its right edge removes it. One
    /// click must never hide a tag that the owner only wanted to look at.
    #[test]
    fn the_word_of_a_card_chip_acts_and_the_cross_removes() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let size = egui::vec2(300.0, 120.0);
        let raw = |events: Vec<egui::Event>| egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
            events,
            ..Default::default()
        };
        // The box of the chip is known after a frame: it is the only 20 px-high rectangle here.
        let first = ctx.run_ui(raw(Vec::new()), |ui| {
            let _ = super::card_chip(ui, "beach 40%", None, false, "body", "remove");
        });
        let mut rects = Vec::new();
        for c in &first.shapes {
            collect_rect_bounds(&c.shape, &mut rects);
        }
        first.drop_without_applying_deltas();
        let chip = *rects
            .iter()
            .find(|r| (r.height() - 20.0).abs() < 0.5)
            .expect("the chip is drawn");
        let hit = |at: egui::Pos2| {
            let mut got = super::ChipClick::None;
            for pressed in [true, false] {
                let out = ctx.run_ui(
                    raw(vec![
                        egui::Event::PointerMoved(at),
                        egui::Event::PointerButton {
                            pos: at,
                            button: egui::PointerButton::Primary,
                            pressed,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ]),
                    |ui| {
                        got = super::card_chip(ui, "beach 40%", None, false, "body", "remove");
                    },
                );
                out.drop_without_applying_deltas();
            }
            got
        };
        assert_eq!(
            hit(egui::pos2(chip.left() + 8.0, chip.center().y)),
            super::ChipClick::Body,
            "the word filters the photos: {chip:?}"
        );
        assert_eq!(
            hit(egui::pos2(chip.right() - 8.0, chip.center().y)),
            super::ChipClick::Remove,
            "the × removes the tag: {chip:?}"
        );
    }

    /// M22.17: a click on a tag in the card is a filter by that tag, not a removal; the accent
    /// marks the tag the photos are filtered by, and a second click clears the filter.
    #[test]
    fn a_click_on_a_tag_in_the_card_filters_the_photos() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-tag-click.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.selected = Some(1);
        app.tags.insert(
            1,
            vec![("beach".to_string(), 0.40), ("sea".to_string(), 0.30)],
        );
        app.tag_dists = embed::tag_distributions(&app.tags);
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame(&mut app, &ctx);
        let chip = |app: &mut App| {
            window_word_rects_at(app, &ctx, size)
                .into_iter()
                .find(|(w, _)| w == "beach 40%")
                .unwrap_or_else(|| panic!("the tag chip is on screen"))
                .1
        };
        // The accent marks the filter and nothing else: no chip is highlighted before a click,
        // even though "beach" is the confident tag of the photo.
        let accented_chip = |app: &mut App| {
            window_fills(app, &ctx).into_iter().any(|(r, c)| {
                (r.height() - 20.0).abs() < 0.5
                    && r.left() > size.x - RIGHT_W
                    && c == super::DARK.accent_soft
            })
        };
        assert!(!accented_chip(&mut app), "no chip is accented before a click");
        let r = chip(&mut app);
        click_at(&mut app, &ctx, egui::pos2(r.left() + 4.0, r.center().y));
        assert_eq!(
            app.tag_filters,
            vec!["beach".to_string()],
            "a click on the tag filters the photos by it"
        );
        assert_eq!(app.tags.get(&1).map(Vec::len), Some(2), "nothing is removed");
        assert!(
            accented_chip(&mut app),
            "the chip of the active filter is in the accent"
        );
        // A second tag joins the filter instead of replacing it (M25.1).
        let r = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "sea 30%")
            .unwrap_or_else(|| panic!("the second tag chip is on screen"))
            .1;
        click_at(&mut app, &ctx, egui::pos2(r.left() + 4.0, r.center().y));
        assert_eq!(
            app.tag_filters,
            vec!["beach".to_string(), "sea".to_string()],
            "the second tag is added to the filter, not swapped in"
        );
        // A second click on one chip removes only that tag.
        let r = chip(&mut app);
        click_at(&mut app, &ctx, egui::pos2(r.left() + 4.0, r.center().y));
        assert_eq!(
            app.tag_filters,
            vec!["sea".to_string()],
            "a second click removes exactly its own tag"
        );
        let r = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "sea 30%")
            .unwrap_or_else(|| panic!("the second tag chip is on screen"))
            .1;
        click_at(&mut app, &ctx, egui::pos2(r.left() + 4.0, r.center().y));
        assert!(app.tag_filters.is_empty(), "the last click clears the set");
        assert!(!accented_chip(&mut app), "no chip is accented after the reset");
    }

    /// M22.17: the same split on an album chip — the word opens the album, only the "×" removes
    /// the photo from it.
    #[test]
    fn the_word_of_an_album_chip_opens_the_album_and_the_cross_removes_the_row() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-album-chip.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.settings.lang = i18n::Lang::En;
        app.selected = Some(1);
        // The album rows are taken as they are: no DB read on this path.
        app.file_albums_for = Some(1);
        app.file_albums = vec![(7, "/tmp/Album".into())];
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame(&mut app, &ctx);
        let r = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "/tmp/Album")
            .expect("the album chip is on screen")
            .1;
        click_at(&mut app, &ctx, egui::pos2(r.left() + 4.0, r.center().y));
        assert!(app.view == View::Albums, "the word opens the album");
        assert_eq!(app.album_selected, Some(7));
        // The "×" removes the album row of the photo and does not navigate anywhere.
        app.view = View::Grid;
        let _ = window_frame(&mut app, &ctx);
        let r = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "/tmp/Album")
            .expect("the album chip is on screen")
            .1;
        click_at(&mut app, &ctx, egui::pos2(r.right() + 8.0, r.center().y));
        assert!(app.view == View::Grid, "the × does not open the album");
        assert_eq!(
            app.file_albums_for, None,
            "the row was removed: the list of the file is reloaded"
        );
    }

    /// M22.16: the right panel follows the composition of `photocat_12` — a 44 px header band on
    /// the panel's whole width with its close target at the right, and the file card on 12 px of
    /// air, so the preview and the blocks never touch the panel's lines.
    #[test]
    fn the_file_card_keeps_the_mock_ups_band_and_air() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-card-air.db"));
        app.items = vec![file_row(1, "image", "a.jpg")];
        app.selected = Some(1);
        app.settings.lang = i18n::Lang::En;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let panel_left = size.x - RIGHT_W;
        let rects = window_rects_at(&mut app, &ctx, size);
        let band = rects
            .iter()
            .find(|r| {
                (r.height() - PANEL_HEAD_H).abs() < 0.5
                    && (r.width() - RIGHT_W).abs() <= 1.5
                    && r.left() >= panel_left - 1.5
            })
            .unwrap_or_else(|| panic!("the header band is drawn: {rects:?}"));
        let title = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "File details")
            .expect("the panel title is on screen")
            .1;
        assert!(
            title.top() >= band.top() && title.bottom() <= band.bottom(),
            "the title stands inside the band: {title:?} in {band:?}"
        );
        // The mock-up's "×" sits 12 px from the panel's right edge, inside the band.
        assert!(
            rects.iter().any(|r| (r.width() - 22.0).abs() < 0.5
                && (r.height() - 22.0).abs() < 0.5
                && (r.right() - (size.x - PANEL_PAD)).abs() < 0.5
                && r.center().y > band.top()
                && r.center().y < band.bottom()),
            "the close target is at the right of the band: {rects:?}"
        );
        // Below the band every block keeps the 12 px air of the card: the preview, the shooting
        // params, the chips and the own-tag field all start on the same column.
        let card: Vec<&egui::Rect> = rects
            .iter()
            // The scroll area paints its ground over the whole panel: the card is what is
            // narrower than the panel minus its two margins. The 1 px scrollbar of the panel is
            // not a card either — the card is taller than the panel since the "Albums" block is
            // always here (M22.20), so the bar appears.
            .filter(|r| {
                r.top() >= band.bottom()
                    && r.left() >= panel_left - 1.5
                    && r.width() <= RIGHT_W - 2.0 * PANEL_PAD + 0.5
                    && r.width() > 8.0
            })
            .collect();
        let left = card
            .iter()
            .map(|r| r.left())
            .fold(f32::INFINITY, f32::min);
        let right = card.iter().map(|r| r.right()).fold(0.0, f32::max);
        assert!(
            (left - (band.left() + PANEL_PAD)).abs() < 1.0,
            "the card starts 12 px inside the panel: {left} against {}",
            band.left() + PANEL_PAD
        );
        assert!(
            (right - (band.right() - PANEL_PAD)).abs() < 1.0,
            "the card ends 12 px before the edge: {right} against {}",
            band.right() - PANEL_PAD
        );
    }

    #[test]
    fn a_portrait_fits_by_height_and_centre() {
        let r = fit_rect(bounds(), egui::vec2(320.0, 640.0));
        assert!((r.height() - 150.0).abs() < 0.01);
        assert!((r.width() - 75.0).abs() < 0.01);
        assert!((r.center().x - bounds().center().x).abs() < 0.01);
        assert!((r.center().y - bounds().center().y).abs() < 0.01);
    }

    #[test]
    fn a_panorama_fits_by_width() {
        let r = fit_rect(bounds(), egui::vec2(1200.0, 400.0));
        assert!((r.width() - 150.0).abs() < 0.01);
        assert!((r.height() - 50.0).abs() < 0.01);
    }

    #[test]
    fn a_square_fills_the_whole_tile() {
        let r = fit_rect(bounds(), egui::vec2(320.0, 320.0));
        assert!((r.width() - 150.0).abs() < 0.01);
        assert!((r.height() - 150.0).abs() < 0.01);
    }

    #[test]
    fn a_zero_size_does_not_break_the_layout() {
        let r = fit_rect(bounds(), egui::vec2(0.0, 0.0));
        assert_eq!(r, bounds());
    }

    /// M22.15: a tile preview covers the frame — the whole tile, no margin. A portrait is cut at
    /// the top and the bottom, the proportions are kept (the picture is never stretched).
    #[test]
    fn a_portrait_covers_the_tile_and_keeps_its_proportions() {
        let r = cover_rect(bounds(), egui::vec2(320.0, 640.0));
        assert!((r.width() - 150.0).abs() < 0.01, "the frame is full: {r:?}");
        assert!((r.height() - 300.0).abs() < 0.01, "the edges are cut: {r:?}");
        assert!((r.center().x - bounds().center().x).abs() < 0.01);
        assert!((r.center().y - bounds().center().y).abs() < 0.01);
    }

    #[test]
    fn a_panorama_covers_the_tile_by_height() {
        let r = cover_rect(bounds(), egui::vec2(1200.0, 400.0));
        assert!((r.width() - 450.0).abs() < 0.01, "the sides are cut: {r:?}");
        assert!((r.height() - 150.0).abs() < 0.01, "the frame is full: {r:?}");
    }

    #[test]
    fn a_square_covers_the_whole_tile() {
        let r = cover_rect(bounds(), egui::vec2(320.0, 320.0));
        assert!((r.width() - 150.0).abs() < 0.01);
        assert!((r.height() - 150.0).abs() < 0.01);
    }

    #[test]
    fn a_zero_size_does_not_break_the_cover() {
        let r = cover_rect(bounds(), egui::vec2(0.0, 0.0));
        assert_eq!(r, bounds());
    }

    /// M22.15 end to end: a portrait preview is drawn bigger than the 4:3 tile frame, so the frame
    /// shows a cropped photograph with no field margins — the state the owner asked to bring back.
    #[test]
    fn the_tile_draws_the_preview_over_the_whole_frame() {
        let dir = std::env::temp_dir().join(format!(
            "photocat-app-tile-fill-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        image::RgbImage::from_pixel(160, 320, image::Rgb([200, 120, 60]))
            .save(dir.join("p.png"))
            .unwrap();
        let mut f = file_row(1, "image", "p.png");
        f.thumb = Some("p.png".into());
        let mut textures = std::collections::HashMap::new();
        let mut missing = std::collections::HashSet::new();
        let picked = BTreeSet::new();
        let ctx = egui::Context::default();
        let raw = || egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            ..Default::default()
        };
        let mut frame = egui::Rect::NOTHING;
        let out = ctx.run_ui(raw(), |ui| {
            let mut cs = super::Cells {
                bg: egui::Color32::BLACK,
                cell_style: db::CellStyle::Tiles,
                time_only: false,
                textures: &mut textures,
                missing: &mut missing,
                thumbs_dir: &dir,
                selected: None,
                picked: &picked,
                clicked: None,
                open_clicked: None,
                show_keep: false,
                keep_clicked: None,
                trash_mode: false,
                restore_clicked: None,
                tile: 150.0,
                lang: i18n::Lang::En,
                budget: 4,
            };
            super::draw_cell(ui, &ctx, &mut cs, &f, 200.0, 150.0, super::ThumbFit::Filling);
            frame = egui::Rect::from_min_size(ui.min_rect().min, egui::vec2(200.0, 150.0));
        });
        let mut meshes = Vec::new();
        for c in &out.shapes {
            collect_meshes(&c.shape, &mut meshes);
        }
        out.drop_without_applying_deltas();
        let picture = meshes
            .first()
            .unwrap_or_else(|| panic!("the preview is drawn: {meshes:?}"));
        assert!(
            (picture.width() - 200.0).abs() < 1.0,
            "the photograph fills the frame's width: {picture:?} in {frame:?}"
        );
        assert!(
            picture.top() < frame.top() && picture.bottom() > frame.bottom(),
            "the portrait is cut above and below instead of leaving margins: {picture:?} in {frame:?}"
        );
        assert!(
            (picture.height() - 400.0).abs() < 1.0,
            "the proportions are kept: {picture:?}"
        );
    }

    #[test]
    fn months_page_across_year_boundaries() {
        assert_eq!(shift_month(2024, 12, 1), (2025, 1));
        assert_eq!(shift_month(2024, 1, -1), (2023, 12));
        assert_eq!(shift_month(2024, 5, 0), (2024, 5));
    }

    #[test]
    fn the_month_length_respects_a_leap_year() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 5), 31);
    }

    fn months(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn empty_months_are_skipped_when_paging() {
        let m = months(&["2024-01", "2024-05", "2024-09"]);
        assert_eq!(shift_to_file_month(&m, 2024, 1, 1), Some((2024, 5)));
        assert_eq!(shift_to_file_month(&m, 2024, 5, -1), Some((2024, 1)));
        assert_eq!(shift_to_file_month(&m, 2024, 5, 1), Some((2024, 9)));
        assert_eq!(shift_to_file_month(&m, 2024, 9, 1), None);
        assert_eq!(shift_to_file_month(&m, 2024, 1, -1), None);
        assert_eq!(shift_to_file_month(&months(&[]), 2024, 5, 1), None);
    }

    #[test]
    fn today_jumps_to_the_nearest_month_with_files() {
        let m = months(&["2024-01", "2024-07"]);
        assert_eq!(nearest_file_month(&m, 2024, 3), Some((2024, 1)));
        assert_eq!(nearest_file_month(&m, 2024, 6), Some((2024, 7)));
        assert_eq!(nearest_file_month(&months(&[]), 2024, 3), None);
    }

    #[test]
    fn the_date_from_a_string_and_the_monday_of_the_week() {
        let d = parse_date("2024-05-15T12:30:00").unwrap();
        assert_eq!(d.to_string(), "2024-05-15");
        assert_eq!(monday_of(d).to_string(), "2024-05-13");
        assert!(parse_date("no date").is_none());
        assert!(parse_date("2024-13-40").is_none());
    }

    #[test]
    fn the_calendar_week_starts_on_monday() {
        assert_eq!(first_weekday_monday(2024, 1), 0); // 1 January 2024 is a Monday
        assert_eq!(first_weekday_monday(2024, 5), 2); // 1 May 2024 is a Wednesday
    }

    /// The filter threshold is the percentile of its own tag, not a global maximum: at 100%
    /// "furniture" keeps its strongest photo, at 0% all of them.
    #[test]
    fn a_tag_filter_follows_its_own_percentile() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.tags.clear();
        for id in 0..120 {
            let v = id as f32 / 120.0;
            app.tags.insert(
                id,
                vec![("мебель".to_string(), v), ("дети".to_string(), 1.0 - v)],
            );
        }
        app.tag_dists = embed::tag_distributions(&app.tags);
        app.settings.tag_show_pct = 100;
        assert!((app.tag_threshold("мебель") - 119.0 / 120.0).abs() < 1e-6);
        assert!((app.tag_threshold("дети") - 1.0).abs() < 1e-6);
        app.settings.tag_show_pct = 0;
        assert!(app.tag_threshold("мебель").abs() < 1e-6);
        let i = app
            .tag_labels()
            .iter()
            .position(|l| l == "мебель")
            .expect("the fixture tag is in the default dictionary");
        assert_eq!(app.tag_counts()[i], 120);
        // A small folder: the percentile is noisy, the raw threshold applies.
        app.tags.retain(|id, _| *id < 50);
        app.settings.tag_show = 0.22;
        assert!((app.tag_threshold("мебель") - 0.22).abs() < 1e-6);
    }

    /// M24.2: a fresh install detects the language from the locale and stores it once, so the window
    /// and the CLI reports agree; the owner's later choice is never overwritten.
    #[test]
    fn the_first_launch_detects_and_stores_the_language_once() {
        let path = std::env::temp_dir().join(format!(
            "photocat-app-first-lang-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let detected = super::first_lang(&path);
        let stored = db::open(&path)
            .ok()
            .and_then(|c| db::get_setting(&c, "lang"));
        assert_eq!(
            stored.as_deref(),
            Some(detected.code()),
            "the first launch writes the detected language to the DB"
        );
        db::open(&path)
            .map(|c| db::set_setting(&c, "lang", "ru"))
            .unwrap()
            .unwrap();
        assert_eq!(
            super::first_lang(&path),
            i18n::Lang::Ru,
            "the owner's choice is what the next launch shows"
        );
    }

    /// The filter takes the labels from the dictionary but does not hide computed tags that are
    /// no longer in it, and honours the minimum percentile of a tag.
    #[test]
    fn the_filter_takes_labels_from_the_dictionary_and_respects_the_percentile() {
        let path = std::env::temp_dir().join(format!(
            "photocat-app-tags-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let mut app = App::new(path);
        assert!(app.tag_labels().iter().any(|l| l == "beach"), "the default is seeded");
        app.tag_defs.push(db::TagDef {
            model: "m".into(),
            id: "custom-1".into(),
            label: "Шкаф".into(),
            prompts: vec!["фотография шкафа".into()],
            enabled: true,
            position: 99,
            min_percentile: 0,
        });
        assert!(app.tag_labels().iter().any(|l| l == "Шкаф"));
        // An old label outside the dictionary is not lost: the weights are computed already.
        app.tags.insert(1, vec![("старый".to_string(), 0.5)]);
        assert!(app.tag_labels().iter().any(|l| l == "старый"));
        // A disabled tag disappears from the filter.
        if let Some(d) = app.tag_defs.iter_mut().find(|d| d.id == "custom-1") {
            d.enabled = false;
        }
        assert!(!app.tag_labels().iter().any(|l| l == "Шкаф"));
        // The minimum percentile of a tag raises its threshold above the common slider.
        app.tags.clear();
        for id in 0..120 {
            app.tags.insert(id, vec![("beach".to_string(), id as f32 / 120.0)]);
        }
        app.tag_dists = embed::tag_distributions(&app.tags);
        app.settings.tag_show_pct = 0;
        if let Some(d) = app.tag_defs.iter_mut().find(|d| d.label == "beach") {
            d.min_percentile = 90;
        }
        let thr = app.tag_threshold("beach");
        assert!(thr > 0.8, "a 90 % threshold must be high, got {thr}");
        app.tag_defs.retain(|d| d.label != "beach");
        assert!(app.tag_threshold("beach").abs() < 1e-6, "no dictionary — a shared slider");
    }

    #[test]
    fn the_weight_percentile_counts_the_share_of_the_weakest() {
        let d = [0.30, 0.20, 0.10];
        assert_eq!(score_percentile(&d, 0.30), Some(67));
        assert_eq!(score_percentile(&d, 0.20), Some(33));
        assert_eq!(score_percentile(&d, 0.10), Some(0));
        assert_eq!(score_percentile(&d, 0.40), Some(100));
        assert_eq!(score_percentile(&[], 0.2), None);
    }

    #[test]
    fn the_calendar_drills_down_to_a_day() {
        assert_eq!(CalScale::Year.deeper(), Some(CalScale::Month));
        assert_eq!(CalScale::Month.deeper(), Some(CalScale::Week));
        assert_eq!(CalScale::Week.deeper(), Some(CalScale::Day));
        assert_eq!(CalScale::Day.deeper(), None);
    }

    // ---- M22.5: the calendar after `photocat_2` / `photocat_12` -----------------------------
    //
    // A month grid of five weeks is 510 px tall, so the day list under it falls below the fold of an
    // 840 px window: the frames below are 1200 px tall, as if the owner had scrolled — the list is
    // in the same one scroll area as the grid.

    /// Two files on one day of May, one in April and nothing else in 2024.
    fn calendar_app() -> App {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-cal-m22.db"));
        let mut first = file_row(1, "image", "breakfast.jpg");
        first.taken_at = Some("2024-05-01T09:10".into());
        first.size = 2048;
        let mut second = file_row(2, "image", "walk.jpg");
        second.taken_at = Some("2024-05-01T14:23".into());
        second.size = 3072;
        let mut april = file_row(3, "image", "garden.jpg");
        april.taken_at = Some("2024-04-30T18:05".into());
        april.size = 1024;
        app.items = vec![first, second, april];
        app.settings.lang = i18n::Lang::En;
        app.view = View::Calendar;
        app.cal_year = 2024;
        app.cal_month = 5;
        app
    }

    /// M22.5: the year layer is 12 month cards, each with a counter and a size in mono digits, and
    /// a line above them saying what the whole year holds. An empty month is drawn, so the gaps of
    /// the archive are visible instead of hidden.
    #[test]
    fn the_year_layer_draws_twelve_months_with_counts_and_sizes() {
        let mut app = calendar_app();
        app.cal_scale = CalScale::Year;
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t == "12 MONTHS OF 2024"),
            "no year heading: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "3 files in the year · 6.0 KiB"),
            "no facts of the year: {texts:?}"
        );
        for month in ["January", "May", "December"] {
            assert!(
                texts.iter().any(|t| t == month),
                "no card “{month}”: {texts:?}"
            );
        }
        // The counter of May is two, the others are zero; the size of May is the sum of its files.
        // Both are painted as plain numbers, so their presence is what is checked.
        assert!(
            texts.iter().filter(|t| *t == "2").count() >= 1,
            "no counter on the May card: {texts:?}"
        );
        assert!(
            texts.iter().filter(|t| *t == "0").count() >= 10,
            "an empty month lost its zero counter: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "5.0 KiB") && texts.iter().any(|t| t == "1.0 KiB"),
            "no size under the month cards: {texts:?}"
        );
    }

    /// M22.5: the picked month carries the accent frame and no other card does; the frame is 1 px of
    /// the palette accent, not the blue of the old calendar.
    #[test]
    fn the_picked_month_and_day_are_framed_in_the_accent() {
        let mut app = calendar_app();
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let accent = super::DARK.accent;
        let framed = |app: &mut App| -> Vec<egui::Rect> {
            window_strokes(app, &ctx, size)
                .into_iter()
                .filter(|(r, s)| {
                    s.color == accent
                        && (s.width - 1.0).abs() < 0.01
                        && r.left() > super::LEFT_W
                        && r.top() > super::TOP_H + 8.0
                })
                .map(|(r, _)| r)
                .collect()
        };
        app.cal_scale = CalScale::Year;
        app.cal_month = 5;
        let year_frames = framed(&mut app);
        assert_eq!(year_frames.len(), 1, "one picked month, not {year_frames:?}");
        // The frame of the picked month sits at the middle of the second row of four columns:
        // January starts at the left edge of the grid, May is the fifth card.
        app.cal_scale = CalScale::Month;
        app.cal_day = None;
        assert!(
            framed(&mut app).is_empty(),
            "a day cell is framed while no day is picked"
        );
        app.cal_day = Some("2024-05-01".into());
        let day_frames = framed(&mut app);
        assert_eq!(day_frames.len(), 1, "one picked day, not {day_frames:?}");
        assert!(
            day_frames[0].width() > 60.0 && day_frames[0].height() > 80.0,
            "the framed day is a cell: {:?}",
            day_frames[0]
        );
    }

    /// M22.5: the day list under the calendar is a list with the time, the name and the size — one
    /// date for every row, so the date part is not repeated. The heading names the day itself.
    #[test]
    fn the_day_strip_shows_the_time_name_and_size() {
        let mut app = calendar_app();
        app.cal_scale = CalScale::Month;
        app.cal_day = Some("2024-05-01".into());
        let ctx = egui::Context::default();
        let texts = window_frame_at(&mut app, &ctx, Vec::new(), egui::vec2(1240.0, 1200.0));
        assert!(
            texts.iter().any(|t| t == "Wed, 01.05.2024"),
            "no heading of the day: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "· 2 files, 5.0 KiB"),
            "no facts of the day: {texts:?}"
        );
        for needle in [
            "09:10 · 2.0 KiB",
            "14:23 · 3.0 KiB",
            "breakfast.jpg",
            "walk.jpg",
        ] {
            assert!(
                texts.iter().any(|t| t == needle),
                "no row “{needle}” in the day list: {texts:?}"
            );
        }
        // The full date would be the caption of a grid tile; inside one day it is noise.
        assert!(
            !texts.iter().any(|t| t.contains("2024-05-01 09:10")),
            "the day list repeats the date: {texts:?}"
        );
        // A day without files says so instead of drawing an empty list.
        app.cal_day = Some("2024-05-03".into());
        let texts = window_frame_at(&mut app, &ctx, Vec::new(), egui::vec2(1240.0, 1200.0));
        assert!(
            texts.iter().any(|t| t == "No files on this day."),
            "an empty day is silent: {texts:?}"
        );
    }

    /// M22.5: an empty month is a stop on the way, not a dead end: the button names the nearest
    /// month that has files (backwards first, as `nearest_file_month`).
    #[test]
    fn an_empty_month_offers_the_nearest_month_with_files() {
        let mut app = calendar_app();
        app.cal_scale = CalScale::Month;
        app.cal_month = 3;
        app.cal_day = None;
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t == "No files in this month."),
            "no message of an empty month: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Go to April 2024"),
            "the way out of an empty month is not named: {texts:?}"
        );
        // June lies forward: with nothing behind it, that is where the button leads.
        app.cal_month = 6;
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t == "Go to May 2024"),
            "no forward way out of an empty month: {texts:?}"
        );
    }

    /// The grid is "moments": the fresh month first, inside a month the fresh ones on top, the
    /// files without a date in the last group (M16, ADR 0033).
    #[test]
    fn the_grid_groups_by_month_from_the_newest() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-ui.db"));
        let mut may_old = file_row(1, "image", "may-old.jpg");
        may_old.taken_at = Some("2024-05-01T08:00".into());
        let mut may_new = file_row(2, "image", "may-new.jpg");
        may_new.taken_at = Some("2024-05-09T10:00".into());
        let mut april = file_row(3, "image", "april.jpg");
        april.taken_at = Some("2024-04-30T09:00".into());
        let mut undated = file_row(4, "image", "undated.jpg");
        undated.taken_at = None;
        app.items = vec![may_old, may_new, april, undated];

        let texts = window_frame(&mut app, &egui::Context::default());
        let pos = |needle: &str| {
            texts
                .iter()
                .position(|t| t == needle)
                .unwrap_or_else(|| panic!("no heading “{needle}”: {texts:?}"))
        };
        let (may, april_pos, no_date) = (pos("May 2024"), pos("April 2024"), pos("No date"));
        assert!(may < april_pos, "May is newer than April: {texts:?}");
        assert!(april_pos < no_date, "“No date” is the last group: {texts:?}");
        // Inside a month the tile captions go from the fresh ones to the old ones.
        let may_new = texts.iter().position(|t| t == "2024-05-09 10:00").unwrap();
        let may_old = texts.iter().position(|t| t == "2024-05-01 08:00").unwrap();
        assert!(may_new < may_old, "inside a month the newest come first: {texts:?}");
    }

    /// Files for M17: two in May, one in April and a photo without a date.
    fn m17_rows() -> Vec<db::FileRow> {
        let mut may_old = file_row(1, "image", "may-old.jpg");
        may_old.taken_at = Some("2024-05-01T08:00".into());
        may_old.size = 10;
        let mut may_new = file_row(2, "image", "may-new.jpg");
        may_new.taken_at = Some("2024-05-09T10:00".into());
        may_new.size = 30;
        let mut april = file_row(3, "image", "april.jpg");
        april.taken_at = Some("2024-04-30T09:00".into());
        april.size = 20;
        let mut undated = file_row(4, "image", "undated.jpg");
        undated.taken_at = None;
        undated.size = 40;
        vec![may_old, may_new, april, undated]
    }

    fn m17_app() -> App {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-m17.db"));
        app.items = m17_rows();
        app
    }

    #[test]
    fn date_order_keeps_undated_last_in_both_directions() {
        let mut app = m17_app();
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.settings.grid_sort = db::GridSort::DateDesc;
        assert_eq!(app.ordered(&visible), vec![1, 0, 2, 3], "the newest first, the undated last");
        app.settings.grid_sort = db::GridSort::DateAsc;
        assert_eq!(app.ordered(&visible), vec![2, 0, 1, 3], "the oldest first, the undated still last");
    }

    #[test]
    fn months_flip_while_undated_stays_last() {
        let mut app = m17_app();
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.settings.grid_sort = db::GridSort::DateDesc;
        let desc = app.ordered(&visible);
        let keys: Vec<String> = month_groups(&desc, &app.items, false)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["2024-05", "2024-04", "no date"]);
        app.settings.grid_sort = db::GridSort::DateAsc;
        let asc = app.ordered(&visible);
        let keys: Vec<String> = month_groups(&asc, &app.items, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["2024-04", "2024-05", "no date"]);
    }

    #[test]
    fn name_and_size_are_sorted() {
        let mut app = m17_app();
        app.items[0].path = "/tmp/b.jpg".into();
        app.items[1].path = "/tmp/A.jpg".into();
        app.items[2].path = "/tmp/c.jpg".into();
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.settings.grid_sort = db::GridSort::Name;
        // The name is case-insensitive: A, b, c, undated.
        assert_eq!(app.ordered(&visible), vec![1, 0, 2, 3]);
        app.settings.grid_sort = db::GridSort::SizeDesc;
        assert_eq!(
            app.ordered(&visible),
            vec![3, 1, 2, 0],
            "the big ones first: 40, 30, 20, 10"
        );
    }

    #[test]
    fn with_semantic_search_the_search_sets_the_order() {
        let mut app = m17_app();
        app.settings.grid_sort = db::GridSort::SizeDesc;
        app.semantic_hits = Some(vec![4, 1, 3]);
        assert_eq!(app.ordered(&app.filtered()), vec![3, 0, 2]);
        // The search results are a flat list by relevance, without month groups.
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            !texts.iter().any(|t| t == "май 2024"),
            "a search does not group by month: {texts:?}"
        );
    }

    #[test]
    fn the_flat_view_draws_the_grid_without_month_headers() {
        let mut app = m17_app();
        app.settings.grid_layout = db::GridLayout::Flat;
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            !texts.iter().any(|t| t == "май 2024"),
            "the flat view must hold no month headings: {texts:?}"
        );
        assert!(texts.iter().any(|t| t.contains("2024-05-09")), "{texts:?}");
    }

    /// M18: the grid order is the same `App::ordered`, so Shift+click takes a range by the
    /// screen, not by ascending id.
    fn m18_order(app: &App) -> Vec<i64> {
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.ordered(&visible)
            .iter()
            .map(|&i| app.items[i].id)
            .collect()
    }

    #[test]
    fn ctrl_and_shift_click_change_the_selection() {
        let mut app = m17_app();
        let order = m18_order(&app);
        assert_eq!(order, vec![2, 1, 3, 4], "the order: the newest first, the undated last");

        app.pick_clicked(2, PickClick::Plain, &order);
        assert_eq!(app.picked, BTreeSet::from([2]));
        assert_eq!(app.selected, Some(2));

        app.pick_clicked(3, PickClick::Toggle, &order);
        assert_eq!(app.picked, BTreeSet::from([2, 3]), "Ctrl+click adds");
        app.pick_clicked(3, PickClick::Toggle, &order);
        assert_eq!(app.picked, BTreeSet::from([2]), "a second Ctrl+click removes");

        app.pick_anchor = Some(2);
        app.pick_clicked(3, PickClick::Range, &order);
        assert_eq!(
            app.picked,
            BTreeSet::from([2, 1, 3]),
            "Shift+click takes a range in grid order"
        );
        assert_eq!(app.selected, Some(3), "the last clicked file is in the inspector");

        // The reverse range gives the same set, and the anchor does not move.
        app.pick_anchor = Some(3);
        app.pick_clicked(2, PickClick::Range, &order);
        assert_eq!(app.picked, BTreeSet::from([2, 1, 3]));

        // The modifiers are read into an action: Ctrl/Cmd and Shift are different things.
        let mut mods = egui::Modifiers::default();
        assert_eq!(pick_kind(&mods), PickClick::Plain);
        mods.command = true;
        assert_eq!(pick_kind(&mods), PickClick::Toggle);
        mods.shift = true;
        assert_eq!(pick_kind(&mods), PickClick::Range);
    }

    #[test]
    fn select_all_takes_only_what_is_visible() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-m18.db"));
        app.settings.lang = i18n::Lang::Ru;
        let mut a = file_row(1, "image", "a.jpg");
        a.size = 10;
        let mut b = file_row(2, "video", "b.mp4");
        b.size = 20;
        let mut c = file_row(3, "image", "c.jpg");
        c.size = 30;
        app.items = vec![a, b, c];
        app.kind_filter = KindFilter::Images;
        app.pick_all();
        assert_eq!(app.picked, BTreeSet::from([1, 3]), "the video is hidden by the filter");
        assert_eq!(app.selected, Some(1), "the first visible file is in the inspector");
        assert!(app.status.starts_with("выбрано 2"), "{}", app.status);
    }

    #[test]
    fn esc_drops_the_selection_and_writes_to_the_status() {
        let mut app = m17_app();
        let lang = app.settings.lang;
        app.pick_all();
        assert_eq!(app.picked.len(), 4);
        let ctx = egui::Context::default();
        window_frame_with(&mut app, &ctx, vec![key_event(egui::Key::Escape)]);
        assert!(app.picked.is_empty(), "Esc drops the selection");
        assert_eq!(app.status, t(lang, "the selection is cleared"));
    }

    #[test]
    fn a_new_filter_or_order_drops_the_selection_with_a_reason() {
        let mut app = m17_app();
        let lang = app.settings.lang;
        app.pick_all();
        app.query = "мая".into();
        app.sync_pick_scope();
        assert!(app.picked.is_empty());
        assert!(
            app.status.contains(t(lang, "the folder and date filter changed")),
            "{}",
            app.status
        );

        // Return the set of files and reset the comparison before changing the order.
        app.query.clear();
        app.sync_pick_scope();
        app.pick_all();
        app.settings.grid_sort = db::GridSort::Name;
        app.sync_pick_scope();
        assert!(app.picked.is_empty());
        assert!(app.status.contains(t(lang, "the order changed")), "{}", app.status);

        // A change of the view is another set of files too.
        app.pick_all();
        app.view = super::View::Dupes;
        app.sync_pick_scope();
        assert!(app.picked.is_empty());
        assert!(app.status.contains(t(lang, "the view changed")), "{}", app.status);
    }

    #[test]
    fn the_chips_row_shows_the_selection_and_the_buttons() {
        let mut app = m17_app();
        app.picked = BTreeSet::from([1, 2]);
        app.selected = Some(2);
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t.starts_with("2 selected · ")),
            "the chips row shows the count and the size: {texts:?}"
        );
        assert!(texts.iter().any(|t| t == "Select all (4)"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Clear selection"), "{texts:?}");
    }

    /// M22.2a: the list scales with the same "Tiles" slider as the tile and the rows. The row
    /// height is 24 + 0.16 × the value, the font sizes 12–14 / 10–12 px.
    #[test]
    fn the_list_scales_with_the_tiles_slider() {
        assert!(
            (list_row_height(100.0) - 40.0).abs() < 0.01,
            "{}",
            list_row_height(100.0)
        );
        assert!((list_row_height(180.0) - 52.8).abs() < 0.01);
        assert!(
            (list_row_height(260.0) - 65.6).abs() < 0.01,
            "{}",
            list_row_height(260.0)
        );
        // The font sizes grow with the row and stay within 12–14 / 10–12.
        assert_eq!(list_name_px(100.0), 12.0);
        assert_eq!(list_name_px(260.0), 14.0);
        assert_eq!(list_meta_px(100.0), 10.0);
        assert_eq!(list_meta_px(260.0), 12.0);
        // One slider value for three views: the list does not reset it.
        let mut app = m17_app();
        app.tile_size = 200.0;
        app.settings.cell_style = db::CellStyle::List;
        assert_eq!(app.tile_size, 200.0);
    }

    /// Draws `draw_list` in a frame without a window and returns the height the two rows took:
    /// that is how the list scale is checked on the real drawing code.
    fn list_frame_height(tile: f32) -> f32 {
        let ctx = egui::Context::default();
        let rows = m17_rows();
        let files: Vec<&db::FileRow> = rows.iter().take(2).collect();
        let picked = BTreeSet::new();
        let mut textures = std::collections::HashMap::new();
        let mut missing = std::collections::HashSet::new();
        let mut cells = super::Cells {
            bg: egui::Color32::BLACK,
            cell_style: db::CellStyle::List,
            textures: &mut textures,
            missing: &mut missing,
            thumbs_dir: std::path::Path::new("/nonexistent/thumbs"),
            selected: None,
            picked: &picked,
            clicked: None,
            open_clicked: None,
            show_keep: false,
            keep_clicked: None,
            trash_mode: false,
            restore_clicked: None,
            tile,
            lang: i18n::Lang::Ru,
            budget: 4,
            time_only: false,
        };
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 400.0),
            )),
            ..Default::default()
        };
        let mut used = 0.0;
        let out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            super::draw_list(ui, &ctx, &mut cells, &files);
            used = ui.min_rect().height();
        });
        out.drop_without_applying_deltas();
        used
    }

    /// M22.2a: the list scales with the slider — two rows are taller at 260 than at 100, and no
    /// row cuts the text vertically.
    #[test]
    fn the_list_draws_taller_rows_with_a_bigger_slider() {
        let small = list_frame_height(100.0);
        let big = list_frame_height(260.0);
        // 40 px and 65.6 px per row, plus the gap under them: the difference matters, not the
        // initial frame offset.
        assert!((small - 84.0).abs() < 8.0, "two rows of 40 px: {small}");
        assert!((big - 135.2).abs() < 8.0, "two rows of 65.6 px: {big}");
        assert!((big - small - 51.2).abs() < 0.5, "{small} → {big}");
    }

    /// M22.19: the tile size moved from the content panel into the top strip; the pill is drawn
    /// in every cell view, so the size never disappears with a view switch.
    #[test]
    fn the_size_slider_shows_in_all_three_views() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.tile_size = 200.0;
        let ctx = egui::Context::default();
        for style in [db::CellStyle::Tiles, db::CellStyle::Rows, db::CellStyle::List] {
            app.settings.cell_style = style;
            let strokes = window_strokes(&mut app, &ctx, egui::vec2(1240.0, 840.0));
            let pill = strokes.iter().find(|(r, s)| {
                s.width < 1.5 && r.top() < 50.0 && (100.0..140.0).contains(&r.width())
            });
            assert!(
                pill.is_some(),
                "the tile size pill is gone in the {style:?} view: {strokes:?}"
            );
        }
    }

    /// M22.3, M25.2: the year block of the left panel is a filter, not a decoration. The counts
    /// come from the dates of the index, a click sets `year_filter` and a second click (or "all
    /// years") clears it. A year without files is not in the list at all, and the year is a filter
    /// of its own — the path/date field keeps its word.
    #[test]
    fn a_year_click_narrows_the_date_filter() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-years.db"));
        let dated = |id: i64, name: &str, at: &str| {
            let mut r = file_row(id, "image", name);
            r.taken_at = Some(at.into());
            r
        };
        let undated = |id: i64, name: &str| {
            let mut r = file_row(id, "image", name);
            r.taken_at = None;
            r
        };
        app.items = vec![
            dated(1, "a.jpg", "2024-05-01T08:00"),
            dated(2, "b.jpg", "2024-09-09T10:00"),
            dated(3, "c.jpg", "2023-01-01T09:00"),
            dated(4, "d.jpg", "2020-12-31T23:00"),
            undated(5, "undated.jpg"),
        ];
        let years = app.year_counts();
        let list: Vec<&str> = years.iter().map(|(y, _)| y.as_str()).collect();
        assert_eq!(list, ["2024", "2023", "2020"], "the newest first: {list:?}");
        assert_eq!(years[0].1, 2, "two files in 2024");
        // Every year of the index: the cloud takes the five newest and knows whether the rest
        // exists, so the count here is not cut (M25.2).
        app.items = (0..9)
            .map(|i| dated(i + 1, "x.jpg", &format!("{}-05-01T08:00", 2010 + i)))
            .collect();
        assert_eq!(app.year_counts().len(), 9);
        assert_eq!(app.year_counts()[0].0, "2018");
        assert_eq!(app.year_counts()[8].0, "2010");

        app.items = vec![
            dated(1, "a.jpg", "2024-05-01T08:00"),
            dated(2, "b.jpg", "2024-09-09T10:00"),
            dated(3, "c.jpg", "2023-01-01T09:00"),
            undated(5, "undated.jpg"),
        ];
        // A semantic search plus a year is a contradiction: the click drops the words.
        app.semantic_hits = Some(vec![1]);
        app.ocr_snippets.insert(1, "text".into());
        app.set_year(Some(super::YearPick::Year("2024".into())));
        assert_eq!(
            app.year_filter,
            Some(super::YearPick::Year("2024".into())),
            "the year has its own filter"
        );
        assert!(app.query.is_empty(), "the year no longer writes into the path field");
        assert!(app.semantic_hits.is_none(), "the words are gone with the year");
        assert!(app.ocr_snippets.is_empty());
        let visible: Vec<String> = app
            .filtered()
            .iter()
            .map(|&i| super::file_name(&app.items[i].path))
            .collect();
        assert_eq!(visible, ["a.jpg", "b.jpg"], "only 2024: {visible:?}");
        // A typed path word survives a year click: the two filters are independent (M25.3).
        app.query = "vacation".into();
        app.set_year(Some(super::YearPick::Year("2024".into())));
        assert_eq!(app.query, "vacation", "the typed word survives a year click");
        app.query.clear();
        // The way back: the filter is empty again and every file is visible.
        app.set_year(None);
        assert!(app.query.is_empty());
        assert!(app.year_filter.is_none());
        assert_eq!(app.filtered().len(), 4, "all years are back");
    }

    /// M25.3: the path word and the year are two filters with two chips and two "×", and the
    /// field explains itself before a key is pressed: the text is a case-insensitive part of the
    /// path or of the ISO date, and the content search lives in the top bar.
    #[test]
    fn the_path_word_and_the_year_are_two_filters_with_two_crosses() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-path-year.db"));
        let dated = |id: i64, path: &str, at: &str| {
            let mut r = file_row(id, "image", path);
            r.taken_at = Some(at.into());
            r
        };
        app.items = vec![
            dated(1, "trip-a.jpg", "2024-05-01T08:00"),
            dated(2, "vacation-b.jpg", "2023-06-01T08:00"),
            dated(3, "vacation-c.jpg", "2024-07-01T08:00"),
        ];
        app.settings.lang = i18n::Lang::En;
        app.query = "vacation".into();
        app.set_year(Some(super::YearPick::Year("2024".into())));
        let visible: Vec<String> = app
            .filtered()
            .iter()
            .map(|&i| super::file_name(&app.items[i].path))
            .collect();
        assert_eq!(
            visible,
            ["vacation-c.jpg"],
            "the word and the year apply together: {visible:?}"
        );
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame_at(&mut app, &ctx, Vec::new(), size);
        let word = |app: &mut App, want: &str| {
            window_word_rects_at(app, &ctx, size)
                .into_iter()
                .find(|(text, _)| text == want)
                .unwrap_or_else(|| panic!("“{want}” is not drawn"))
                .1
        };
        let r = word(&mut app, "path/date: vacation ×");
        click_at(&mut app, &ctx, r.center());
        assert!(app.query.is_empty(), "the path chip removes the word");
        assert_eq!(
            app.year_filter,
            Some(super::YearPick::Year("2024".into())),
            "the year survives the word"
        );
        let r = word(&mut app, "year: 2024 ×");
        click_at(&mut app, &ctx, r.center());
        assert!(app.year_filter.is_none(), "the year chip removes the year");
        // The tooltip is in the table and reads in both languages.
        let ru = i18n::t(i18n::Lang::Ru, super::PATH_DATE_HINT);
        assert_ne!(ru, super::PATH_DATE_HINT, "the hint is not in PAIRS");
        assert!(
            ru.chars().any(|c| c.is_alphabetic() && !c.is_ascii()),
            "the hint is not translated: {ru}"
        );
        assert_eq!(
            i18n::t(i18n::Lang::En, super::PATH_DATE_HINT),
            super::PATH_DATE_HINT
        );
    }

    /// M25.4: clearing the previews nulls the `thumb` column, and the index sees a file without a
    /// preview as one that must be read again — otherwise a cleared cache would never heal.
    #[test]
    fn a_cleared_preview_makes_the_file_processed_again() {
        let db_path = std::env::temp_dir().join("photocat-test-cleared-thumbs.db");
        let conn = crate::db::open(&db_path).expect("the test DB opens");
        conn.execute("DELETE FROM files", [])
            .expect("the test DB starts empty");
        let row = crate::db::FileRow {
            id: 0,
            path: "/tmp/a.jpg".into(),
            size: 10,
            mtime: 100,
            sha256: "abc".into(),
            kind: "image".into(),
            width: 1,
            height: 1,
            taken_at: None,
            taken_src: "file".into(),
            lat: None,
            lon: None,
            camera: None,
            dhash: None,
            sharpness: None,
            duration_s: None,
            thumb: Some("ab/x.jpg".into()),
            decision: None,
            trashed_at: None,
        };
        crate::db::insert(&conn, &row).expect("the row is inserted");
        let existing = || {
            let rows = crate::db::load_all(&conn).expect("the rows are read");
            let r = &rows[0];
            crate::index::Existing {
                size: r.size,
                mtime: r.mtime,
                sha_ok: !r.sha256.is_empty(),
                thumb: r.thumb.is_some(),
                duration: r.duration_s.is_some(),
                kind: r.kind.clone(),
            }
        };
        assert!(
            crate::index::can_skip(Some(&existing()), 10, 100, "image"),
            "an unchanged file with a preview is skipped"
        );
        crate::storage::forget_thumbs(&db_path).expect("the preview paths are cleared");
        assert!(
            !crate::index::can_skip(Some(&existing()), 10, 100, "image"),
            "a file without a preview is read again"
        );
    }

    /// M25.4: the two "missing" chips show how many files have no date and how many have no
    /// preview (over the whole index), narrow the grid together and with each other, are dead at
    /// zero, and the chips row removes each filter on its own.
    #[test]
    fn the_no_date_and_no_preview_chips_filter_the_grid() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-missing.db"));
        let dir = std::env::temp_dir().join("photocat-test-missing-thumbs");
        std::fs::create_dir_all(dir.join("ab")).expect("the test thumbs folder is made");
        std::fs::write(dir.join("ab/on.jpg"), b"x").expect("the test preview is written");
        app.thumbs_dir = dir;
        let row = |id: i64, name: &str, at: Option<&str>, thumb: Option<&str>| {
            let mut r = file_row(id, "image", name);
            r.taken_at = at.map(|s| s.to_string());
            r.thumb = thumb.map(|s| s.to_string());
            r
        };
        app.items = vec![
            row(1, "dated-on.jpg", Some("2024-05-01T08:00"), Some("ab/on.jpg")),
            row(2, "dated-gone.jpg", Some("2024-06-01T08:00"), Some("ab/gone.jpg")),
            row(3, "undated.jpg", None, None),
        ];
        app.refresh_missing_previews();
        app.settings.lang = i18n::Lang::En;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame_at(&mut app, &ctx, Vec::new(), size);
        let word = |app: &mut App, want: &str| {
            window_word_rects_at(app, &ctx, size)
                .into_iter()
                .find(|(text, _)| text == want)
                .unwrap_or_else(|| panic!("“{want}” is not drawn"))
                .1
        };
        // The count is over the whole index: one file has no date, two have no preview (one row
        // never got a preview, one names a file that is gone from the cache).
        let r = word(&mut app, "No preview · 2");
        click_at(&mut app, &ctx, r.center());
        assert!(app.only_no_preview, "the chip is a toggle");
        let names = |app: &App| -> Vec<String> {
            app.filtered()
                .iter()
                .map(|&i| super::file_name(&app.items[i].path))
                .collect()
        };
        assert_eq!(names(&app), ["dated-gone.jpg", "undated.jpg"]);
        // The two filters work together: only the file with neither a date nor a preview passes.
        let r = word(&mut app, "No date · 1");
        click_at(&mut app, &ctx, r.center());
        assert!(app.only_no_date && app.only_no_preview);
        assert_eq!(names(&app), ["undated.jpg"]);
        // Each "×" of the chips row removes exactly its own filter.
        let r = word(&mut app, "No preview ×");
        click_at(&mut app, &ctx, r.center());
        assert!(!app.only_no_preview && app.only_no_date);
        let r = word(&mut app, "No date ×");
        click_at(&mut app, &ctx, r.center());
        assert!(!app.only_no_date);
        assert_eq!(names(&app).len(), 3, "every file is back");
        // At zero the chip is dead: a filter that promises nothing does not invite a click.
        for r in &mut app.items {
            r.taken_at = Some("2024-01-01T00:00".into());
            r.thumb = Some("ab/on.jpg".into());
        }
        app.refresh_missing_previews();
        let _ = window_frame_at(&mut app, &ctx, Vec::new(), size);
        let r = word(&mut app, "No date · 0");
        click_at(&mut app, &ctx, r.center());
        assert!(!app.only_no_date, "a chip with zero files does nothing");
    }

    /// M25.2: the cloud keeps the five newest years and a "…" that filters exactly the older
    /// ones; a second click clears either pick and "All years" is the one way back. No number is
    /// drawn inside a chip.
    #[test]
    fn the_years_cloud_keeps_five_and_filters_the_older_ones() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-years-cloud.db"));
        app.items = (0..21)
            .map(|i| {
                let mut r = file_row(i + 1, "image", "y.jpg");
                r.taken_at = Some(format!("{}-05-01T08:00", 2004 + i));
                r
            })
            .collect();
        app.settings.lang = i18n::Lang::En;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame_at(&mut app, &ctx, Vec::new(), size);
        let drawn = |app: &mut App| window_word_rects_at(app, &ctx, size);
        let texts: Vec<String> = drawn(&mut app).into_iter().map(|(t, _)| t).collect();
        for year in ["2024", "2023", "2022", "2021", "2020"] {
            assert!(texts.iter().any(|t| t == year), "no chip “{year}”: {texts:?}");
        }
        assert!(
            !texts.iter().any(|t| t == "2019" || t == "2004"),
            "the cloud never grows: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "…"),
            "no ellipsis for the older years: {texts:?}"
        );
        // No number is drawn inside a chip: the count lives in the hint.
        assert!(
            !texts.iter().any(|t| t == "2020 · 1 file"),
            "the count is not a chip: {texts:?}"
        );
        let at = |app: &mut App, want: &str| {
            drawn(app)
                .into_iter()
                .find(|(t, _)| t == want)
                .unwrap_or_else(|| panic!("“{want}” is not drawn"))
                .1
        };
        // The "…" narrows the grid to exactly the years older than the cloud's oldest one.
        let r = at(&mut app, "…");
        click_at(&mut app, &ctx, r.center());
        assert_eq!(
            app.year_filter,
            Some(super::YearPick::Older("2020".into())),
            "the ellipsis filters the older years"
        );
        let mut years: Vec<String> = app
            .filtered()
            .iter()
            .map(|&i| app.items[i].taken_at.clone().unwrap()[0..4].to_string())
            .collect();
        years.sort();
        years.dedup();
        assert_eq!(
            years,
            ["2004", "2005", "2006", "2007", "2008", "2009", "2010", "2011", "2012", "2013",
             "2014", "2015", "2016", "2017", "2018", "2019"],
            "the older years only"
        );
        // The chips row names the pick.
        let texts = window_frame_at(&mut app, &ctx, Vec::new(), size);
        assert!(
            texts.iter().any(|t| t == "older than 2020 ×"),
            "the chips row has no year chip: {texts:?}"
        );
        // A second click on "…" clears it.
        let r = at(&mut app, "…");
        click_at(&mut app, &ctx, r.center());
        assert!(app.year_filter.is_none(), "a second click clears the ellipsis");
        // "All years" is drawn only while a pick is on and clears it.
        let texts = drawn(&mut app).into_iter().map(|(t, _)| t).collect::<Vec<_>>();
        assert!(
            !texts.iter().any(|t| t == "All years"),
            "no way back while the filter is empty: {texts:?}"
        );
        let r = at(&mut app, "2021");
        click_at(&mut app, &ctx, r.center());
        assert_eq!(
            app.year_filter,
            Some(super::YearPick::Year("2021".into()))
        );
        let r = at(&mut app, "All years");
        click_at(&mut app, &ctx, r.center());
        assert!(app.year_filter.is_none(), "All years is the way back");
    }

    /// M22.3: the year rows are drawn with their counts, and a year with zero files is not drawn.
    #[test]
    fn the_years_block_draws_the_newest_five_with_counts() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-years-frame.db"));
        let dated = |id: i64, name: &str, at: &str| {
            let mut r = file_row(id, "image", name);
            r.taken_at = Some(at.into());
            r
        };
        app.items = vec![
            dated(1, "a.jpg", "2024-05-01T08:00"),
            dated(2, "b.jpg", "2024-09-09T10:00"),
            dated(3, "c.jpg", "2023-01-01T09:00"),
            dated(4, "d.jpg", "2020-12-31T23:00"),
        ];
        app.settings.lang = i18n::Lang::Ru;
        let ctx = egui::Context::default();
        let _ = window_frame(&mut app, &ctx);
        let texts = window_frame(&mut app, &ctx);
        assert!(texts.iter().any(|t| t == "ГОДЫ"), "no year block: {texts:?}");
        for year in ["2024", "2023", "2020"] {
            assert!(texts.iter().any(|t| t == year), "no row “{year}”: {texts:?}");
        }
        for empty in ["2022", "2021", "2019"] {
            assert!(!texts.iter().any(|t| t == empty), "an empty year is drawn: {texts:?}");
        }
        // "All years" appears only while a year is picked: a way back, not a permanent row.
        assert!(!texts.iter().any(|t| t == "Все годы"), "{texts:?}");
        app.set_year(Some(super::YearPick::Year("2024".into())));
        let _ = window_frame(&mut app, &ctx);
        let texts = window_frame(&mut app, &ctx);
        assert!(texts.iter().any(|t| t == "Все годы"), "{texts:?}");
    }

    /// M22.3: the panel budget. At 840 px the folder block, two navigation groups and the filter are
    /// pinned, and the year rows open the scrolling part — so all five are seen without a scroll, and
    /// they stand above the summary, which is pinned at the bottom.
    #[test]
    fn the_left_panel_fits_the_years_above_the_summary() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-budget.db"));
        app.items = (0..5)
            .map(|i| {
                let mut r = file_row(i + 1, "image", "y.jpg");
                r.taken_at = Some(format!("{}-05-01T08:00", 2020 + i));
                r
            })
            .collect();
        app.settings.lang = i18n::Lang::Ru;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_layout(&mut app, &ctx, size);
        let drawn = window_layout(&mut app, &ctx, size);
        let at = |needle: &str| {
            drawn
                .iter()
                .find(|(text, _)| text == needle)
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("“{needle}” is not drawn: {drawn:?}"))
        };
        // Every year row is drawn: the guard inside `year_row` skips a row that is out of the
        // viewport, so a drawn oldest row means the whole block is seen without a scroll.
        for year in ["2024", "2023", "2022", "2021", "2020"] {
            at(year);
        }
        // The oldest row is the last one of the block; it must stand above the summary, which is
        // pinned at the bottom of the panel, and above the bottom strip.
        let oldest = at("2020");
        let summary = at("Сводка");
        assert!(
            oldest.y + 20.0 <= summary.y,
            "the year rows are cut off by the summary: {oldest:?} vs {summary:?}"
        );
        assert!(
            oldest.y + 20.0 <= 840.0 - 28.0,
            "a year row is under the bottom strip: {oldest:?}"
        );
        // Picking a year adds "All years" without growing the block: the chips wrap, so the
        // summary does not move and nothing below the years is pushed off.
        app.set_year(Some(super::YearPick::Year("2024".into())));
        let _ = window_layout(&mut app, &ctx, size);
        let picked = window_layout(&mut app, &ctx, size);
        // The picked year is also named in the chip of the grid and in the hint of the cloud, so
        // the last match inside the left panel is the year chip itself.
        let at_picked = |needle: &str| {
            picked
                .iter()
                .rfind(|(text, pos)| text == needle && pos.x < super::LEFT_W)
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("“{needle}” is not drawn after a pick: {picked:?}"))
        };
        assert_eq!(at_picked("Все годы").y, at_picked("2024").y, "All years on the same row");
        assert_eq!(
            at_picked("Сводка").y,
            summary.y,
            "the summary stays put when a year is picked"
        );
        at_picked("2020");
    }

    /// M22.3: a tag chip only changes colour, never its place — so the panel does not jump when a
    /// tag is picked. The frame draws the chips twice and compares every word and its position.
    #[test]
    fn the_tag_chips_do_not_jump_when_a_tag_is_picked() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-chips.db"));
        app.items = m17_rows();
        app.tags.insert(
            1,
            vec![("beach".into(), 0.9), ("mountains".into(), 0.4)],
        );
        app.tag_dists = embed::tag_distributions(&app.tags);
        app.settings.lang = i18n::Lang::En;
        app.tag_filters.clear();
        let _ = chips_layout(&mut app, 236.0);
        let (plain_size, plain) = chips_layout(&mut app, 236.0);
        app.tag_filters = vec!["beach".into()];
        let _ = chips_layout(&mut app, 236.0);
        let (picked_size, picked) = chips_layout(&mut app, 236.0);
        assert_eq!(
            plain.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            picked.iter().map(|(t, _)| t.as_str()).collect::<Vec<_>>(),
            "the same chips in the same order"
        );
        assert_eq!(plain_size, picked_size, "the row keeps its size");
        for (a, b) in plain.iter().zip(&picked) {
            assert_eq!(a.1, b.1, "“{}” moved after the click", a.0);
        }
        // The picked chip is there with the count of photos after a middle dot.
        assert!(
            picked.iter().any(|(t, _)| t == "beach · 1"),
            "no chip with a count: {picked:?}"
        );
        assert!(
            !picked.iter().any(|(t, _)| t == "all tags"),
            "the permanent “all tags” chip is gone (M25.1): {picked:?}"
        );
    }

    /// M25.1: a click on a tag chip adds it to the filter, a second click removes it, a reset
    /// beside the "Tags" heading clears the set. The grid keeps only the photos that carry every
    /// picked tag.
    #[test]
    fn the_panel_tag_cloud_accumulates_the_filter() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-tags-and.db"));
        app.items = vec![
            file_row(1, "image", "a.jpg"),
            file_row(2, "image", "b.jpg"),
            file_row(3, "image", "c.jpg"),
        ];
        app.tags.insert(1, vec![("beach".into(), 0.9)]);
        app.tags.insert(
            2,
            vec![("beach".into(), 0.8), ("document".into(), 0.7)],
        );
        app.tags.insert(3, vec![("document".into(), 0.6)]);
        app.tag_dists = embed::tag_distributions(&app.tags);
        app.settings.lang = i18n::Lang::En;
        let click = |app: &mut App, word: &str| {
            let _ = chips_layout(app, 236.0);
            let (_, chips) = chips_layout(app, 236.0);
            let pos = chips
                .iter()
                .find(|(text, _)| text == word)
                .unwrap_or_else(|| panic!("the chip “{word}” is drawn"))
                .1;
            chips_click(app, 236.0, pos + egui::vec2(4.0, 5.0));
        };
        click(&mut app, "beach · 2");
        assert_eq!(app.tag_filters, vec!["beach".to_string()]);
        click(&mut app, "document · 2");
        assert_eq!(
            app.tag_filters,
            vec!["beach".to_string(), "document".to_string()],
            "the second tag joins the filter"
        );
        let names: Vec<String> = app
            .filtered()
            .iter()
            .map(|&i| super::file_name(&app.items[i].path))
            .collect();
        assert_eq!(names, ["b.jpg"], "a file must carry every picked tag: {names:?}");
        // A second click on one chip leaves the other tag in the filter.
        click(&mut app, "beach · 2");
        assert_eq!(app.tag_filters, vec!["document".to_string()]);
        // Two tags in the filter: the chips row says both, and each "×" removes exactly one.
        app.tag_filters = vec!["beach".to_string(), "document".to_string()];
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame_at(&mut app, &ctx, Vec::new(), size);
        let word = |app: &mut App, want: &str| {
            window_word_rects_at(app, &ctx, size)
                .into_iter()
                .find(|(text, _)| text == want)
                .unwrap_or_else(|| panic!("“{want}” is on screen"))
                .1
        };
        let r = word(&mut app, "tag: beach ×");
        click_at(&mut app, &ctx, r.center());
        assert_eq!(
            app.tag_filters,
            vec!["document".to_string()],
            "the “×” of a chip removes exactly its own tag"
        );
        let r = word(&mut app, "tag: document ×");
        click_at(&mut app, &ctx, r.center());
        assert!(app.tag_filters.is_empty(), "the last chip clears the row");
        // "Reset" clears the whole set in one press.
        app.tag_filters = vec!["beach".to_string(), "document".to_string()];
        let _ = window_frame_at(&mut app, &ctx, Vec::new(), size);
        let r = word(&mut app, "Reset");
        click_at(&mut app, &ctx, r.center());
        assert!(app.tag_filters.is_empty(), "Reset clears every picked tag");
    }

    /// M25.1: the reset beside the "Tags" heading is drawn only while a tag is picked and clears
    /// the whole set — an empty filter spends no room on a reset.
    #[test]
    fn the_reset_beside_the_tags_heading_clears_the_filter() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-tags-reset.db"));
        app.settings.lang = i18n::Lang::En;
        let ctx = egui::Context::default();
        let plain = tags_heading_frame(&mut app, &ctx, Vec::new());
        app.tag_filters = vec!["beach".to_string(), "document".to_string()];
        let with_reset = tags_heading_frame(&mut app, &ctx, Vec::new());
        let target = |rects: &[egui::Rect]| {
            rects
                .iter()
                .find(|r| (r.width() - 20.0).abs() < 0.5 && (r.height() - 20.0).abs() < 0.5)
                .copied()
        };
        assert!(
            target(&plain).is_none(),
            "no reset target while the filter is empty: {plain:?}"
        );
        let at = target(&with_reset)
            .unwrap_or_else(|| panic!("the reset target is drawn while a tag is picked"))
            .center();
        let press = |pressed: bool| {
            vec![
                egui::Event::PointerMoved(at),
                egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]
        };
        let _ = tags_heading_frame(&mut app, &ctx, press(true));
        let _ = tags_heading_frame(&mut app, &ctx, press(false));
        assert!(
            app.tag_filters.is_empty(),
            "the reset clears every picked tag: {:?}",
            app.tag_filters
        );
    }

    // ---- M22.4: the month heading pinned while scrolling ---------------------------------

    /// Two months of snapshots, each taller than one screen and shorter than three.
    fn sticky_rows() -> Vec<db::FileRow> {
        let mut rows = Vec::new();
        let mut id = 1;
        for month in ["05", "04"] {
            for n in 0..25 {
                let mut r = file_row(id, "image", &format!("m{month}-{n:02}.jpg"));
                r.taken_at = Some(format!("2024-{month}-{:02}T08:00", 1 + n % 28));
                r.size = 1024;
                rows.push(r);
                id += 1;
            }
        }
        rows
    }

    /// M22.4: the heading of the group under the top edge stays at the top of the grid area. After
    /// less than one group of scrolling the first month is still named there (its own heading has
    /// gone above the edge, the pinned one is drawn instead); after a whole group the next month
    /// has taken its place and the first one is gone.
    #[test]
    fn the_month_heading_stays_pinned_while_its_group_is_on_screen() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-sticky.db"));
        app.items = sticky_rows();
        app.settings.lang = i18n::Lang::En;
        app.settings.grid_layout = db::GridLayout::Months;
        let ctx = egui::Context::default();
        let mut clock = 0.0_f64;
        let at_top = |drawn: &[(String, egui::Pos2)], needle: &str| {
            drawn
                .iter()
                .filter(|(t, _)| t == needle)
                .map(|(_, pos)| *pos)
                .min_by(|a, b| a.y.total_cmp(&b.y))
        };
        // The heading of the first group as drawn in its own place: the top edge of the grid area.
        let start = scrolled_grid(&mut app, &ctx, &mut clock, 0.0);
        let may_top = at_top(&start, "May 2024").expect("no May heading in the first frame");
        let top_caption = |drawn: &[(String, egui::Pos2)]| {
            drawn
                .iter()
                .filter(|(t, _)| t.ends_with(".jpg"))
                .min_by(|a, b| a.1.y.total_cmp(&b.1.y))
                .cloned()
        };
        let first_caption = top_caption(&start).expect("no caption is drawn in the first frame");
        // Less than one group: the May heading is drawn pinned at the top edge although the tiles
        // have moved — and it is the only “May 2024” on the screen, so the pinned row is the one.
        let scrolled = scrolled_grid(&mut app, &ctx, &mut clock, 1.0);
        let moved_caption = top_caption(&scrolled).expect("no caption is drawn after a scroll");
        assert!(
            moved_caption.0 != first_caption.0 || moved_caption.1.y < first_caption.1.y - 1.0,
            "the grid did not scroll: {first_caption:?} vs {moved_caption:?}"
        );
        let may: Vec<egui::Pos2> = scrolled
            .iter()
            .filter(|(t, _)| t == "May 2024")
            .map(|(_, pos)| *pos)
            .collect();
        assert_eq!(may.len(), 1, "expected exactly one pinned heading: {scrolled:?}");
        assert!(
            (may[0].y - may_top.y).abs() <= 1.5,
            "the pinned heading is not at the top edge: {:?} vs {may_top:?}",
            may[0]
        );
        // A whole group further: April's heading has pushed May's out and is pinned itself.
        let further = scrolled_grid(&mut app, &ctx, &mut clock, 4.0);
        let april = at_top(&further, "April 2024").expect("no April heading after a scroll");
        assert!(
            (april.y - may_top.y).abs() <= 1.5,
            "April is not pinned at the top edge: {april:?} vs {may_top:?}"
        );
        assert!(
            !further.iter().any(|(t, _)| t == "May 2024"),
            "the May heading did not give way to the next group: {further:?}"
        );
    }

    /// M22.4: the next group's heading pushes the pinned one out instead of taking its place at
    /// once: the pinned row slides up by exactly the overlap and disappears under the top edge.
    #[test]
    fn the_next_month_heading_pushes_the_pinned_one_out() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let may_y = |next_top: f32| -> f32 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(900.0, 600.0),
                )),
                ..Default::default()
            };
            let out = ctx.run_ui(raw, |ui| {
                let viewport = egui::Rect::from_min_size(
                    egui::pos2(100.0, 200.0),
                    egui::vec2(600.0, 300.0),
                );
                let heads = [
                    super::MonthHead {
                        top: -40.0,
                        height: 21.0,
                        key: "2024-05".into(),
                        files: 2,
                        bytes: 1024,
                    },
                    super::MonthHead {
                        top: next_top,
                        height: 21.0,
                        key: "2024-04".into(),
                        files: 2,
                        bytes: 1024,
                    },
                ];
                super::draw_sticky_month(ui, viewport, &heads, i18n::Lang::En);
            });
            let mut texts = Vec::new();
            for c in &out.shapes {
                collect_text_pos(&c.shape, &mut texts);
            }
            out.drop_without_applying_deltas();
            texts
                .iter()
                .find(|(t, _)| t == "May 2024")
                .map(|(_, pos)| pos.y)
                .unwrap_or_else(|| panic!("no pinned heading: {texts:?}"))
        };
        // The next heading is far below: the pinned one sits on the top edge (200 px).
        let pinned = may_y(400.0);
        assert!((pinned - 200.0).abs() < 8.0, "not on the top edge: {pinned}");
        // The next heading overlaps the last 6 px of the pinned row: it is pushed up by 6 px.
        let pushed = may_y(215.0);
        assert!(
            (pushed - (pinned - 6.0)).abs() < 0.6,
            "the heading did not slide out by the overlap: {pinned} → {pushed}"
        );
    }

    /// The caption band is 22 px: a long name must be cut with an ellipsis, not wrapped — a wrapped
    /// first line went over the preview (found while checking M22.3 on the owner's archive).
    #[test]
    fn a_long_caption_name_is_cut_to_one_line() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 200.0),
            )),
            ..Default::default()
        };
        let mut galleys = Vec::new();
        let out = ctx.run_ui(raw, |ui| {
            for (name, width) in [
                ("creeper_logotip_2026-10-16_18-38-13.png", 80.0),
                ("a.jpg", 80.0),
                ("очень_длинное_имя_снимка_на_русском.jpg", 40.0),
            ] {
                galleys.push(super::caption_name(
                    ui,
                    name,
                    egui::FontId::monospace(10.0),
                    egui::Color32::WHITE,
                    width,
                ));
            }
        });
        out.drop_without_applying_deltas();
        for g in &galleys {
            assert_eq!(g.rows.len(), 1, "a caption name must stay on one line: {g:?}");
        }
        // egui keeps `text()` as it was given and cuts the row visually; the cut is the width.
        assert!(
            galleys[0].size().x <= 80.5,
            "a long name must not run past its place: {}",
            galleys[0].size().x
        );
        assert!(
            galleys[2].size().x <= 40.5,
            "a narrow tile keeps the name inside: {}",
            galleys[2].size().x
        );
        assert_eq!(galleys[1].text(), "a.jpg", "a short name is untouched");
        assert_eq!(galleys[1].rows.len(), 1);
    }

    #[test]
    fn the_card_writes_how_many_are_selected() {
        let mut app = m17_app();
        app.picked = BTreeSet::from([1, 2]);
        app.selected = Some(2);
        app.settings.lang = i18n::Lang::Ru;
        let texts = card_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t.starts_with("в выделении 2")),
            "the inspector shows the selection size: {texts:?}"
        );
    }
    #[test]
    fn the_album_grid_shows_only_its_files_and_the_album_order() {
        let mut app = m17_app();
        app.view = View::Albums;
        app.album_selected = Some(7);
        // Album order is data: id 3, then 1, then 4. File 2 is not in the album.
        app.album_files = vec![3, 1, 4];
        app.album_set = [3, 1, 4].into_iter().collect();
        app.settings.grid_sort = db::GridSort::Album;
        let visible = app.filtered();
        assert_eq!(visible.len(), 3);
        assert!(!visible.contains(&1), "file 2 is not in the album");
        assert_eq!(app.ordered(&visible), vec![2, 0, 3], "the order comes from the album");
        // Outside an album "Album order" reads as the date ↓ instead of staying silent.
        app.view = View::Grid;
        app.album_selected = None;
        assert_eq!(app.ordered(&visible), vec![0, 2, 3], "outside an album — date ↓");
        // "All files" inside an album is a temporary look: the contents are not narrowed.
        app.view = View::Albums;
        app.album_selected = Some(7);
        app.album_browse_all = true;
        assert_eq!(app.filtered().len(), app.items.len());
    }

    #[test]
    fn the_album_chip_shows_the_name_and_clears() {
        let mut app = m17_app();
        app.view = View::Albums;
        app.album_selected = Some(7);
        app.albums = vec![db::AlbumNode {
            id: 7,
            parent_id: 0,
            position: 0,
            name: "Дача 2024".into(),
            note: String::new(),
            updated_at: "1714557600".into(),
            depth: 0,
            files: 3,
            trashed: 0,
            subalbums: 0,
            bytes: 30,
            covers: Vec::new(),
        }];
        let chips = app.chips_snapshot();
        assert_eq!(chips.album.as_deref(), Some("Дача 2024"));
        assert!(chips.pickable, "the selection works inside an open album");
        // In "All files" there is no album chip: the grid there is not narrowed by the album.
        app.view = View::Grid;
        assert!(app.chips_snapshot().album.is_none());
        app.view = View::Albums;
        // Closing the album returns the date ↓ and dims the grid.
        app.album_close();
        assert_eq!(app.album_selected, None);
        assert_eq!(app.settings.grid_sort, db::GridSort::DateDesc);
        assert!(app.chips_snapshot().album.is_none());
    }

    #[test]
    fn the_window_draws_the_album_tree_and_the_add_buttons() {
        let path = std::env::temp_dir().join(format!(
            "photocat-album-frame-{}.db",
            std::process::id()
        ));
        let album;
        {
            let conn = db::open(&path).unwrap();
            db::insert(&conn, &file_row(0, "image", "album.jpg")).unwrap();
            let file = conn
                .query_row("SELECT id FROM files WHERE path=?1", ["/tmp/album.jpg"], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap();
            album = db::create_album(&conn, 0, "Дача 2024", "").unwrap();
            db::add_to_album(&conn, album, &[file]).unwrap();
        }
        let mut app = App::new(path.clone());
        app.settings.lang = i18n::Lang::Ru;
        app.view = View::Albums;
        app.album_open(album);
        let ctx = egui::Context::default();
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t.contains("Дача 2024")),
            "the tree and the picked album are visible: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("Добавить выбранные")),
            "the add button is in place: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("Выгрузить на диск")),
            "the export (M20) is marked: {texts:?}"
        );
        // Nothing is created on disk: an album is DB rows only.
        assert!(!std::path::Path::new("/tmp/album.jpg").exists());
    }

    #[test]
    fn the_export_dialog_builds_a_plan_and_writes_nothing_into_the_archive() {
        let path = std::env::temp_dir().join(format!(
            "photocat-export-frame-{}.db",
            std::process::id()
        ));
        let out = std::env::temp_dir().join(format!(
            "photocat-export-out-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&out).unwrap();
        let album;
        {
            let conn = db::open(&path).unwrap();
            db::insert(&conn, &file_row(0, "image", "export.jpg")).unwrap();
            let file = conn
                .query_row("SELECT id FROM files WHERE path=?1", ["/tmp/export.jpg"], |r| {
                    r.get::<_, i64>(0)
                })
                .unwrap();
            album = db::create_album(&conn, 0, "На печать", "").unwrap();
            db::add_to_album(&conn, album, &[file]).unwrap();
        }
        let mut app = App::new(path);
        app.export_dialog_open(album);
        app.export_out = out.to_string_lossy().to_string();
        app.export_name = "{n}_{orig}".into();
        let plan = app.export_build().expect("the plan is built from the DB row");
        assert_eq!(plan.files(), 1);
        assert!(
            plan.items[0].rel.ends_with("01_export.jpg"),
            "{:?}",
            plan.items[0].rel
        );
        assert!(!out.join("На_печать").exists(), "the plan creates nothing");

        // A folder inside the archive is a refusal with a clear text, not an export into the originals.
        let archive = out.join("archive");
        std::fs::create_dir_all(archive.join("sub")).unwrap();
        app.settings.roots = vec![archive.to_string_lossy().to_string()];
        app.export_out = archive.join("sub").to_string_lossy().to_string();
        let err = app.export_build().unwrap_err().to_string();
        assert!(err.contains("inside the archive folder"), "{err}");
    }

    /// The "Integrity" screen exists without a terminal: the fields, three actions, the hint.
    #[test]
    fn the_integrity_screen_shows_the_fields_and_the_buttons() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity.db"));
        app.settings.lang = i18n::Lang::Ru;
        app.view = View::Integrity;
        let texts = window_frame(&mut app, &egui::Context::default());
        for want in [
            "Целостность",
            "Каталог A (архив):",
            "Каталог B (копия):",
            "Файл слепка:",
            "Показать все пути",
            "Сверить копии",
            "Записать слепок",
            "Проверить по слепку",
        ] {
            assert!(
                texts.iter().any(|t| t == want),
                "missing “{want}”: {texts:?}"
            );
        }
    }

    /// Comparing one and the same folder with itself is not a check but a waste of time: the
    /// refusal comes before the background thread starts.
    #[test]
    fn the_check_refuses_one_path_twice() {
        let dir = std::env::temp_dir().join(format!(
            "photocat-integrity-same-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity.db"));
        app.settings.lang = i18n::Lang::Ru;
        app.integrity_a = dir.to_string_lossy().to_string();
        app.integrity_b = dir.to_string_lossy().to_string();
        let err = app
            .integrity_plan(IntegrityJob::Verify)
            .err()
            .expect("identical paths are an error");
        assert!(err.contains("один и тот же путь"), "{err}");
        app.integrity_start(IntegrityJob::Verify);
        assert!(app.integrity_rx.is_none(), "the pass does not start");
    }

    /// An existing manifest is not overwritten by the first press: a confirmation comes first, and
    /// only it unlocks the start. The file stays intact until the confirmation.
    #[test]
    fn the_manifest_requires_a_confirmation_to_overwrite() {
        let dir = std::env::temp_dir().join(format!(
            "photocat-integrity-manifest-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let manifest = dir.join("list.sha256");
        std::fs::write(&manifest, b"old").unwrap();
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity.db"));
        app.settings.lang = i18n::Lang::Ru;
        app.integrity_a = dir.to_string_lossy().to_string();
        app.integrity_manifest = manifest.to_string_lossy().to_string();
        app.integrity_start(IntegrityJob::Manifest);
        assert!(app.integrity_rx.is_none(), "nothing is written without a confirmation");
        assert!(app.integrity_overwrite, "a confirmation is expected");
        assert!(app.integrity_error.contains("уже есть"), "{}", app.integrity_error);
        assert_eq!(std::fs::read(&manifest).unwrap(), b"old", "the old manifest is intact");
        // The paths are parsed on the confirmation too: the plan is ready, the thread is not started yet.
        assert!(matches!(
            app.integrity_plan(IntegrityJob::Manifest),
            Ok(IntegrityPlan::Manifest(_, _))
        ));
    }

    /// The background pass is not a stub: the thread computes sha256 and returns the report
    /// through `mpsc`, which the window frame then takes.
    #[test]
    fn the_check_runs_in_the_background_and_returns_a_report() {
        let base = std::env::temp_dir().join(format!(
            "photocat-integrity-bg-{}",
            std::process::id()
        ));
        let a = base.join("a");
        let b = base.join("b");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("x.jpg"), b"same").unwrap();
        std::fs::write(b.join("x.jpg"), b"same").unwrap();
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity.db"));
        app.integrity_a = a.to_string_lossy().to_string();
        app.integrity_b = b.to_string_lossy().to_string();
        app.integrity_start(IntegrityJob::Verify);
        let rx = app.integrity_rx.take().expect("the pass is started");
        let (job, res) = rx
            .recv_timeout(std::time::Duration::from_secs(20))
            .expect("the pass returned a result");
        assert_eq!(job, IntegrityJob::Verify);
        match res.expect("no error") {
            IntegrityOutcome::Verify { report, .. } => {
                assert!(report.identical(), "the copies are identical");
                assert_eq!(report.same, 1);
            }
            _ => panic!("not the expected report kind"),
        }
    }

    /// The finished report is visible in words: the verdict, the counters and the copy button take
    /// the text from `verify` instead of building their own.
    #[test]
    fn the_check_result_shows_on_the_screen() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-integrity.db"));
        app.settings.lang = i18n::Lang::Ru;
        app.view = View::Integrity;
        app.integrity_result = Some(IntegrityOutcome::Verify {
            a: std::path::PathBuf::from("/tmp/a"),
            b: std::path::PathBuf::from("/tmp/b"),
            report: crate::verify::Report {
                files_a: 2,
                files_b: 2,
                only_a: Vec::new(),
                only_b: Vec::new(),
                size_diff: Vec::new(),
                content_diff: Vec::new(),
                read_errors: Vec::new(),
                same: 2,
                hashed: 2,
                hashed_bytes: 20,
                unchecked: 0,
                scan_errors: 0,
                skipped_a: 0,
                skipped_b: 0,
                cancelled: false,
                seconds: 1.0,
                lang: i18n::Lang::Ru,
            },
        });
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "копии совпадают"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Скопировать отчёт"), "{texts:?}");
        assert!(
            texts.iter().any(|t| t.contains("вердикт: копии совпадают")),
            "the report prints the same wording: {texts:?}"
        );
    }

    /// M10: the term is over — there is a banner with words and two buttons, but the pass itself
    /// is not started. "Remind me later" hides the banner until the next launch.
    #[test]
    fn the_banner_reminds_about_the_check_and_does_not_start_it() {
        // The name is unique rather than "over the previous DB": the tests do not delete files.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "photocat-reminder-banner-{}-{stamp}.db",
            std::process::id()
        ));
        {
            let c = db::open(&path).unwrap();
            let s = db::Settings {
                verify_root: "/tmp/архив".into(),
                verify_manifest: "/tmp/архив.sha256".into(),
                verify_interval_days: 30,
                verify_last_at: (chrono::Utc::now() - chrono::Duration::days(31)).to_rfc3339(),
                verify_last_ok: true,
                ..Default::default()
            };
            db::save_settings(&c, &s).unwrap();
        }
        let mut app = App::new(path);
        assert!(
            matches!(app.verify_due, db::VerifyDue::Due { days_over: 1 }),
            "must be a day overdue: {:?}",
            app.verify_due
        );
        assert!(!app.settings.verify_manifest.is_empty(), "the manifest is lost");
        assert!(app.settings.verify_interval_days == 30);
        // The panel remembers its height from the previous frame: a banner added to it is visible
        // on the second frame (as the window on a 60 fps screen).
        let ctx = egui::Context::default();
        // The banner text is checked in Russian: the language is a settings field, the test sets it itself.
        app.settings.lang = i18n::Lang::Ru;
        let _ = window_frame(&mut app, &ctx);
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t == "Пора проверить архив"),
            "no banner: {texts:?}"
        );
        assert!(texts.iter().any(|t| t == "Проверить"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Напомнить позже"), "{texts:?}");
        assert!(app.integrity_rx.is_none(), "the banner does not start the pass itself");
        assert!(app.integrity_progress.is_none());
        // "Remind me later" is a session decision: the term stays the same, the banner is hidden.
        app.verify_banner_hidden = true;
        let _ = window_frame(&mut app, &ctx);
        let texts = window_frame(&mut app, &ctx);
        assert!(
            !texts.iter().any(|t| t == "Пора проверить архив"),
            "the banner is hidden until a restart: {texts:?}"
        );
    }

    /// M21, ADR 0040: the language is a settings field, not a global variable, so the test sets it
    /// itself. The shell of the window (navigation, top bar, filter) is translated completely.
    #[test]
    fn english_translates_the_window_shell() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test-i18n.db"));
        app.settings.lang = i18n::Lang::En;
        let texts = window_frame(&mut app, &egui::Context::default());
        for needle in [
            "All files", "Albums", "Settings", "LIBRARY", "FILTER", "All", "Photos", "Videos",
        ] {
            assert!(
                texts.iter().any(|t| t == needle),
                "missing “{needle}”: {texts:?}"
            );
        }
        // The content of the screens (the view heading, the chips, the card) is translated by the
        // next M21 steps: here the shell is checked — the navigation, the top bar, the filter.
        assert!(
            texts.iter().any(|t| t == "Synced"),
            "no English state word: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t == "Search"),
            "no English search button: {texts:?}"
        );
        app.settings.lang = i18n::Lang::Ru;
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "Все файлы"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Настройки"), "{texts:?}");
    }

    /// The tiles go in rows by the frame proportion: the preview fills the tile completely, the
    /// width follows the frame, the row height follows the "Tiles" slider.
    #[test]
    fn grid_rows_are_built_by_proportions() {
        use super::{frame_aspect, row_fit};
        // 10 frames of 4:3 in 700 px at a target height of 150: 4 in a row, height ~130.
        let a = [4.0 / 3.0; 10];
        let (n, h) = row_fit(&a, 700.0, 150.0, 2.0);
        assert_eq!(n, 4, "a row must hold four 4:3 frames");
        assert!((h - 130.0).abs() < 1.0, "the row height is {h}");
        // A lone panorama is not stretched above the target height (600 px of width).
        let (n, h) = row_fit(&[4.0], 700.0, 150.0, 2.0);
        assert_eq!(n, 1);
        assert_eq!(h, 150.0);
        // A panorama and an ordinary frame share a row: the row width is exactly 700 px.
        let (n, h) = row_fit(&[4.0, 4.0 / 3.0], 700.0, 150.0, 2.0);
        assert_eq!(n, 2);
        assert!((h * (4.0 + 4.0 / 3.0) + 2.0 - 700.0).abs() < 1.0, "h={h}");
        // A narrow window: one 4:3 frame in the row, the height no more than the target.
        let (n, h) = row_fit(&[4.0 / 3.0, 4.0 / 3.0], 200.0, 150.0, 2.0);
        assert_eq!(n, 1);
        assert!(h <= 150.0, "the height is {h}");
        // The proportion comes from the file size; a broken file gives 4:3.
        let mut f = file_row(1, "image", "a.jpg");
        assert!((frame_aspect(&f) - 4.0 / 3.0).abs() < 1e-6);
        f.width = 0;
        f.height = 0;
        assert!((frame_aspect(&f) - 4.0 / 3.0).abs() < 1e-6);
        f.width = 240;
        f.height = 320;
        assert!(frame_aspect(&f) < 1.0, "a portrait must be narrower than it is tall");
    }

    /// M22, ADR 0044: the two palettes of the mock-up and the choice in the settings. The theme is
    /// settings data, not a global variable, so the test sets it itself.
    #[test]
    fn the_theme_switches_the_palette_and_persists() {
        let ctx = egui::Context::default();
        super::apply_theme(&ctx, db::ThemeMode::Light);
        assert_eq!(ctx.theme(), egui::Theme::Light);
        let light = ctx.style_of(egui::Theme::Light);
        assert!(!light.visuals.dark_mode, "the light palette was not applied");
        assert_eq!(light.visuals.panel_fill, super::LIGHT.panel);
        // M22.11: the light accent is the design system's `amber-focus`, not the drawn screen's
        // `#d97706`, which reads below AA under the words it colours.
        assert_eq!(super::LIGHT.accent, egui::Color32::from_rgb(0xb4, 0x53, 0x09));
        super::apply_theme(&ctx, db::ThemeMode::Dark);
        assert_eq!(ctx.theme(), egui::Theme::Dark);
        let dark = ctx.style_of(egui::Theme::Dark);
        assert!(dark.visuals.dark_mode, "the dark palette was not applied");
        assert_eq!(dark.visuals.panel_fill, super::DARK.panel);
        // The radii of the mock-up: 4 for buttons and chips, 6 for menus, 8 for cards and dialogs.
        assert_eq!(
            dark.visuals.widgets.inactive.corner_radius,
            egui::CornerRadius::same(super::RADIUS_CTRL as u8)
        );
        assert_eq!(
            dark.visuals.menu_corner_radius,
            egui::CornerRadius::same(super::RADIUS_ITEM as u8)
        );
        assert_eq!(
            dark.visuals.window_corner_radius,
            egui::CornerRadius::same(super::RADIUS_CARD as u8)
        );
        // The theme is stored in `settings` (key/value, the schema does not change).
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "photocat-theme-{}-{stamp}.db",
            std::process::id()
        ));
        {
            let c = db::open(&path).unwrap();
            let s = db::Settings {
                theme: db::ThemeMode::Light,
                ..Default::default()
            };
            db::save_settings(&c, &s).unwrap();
        }
        let c = db::open(&path).unwrap();
        assert_eq!(db::load_settings(&c).theme, db::ThemeMode::Light);
        // A fresh install opens the dark screen of the mock-up: the choice is a change, not a state.
        assert_eq!(db::Settings::default().theme, db::ThemeMode::Dark);
    }

    /// M22.11: contrast is a number, not a taste. AA body text is 4.5:1, a dim hint 3.0:1; the
    /// accent words — a picked chip, the active row, a confident tag — are body text. The pairs
    /// below are the ones the screens actually paint: the field tone carries only body and dim
    /// words (an idle chip, a disabled row), never an accent one.
    #[test]
    fn both_themes_pass_the_contrast_ratio() {
        for (name, p) in [("dark", super::DARK), ("light", super::LIGHT)] {
            for (ground_name, ground) in [
                ("canvas", p.canvas),
                ("panel", p.panel),
                ("card", p.card),
                ("field", p.field),
            ] {
                for (what, tone, need) in [
                    ("text", p.text, 4.5),
                    ("text_med", p.text_med, 4.5),
                    ("text_dim", p.text_dim, 3.0),
                ] {
                    let ratio = contrast(tone, ground);
                    assert!(
                        ratio >= need,
                        "{name}: {what} on {ground_name} is {ratio:.2}:1, below {need}"
                    );
                }
            }
            // The accent words: a picked chip and the active row on the selection fill, a
            // confident tag and a mode value on the card, a banner word on the panel.
            for (ground_name, ground) in [
                ("canvas", p.canvas),
                ("panel", p.panel),
                ("card", p.card),
                ("selection", over(p.accent_soft, p.card)),
            ] {
                let ratio = contrast(p.accent, ground);
                assert!(
                    ratio >= 4.5,
                    "{name}: accent on {ground_name} is {ratio:.2}:1, below 4.5"
                );
            }
            // The one red word, on the grounds it is drawn on (a danger button is transparent).
            for (ground_name, ground) in [
                ("canvas", p.canvas),
                ("panel", p.panel),
                ("card", p.card),
            ] {
                let ratio = contrast(p.danger, ground);
                assert!(
                    ratio >= 4.5,
                    "{name}: danger on {ground_name} is {ratio:.2}:1, below 4.5"
                );
            }
            // A filled accent button: white on amber in light, ink on amber in dark — idle and
            // under the cursor, because the hover tone is the darker one.
            for (state, fill) in [("idle", p.accent), ("hovered", p.accent_hover)] {
                let ratio = contrast(p.on_accent, fill);
                assert!(
                    ratio >= 4.5,
                    "{name}: text on the {state} accent fill is {ratio:.2}:1, below 4.5"
                );
            }
        }
    }

    /// M22.11: the light ground is neutral — a warm canvas casts on the photograph, and a
    /// photograph is the one thing the window must not judge. The well of a tile is the field
    /// token (the mock-up's `surface-container-highest`), so the letterbox around a preview comes
    /// from the theme and never from the accent.
    #[test]
    fn the_light_ground_is_neutral_and_the_tile_well_is_the_field() {
        let p = super::LIGHT;
        for (name, tone) in [
            ("canvas", p.canvas),
            ("panel", p.panel),
            ("card", p.card),
            ("field", p.field),
            ("border", p.border),
        ] {
            let [r, g, b, _] = tone.to_srgba_unmultiplied();
            assert!(
                r <= g && g <= b,
                "the light {name} is warm: {tone:?}"
            );
        }
        // The cool `canvas-base` of the light design system, not its warm Material `background`.
        assert_eq!(
            p.canvas,
            egui::Color32::from_rgb(0xf8, 0xfa, 0xfc),
            "the light canvas is not the neutral `canvas-base`"
        );
        // The grid paints the well of every tile in that tone: the letterbox around a preview is
        // the neutral field in both themes.
        for theme in [super::DARK, super::LIGHT] {
            let mut app = calendar_app();
            app.view = View::Grid;
            let ctx = egui::Context::default();
            let mode = if theme.dark {
                db::ThemeMode::Dark
            } else {
                db::ThemeMode::Light
            };
            let parts = window_parts_in(&mut app, &ctx, egui::vec2(1240.0, 840.0), mode);
            let frames = parts
                .fills
                .iter()
                .filter(|(r, c)| *c == theme.field && r.width() > 80.0 && r.height() > 60.0)
                .count();
            assert!(
                frames >= 3,
                "the tile wells are not the field tone in this theme: {frames} of them, {:?}",
                parts.fills
            );
        }
    }

    /// M22.11: every screen is looked at in both palettes — its words, fills and lines stay the
    /// tokens of the theme in use, so the light window is not the dark screen with a white ground.
    /// The four screens without a mock-up and the seven dialogs check themselves in their own
    /// tests; here are the main screen, the calendar in its layers, the albums and the full screen.
    #[test]
    fn every_screen_paints_only_the_tokens_of_its_theme() {
        let ctx = egui::Context::default();
        for p in [super::DARK, super::LIGHT] {
            let mut app = calendar_app();
            app.view = View::Grid;
            assert_palette(&mut app, &ctx, "grid", p);
            for scale in [CalScale::Year, CalScale::Month, CalScale::Week, CalScale::Day] {
                let mut app = calendar_app();
                app.cal_scale = scale;
                app.cal_day = Some("2024-05-01".into());
                assert_palette(&mut app, &ctx, "calendar", p);
            }
            let mut app = calendar_app();
            app.view = View::Albums;
            app.albums = vec![db::AlbumNode {
                id: 1,
                parent_id: 0,
                position: 0,
                name: "Summer".into(),
                note: String::new(),
                updated_at: "1714557600".into(),
                depth: 0,
                files: 3,
                trashed: 0,
                subalbums: 0,
                bytes: 6144,
                covers: vec!["/tmp/a.jpg".into()],
            }];
            app.album_selected = Some(1);
            app.album_set = app.items.iter().map(|f| f.id).collect();
            assert_palette(&mut app, &ctx, "albums", p);
            let mut app = calendar_app();
            app.selected = Some(1);
            app.viewer = Some(viewer_with(vec![1], 0));
            assert_palette(&mut app, &ctx, "full screen", p);
        }
    }

    /// The language choice lives in the DB (`settings.lang`), not in the process memory: after a
    /// restart the window opens in the language the owner chose.
    #[test]
    fn the_interface_language_is_saved_in_the_db() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "photocat-i18n-{}-{stamp}.db",
            std::process::id()
        ));
        {
            let c = db::open(&path).unwrap();
            let s = db::Settings {
                lang: i18n::Lang::Ru,
                ..Default::default()
            };
            db::save_settings(&c, &s).unwrap();
        }
        let app = App::new(path);
        assert_eq!(app.settings.lang, i18n::Lang::Ru, "the language is lost on a restart");
    }

    /// Runs a background pass to the result through window frames: that is how the writing of the
    /// outcome into the settings is checked, not just the thread itself.
    fn poll_integrity_to_end(app: &mut App) {
        let ctx = egui::Context::default();
        for _ in 0..300 {
            let _ = window_frame(app, &ctx);
            if app.integrity_rx.is_none() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the pass did not finish within 3 s");
    }

    /// M10: a successful manifest and check write the date, the outcome and the report into the
    /// settings and into `<cache>/photocat/verify-last.txt`; the reminder does not fire after that.
    #[test]
    fn a_successful_check_writes_the_result_and_clears_the_reminder() {
        let base = std::env::temp_dir().join(format!(
            "photocat-reminder-ok-{}",
            std::process::id()
        ));
        let dir = base.join("archive");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.jpg"), b"data").unwrap();
        let manifest = base.join("list.sha256");
        let db_path = base.join("photocat.db");
        {
            let c = db::open(&db_path).unwrap();
            let s = db::Settings {
                verify_root: dir.to_string_lossy().into_owned(),
                verify_manifest: manifest.to_string_lossy().into_owned(),
                verify_interval_days: 30,
                verify_last_at: (chrono::Utc::now() - chrono::Duration::days(31)).to_rfc3339(),
                // The report and its words are checked in Russian: it is the second language of the project.
                lang: i18n::Lang::Ru,
                ..Default::default()
            };
            db::save_settings(&c, &s).unwrap();
        }
        let mut app = App::new(db_path.clone());
        assert!(matches!(app.verify_due, db::VerifyDue::Due { .. }));
        app.integrity_a = dir.to_string_lossy().into_owned();
        app.integrity_manifest = manifest.to_string_lossy().into_owned();
        // A manifest is a full pass over the archive too: it sets the reference point of the term.
        app.integrity_start(IntegrityJob::Manifest);
        poll_integrity_to_end(&mut app);
        {
            let c = db::open(&db_path).unwrap();
            let s = db::load_settings(&c);
            assert!(!s.verify_last_at.is_empty(), "the pass date is written");
            assert!(s.verify_last_ok);
            assert!(s.verify_last_report.contains("слепок"), "{}", s.verify_last_report);
        }
        assert!(
            matches!(app.verify_due, db::VerifyDue::NotYet { .. }),
            "the due date has not passed after writing a manifest: {:?}",
            app.verify_due
        );
        // A check against this manifest writes its own outcome and the same report file.
        app.integrity_start(IntegrityJob::Check);
        poll_integrity_to_end(&mut app);
        let c = db::open(&db_path).unwrap();
        let s = db::load_settings(&c);
        assert!(s.verify_last_ok);
        assert!(
            s.verify_last_report.contains("каталог:"),
            "{}",
            s.verify_last_report
        );
        let file = db::cache_dir().join("verify-last.txt");
        let text = std::fs::read_to_string(&file).unwrap();
        assert_eq!(text, s.verify_last_report, "the file and the settings hold one report");
        assert!(text.contains("совпал"), "{text}");
    }

    /// M10: a check error (a broken manifest) does not erase the previous successful report and date.
    #[test]
    fn a_failed_check_keeps_the_previous_report() {
        let base = std::env::temp_dir().join(format!(
            "photocat-reminder-error-{}",
            std::process::id()
        ));
        let dir = base.join("archive");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.jpg"), b"data").unwrap();
        let manifest = base.join("broken.sha256");
        std::fs::write(&manifest, b"not a sha256 line\n").unwrap();
        let db_path = base.join("photocat.db");
        let old_at = "2026-10-01T10:00:00+00:00";
        {
            let c = db::open(&db_path).unwrap();
            let s = db::Settings {
                verify_root: dir.to_string_lossy().into_owned(),
                verify_manifest: manifest.to_string_lossy().into_owned(),
                verify_interval_days: 30,
                verify_last_at: old_at.into(),
                verify_last_ok: true,
                verify_last_report: "старый успешный отчёт".into(),
                ..Default::default()
            };
            db::save_settings(&c, &s).unwrap();
        }
        let mut app = App::new(db_path.clone());
        app.integrity_a = dir.to_string_lossy().into_owned();
        app.integrity_manifest = manifest.to_string_lossy().into_owned();
        app.integrity_start(IntegrityJob::Check);
        let ctx = egui::Context::default();
        for _ in 0..200 {
            let _ = window_frame(&mut app, &ctx);
            if app.integrity_rx.is_none() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert!(app.integrity_rx.is_none(), "the pass has finished");
        assert!(
            !app.integrity_error.is_empty(),
            "a broken manifest is an error in the status"
        );
        let c = db::open(&db_path).unwrap();
        let s = db::load_settings(&c);
        assert_eq!(s.verify_last_report, "старый успешный отчёт");
        assert_eq!(s.verify_last_at, old_at);
        assert!(s.verify_last_ok);
    }
    fn album_node(id: i64, parent: i64, name: &str, files: i64, subs: i64) -> db::AlbumNode {
        db::AlbumNode {
            id,
            parent_id: parent,
            position: 0,
            name: name.into(),
            note: String::new(),
            updated_at: "1714557600".into(),
            depth: usize::from(parent != 0),
            files,
            trashed: 0,
            subalbums: subs,
            bytes: 2 * 1024 * 1024,
            covers: Vec::new(),
        }
    }

    /// M22.20: an album card is one model — the width from the "Tiles" slider, a 16:10 canvas
    /// cover with the mock-up's 10 px of air, the name at 13 px and the mono facts at 10 px.
    #[test]
    fn the_album_card_follows_the_tiles_slider() {
        let small = album_card_size(100.0);
        let big = album_card_size(260.0);
        assert_eq!(small.x, 100.0);
        assert_eq!(big.x, 260.0);
        // 10 px pad + a 50 px cover (80 × 10/16) + 8 + 18 (name) + 4 + 14 (facts) + 10 px pad.
        assert!((small.y - 114.0).abs() < 0.01, "{}", small.y);
        // 10 + a 150 px cover (240 × 10/16) + 8 + 18 + 4 + 14 + 10.
        assert!((big.y - 214.0).abs() < 0.01, "{}", big.y);
    }

    /// Draws one sub-album card in a frame without a window and returns the height it took: the
    /// card model is checked on the real drawing code, not on the formula alone.
    fn album_card_frame_height(tile: f32) -> f32 {
        let ctx = egui::Context::default();
        let node = album_node(2, 1, "May", 12, 0);
        let picked = BTreeSet::new();
        let mut textures = std::collections::HashMap::new();
        let mut missing = std::collections::HashSet::new();
        let mut cells = super::Cells {
            bg: egui::Color32::BLACK,
            cell_style: db::CellStyle::Tiles,
            textures: &mut textures,
            missing: &mut missing,
            thumbs_dir: std::path::Path::new("/nonexistent/thumbs"),
            selected: None,
            picked: &picked,
            clicked: None,
            open_clicked: None,
            show_keep: false,
            keep_clicked: None,
            trash_mode: false,
            restore_clicked: None,
            tile,
            lang: i18n::Lang::En,
            budget: 4,
            time_only: false,
        };
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(900.0, 400.0),
            )),
            ..Default::default()
        };
        let mut used = 0.0;
        let out = ctx.run_ui(raw, |ui| {
            let ctx = ui.ctx().clone();
            super::draw_album_cell(
                ui,
                &ctx,
                &mut cells,
                &node,
                &[],
                tile,
                false,
                i18n::Lang::En,
            );
            used = ui.min_rect().height();
        });
        out.drop_without_applying_deltas();
        used
    }

    /// M22.20: the card grows with the slider: a 114 px card at 100, a 214 px one at 260, and the
    /// drawn height is the model's.
    #[test]
    fn the_album_card_draws_at_the_model_height() {
        let small = album_card_frame_height(100.0);
        let big = album_card_frame_height(260.0);
        assert!((small - 114.0).abs() < 0.01, "a card at 100: {small}");
        assert!((big - 214.0).abs() < 0.01, "a card at 260: {big}");
    }

    /// M22.6: the cards scale with the slider, so twelve of them at 260 must not push the grid out
    /// of the window: the sub-albums scroll and the snapshots of the open album stay visible.
    #[test]
    fn many_sub_albums_keep_the_grid_visible() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.tile_size = 260.0;
        app.view = View::Albums;
        app.albums = vec![album_node(1, 0, "Summer 2024", 4, 12)];
        for i in 0..12 {
            app.albums
                .push(album_node(i as i64 + 2, 1, &format!("Month {}", i + 1), 3, 0));
        }
        app.album_selected = Some(1);
        app.album_set = app.items.iter().map(|f| f.id).collect();
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t.starts_with("may-")),
            "the snapshots of the album are pushed out by its sub-albums: {texts:?}"
        );
    }

    /// M22.6: the open album is a card header — the name at 14 px with "N files · M sub-albums ·
    /// size" next to it — and the heading of the view stays "Albums", so the name is printed once.
    #[test]
    fn the_open_album_gets_a_card_header() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        app.albums = vec![
            album_node(1, 0, "Summer 2024", 3, 1),
            album_node(2, 1, "May", 12, 0),
        ];
        app.album_selected = Some(1);
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "Albums"), "{texts:?}");
        assert!(
            !texts.iter().any(|t| t == "Album “Summer 2024”"),
            "the album name is printed twice: {texts:?}"
        );
        assert!(texts.iter().any(|t| t == "2 albums"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Summer 2024"), "{texts:?}");
        assert!(
            texts.iter().any(|t| t == "3 files · 1 sub-album · 2.0 MiB"),
            "the card header lost its facts: {texts:?}"
        );
        // The sub-album card: the name, the count of files on the cover and the facts under it —
        // the size and the date of the last change (M22.20).
        assert!(texts.iter().any(|t| t == "May"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "12 files"), "{texts:?}");
        assert!(
            texts.iter().any(|t| t.starts_with("2.0 MiB · ")),
            "the card lost its facts: {texts:?}"
        );
    }

    /// M22.20: with no album open the screen is the mock-up's grid of album cards — the cover of
    /// the first snapshot, the counters on it, the name and "size · date" under it, and no actions
    /// of an album that is not open.
    #[test]
    fn the_root_of_the_albums_is_a_grid_of_cards() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        app.albums = vec![
            album_node(1, 0, "Summer 2024", 3, 0),
            album_node(2, 0, "Dacha", 0, 2),
        ];
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "Summer 2024"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "Dacha"), "{texts:?}");
        // The counters of the mock-up on the cover: the files bottom right, the sub-albums top left.
        assert!(texts.iter().any(|t| t == "3 files"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "2 sub-albums"), "{texts:?}");
        assert!(
            texts.iter().any(|t| t.starts_with("2.0 MiB · ")),
            "the card carries the size and the date: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t == "Delete the album"),
            "the actions belong to the open album: {texts:?}"
        );
        // The heading of the view is not printed twice: "Albums" is the ribbon's, the cards are
        // named by the albums themselves.
        assert_eq!(
            texts.iter().filter(|t| t.as_str() == "Albums").count(),
            2,
            "the nav item, the card section of the file card"
        );
    }

    /// M22.20: the cards of one level are the children of that level only, filtered by a part of
    /// the name and ordered by the chosen look — the album order stays the data of the export.
    #[test]
    fn the_album_filter_and_order_keep_the_album_order_of_the_export() {
        let albums = vec![
            album_node(1, 0, "Май", 5, 0),
            album_node(2, 0, "Август", 9, 0),
            album_node(3, 1, "День 1", 2, 0),
        ];
        let names = |cards: Vec<db::AlbumNode>| {
            cards.into_iter().map(|n| n.name).collect::<Vec<_>>()
        };
        assert_eq!(
            names(super::album_cards_of(&albums, 0, "", db::AlbumSort::Position)),
            ["Май", "Август"]
        );
        assert_eq!(
            names(super::album_cards_of(&albums, 0, "АВГ", db::AlbumSort::Position)),
            ["Август"],
            "the filter ignores the case"
        );
        assert_eq!(
            names(super::album_cards_of(&albums, 0, "", db::AlbumSort::Count)),
            ["Август", "Май"]
        );
        assert_eq!(
            names(super::album_cards_of(&albums, 1, "", db::AlbumSort::Position)),
            ["День 1"],
            "a sub-album is a card of its own level"
        );
        assert!(names(super::album_cards_of(&albums, 0, "нет такого", db::AlbumSort::Position)).is_empty());
    }

    /// The date of a card comes from the unix seconds the DB keeps: a broken stamp prints nothing
    /// instead of a wrong date.
    #[test]
    fn the_album_date_is_dd_mm_yyyy_or_nothing() {
        let date = super::album_date("1714557600");
        assert_eq!(date.len(), 10, "dd.mm.yyyy: {date}");
        assert_eq!(&date[2..3], ".", "{date}");
        assert_eq!(&date[5..6], ".", "{date}");
        assert_eq!(super::album_date(""), "");
        assert_eq!(super::album_date("not a stamp"), "");
    }

    /// M22.20: "add photos to an album" is one dialog for the file card and for the selection: the
    /// open album is the target by default, a typed name makes a root album and fills it, and a
    /// repeat does not duplicate rows. Rows only — no file is created.
    #[test]
    fn the_album_picker_adds_the_photos_and_makes_a_new_album() {
        let path = std::env::temp_dir().join(format!(
            "photocat-album-pick-{}.db",
            std::process::id()
        ));
        let (album, files) = {
            let conn = db::open(&path).unwrap();
            db::insert(&conn, &file_row(0, "image", "pick-one.jpg")).unwrap();
            db::insert(&conn, &file_row(1, "image", "pick-two.jpg")).unwrap();
            let files: Vec<i64> = {
                let mut st = conn.prepare("SELECT id FROM files ORDER BY id").unwrap();
                let rows = st.query_map([], |r| r.get(0)).unwrap();
                rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
            };
            let album = db::create_album(&conn, 0, "May", "").unwrap();
            (album, files)
        };
        let mut app = App::new(path.clone());
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        app.album_open(album);
        // From the card of one photo: the open album is the target.
        app.album_pick_open(vec![files[0]]);
        assert_eq!(
            app.album_pick.as_ref().unwrap().target,
            album,
            "the open album is the target by default"
        );
        // From the selection of the grid: two photos into a new album in the same press.
        app.album_pick_open(vec![files[0], files[1]]);
        if let Some(pick) = app.album_pick.as_mut() {
            pick.new_name = "For printing".into();
        }
        app.album_pick_commit();
        assert!(app.album_pick.is_none(), "the dialog closes after the add");
        let conn = db::open(&path).unwrap();
        let made = app
            .albums
            .iter()
            .find(|n| n.name == "For printing")
            .expect("the new album is made by the same press")
            .id;
        assert_eq!(db::album_items(&conn, made).unwrap().len(), 2);
        assert!(db::album_items(&conn, album).unwrap().is_empty());
        // A repeat adds nothing twice.
        app.album_pick_open(vec![files[0]]);
        if let Some(pick) = app.album_pick.as_mut() {
            pick.target = made;
        }
        app.album_pick_commit();
        assert_eq!(db::album_items(&conn, made).unwrap().len(), 2);
        // The photos are rows only: nothing appears on disk.
        assert!(!std::path::Path::new("/tmp/pick-one.jpg").exists());
    }

    /// M22.20: the dialog itself — the facts of the selection, the tree with the paths, the field
    /// of a new album and the one accent button.
    #[test]
    fn the_album_picker_draws_the_tree_and_the_photos() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.albums = vec![
            album_node(1, 0, "Summer 2024", 3, 1),
            album_node(2, 1, "May", 12, 0),
        ];
        app.album_pick_open(vec![1, 2]);
        let ctx = egui::Context::default();
        let texts = modal_frame(&mut app, &ctx, App::album_pick_modal);
        assert!(texts.iter().any(|w| w == "Add to an album"), "{texts:?}");
        assert!(
            texts.iter().any(|w| w == "Add 2 photos"),
            "the primary button names the count: {texts:?}"
        );
        assert!(
            texts.iter().any(|w| w.starts_with("Summer 2024 / May · ")),
            "the nested album is offered with its path: {texts:?}"
        );
        assert!(texts.iter().any(|w| w == "or a new album:"), "{texts:?}");
        assert!(texts.iter().any(|w| w == "Cancel"), "{texts:?}");
    }

    /// M22.20: with albums around, the selection bar carries the way into them; without albums
    /// the word is not promised.
    #[test]
    fn the_selection_bar_offers_the_way_into_an_album() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.view = View::Grid;
        app.picked = app.items.iter().take(2).map(|f| f.id).collect();
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            !texts.iter().any(|t| t.starts_with("Add to an album…")),
            "no albums — no road into them: {texts:?}"
        );
        app.albums = vec![album_node(1, 0, "Summer 2024", 0, 0)];
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t == "Add to an album… (2)"),
            "the bar offers the way into an album: {texts:?}"
        );
    }

    /// M22.20: the control row of the albums screen — the filter, the order, the look and the main
    /// action — has to fit the content panel at the narrowest working window. A row that does not
    /// fit pushes its first word under the left panel, where nobody looks for it.
    #[test]
    fn the_album_control_row_fits_the_content_panel() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        app.albums = vec![album_node(1, 0, "Summer 2024", 3, 0)];
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let left = super::LEFT_W;
        let right = size.x - super::RIGHT_W;
        let words = window_word_rects_at(&mut app, &ctx, size);
        for name in ["Filter the albums", "+ New album", "Cards", "Sort"] {
            let found: Vec<&egui::Rect> = words
                .iter()
                .filter(|(w, _)| w == name)
                .map(|(_, r)| r)
                .collect();
            assert!(!found.is_empty(), "the control “{name}” is on screen: {words:?}");
            for r in found {
                assert!(
                    r.left() >= left - 0.5 && r.right() <= right + 0.5,
                    "“{name}” leaves the content panel: {r:?} in [{left}, {right}]"
                );
            }
        }
    }

    /// M22.23: the "Cards / List" switch of the Albums control row is the one look switch of the
    /// screen. Inside an album it changes the photos too — the owner pressed it and the snapshots
    /// stayed tiles — and it leaves the global cell view of the other screens as it was.
    #[test]
    fn the_album_look_switch_changes_the_photos() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "photocat-album-look-{}-{stamp}.db",
            std::process::id()
        ));
        let album;
        {
            let conn = db::open(&path).unwrap();
            let mut ids = Vec::new();
            for i in 0..2 {
                let name = format!("{i}.jpg");
                db::insert(&conn, &file_row(0, "image", &name)).unwrap();
                ids.push(
                    conn.query_row(
                        "SELECT id FROM files WHERE path=?1",
                        [format!("/tmp/{name}")],
                        |r| r.get::<_, i64>(0),
                    )
                    .unwrap(),
                );
            }
            album = db::create_album(&conn, 0, "Summer 2024", "").unwrap();
            db::add_to_album(&conn, album, &ids).unwrap();
        }
        let mut app = App::new(path);
        app.settings.lang = i18n::Lang::Ru;
        // The global view of the other screens stays what the owner chose for them.
        app.settings.cell_style = db::CellStyle::Rows;
        app.view = View::Albums;
        app.album_open(album);
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame(&mut app, &ctx);
        assert!(
            !app.settings.album_list,
            "the album opens on cards: {:?}",
            app.settings.album_list
        );
        assert_eq!(app.grid_cell_style(), db::CellStyle::Tiles, "the photos are tiles");
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t == "Все файлы"),
            "the album switch speaks Russian: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t == "All files"),
            "no English “All files” in the Russian window: {texts:?}"
        );
        let at = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, r)| w == "Список" && r.center().y > 100.0)
            .expect("the List button of the album control row is on screen")
            .1;
        click_at(&mut app, &ctx, at.center());
        assert!(app.settings.album_list, "List is chosen by the click");
        assert_eq!(
            app.grid_cell_style(),
            db::CellStyle::List,
            "the photos of the album follow the switch"
        );
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t.contains("0.jpg")),
            "the snapshots are drawn: {texts:?}"
        );
        let at = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, r)| w == "Плитки" && r.center().y > 100.0)
            .expect("the Cards button is on screen")
            .1;
        click_at(&mut app, &ctx, at.center());
        assert!(!app.settings.album_list, "Cards is chosen by the click");
        assert_eq!(app.grid_cell_style(), db::CellStyle::Tiles, "the photos are tiles again");
        assert_eq!(
            app.settings.cell_style,
            db::CellStyle::Rows,
            "the album switch does not rewrite the global cell view"
        );
    }

    /// M22.23: the Albums screen has one look switch. The ribbon's "Tile / Rows / List" is hidden
    /// there — otherwise two "List" buttons stand above each other and the upper one does nothing.
    #[test]
    fn the_albums_view_holds_one_look_switch() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "photocat-album-ribbon-{}-{stamp}.db",
            std::process::id()
        ));
        let root;
        {
            let conn = db::open(&path).unwrap();
            root = db::create_album(&conn, 0, "Summer 2024", "").unwrap();
            db::create_album(&conn, root, "May", "").unwrap();
        }
        let mut app = App::new(path);
        app.settings.lang = i18n::Lang::Ru;
        app.view = View::Albums;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let check = |app: &mut App, where_: &str| {
            let _ = window_frame(app, &ctx);
            let words = window_word_rects_at(app, &ctx, size);
            let lists: Vec<&egui::Rect> = words
                .iter()
                .filter(|(w, _)| w == "Список")
                .map(|(_, r)| r)
                .collect();
            assert_eq!(
                lists.len(),
                1,
                "{where_}: one List, the album control row's: {words:?}"
            );
            assert!(
                lists[0].center().y > 100.0,
                "{where_}: the List is the album look switch: {:?}",
                lists[0]
            );
            assert!(
                words
                    .iter()
                    .any(|(w, r)| w == "Плитки" && r.center().y > 100.0),
                "{where_}: the Cards button of the album row is drawn: {words:?}"
            );
            for name in ["Плитка", "Ряды"] {
                assert!(
                    !words.iter().any(|(w, _)| w == name),
                    "{where_}: the ribbon's “{name}” is hidden: {words:?}"
                );
            }
        };
        check(&mut app, "the root of the albums");
        app.album_open(root);
        check(&mut app, "inside an album");
    }

    /// M22.24: an album tree row carries a folder glyph, and the whole row — the glyph too — opens
    /// the album: the icon is not a dead spot next to the text.
    #[test]
    fn a_click_on_the_folder_glyph_of_a_tree_row_opens_the_album() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "photocat-album-glyph-{}-{stamp}.db",
            std::process::id()
        ));
        let child;
        {
            let conn = db::open(&path).unwrap();
            let root = db::create_album(&conn, 0, "Summer 2024", "").unwrap();
            child = db::create_album(&conn, root, "May", "").unwrap();
        }
        let mut app = App::new(path);
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        app.album_open(app.albums.iter().find(|n| n.parent_id == 0).map(|n| n.id).unwrap());
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame(&mut app, &ctx);
        let at = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .filter(|(w, _)| w == "May")
            .min_by(|a, b| a.1.top().total_cmp(&b.1.top()))
            .expect("the tree row of the sub-album is on screen")
            .1;
        // The glyph sits 12 px left of the name; the row, not only the word, answers the click.
        click_at(&mut app, &ctx, egui::pos2(at.left() - 12.0, at.center().y));
        assert_eq!(app.album_selected, Some(child), "the folder glyph opens the album");
    }

    /// M22.20: the breadcrumb is the way up from an open album — one press per level, and the
    /// root is the card grid again.
    #[test]
    fn the_breadcrumb_goes_back_to_the_root_of_the_albums() {
        let path = std::env::temp_dir().join(format!(
            "photocat-album-breadcrumb-{}.db",
            std::process::id()
        ));
        let child;
        {
            let conn = db::open(&path).unwrap();
            let root = db::create_album(&conn, 0, "Summer 2024", "").unwrap();
            child = db::create_album(&conn, root, "May", "").unwrap();
        }
        let mut app = App::new(path);
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        app.album_open(child);
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame(&mut app, &ctx);
        let r = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "All albums")
            .expect("the breadcrumb is on screen")
            .1;
        click_at(&mut app, &ctx, r.center());
        assert_eq!(app.album_selected, None, "the root of the albums");
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t == "Summer 2024"),
            "the root is the card grid of the top level: {texts:?}"
        );
    }

    /// M22.21: the root of the albums is the card grid, so a card there must open its album. The
    /// action used to be dropped at the root, where no album was open yet.
    #[test]
    fn a_click_on_an_album_card_opens_it_from_the_root() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "photocat-album-open-{}-{stamp}.db",
            std::process::id()
        ));
        let root;
        {
            let conn = db::open(&path).unwrap();
            root = db::create_album(&conn, 0, "Summer 2024", "").unwrap();
            db::create_album(&conn, root, "May", "").unwrap();
        }
        let mut app = App::new(path);
        app.settings.lang = i18n::Lang::En;
        app.view = View::Albums;
        assert_eq!(app.album_selected, None, "the screen starts at the root");
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let _ = window_frame(&mut app, &ctx);
        let at = window_word_rects_at(&mut app, &ctx, size)
            .into_iter()
            .find(|(w, _)| w == "Summer 2024")
            .expect("the album card is on screen")
            .1;
        click_at(&mut app, &ctx, at.center());
        assert_eq!(app.album_selected, Some(root), "the card opens the album");
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t == "May"),
            "the sub-albums of the opened album are shown: {texts:?}"
        );
    }

    /// M22.21: an album cover is a mosaic of at most four photos: one fills the frame, two split
    /// it into columns, three give the first a half and the others the right stack, four make a
    /// 2×2 grid. The cells never overlap, so no photo hides another.
    #[test]
    fn the_album_cover_is_a_mosaic_of_at_most_four_photos() {
        let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 100.0));
        assert!(super::cover_cells(rect, 0).is_empty());
        assert_eq!(super::cover_cells(rect, 1), vec![rect]);
        assert_eq!(super::cover_cells(rect, 2).len(), 2);
        assert_eq!(super::cover_cells(rect, 3).len(), 3);
        let four = super::cover_cells(rect, 4);
        assert_eq!(four.len(), 4);
        assert_eq!(
            super::cover_cells(rect, 9).len(),
            4,
            "four photos are the maximum, however many the album has"
        );
        for cell in &four {
            assert!(
                cell.left() >= rect.left() - 0.01
                    && cell.top() >= rect.top() - 0.01
                    && cell.right() <= rect.right() + 0.01
                    && cell.bottom() <= rect.bottom() + 0.01,
                "the cell {cell:?} leaves the cover {rect:?}"
            );
        }
        assert!((four[0].width() - (rect.width() - 2.0) / 2.0).abs() < 0.01);
        assert!(!four[0].intersects(four[1]), "the columns overlap");
        assert!(!four[0].intersects(four[2]), "the rows overlap");
    }

    /// M22.21: the mosaic really paints four previews — a larger album is cut to four, not to one.
    #[test]
    fn the_album_cover_paints_four_previews() {
        let dir = std::env::temp_dir().join(format!(
            "photocat-album-mosaic-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let mut rows = Vec::new();
        for i in 0..6 {
            let name = format!("p{i}.png");
            image::RgbImage::from_pixel(160, 120, image::Rgb([40 * i as u8, 120, 60]))
                .save(dir.join(&name))
                .unwrap();
            let mut f = file_row(i as i64 + 1, "image", &name);
            f.thumb = Some(name);
            rows.push(f);
        }
        let covers: Vec<&db::FileRow> = rows.iter().collect();
        let mut textures = std::collections::HashMap::new();
        let mut missing = std::collections::HashSet::new();
        let picked = BTreeSet::new();
        let ctx = egui::Context::default();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(400.0, 400.0),
            )),
            ..Default::default()
        };
        let out = ctx.run_ui(raw, |ui| {
            let mut cs = super::Cells {
                bg: egui::Color32::BLACK,
                cell_style: db::CellStyle::Tiles,
                time_only: false,
                textures: &mut textures,
                missing: &mut missing,
                thumbs_dir: &dir,
                selected: None,
                picked: &picked,
                clicked: None,
                open_clicked: None,
                show_keep: false,
                keep_clicked: None,
                trash_mode: false,
                restore_clicked: None,
                tile: 150.0,
                lang: i18n::Lang::En,
                budget: 8,
            };
            let cover = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 100.0));
            let drawn =
                super::paint_cover_mosaic(ui, &ctx, &mut cs, &covers, cover, egui::Color32::BLACK);
            assert_eq!(drawn, 4, "four previews of the six are drawn");
        });
        let mut meshes = Vec::new();
        for c in &out.shapes {
            collect_meshes(&c.shape, &mut meshes);
        }
        out.drop_without_applying_deltas();
        assert_eq!(meshes.len(), 4, "the cover is a mosaic of four pictures: {meshes:?}");
        // M22.29: a cell has no margins — every preview covers its cell and is cut on the sides
        // that do not fit, so the `canvas` shows only through the seams.
        let cover = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(160.0, 100.0));
        for (i, (mesh, cell)) in meshes
            .iter()
            .zip(super::cover_cells(cover, 4))
            .enumerate()
        {
            assert!(
                mesh.left() <= cell.left() + 0.01
                    && mesh.top() <= cell.top() + 0.01
                    && mesh.right() >= cell.right() - 0.01
                    && mesh.bottom() >= cell.bottom() - 0.01,
                "the preview {i} leaves a margin in its cell: {mesh:?} against {cell:?}"
            );
        }
    }

    /// M22.20: the album list row grows with the "Tiles" slider, like the file list.
    #[test]
    fn the_album_list_row_scales_with_the_slider() {
        assert!((super::album_row_height(100.0) - 38.0).abs() < 0.01);
        assert!((super::album_row_height(260.0) - 57.0).abs() < 0.01);
        // The clamps keep a row legible at both ends of the slider.
        assert!((super::album_row_height(20.0) - 36.0).abs() < 0.01);
        assert!((super::album_row_height(600.0) - 58.0).abs() < 0.01);
    }

    /// M22.20: the file card carries the mock-up's dashed "+ Add" even when the photo is in no
    /// album yet — otherwise the way into an album would be invisible.
    #[test]
    fn the_file_card_carries_the_dashed_add_to_an_album() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.selected = Some(app.items[0].id);
        app.file_albums_for = Some(app.items[0].id);
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "Albums"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "+ Add"), "{texts:?}");
    }

    /// The inventory of the icon set: every glyph paints inside its box. `Icon` has no `ALL` in the
    /// production code (the compiler would warn it is never read), so the list lives here and the
    /// test below compares it with the variants of the enum in the source: a new glyph cannot be
    /// added without being drawn by a test (M22.8).
    fn all_icons() -> Vec<super::Icon> {
        use super::Icon::*;
        vec![
            Search, Close, Settings, DockToRight, Folder, FolderSpecial, CreateNewFolder, Sync,
            Refresh, PhotoLibrary, Photo, Videocam, CollectionsBookmark, CalendarMonth,
            CalendarToday, Face, BurstMode, Verified, Apps, GridView, ViewAgenda, ViewList, Sort, Add,
            Remove, Edit, MoveTo, ContentCopy, Download, Upload, Check, CheckCircle, Pick, NotHim,
            Delete,
            Restore, Stop, ArrowUp, ArrowDown, ArrowBack, ChevronLeft, ChevronRight, ZoomIn,
            ZoomOut, FitScreen, OneToOne, Rotate, OpenInNew, Info, Location, Slideshow,
            TextFields,
            Preview, CompareArrows, Description, Psychology, Label, Inventory, Terminal,
            AutoAwesome, Calculate, Cleaning, Prev, Next, Play, Pause, Volume,
        ]
    }

    /// M22.8: every icon of the set is geometry, and the geometry stays inside the 2s box of the
    /// glyph. An empty arm would leave a button with a hole where the mock-up has a sign.
    #[test]
    fn every_icon_paints_inside_its_box() {
        let ctx = egui::Context::default();
        let centre = egui::pos2(100.0, 100.0);
        for icon in all_icons() {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(200.0, 200.0),
                )),
                ..Default::default()
            };
            let out = ctx.run_ui(raw, |ui| {
                super::paint_icon(ui.painter(), centre, 6.0, icon, egui::Color32::WHITE);
            });
            let mut bounds = None;
            for c in &out.shapes {
                collect_bounds(&c.shape, &mut bounds);
            }
            out.drop_without_applying_deltas();
            let bounds = bounds.unwrap_or_else(|| panic!("{icon:?} paints nothing"));
            let glyph_box =
                egui::Rect::from_center_size(centre, egui::vec2(14.0, 14.0)).expand(1.0);
            assert!(
                glyph_box.contains_rect(bounds),
                "{icon:?} leaves its box: {bounds:?} outside {glyph_box:?}"
            );
        }
    }

    /// The list above is the whole enum: a variant added to `Icon` without a line here fails, so
    /// nobody can add an icon that no test ever paints.
    #[test]
    fn the_icon_inventory_covers_the_enum() {
        let src = include_str!("app.rs");
        let head = src.find("enum Icon {").expect("the Icon enum is gone");
        let body = &src[head + "enum Icon {".len()..];
        let body = &body[..body.find("\n}").expect("an unterminated enum")];
        let mut declared = Vec::new();
        for line in body.lines() {
            let word = line.trim().trim_end_matches(',');
            if word.is_empty() || word.starts_with("//") || !word.starts_with(|c: char| c.is_ascii_uppercase()) {
                continue;
            }
            declared.push(word.to_string());
        }
        let mut drawn: Vec<String> = all_icons().iter().map(|i| format!("{i:?}")).collect();
        declared.sort();
        drawn.sort();
        assert_eq!(declared, drawn, "the icon inventory and the enum disagree");
    }

    /// M22.8: a button keeps its word and puts the icon before it — the icon never replaces the
    /// word, and the word is still drawn by the same call, so every frame test keeps reading it.
    #[test]
    fn a_word_button_draws_the_icon_before_the_word() {
        let ctx = egui::Context::default();
        super::style(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(300.0, 120.0),
            )),
            ..Default::default()
        };
        let mut rect = egui::Rect::NOTHING;
        let out = ctx.run_ui(raw, |ui| {
            rect = ui
                .add(super::WordButton::new(super::Icon::Search, "Find"))
                .rect;
        });
        // The icon is every shape of the frame except the button's own background rect.
        fn icon_bounds(shape: &egui::Shape, frame: egui::Rect, out: &mut Option<egui::Rect>) {
            match shape {
                egui::Shape::Rect(r) if r.rect == frame => {}
                egui::Shape::Vec(v) => {
                    for s in v {
                        icon_bounds(s, frame, out);
                    }
                }
                s => collect_bounds(s, out),
            }
        }
        let mut texts = Vec::new();
        let mut icon = None;
        for c in &out.shapes {
            collect_text_pos(&c.shape, &mut texts);
            icon_bounds(&c.shape, rect, &mut icon);
        }
        out.drop_without_applying_deltas();
        let (word, pos) = texts
            .iter()
            .find(|(text, _)| text == "Find")
            .expect("the word of the button is not drawn");
        assert!(!word.is_empty());
        let icon = icon.expect("no icon next to the word");
        assert!(
            icon.left() >= rect.left() - 0.5 && icon.right() <= pos.x,
            "the icon is not left of the word: {icon:?} vs {pos:?}"
        );
        assert!(
            rect.width() > 40.0,
            "a button with an icon is wider than its word: {:?}",
            rect.width()
        );
        assert!(
            rect.contains_rect(icon),
            "the icon left the button: {icon:?} outside {rect:?}"
        );
    }

    /// M22.8: the section list of the settings dialog is a caps line over the sections, each with
    /// its icon, and the active row carries the 2 px accent edge of the mock-up.
    #[test]
    fn the_settings_sections_are_caps_with_icons_and_an_accent_edge() {
        let mut app = calendar_app();
        app.settings_open = true;
        app.settings_tab = SettingsTab::Interface;
        let ctx = egui::Context::default();
        // A modal does not know its size on the first frame, so the dialog is drawn twice.
        let _ = window_frame(&mut app, &ctx);
        let texts = window_frame(&mut app, &ctx);
        assert!(
            texts.iter().any(|t| t == "SETTINGS SECTIONS"),
            "no caps heading over the section list: {texts:?}"
        );
        for section in ["Interface", "AI model", "Storage", "External tools"] {
            assert!(
                texts.iter().any(|t| t == section),
                "section {section} is missing: {texts:?}"
            );
        }
        let _ = window_fills(&mut app, &ctx);
        let fills = window_fills(&mut app, &ctx);
        // The modal fades in, so the edge arrives with a multiplied alpha: the hue is what the
        // accent is, the alpha is the animation of the dialog.
        let accent = super::DARK.accent.to_srgba_unmultiplied();
        let same_hue = |c: egui::Color32| {
            let u = c.to_srgba_unmultiplied();
            (0..3).all(|i| (u[i] as i16 - accent[i] as i16).abs() <= 2)
        };
        // The left panel keeps an edge of its own on the active view (M22.11): it sits at the
        // panel's padding, so the section list of the dialog is found to the right of x = 100.
        let mut edges: Vec<egui::Rect> = fills
            .iter()
            .filter(|(r, c)| {
                (r.width() - 2.0).abs() < 0.01
                    && (r.height() - super::NAV_H).abs() < 0.01
                    && r.left() > 100.0
                    && same_hue(*c)
            })
            .map(|(r, _)| *r)
            .collect();
        edges.dedup();
        assert_eq!(edges.len(), 1, "the accent edge of the active section: {edges:?}");
        assert!(
            edges[0].left() > 200.0,
            "the edge is outside the section list: {:?}",
            edges[0]
        );
    }

    /// M22.8: every action of the window goes through `WordButton`, so every button carries an
    /// icon next to its word. The exceptions are named with their reason: a 20 px target inside a
    /// tile or a row cannot hold both, and the "×" of a chip is part of the chip.
    #[test]
    fn every_button_of_the_window_has_an_icon() {
        let src = include_str!("app.rs");
        // Only the production code: the test module constructs widgets of its own.
        let production = &src[..src.find("mod tests {").expect("no test module")];
        const ALLOWED: &[&str] = &[
            // The "×" of a filter chip is part of the chip word, not a button of its own.
            "format!(\"{text} ×\")",
            // The collapse triangle of the album tree: a glyph inside a 20 px row.
            "if open { \"▾\" } else { \"▸\" }",
            // "×" removes a row from an album: a 20 px target in the inspector.
            ".small_button(\"×\")",
            // Per-tile actions are placed into a fixed 20 px rect (`ui.place`): a word and an
            // icon do not fit there, so the word stays alone (the per-tile targets — the two of
            // M22.26 and the trash's restore of M22.27 — are icon-only circles drawn by
            // `tile_icon_target`, not buttons of egui).
            "t(cs.lang, \"keep\")",
        ];
        let mut offenders = Vec::new();
        for marker in [
            "egui::Button::new(",
            // A chained `.button(` is a button too; the old scan looked for `ui.button(` only and
            // missed "Open full screen", "Check now" and four more (M22.13).
            ".button(",
            ".small_button(",
            // The one menu of the window is a `WordButton` over `Popup::menu`; a plain
            // `menu_button` carries words only, which the mock-up does not do (M22.13).
            "ui.menu_button(",
            "primary_button(",
            "danger_button(",
        ] {
            let mut rest = production;
            while let Some(i) = rest.find(marker) {
                let line_start = rest[..i].rfind('\n').map_or(0, |n| n + 1);
                let line = &rest[line_start..];
                let line = &line[..line.find('\n').unwrap_or(line.len())];
                rest = &rest[i + marker.len()..];
                if line.trim_start().starts_with("//") {
                    continue;
                }
                let near = format!("{marker}{}", &rest[..rest.len().min(120)]);
                if !ALLOWED.iter().any(|a| near.contains(a)) {
                    offenders.push(line.to_string());
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "a button without an icon: {}",
            offenders.join(" | ")
        );
    }

    /// The settings dialog on the real DB: sixteen 1240×840 frames with the section list, the
    /// icons and the mono values. The numbers go to `docs/ARCHITECTURE.md` § Measurements (M22.8).
    /// `PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored settings_measurement --nocapture`.
    #[test]
    #[ignore]
    fn settings_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        let mut app = App::new(std::path::PathBuf::from(path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        app.settings_open = true;
        for section in SettingsTab::ALL {
            app.settings_tab = section;
            let ctx = egui::Context::default();
            super::style(&ctx);
            let mut frame = eframe::Frame::_new_kittest();
            let size = egui::vec2(1240.0, 840.0);
            let mut drive = |app: &mut App, t: f64| -> f32 {
                let raw = egui::RawInput {
                    time: Some(t),
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                };
                let start = Instant::now();
                let out = ctx.run_ui(raw, |ui| eframe::App::ui(app, ui, &mut frame));
                let ms = start.elapsed().as_secs_f32() * 1000.0;
                out.drop_without_applying_deltas();
                ms
            };
            let mut t = 0.0_f64;
            // A modal knows its size on the second frame: three warm-up frames.
            for _ in 0..3 {
                drive(&mut app, t);
                t += 1.0 / 60.0;
            }
            let mut times = Vec::new();
            for _ in 0..16 {
                times.push(drive(&mut app, t));
                t += 1.0 / 60.0;
            }
            times.sort_by(|a, b| a.total_cmp(b));
            let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
            println!(
                "corpus {n} files, settings «{}»: p50 {:.2} ms ({:.0} fps), p95 {:.2} ms, p99 {:.2} ms",
                section.label(app.settings.lang),
                p(0.50),
                1000.0 / p(0.50),
                p(0.95),
                p(0.99)
            );
        }
        // The list keeps its width and the pane its 520 px: the dialog is 760×620 as the spec says.
        app.settings_tab = SettingsTab::Interface;
        let ctx = egui::Context::default();
        // A modal knows its size only on the second frame.
        let _ = window_layout(&mut app, &ctx, egui::vec2(1240.0, 840.0));
        let _ = window_layout(&mut app, &ctx, egui::vec2(1240.0, 840.0));
        let drawn = window_layout(&mut app, &ctx, egui::vec2(1240.0, 840.0));
        let caps = i18n::t(app.settings.lang, "Settings sections").to_uppercase();
        let heading = drawn
            .iter()
            .find(|(text, _)| *text == caps)
            .map(|(_, pos)| *pos);
        println!("section list heading at {heading:?}");
    }
    /// M22.8: the section list of the settings dialog is translated in both languages, and the mono
    /// value of a section never touches its word: the widest label of the list makes the value be
    /// dropped when it does not fit.
    #[test]
    fn the_settings_section_list_holds_both_languages() {
        let ctx = egui::Context::default();
        for lang in i18n::Lang::ALL {
            let mut app = calendar_app();
            app.settings.lang = lang;
            app.settings_open = true;
            app.settings_tab = SettingsTab::Interface;
            let _ = window_frame(&mut app, &ctx);
            let drawn = window_layout(&mut app, &ctx, egui::vec2(1240.0, 840.0));
            let words: Vec<&str> = drawn.iter().map(|(text, _)| text.as_str()).collect();
            for section in SettingsTab::ALL {
                let label = section.label(lang);
                // The list starts at the left edge of the 760 px dialog and the icon takes 30 px,
                // so a row's word lies past x 240; the same word in the left panel does not count.
                let pos = drawn
                    .iter()
                    .find(|(text, pos)| text == label && pos.x > 240.0)
                    .map(|(_, pos)| *pos)
                    .unwrap_or_else(|| panic!("section {label} is missing from the list: {words:?}"));
                assert!(
                    pos.x < 440.0,
                    "the word of {label} left the section list: {pos:?}"
                );
            }
            assert!(
                drawn
                    .iter()
                    .any(|(text, _)| *text == i18n::t(lang, "Settings sections").to_uppercase()),
                "the caps heading is missing under {lang:?}"
            );
        }
    }
    /// M22.19: the title, its facts and the controls of the view share one 44 px ribbon, as in
    /// `photocat_1`; the tile size moved into the top strip, so nothing is drawn over the title.
    #[test]
    fn the_content_controls_keep_their_own_row() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::En;
        app.view = View::Grid;
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        let drawn = window_layout(&mut app, &ctx, size);
        let at = |word: &str| -> egui::Pos2 {
            drawn
                .iter()
                .find(|(text, _)| text == word)
                .map(|(_, pos)| *pos)
                .unwrap_or_else(|| panic!("“{word}” is not drawn"))
        };
        let title = drawn
            .iter()
            .find(|(text, pos)| text == "All files" && pos.x > super::LEFT_W)
            .map(|(_, pos)| *pos)
            .expect("the view title is not drawn");
        let tile = at("Tile");
        let rows = at("Rows");
        let list = at("List");
        let sort = at("Sort");
        let months = at("By months");
        // The ribbon is one row: the title and every control stand on the same line.
        for word in [tile, rows, list, sort, months] {
            assert!(
                (word.y - title.y).abs() <= 6.0,
                "the ribbon is not one row: {title:?} vs {word:?}"
            );
        }
        // Nothing reaches into the right panel (it starts at 1240 − 280), and the controls stay
        // right of the title.
        let widest = [tile, rows, list, sort, months]
            .iter()
            .map(|p| p.x)
            .fold(0.0f32, f32::max);
        assert!(
            widest < 950.0,
            "the controls run into the details panel: the last one starts at {widest}"
        );
        assert!(
            tile.x > title.x,
            "the controls start left of the title: {tile:?} vs {title:?}"
        );
        // The tile size moved to the top strip: the heading keeps the name and the facts.
        assert!(
            !drawn.iter().any(|(text, _)| text.starts_with("Tiles ")),
            "the size label is back in the content panel: {drawn:?}"
        );
    }

    /// An app with an album tree of two nodes, a root with two indexed files and one file outside
    /// it: the dialogs need albums, counted folders and a trash row (M22.10).
    fn dialog_app(name: &str) -> App {
        // A fresh DB per run: an album is unique among its siblings, and a test may not delete a
        // file (check.sh), so the name of the file carries the process and the moment.
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!(
            "photocat-dialogs-{}-{stamp}-{name}.db",
            std::process::id()
        ));
        let root = format!("/tmp/photocat-dialogs-archive-{stamp}-{name}");
        {
            let conn = db::open(&path).expect("a temporary DB");
            let mut a = file_row(0, "image", "a.jpg");
            a.path = format!("{root}/2024/a.jpg");
            let mut b = file_row(0, "image", "b.jpg");
            b.path = format!("{root}/2024/b.jpg");
            let mut outside = file_row(0, "image", "c.jpg");
            outside.path = "/tmp/photocat-dialogs-elsewhere/c.jpg".into();
            for row in [&a, &b, &outside] {
                db::insert(&conn, row).expect("a file row");
            }
            let id_of = |p: &str| -> i64 {
                conn.query_row("SELECT id FROM files WHERE path=?1", [p], |r| r.get(0))
                    .expect("a file id")
            };
            let (first, second) = (id_of(&a.path), id_of(&b.path));
            let parent = db::create_album(&conn, 0, "Dacha 2024", "").expect("an album");
            let child = db::create_album(&conn, parent, "May", "").expect("an album");
            db::add_to_album(&conn, parent, &[first]).expect("an album row");
            db::add_to_album(&conn, child, &[second]).expect("an album row");
        }
        let mut app = App::new(path);
        app.settings.lang = i18n::Lang::En;
        app.roots = vec![root.clone()];
        app.settings.roots = vec![root];
        app.refresh_root_counts();
        app
    }

    /// M22.10: every dialog of the window is a card — the `card` tone, a 1 px `border` line, the
    /// 8 px radius and its own air inside — with one accent button at most and no 16 px title: the
    /// type scale stops at 14. The trash confirmation is the one dialog whose button touches the
    /// disk, so it wears `danger` and no accent at all; the settings dialog has no primary action,
    /// because every change is saved at once.
    #[test]
    fn every_dialog_is_a_card_with_one_accent_button() {
        let ctx = egui::Context::default();
        let radius = egui::CornerRadius::same(super::RADIUS_CARD as u8);
        // Both themes: a dialog is a card in each of them, and the red of the OS-trash path has to
        // stay a token of the theme it is drawn in (M22.11).
        for (mode, th) in [
            (db::ThemeMode::Dark, super::DARK),
            (db::ThemeMode::Light, super::LIGHT),
        ] {
            let allowed = palette(th);
            for which in [
                Dialog::Roots,
                Dialog::Picker,
                Dialog::Storage,
                Dialog::Album,
                Dialog::Export,
                Dialog::Trash,
                Dialog::Settings,
            ] {
                let mut app = dialog_app("cards");
                app.settings.theme = mode;
                let draw = open_dialog(&mut app, which);
                let parts = dialog_parts(&mut app, &ctx, draw);
                // The frame of the dialog: the card tone, a 1 px line, the 8 px radius.
                let card = parts
                    .rects
                    .iter()
                    .filter(|(_, fill, stroke, r)| {
                        *fill == th.card && stroke.color == th.border && *r == radius
                    })
                    .max_by(|a, b| a.0.width().total_cmp(&b.0.width()))
                    .cloned();
                let card = card.unwrap_or_else(|| {
                    panic!("{which:?}: the dialog is not a card: {:?}", parts.rects)
                });
                assert!(
                    card.0.width() > 400.0 && card.0.height() > 120.0,
                    "{which:?}: the card frame is too small to be the dialog: {:?}",
                    card.0
                );
                // One accent button: a pressed target at least 20 px high. A 2 px accent edge of a
                // picked row is a fill too, so the height and the width both have to be a button's.
                let accents: Vec<egui::Rect> = parts
                    .fills
                    .iter()
                    .filter(|(r, c)| {
                        *c == th.accent && r.width() > 20.0 && r.height() >= 20.0
                    })
                    .map(|(r, _)| *r)
                    .collect();
                if matches!(which, Dialog::Trash) {
                    assert!(
                        accents.is_empty(),
                        "{which:?}: the path to the OS trash must not wear the accent: {accents:?}"
                    );
                    assert!(
                        parts
                            .rects
                            .iter()
                            .any(|(_, _, s, _)| s.color == th.danger && s.width > 0.5),
                        "{which:?}: the danger border is missing: {:?}",
                        parts.rects
                    );
                } else if which == Dialog::Settings {
                    // Every change is saved at once: the settings dialog has no primary action.
                    assert!(
                        accents.is_empty(),
                        "{which:?}: the settings dialog has no main action: {accents:?}"
                    );
                } else {
                    assert_eq!(
                        accents.len(),
                        1,
                        "{which:?}: the dialog has {} accent buttons: {accents:?}",
                        accents.len()
                    );
                }
                // The type scale has no 16: a dialog's title is a 14 px subheading.
                assert!(
                    !parts.texts.iter().any(|(_, f, _)| f.size == 16.0),
                    "{which:?}: a 16 px word is outside the type scale: {:?}",
                    parts
                        .texts
                        .iter()
                        .filter(|(_, f, _)| f.size == 16.0)
                        .map(|(t, _, _)| t.clone())
                        .collect::<Vec<_>>()
                );
                // Nothing here is a colour of its own: every word, fill and line is a token.
                let ok = |c: &egui::Color32, what: &str| {
                    assert!(
                        allowed.iter().any(|p| p == c) || c.a() < 255,
                        "{which:?} paints {what} off the palette: {c:?}"
                    );
                };
                for (text, _, color) in &parts.texts {
                    ok(color, &format!("“{text}”"));
                }
                for (_, color) in &parts.fills {
                    ok(color, "a fill");
                }
                for (_, _, stroke, _) in &parts.rects {
                    if *stroke != egui::Stroke::NONE {
                        ok(&stroke.color, "a line");
                    }
                }
            }
        }
    }

    /// M22.10: the numbers of a dialog are in the numeric family — the files of a counted folder,
    /// the volume of the trash, the albums and rows of a delete, the export's progress and the
    /// report it wrote. A number inside a translated sentence cannot be turned monospace, so the
    /// dialogs carry a facts row of their own, exactly like the album header (M22.6).
    #[test]
    fn the_dialogs_put_their_numbers_in_the_numeric_family() {
        let ctx = egui::Context::default();
        let mono11 = egui::FontId::monospace(11.0);
        let find = |parts: &DialogParts, word: &str| -> (String, egui::FontId, egui::Color32) {
            parts
                .texts
                .iter()
                .find(|(t, _, _)| t == word)
                .cloned()
                .unwrap_or_else(|| panic!("no “{word}”: {:?}", parts.texts))
        };

        // The folder list: how many indexed files lie in each root.
        let mut app = dialog_app("numbers-roots");
        let draw = open_dialog(&mut app, Dialog::Roots);
        let parts = dialog_parts(&mut app, &ctx, draw);
        let (_, font, color) = find(&parts, "2 files");
        assert_eq!(font, mono11, "the count of a folder is mono 11");
        assert_eq!(color, super::DARK.text_med);

        // The album delete: the names stay words, the counters are a row of their own.
        let mut app = dialog_app("numbers-album");
        let draw = open_dialog(&mut app, Dialog::Album);
        let parts = dialog_parts(&mut app, &ctx, draw);
        let (_, font, _) = find(&parts, "2 albums · 2 files");
        assert_eq!(font, mono11, "the counters of a delete are mono 11");
        find(&parts, "The album rows are removed; the files stay on disk.");

        // The trash: the counted files and their volume.
        let mut app = dialog_app("numbers-trash");
        let draw = open_dialog(&mut app, Dialog::Trash);
        let parts = dialog_parts(&mut app, &ctx, draw);
        let (_, font, _) = find(&parts, "1 file · 1 B");
        assert_eq!(font, mono11, "the volume of the trash is mono 11");
        find(&parts, "Will be moved to the OS trash.");

        // The export: the progress of the copy and the report as it is written to a file.
        let mut app = dialog_app("numbers-export");
        let draw = open_dialog(&mut app, Dialog::Export);
        let progress = db::Progress::default();
        progress.set_total(8);
        for _ in 0..3 {
            progress.tick();
        }
        app.export_progress = Some(std::sync::Arc::new(progress));
        app.export_report = Some(crate::organize::ExportReport {
            copied: 2,
            bytes: 2048,
            lang: i18n::Lang::En,
            ..Default::default()
        });
        let parts = dialog_parts(&mut app, &ctx, draw);
        let (_, font, _) = find(&parts, "export: 3/8");
        assert_eq!(font, mono11, "the export progress is mono 11");
        let report = parts
            .texts
            .iter()
            .find(|(t, _, _)| t.starts_with("exported 2 files"))
            .unwrap_or_else(|| panic!("no report body: {:?}", parts.texts));
        assert_eq!(report.1, mono11, "the report body is mono 11");
        find(&parts, "Copy the report");
    }

    /// M22.10: the export dialog was the last surface of the Russian window that answered in
    /// English — "Close" and "Copy the report" were written past `t()`. Now the dialog speaks the
    /// chosen language like the folder list, the storage confirmation and the trash.
    #[test]
    fn the_export_dialog_speaks_the_chosen_language() {
        let ctx = egui::Context::default();
        let mut app = dialog_app("export-i18n");
        app.settings.lang = i18n::Lang::Ru;
        let draw = open_dialog(&mut app, Dialog::Export);
        app.export_report = Some(crate::organize::ExportReport {
            copied: 1,
            lang: i18n::Lang::Ru,
            ..Default::default()
        });
        let parts = dialog_parts(&mut app, &ctx, draw);
        let words: Vec<&str> = parts.texts.iter().map(|(t, _, _)| t.as_str()).collect();
        for want in ["Выгрузить альбом на диск", "Закрыть", "Скопировать отчёт"] {
            assert!(
                words.contains(&want),
                "missing “{want}” in the Russian window: {words:?}"
            );
        }
        for english in ["Close", "Copy the report", "Export the album to disk"] {
            assert!(
                !words.contains(&english),
                "the English word “{english}” is left in the Russian window: {words:?}"
            );
        }
    }


    /// The dialogs on the real DB: the folder list over 2234 files, the export with a 500-line
    /// plan, the album delete, the storage confirmation, the trash and the settings dialog.
    /// Sixteen frames of each after four warm-up frames; the modal has to find its size and finish
    /// fading first. `PHOTOCAT_BENCH_DB=<a copy of photocat.db> cargo test --release -- --ignored
    /// dialogs_measurement --nocapture`. The number goes to `docs/ARCHITECTURE.md` § Measurements.
    #[test]
    #[ignore]
    fn dialogs_measurement() {
        use std::time::Instant;
        let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
            eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
            return;
        };
        // An album with 500 files: the export dialog then draws a plan of 500 lines.
        let out = std::env::temp_dir().join("photocat-dialogs-measure-out");
        std::fs::create_dir_all(&out).expect("a target folder");
        let album = {
            let conn = db::open(std::path::Path::new(&path)).expect("the bench DB");
            let mut app = App::new(std::path::PathBuf::from(&path));
            let id = match app.albums.first().map(|n| n.id) {
                Some(id) => id,
                None => db::create_album(&conn, 0, "Measurement", "").expect("an album"),
            };
            let ids: Vec<i64> = app.items.iter().take(500).map(|f| f.id).collect();
            db::add_to_album(&conn, id, &ids).expect("the album rows");
            app.settings.roots = app.roots.clone();
            id
        };
        let mut app = App::new(std::path::PathBuf::from(&path));
        let n = app.items.len();
        assert!(n > 100, "{n} files in the index — that is not the corpus");
        app.export_out = out.to_string_lossy().to_string();
        app.export_plan_for = Some(app.export_plan_key());
        let ctx = egui::Context::default();
        let size = egui::vec2(1240.0, 840.0);
        for which in [
            Dialog::Roots,
            Dialog::Picker,
            Dialog::Storage,
            Dialog::Album,
            Dialog::Export,
            Dialog::Trash,
            Dialog::Settings,
        ] {
            let draw = open_dialog(&mut app, which);
            if which == Dialog::Export {
                app.export_out = out.to_string_lossy().to_string();
                app.export_dialog_open(album);
                app.export_show_plan();
                assert!(
                    app.export_plan.as_ref().is_some_and(|p| p.files() > 100),
                    "the plan of the export dialog is empty"
                );
            }
            let drive = |app: &mut App, t: f64| -> f32 {
                let raw = egui::RawInput {
                    time: Some(t),
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    ..Default::default()
                };
                let start = Instant::now();
                let out = ctx.run_ui(raw, |ui| {
                    let ctx = ui.ctx().clone();
                    draw(app, &ctx);
                });
                let ms = start.elapsed().as_secs_f32() * 1000.0;
                out.drop_without_applying_deltas();
                ms
            };
            let mut t = 0.0_f64;
            for _ in 0..4 {
                drive(&mut app, t);
                t += 1.0 / 60.0;
            }
            let mut times = Vec::new();
            for _ in 0..16 {
                times.push(drive(&mut app, t));
                t += 1.0 / 60.0;
            }
            times.sort_by(|a, b| a.total_cmp(b));
            let p = |q: f32| times[((times.len() - 1) as f32 * q) as usize];
            println!(
                "corpus {n} files, dialog {which:?}: p50 {:.2} ms ({:.0} fps), p95 {:.2} ms, p99 {:.2} ms",
                p(0.50),
                1000.0 / p(0.50),
                p(0.95),
                p(0.99)
            );
        }
    }

}
