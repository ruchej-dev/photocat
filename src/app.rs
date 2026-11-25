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

/// The full-screen viewer: one photo over the whole grid, with a slideshow and text.
struct Viewer {
    ids: Vec<i64>,
    idx: usize,
    zoom: f32,
    /// The offset of the zoomed frame from the centre, in screen points. On a file change and
    /// at the "fit" scale it is zero, the frame is centred (docs/UI.md).
    pan: egui::Vec2,
    show_text: bool,
    slideshow: bool,
    interval_s: u64,
    last_advance: std::time::Instant,
}

/// A section of the "Settings" dialog: rare settings do not take the screen from the grid.
#[derive(Clone, Copy, PartialEq)]
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
    Up,
    Down,
    AddPicked,
    RemovePicked,
    FileUp,
    FileDown,
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

/// The set of files the selection was made in: the view, the filter, the search, the order.
/// While it stays the same the selection lives; a change of any field clears it (M18), so
/// that an action does not touch files that are no longer on the screen.
#[derive(Clone, PartialEq)]
struct PickScope {
    view: View,
    query: String,
    kind: KindFilter,
    tag: Option<String>,
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
    danger: egui::Color32::from_rgb(0xef, 0x44, 0x44),
    dark: true,
};

/// The light palette of the mock-up: cold slate, the same amber with AA contrast on white.
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
    accent: egui::Color32::from_rgb(0xd9, 0x77, 0x06),
    accent_hover: egui::Color32::from_rgb(0xb4, 0x53, 0x09),
    on_accent: egui::Color32::from_rgb(0xff, 0xff, 0xff),
    accent_soft: egui::Color32::from_rgb(0xfe, 0xf3, 0xc7),
    danger: egui::Color32::from_rgb(0xdc, 0x26, 0x26),
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

/// Layout metrics: change only together with docs/UI.md.
const LEFT_W: f32 = 260.0;
/// The gap between tiles — 10 px from the mock-up (`gap-2.5`), not the former 2 px.
const TILE_GAP: f32 = 10.0;
/// The caption strip under a preview: the name and the date in monospaced digits.
const CELL_CAPTION_H: f32 = 22.0;
/// The height of a navigation item and of a filter row.
const NAV_H: f32 = 28.0;
const RIGHT_W: f32 = 280.0;
/// The top bar is 32 px and the bottom one 28 px — mock-up metrics, not a matter of taste.
const TOP_H: f32 = 32.0;
const STATUS_H: f32 = 28.0;
/// The mock-up radii (Tailwind in the drawn screens): `rounded` = 4 — buttons, chips,
/// badges; `rounded-md` = 6 — navigation items, menus; `rounded-lg` = 8 — tile cards,
/// search fields, dialogs. The prose of `DESIGN.md` about 0 px loses to the drawn screens —
/// the same way as in the palette (ADR 0044).
const RADIUS_CTRL: f32 = 4.0;
const RADIUS_ITEM: f32 = 6.0;
const RADIUS_CARD: f32 = 8.0;
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

/// The primary button of a screen: filled with the accent, the text in the background colour of the palette.
fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Button<'static> {
    let t = theme(ui);
    egui::Button::new(egui::RichText::new(text).color(t.on_accent).strong())
        .fill(t.accent)
}

/// A dangerous action: an error-coloured border, no fill; a faint tint on hover. The word
/// "delete" remains only for the OS trash and an album row in the DB (AGENTS.md §1).
fn danger_button(ui: &egui::Ui, text: &str) -> egui::Button<'static> {
    let t = theme(ui);
    egui::Button::new(egui::RichText::new(text).color(t.danger))
        .fill(egui::Color32::TRANSPARENT)
        .stroke(egui::Stroke::new(1.0, t.danger))
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

/// Cut the top corners of a preview to the card radius: the picture is drawn rectangular,
/// so the outer corners are returned to the background the tile lies on.
/// The caption strip closes the bottom corners, they need no work.
fn round_preview(ui: &egui::Ui, rect: egui::Rect, radius: f32, bg: egui::Color32) {
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
    let (l, t, rt) = (rect.left(), rect.top(), rect.right());
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
    for pts in [nw, ne] {
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

/// A section heading in a panel: 11 px, secondary colour, no caps.
fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).size(11.0).color(dim(ui)));
    ui.add_space(2.0);
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
    text: &str,
    count: Option<String>,
) {
    let selected = *current == view;
    let th = theme(ui);
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NAV_H), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        // The active item: filled with the field tone and the text in the accent, radius 6 px —
        // as in the drawn screen (the row is `rounded-md` there, without a left bar).
        let fill = if selected {
            th.field
        } else if resp.hovered() {
            th.card
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
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
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

/// A cell of the year list: 20 px high and a fixed share of the panel width, the year in
/// monospace on the left, the number of files in mono 10 at the right edge, the picked year in the
/// accent. The geometry does not depend on the state, so the list does not twitch on a click
/// (M22.3).
fn year_row(ui: &mut egui::Ui, width: f32, text: &str, count: Option<usize>, selected: bool) -> bool {
    let th = theme(ui);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 20.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            th.accent_soft
        } else if resp.hovered() {
            th.card
        } else {
            egui::Color32::TRANSPARENT
        };
        if fill != egui::Color32::TRANSPARENT {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(RADIUS_CTRL as u8), fill);
        }
        let color = if selected {
            th.accent
        } else if resp.hovered() {
            th.text
        } else {
            th.text_med
        };
        ui.painter().text(
            rect.left_center() + egui::vec2(6.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::monospace(11.0),
            color,
        );
        if let Some(n) = count {
            ui.painter().text(
                rect.right_center() + egui::vec2(-6.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                n.to_string(),
                egui::FontId::monospace(10.0),
                th.text_dim,
            );
        }
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// An item of the left list in the "Settings" dialog: the same geometry as the navigation.
fn settings_nav_item(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            selected_fill(ui)
        } else if resp.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(RADIUS_ITEM as u8), fill);
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(13.0),
            if selected {
                accent(ui)
            } else {
                theme(ui).text_med
            },
        );
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
    let th = theme(ui);
    let font = egui::FontId::monospace(11.0);
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font.clone(), th.text_med);
    let size = egui::vec2(galley.size().x + 14.0, 20.0);
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
    resp.clicked()
}

/// A snapshot of the active filters for the row of chips.
struct Chips {
    query: String,
    tag: Option<String>,
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

/// Which filters a click on a chip asks to remove.
#[derive(Default, Clone, Copy)]
struct FilterClear {
    query: bool,
    tag: bool,
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

/// The row of chips above the grid: what is on is visible, "×" removes exactly that, the total is on the right.
/// The row always exists ("no filters" when the filter is empty), so the view heading does not jump.
fn draw_filter_chips(ui: &mut egui::Ui, chips: &Chips, found: (usize, usize)) -> FilterClear {
    let lang = chips.lang;
    let mut clear = FilterClear::default();
    let mut any = false;
    ui.horizontal(|ui| {
        if !chips.query.is_empty() {
            any = true;
            let text = format!("{} {}", t(lang, "path/date:"), shorten(&chips.query, 24));
            if filter_chip(ui, &text, lang) {
                clear.query = true;
            }
        }
        if let Some(tag) = &chips.tag {
            any = true;
            let text = format!("{} {}", t(lang, "tag:"), shorten(tag, 24));
            if filter_chip(ui, &text, lang) {
                clear.tag = true;
            }
        }
        if let Some(album) = &chips.album {
            any = true;
            let name = shorten(album, 22);
            let text = i18n::fill(lang, "Album “{}”", &[name]);
            if filter_chip(ui, &text, lang) {
                clear.album = true;
            }
        }
        if let Some(label) = chips.kind.chip(chips.lang) {
            any = true;
            if filter_chip(ui, label, lang) {
                clear.kind = true;
            }
        }
        if let Some(n) = chips.hits {
            any = true;
            if filter_chip(ui, &i18n::relevance(lang, n), lang) {
                clear.hits = true;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(if any {
                    i18n::found(lang, found.0, found.1)
                } else {
                    t(lang, "no filters").to_string()
                })
                .size(11.0)
                .color(dim(ui)),
            );
            // The multi-select (M18): the count and the two buttons are always in one place, so
            // that the chip row does not jump on clicks.
            if chips.pickable {
                if chips.picked > 0
                    && ui
                        .add(
                            egui::Button::new(
                                egui::RichText::new(t(lang, "Clear selection")).size(11.0),
                            )
                            .small(),
                        )
                        .on_hover_text(t(
                            lang,
                            "clears the selection in this grid; Esc does the same",
                        ))
                        .clicked()
                {
                    clear.clear_pick = true;
                }
                if ui
                    .add(
                        egui::Button::new(
                            egui::RichText::new(i18n::select_all(lang, found.0)).size(11.0),
                        )
                        .small(),
                    )
                    .on_hover_text(t(
                        lang,
                        "marks everything visible now; Ctrl+A does the same",
                    ))
                    .clicked()
                {
                    clear.pick_all = true;
                }
                if chips.picked > 0 {
                    let size = db::human_in(lang, chips.picked_bytes);
                    let text = i18n::fill(
                        lang,
                        "{} selected · {}",
                        &[chips.picked.to_string(), size.to_string()],
                    );
                    ui.label(egui::RichText::new(text).size(11.0).color(accent(ui)));
                }
            }
        });
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

/// The album tree: an indent by depth, a collapse triangle, counters.
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
            if ui
                .selectable_label(selected == Some(node.id), text)
                .on_hover_text(if !node.note.trim().is_empty() {
                    node.note.clone()
                } else if node.cover.is_some() {
                    t(lang, "open the album; the cover is the first photo in its order").to_string()
                } else {
                    t(lang, "open the album").to_string()
                })
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

/// The size of a sub-album card (M22.6): the width from the "Tiles" slider, a 16:10 canvas cover,
/// the name at 13 px and the mono facts at 10 px — one card model for every sub-album.
fn album_card_size(tile: f32) -> egui::Vec2 {
    const PAD: f32 = 8.0;
    let w = tile.clamp(100.0, 260.0);
    let cover = ((w - 2.0 * PAD) * 10.0 / 16.0).round();
    egui::vec2(w, PAD + cover + 6.0 + 16.0 + 14.0 + PAD)
}

/// A sub-album card (M22.6): the mock-up's card — a canvas cover with the preview of the album's
/// first snapshot fitted whole, the name, the count of files and the size; a click opens the album.
fn draw_album_cell(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    cs: &mut Cells,
    node: &db::AlbumNode,
    cover: Option<&FileRow>,
    tile: f32,
    lang: i18n::Lang,
) -> bool {
    const PAD: f32 = 8.0;
    let th = theme(ui);
    let size = album_card_size(tile);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_filled(rect, RADIUS_CARD, th.card);
        // A 1 px line on every card, the accent one under the cursor (the mock-up's hover).
        painter.rect_stroke(
            rect,
            RADIUS_CARD,
            egui::Stroke::new(1.0, if resp.hovered() { th.accent } else { th.border }),
            egui::StrokeKind::Inside,
        );
        // The cover area: a 16:10 canvas with the snapshot fitted whole — a preview is never cropped.
        let cover_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + PAD, rect.top() + PAD),
            egui::vec2(size.x - 2.0 * PAD, size.y - 2.0 * PAD - 36.0),
        );
        painter.rect_filled(cover_rect, RADIUS_CTRL, th.canvas);
        let drawn = cover.is_some_and(|f| paint_thumb(ui, ctx, cs, f, cover_rect));
        if !drawn {
            // An album without a snapshot keeps the empty canvas and a folder mark, not a placeholder.
            painter.text(
                cover_rect.center(),
                egui::Align2::CENTER_CENTER,
                "▸",
                egui::FontId::proportional(18.0),
                th.text_dim,
            );
        }
        let inner = size.x - 2.0 * PAD;
        let name_top = cover_rect.bottom() + 6.0;
        let facts_top = name_top + 17.0;
        // The texts are clipped to the card: a long name is cut with an ellipsis, not spilling out.
        let clipped = painter.with_clip_rect(rect.shrink(1.0));
        clipped.text(
            egui::pos2(rect.left() + PAD, name_top),
            egui::Align2::LEFT_TOP,
            shorten(&node.name, (inner / 7.0).max(6.0) as usize),
            egui::FontId::proportional(13.0),
            th.text,
        );
        clipped.text(
            egui::pos2(rect.left() + PAD, facts_top),
            egui::Align2::LEFT_TOP,
            i18n::count(lang, node.files as usize, "file"),
            egui::FontId::monospace(10.0),
            th.text_med,
        );
        clipped.text(
            egui::pos2(rect.right() - PAD, facts_top),
            egui::Align2::RIGHT_TOP,
            db::human_in(lang, node.bytes),
            egui::FontId::monospace(10.0),
            th.text_dim,
        );
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }
    }
    // The sub-albums cannot be counted on a 100 px card: they are in the tree and in the hint.
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
        if running && ui.button("Stop").clicked() {
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
    /// The albums of the selected file for the card: a cache for one file.
    file_albums: Vec<(i64, String)>,
    file_albums_for: Option<i64>,
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
    /// The weight distribution of every tag (descending) and of the best tag of a file: the
    /// percentile threshold of the filter is computed from it, without sorting on every frame.
    tag_dists: HashMap<String, Vec<f32>>,
    top_scores: Vec<f32>,
    /// The owner's own tags: file_id → labels.
    user_tags: HashMap<i64, Vec<String>>,
    user_tag_input: String,
    tag_filter: Option<String>,
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
            file_albums: Vec::new(),
            file_albums_for: None,
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
                tag: None,
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
            top_scores: Vec::new(),
            user_tags: HashMap::new(),
            user_tag_input: String::new(),
            tag_filter: None,
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
                self.status = i18n::fill(
                    lang,
                    "{} in the index, {}",
                    &[
                        i18n::count(lang, rows.len(), "file"),
                        db::human_in(settings.lang, rows.iter().map(|r| r.size).sum()),
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
                self.trash_items = trashed;
                self.tag_dists = embed::tag_distributions(&tags);
                self.top_scores = embed::top_score_distribution(&tags);
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
            let names: Vec<&str> = subtree.iter().map(|n| n.name.as_str()).collect();
            self.album_delete_text = i18n::fill(lang, "Will be deleted: {}. Albums {}, file rows {}. The files stay on disk.", &[names
                    .iter()
                    .map(|n| format!("«{n}»"))
                    .collect::<Vec<_>>()
                    .join(", ").to_string(), subtree.len().to_string(), rows.to_string()]);
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
            egui::RichText::new(t(lang, "comparing copies and a sha256 manifest"))
                .size(12.0)
                .color(dim(ui)),
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
                            .hint_text(t(lang, "/home/…/Photos from the phone"))
                            .desired_width(460.0),
                    )
                    .changed();
                ui.end_row();
                ui.label(format!("{}:", t(lang, "Folder B (copy)")));
                ui.add(
                    egui::TextEdit::singleline(&mut self.integrity_b)
                        .hint_text(t(lang, "/mnt/backup/Photos from the phone"))
                        .desired_width(460.0),
                );
                ui.end_row();
                ui.label(format!("{}:", t(lang, "Manifest file")));
                manifest_changed |= ui
                    .add(
                        egui::TextEdit::singleline(&mut self.integrity_manifest)
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
            let b = primary_button(ui, t(lang, "Compare the copies"));
            if ui
                .add_enabled(!running, b)
                .on_hover_text(t(lang, "compare folder A and folder B byte by byte"))
                .clicked()
            {
                verify_clicked = true;
            }
            if ui
                .add_enabled(!running, egui::Button::new(manifest_label))
                .on_hover_text(t(lang, "sha256 of every file in folder A into the manifest"))
                .clicked()
            {
                manifest_clicked = true;
            }
            if ui
                .add_enabled(!running, egui::Button::new(t(lang, "Check against the manifest")))
                .on_hover_text(t(lang, "compare folder A with the saved manifest"))
                .clicked()
            {
                check_clicked = true;
            }
            if self.integrity_overwrite && ui.button(t(lang, "Cancel")).clicked() {
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
                if ui.button(t(lang, "Copy the last report")).clicked() {
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
                if ui.button(t(lang, "Copy the report")).clicked() {
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
        let Some(id) = self.album_selected else {
            return;
        };
        let res: Result<String> = (|| {
            let conn = db::open(&self.db_path)?;
            match action {
                AlbumAction::Select(other) => {
                    self.album_open(other);
                    Ok(String::new())
                }
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-settings")).show(ctx, |ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(760.0, 620.0),
                egui::Layout::top_down(egui::Align::Min),
                |ui| {
                    ui.set_min_size(egui::vec2(760.0, 620.0));
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        ui.add_space(16.0);
                        ui.label(egui::RichText::new(t(lang, "Settings")).size(16.0));
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
                                    if ui.button(t(lang, "Close")).clicked() {
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
                            for t in SettingsTab::ALL {
                                if settings_nav_item(ui, tab == t, t.label(lang)) {
                                    tab = t;
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-roots")).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new(t(lang, "Archive folders")).size(16.0));
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
                    for root in &roots {
                        ui.horizontal(|ui| {
                            ui.label(root);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui
                                        .small_button(t(lang, "Remove from the index"))
                                        .on_hover_text(t(lang, "files on disk are not deleted"))
                                        .clicked()
                                    {
                                        remove = Some(root.clone());
                                    }
                                },
                            );
                        });
                    }
                });
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui.button(t(lang, "Add a folder…")).clicked() {
                    add = true;
                }
                let b = primary_button(ui, t(lang, "Index all"));
                if ui
                    .add_enabled(!roots.is_empty(), b)
                    .clicked()
                {
                    index_all = true;
                }
                if ui.button(t(lang, "Close")).clicked() {
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-picker")).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new(t(lang, "Add a folder")).size(16.0));
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
                    && ui.button(t(lang, "Up")).clicked()
                {
                    nav = Some(parent.to_path_buf());
                }
                let b = primary_button(ui, t(lang, "Add and index"));
                if ui
                    .add(b)
                    .clicked()
                {
                    add = true;
                    add_and_index = true;
                }
                if ui.button(t(lang, "Add without indexing")).clicked() {
                    add = true;
                }
                if ui.button(t(lang, "Cancel")).clicked() {
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
                        if ui.button(format!("{name}/")).clicked() {
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-storage")).show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new(title).size(16.0));
            ui.add_space(4.0);
            ui.label(text);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let b = primary_button(ui, button);
                if ui.add(b).clicked() {
                    confirmed = true;
                }
                if ui.button(t(lang, "Cancel")).clicked() {
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
                StorageAction::Thumbs => storage::clear_thumbs(),
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-album")).show(ctx, |ui| {
            ui.set_min_width(480.0);
            ui.label(egui::RichText::new(dialog.title(lang)).size(16.0));
            ui.add_space(6.0);
            match dialog {
                AlbumDialog::Delete => {
                    ui.label(&delete_text);
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
                let b = primary_button(ui, label);
                if ui.add(b).clicked() {
                    confirm = true;
                }
                if ui.button("Cancel").clicked() {
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-export")).show(ctx, |ui| {
            ui.set_min_width(640.0);
            ui.label(egui::RichText::new(t(lang, "Export the album to disk")).size(16.0));
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
                if ui.button(t(lang, "Show the plan")).clicked() {
                    show_plan = true;
                }
                if ui
                    .add_enabled(
                        !running,
                        egui::Button::new(i18n::fill(lang, "Export {} files", &[files.to_string()])),
                    )
                    .on_hover_text(t(lang, "copies, not links; a repeated pass duplicates nothing"))
                    .clicked()
                {
                    start = true;
                }
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
            if let Some(text) = &progress_text {
                ui.label(egui::RichText::new(text).size(11.0).color(dim(ui)));
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
                ui.separator();
                ui.label(&report_text);
                for line in errors.iter().take(5) {
                    ui.label(egui::RichText::new(line).size(11.0).color(dim(ui)));
                }
                if ui.button("Copy the report").clicked() {
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
        let resp = egui::Modal::new(egui::Id::unique("photocat-trash")).show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new(t(lang, "Empty the trash?")).size(16.0));
            ui.add_space(4.0);
            ui.label({
                    i18n::fill(
                        lang,
                        "Will be moved to the OS trash: {} files, {}.",
                        &[n.to_string(), db::human_in(lang, bytes)],
                    )
                });
            ui.label(
                egui::RichText::new(t(
                    lang,
                    "Nothing is deleted for good inside the app: the files stay in the OS trash until you empty it yourself. The originals will disappear from their folders in the archive — the app cannot undo that.",
                ))
                .size(12.0),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let b = primary_button(ui, t(lang, "Move to the OS trash"));
                if ui
                    .add(b)
                    .clicked()
                {
                    confirmed = true;
                }
                if ui.button(t(lang, "Cancel")).clicked() {
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
        ui.label(egui::RichText::new(tab.label(lang)).size(16.0));
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
        if ui.button(t(lang, "Check the model")).clicked() {
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
            if ui.button(t(lang, "Apply the path")).clicked() {
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
            let b = primary_button(ui, t(lang, "Download the model"));
            if ui
                .add_enabled(self.download_consent && !busy, b)
                .clicked()
            {
                self.start_download();
            }
            if ui
                .add_enabled(
                    self.embed_rx.is_none() && embed::model_present(&self.settings),
                    egui::Button::new(t(lang, "Compute tags")),
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
                if ui.button(t(lang, "Remove from the dictionary")).clicked() {
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
                    egui::Button::new(t(lang, "Add to the dictionary")),
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
                .add_enabled(can, egui::Button::new(title))
                .on_hover_text(t(
                    lang,
                    "recompute the tag weights from the vectors already stored, without running CLIP over the photos again",
                ))
                .clicked();
            export = ui
                .button(t(lang, "Export JSON"))
                .on_hover_text(t(
                    lang,
                    "the dictionary without paths or photos — it can be moved to another machine",
                ))
                .clicked();
            import = ui
                .button(t(lang, "Import JSON"))
                .on_hover_text(t(
                    lang,
                    "replace the model dictionary with a file; the old weights disappear after a recompute",
                ))
                .clicked();
            reset = ui
                .button(t(lang, "Restore the defaults"))
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
            let b = primary_button(ui, t(lang, "Download the face model"));
            if ui
                .add_enabled(
                    self.face_download_consent && self.face_download_rx.is_none(),
                    b,
                )
                .clicked()
            {
                self.start_face_download();
            }
            if ui.button(t(lang, "Check the face model")).clicked() {
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
        if ui.button(t(lang, "Compute the used space")).clicked() {
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
            .button(t(lang, "Clear the previews…"))
            .on_hover_text(t(lang, "only <cache>/photocat/thumbs; the archive is untouched"))
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Thumbs);
        }
        if ui
            .button(t(lang, "Clear the face crops…"))
            .on_hover_text(t(
                lang,
                "the crops and the face rows; after that “Find faces” runs again",
            ))
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Faces);
        }
        if ui
            .button(t(lang, "Reset the index…"))
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
            .add(egui::Button::new(t(lang, "Check and save the programs")))
            .clicked()
        {
            self.apply_tools();
        }
    }

    fn reset_filters(&mut self) {
        let lang = self.settings.lang;
        self.query.clear();
        self.kind_filter = KindFilter::All;
        self.tag_filter = None;
        self.semantic_hits = None;
        self.ocr_snippets.clear();
        self.status = t(lang, "the filters are reset").into();
    }

    /// The 32 px top bar: the mark, the search, the state and the way into "Settings".
    /// The folder and "Start indexing" live in the left panel: they are work with the archive, not a search.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let th = theme(ui);
        // The bar does not wrap: at 1240 px the search shrinks, and a long state counter
        // (EXIF, syncing) is shown only in a wide window.
        let wide = ui.available_width() >= 1400.0;
        let field_w = if wide { 340.0 } else { 240.0 };
        ui.horizontal(|ui| {
            ui.set_min_height(TOP_H - 6.0);
            ui.label(egui::RichText::new("photocat").size(14.0).strong());
            ui.label(
                egui::RichText::new(t(lang, "family archive"))
                    .size(10.0)
                    .color(th.text_dim),
            );
            // The separator between the mark and the search is a 1 px line, not an eyeballed gap.
            let (bar, _) =
                ui.allocate_exact_size(egui::vec2(1.0, 16.0), egui::Sense::hover());
            ui.painter().rect_filled(bar, 0.0, th.border_strong);
            ui.add_space(2.0);

            // The search mode is a compact switch before the field (PhotoPrism).
            let mut text_mode = self.search_ocr;
            if ui
                .selectable_label(!text_mode, egui::RichText::new(t(lang, "Semantic")).size(11.0))
                .on_hover_text(t(lang, t(lang, "search by the content of the photos")))
                .clicked()
            {
                text_mode = false;
            }
            if ui
                .selectable_label(text_mode, egui::RichText::new(t(lang, "By text")).size(11.0))
                .on_hover_text(t(lang, t(lang, "search the recognized text")))
                .clicked()
            {
                text_mode = true;
            }
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
            let ready = if self.search_ocr {
                self.ocr_files > 0
            } else {
                embed::model_present(&self.settings)
            };
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.semantic_query)
                    .hint_text(if self.search_ocr {
                        t(lang, "receipt, invoice, 2024")
                    } else {
                        t(lang, "cake, sea, furniture, document")
                    })
                    .desired_width(field_w),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui
                .add_enabled(ready, egui::Button::new(t(lang, "Search")))
                .clicked()
                || (enter && ready)
            {
                self.start_search();
            }
            if ui
                .add_enabled(self.semantic_hits.is_some(), egui::Button::new(t(lang, "Reset")))
                .clicked()
            {
                self.semantic_hits = None;
                self.ocr_snippets.clear();
                self.status = t(lang, "search cleared").into();
            }
            if self.search_rx.is_some() {
                ui.label(
                    egui::RichText::new(t(lang, "loading model…"))
                        .size(11.0)
                        .color(dim(ui)),
                );
            } else if !ready {
                ui.label(
                    egui::RichText::new(if self.search_ocr {
                        t(lang, "no recognized text yet")
                    } else {
                        t(lang, "model not downloaded")
                    })
                    .size(11.0)
                    .color(dim(ui)),
                );
            }

            // On the right — the state and the way into the service windows. The numbers are monospaced so
            // that the counter does not jostle its neighbours during indexing.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(t(lang, "Settings")).clicked() {
                    self.settings_open = true;
                }
                if ui
                    .button(if self.details_open {
                        t(lang, "Hide details")
                    } else {
                        t(lang, "Details")
                    })
                    .on_hover_text(t(lang, "right panel with date, camera, tags and text"))
                    .clicked()
                {
                    self.details_open = !self.details_open;
                }
                if ui
                    .button(format!("{} ({})", t(lang, "Folders"), self.roots.len()))
                    .on_hover_text(t(
                        lang,
                        "add several folders, drop a folder from the index",
                    ))
                    .clicked()
                {
                    self.roots_open = true;
                }
                if wide {
                    let facts = i18n::fill(lang, "{} files · {} with EXIF · synced", &[self.items.len().to_string(), self.items
                                .iter()
                                .filter(|r| r.taken_src == "exif")
                                .count().to_string()]);
                    ui.label(num(ui, facts, 10.0).color(dim(ui)));
                }
            });
        });

        // Progress is the only third row: it exists only while a pass is running.
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
        let r = ui.max_rect();
        ui.painter()
            .hline(r.x_range(), r.bottom() - 0.5, egui::Stroke::new(1.0, th.border));
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
                .button(t(lang, "Check now"))
                .on_hover_text(t(lang, "read the archive and compare it with the saved manifest"))
                .clicked()
            {
                open = true;
            }
            if ui
                .button(t(lang, "Remind me later"))
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

    /// The 28 px bottom bar: the fact of the last action on the left, the counters and the selection on the right.
    /// The numbers are monospaced: a counter does not jostle its neighbours during indexing (mock-up: process deck).
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let th = theme(ui);
        let top = ui.max_rect();
        ui.painter()
            .hline(top.x_range(), top.top() + 0.5, egui::Stroke::new(1.0, th.border));
        ui.horizontal(|ui| {
            ui.set_min_height(STATUS_H - 6.0);
            ui.label(
                egui::RichText::new(if self.status.is_empty() {
                    "—"
                } else {
                    &self.status
                })
                .size(11.0)
                .color(th.text_med),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if let Some(f) = self
                    .selected
                    .and_then(|id| self.items.iter().find(|r| r.id == id))
                {
                    let date = f
                        .taken_at
                        .as_deref()
                        .map(|d| d.replace('T', " ").chars().take(16).collect::<String>())
                        .unwrap_or_else(|| t(lang, "no date").into());
                    let text = if self.picked.len() > 1 {
                        format!(
                            "{} {} · {} · {date}",
                            t(lang, t(lang, "Picked")),
                            self.picked.len(),
                            file_name(&f.path)
                        )
                    } else {
                        format!("{}: {} · {date}", t(lang, t(lang, "Picked")), file_name(&f.path))
                    };
                    ui.label(num(ui, text, 10.0).color(th.text_med));
                    let (bar, _) =
                        ui.allocate_exact_size(egui::vec2(1.0, 14.0), egui::Sense::hover());
                    ui.painter().rect_filled(bar, 0.0, th.border_strong);
                }
                let total = i18n::fill(lang, "{}", &[i18n::count(lang, self.items.len(), "file")]);
                ui.label(num(ui, total, 10.0).color(th.text_dim));
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
        Chips {
            query: self.query.trim().to_string(),
            tag: self.tag_filter.clone(),
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
    fn left_panel(&mut self, ui: &mut egui::Ui, show_card: bool) {
        let lang = self.settings.lang;
        // The folder and "Start indexing" are work with the archive, not a search: they live here, while
        // the top bar stays a single 32 px row (mock-up: working folder block).
        ui.add_space(6.0);
        caps_label(ui, t(lang, "Folder"));
        ui.add_space(2.0);
        let resp = ui.add(
            egui::TextEdit::singleline(&mut self.path_input)
                .hint_text(t(lang, "path to a folder; several — with the “Folders” button"))
                .desired_width(f32::INFINITY),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.add_space(4.0);
        let index = primary_button(ui, t(lang, "Start indexing"));
        ui.horizontal(|ui| {
            if ui.add(index).clicked() || enter {
                self.start_index();
            }
            if ui.button(t(lang, "Refresh")).clicked() {
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
            t(lang, "All files"),
            Some(self.items.len().to_string()),
        );
        nav_item(
            ui,
            &mut self.view,
            View::Albums,
            t(lang, "Albums"),
            Some(self.albums.len().to_string()),
        );
        nav_item(ui, &mut self.view, View::Calendar, t(lang, "Calendar"), None);
        nav_item(
            ui,
            &mut self.view,
            View::Faces,
            t(lang, "People"),
            Some(self.face_clusters.len().to_string()),
        );
        nav_section(ui, t(lang, "TOOLS"));
        nav_item(
            ui,
            &mut self.view,
            View::Dupes,
            t(lang, "Duplicates & series"),
            Some(self.groups.len().to_string()),
        );
        nav_item(ui, &mut self.view, View::Integrity, t(lang, "Integrity"), None);
        nav_item(
            ui,
            &mut self.view,
            View::Trash,
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
        );
        ui.add_space(4.0);
        // The format is one choice instead of a checkbox: everything, photos or video (PhotoPrism: Images/Video).
        ui.horizontal(|ui| {
            for kind in [KindFilter::All, KindFilter::Images, KindFilter::Videos] {
                if ui
                    .selectable_label(
                        self.kind_filter == kind,
                        egui::RichText::new(kind.label(lang)).size(12.0),
                    )
                    .clicked()
                {
                    self.kind_filter = kind;
                }
            }
        });
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
                    View::Faces => self.faces_controls(ui),
                    _ => {}
                }
                self.model_block(ui);
                self.ocr_block(ui);
                if show_card {
                    ui.add_space(8.0);
                    hairline(ui);
                    self.file_card(ui);
                    ui.add_space(8.0);
                    hairline(ui);
                    self.stats_block(ui);
                }
            });
        panel_edge(ui, false);
    }

    /// The years of the archive: the five newest with the number of files, the picked one in the
    /// accent. "All years" is the way back and is drawn only while a year is picked, so the panel
    /// does not spend a row on a filter that is already empty (M22.3).
    fn years_block(&mut self, ui: &mut egui::Ui, years: &[(String, usize)]) {
        let lang = self.settings.lang;
        let picked = self.picked_year();
        // Two columns of three rows: a single column of five years costs 130 px and pushes the
        // model block out of the panel on the default 840 px window. Six cells are three rows
        // whether or not a year is picked, so the block never changes height on a click.
        // text, count, selected, the year the click sets (None — "All years", an empty filter)
        let mut cells: Vec<(String, Option<usize>, bool, Option<String>)> = Vec::new();
        if picked.is_some() {
            cells.push((t(lang, "All years").to_string(), None, false, None));
        }
        for (year, count) in years {
            let selected = picked.as_deref() == Some(year.as_str());
            cells.push((year.clone(), Some(*count), selected, Some(year.clone())));
        }
        let gap = 4.0;
        let w = ((ui.available_width() - gap) / 2.0).max(60.0);
        let mut set: Option<Option<String>> = None;
        for row in cells.chunks(2) {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for (text, count, selected, year) in row {
                    if year_row(ui, w, text, *count, *selected) {
                        // A second click on the picked year clears it: a toggle, as a chip behaves.
                        set = Some(if *selected { None } else { year.clone() });
                    }
                }
            });
        }
        if let Some(year) = set {
            self.pick_year(year.as_deref());
        }
    }

    /// The years of the index with the number of files: the newest first, five at most. A file
    /// without a date belongs to no year and is not counted (M22.3).
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
        out.truncate(5);
        out
    }

    /// The year the date filter holds, if it holds exactly a year.
    fn picked_year(&self) -> Option<String> {
        let q = self.query.trim();
        (q.len() == 4 && q.bytes().all(|b| b.is_ascii_digit())).then(|| q.to_string())
    }

    /// A year clicked in the panel: the date filter is the query field, so the year goes there.
    /// A semantic search and the text snippets are dropped — a year is a date, not words.
    fn pick_year(&mut self, year: Option<&str>) {
        self.query = year.unwrap_or_default().to_string();
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
            let b = primary_button(ui, t(lang, "Find groups"));
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
                .add_enabled(has_groups, egui::Button::new(t(lang, "Export the plan")))
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
                .add_enabled(has_groups, egui::Button::new(t(lang, "Copy the list")))
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

    /// The context of the "Faces" view: the threshold, the pass over the photos, the regrouping and the list.
    fn faces_controls(&mut self, ui: &mut egui::Ui) {
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
                if ui.button(t(lang, "Face settings")).clicked() {
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
                    && ui.button(t(lang, "Stop")).clicked()
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
                    .add_enabled(model && self.face_todo > 0, egui::Button::new(scan_label))
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
                        egui::Button::new(t(lang, "Rebuild groups")),
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
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(t(lang, "Groups"))
                    .size(11.0)
                    .color(dim(ui)),
            );
            let clusters = self.face_clusters.clone();
            egui::ScrollArea::vertical()
                .id_salt("face-clusters")
                .max_height(220.0)
                .show(ui, |ui| {
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
                            let line = format!(
                                "{label} · {}",
                                i18n::count(lang, c.photos as usize,"photo")
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
                });
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
                        egui::Button::new(if self.face_together {
                            format!("{} ({n})", t(lang, "Show the photos (together)"))
                        } else {
                            format!("{} ({n})", t(lang, "Photos with everyone together"))
                        }),
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
                        egui::Button::new(t(lang, "Compute tags")),
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
                // The number next to a tag is how many photos carry it at the current threshold.
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
            if ui.button(t(lang, "Model settings")).clicked() {
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
                if ui.button(t(lang, "Stop")).clicked()
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
                        egui::Button::new(label),
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
        let no_date = self.items.iter().filter(|r| r.taken_at.is_none()).count();
        let no_thumb = self.items.iter().filter(|r| r.thumb.is_none()).count();
        stat_row(ui, t(lang, "Files:"), &n.to_string());
        stat_row(ui, t(lang, "Size:"), &db::human_in(lang, bytes));
        stat_row(ui, t(lang, "No date:"), &no_date.to_string());
        stat_row(ui, t(lang, "No preview:"), &no_thumb.to_string());
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

    /// The right panel: details and actions for the selected file.
    fn details_panel(&mut self, ui: &mut egui::Ui) {
        panel_edge(ui, true);
        let lang = self.settings.lang;
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(t(lang, "File details")).size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(t(lang, "Hide"))
                    .on_hover_text(t(lang, "free up room for the grid and calendar"))
                    .clicked()
                {
                    self.details_open = false;
                }
            });
        });
        ui.add_space(4.0);
        egui::ScrollArea::vertical()
            .id_salt("details-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| self.file_card(ui));
    }

    /// The file card: a preview, metadata, tags and text. One for the whole interface.
    fn file_card(&mut self, ui: &mut egui::Ui) {
        let lang = self.settings.lang;
        let Some(id) = self.selected else {
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

        let w = ui.available_width();
        let h = (w * 0.75).min(210.0);
        let (rect, _) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::hover());
        ui.painter()
            .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
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
            bg: ui.visuals().extreme_bg_color,
            cell_style: db::CellStyle::Tiles,
            time_only: false,
        };
        if !paint_thumb(ui, &ctx, &mut cells, &f, rect.shrink(1.0)) {
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
        ui.add_space(8.0);
        // The file name is the main thing, the path is its caption (the PhotoPrism inspector).
        ui.label(egui::RichText::new(file_name(&f.path)).size(14.0));
        ui.label(egui::RichText::new(&f.path).size(11.0).color(dim(ui)));
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
            if ui
                .button(t(lang, "Open full screen"))
                .on_hover_text(t(lang, "double-clicking a tile does the same"))
                .clicked()
            {
                self.open_viewer(id);
            }
            let b = danger_button(ui, t(lang, "To trash"));
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
        ui.add_space(4.0);

        let date = format!(
            "{} ({})",
            f.taken_at
                .as_deref()
                .map(|d| d.replace('T', " ").chars().take(16).collect::<String>())
                .unwrap_or_else(|| "—".into()),
            date_source(&f.taken_src, lang)
        );
        meta_row(ui, t(lang, "Date"), &date);
        if let Some(c) = &f.camera {
            meta_row(ui, t(lang, "Camera"), c);
        }
        if let (Some(a), Some(b)) = (f.lat, f.lon) {
            meta_row(ui, "GPS", &format!("{a:.5}, {b:.5}"));
        }
        let mut size = format!("{}×{} · {}", f.width, f.height, db::human_in(lang, f.size));
        if f.kind == "video"
            && let Some(d) = f.duration_s
        {
            size.push_str(&format!(" · ▶ {}", db::duration(d)));
        }
        meta_row(ui, t(lang, "Size"), &size);
        meta_row(
            ui,
            "SHA256",
            &format!("{}…", &f.sha256[..16.min(f.sha256.len())]),
        );

        // The albums of the file (M19): a row in the inspector, "×" removes only the album row.
        // Nothing changes on disk.
        if self.file_albums_for != Some(id) {
            self.file_albums_for = Some(id);
            self.file_albums = db::open(&self.db_path)
                .and_then(|c| db::album_refs_of_file(&c, id))
                .unwrap_or_default();
        }
        if !self.file_albums.is_empty() {
            section_label(ui, t(lang, "Albums"));
            let list = self.file_albums.clone();
            let mut remove_from: Option<i64> = None;
            for (album_id, path) in &list {
                ui.horizontal(|ui| {
                    if ui
                        .small_button("×")
                        .on_hover_text(t(
                            lang,
                            "remove the photo from this album; the file on disk is unchanged",
                        ))
                        .clicked()
                    {
                        remove_from = Some(*album_id);
                    }
                    ui.label(egui::RichText::new(shorten(path, 34)).size(12.0));
                });
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

        section_label(ui, t(lang, "AI tags"));
        // Confidence comes from the margin of the first tag over the second (threshold and margin in the settings).
        // Without a margin "document" is not highlighted: an honest "not sure" and the top 3,
        // the other weights under a collapsible row, so as not to pass noise off as an answer.
        let confident = self.tags.get(&id).and_then(|tags| {
            embed::confident_tag_with(tags, self.settings.tag_show, self.settings.tag_margin)
                .map(|(tag, _)| tag.to_string())
        });
        let mut hide: Option<String> = None;
        if let Some(tags) = self.tags.get(&id).cloned() {
            match &confident {
                Some(tag) => {
                    ui.label(
                        egui::RichText::new(format!("{} {tag}", t(lang, "tag:")))
                            .size(12.0)
                            .color(accent(ui)),
                    );
                }
                None if !tags.is_empty() => {
                    ui.label(
                        egui::RichText::new(t(lang, "tag: not sure — the top weights are close"))
                            .size(12.0)
                            .color(dim(ui)),
                    );
                }
                None => {}
            }
            ui.add_space(2.0);
            let spacing = ui.spacing_mut().item_spacing.y;
            ui.spacing_mut().item_spacing.y = 2.0;
            let top = if confident.is_some() { tags.len() } else { 3.min(tags.len()) };
            for (tag, score) in tags.iter().take(top) {
                let pct = self.tag_percentile(tag, *score);
                tag_row(ui, tag, *score, pct, confident.as_deref(), &mut hide, lang);
            }
            if confident.is_none() && tags.len() > top {
                egui::CollapsingHeader::new(format!("{} ({})", t(lang, "all tags"), tags.len()))
                    .id_salt(("card_tags", id))
                    .show(ui, |ui| {
                        for (tag, score) in tags.iter().skip(top) {
                            let pct = self.tag_percentile(tag, *score);
                            tag_row(ui, tag, *score, pct, None, &mut hide, lang);
                        }
                    });
            }
            ui.spacing_mut().item_spacing.y = spacing;
        } else {
            ui.label(
                egui::RichText::new(t(lang, "tag: not computed"))
                    .size(12.0)
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
                .button(format!(
                    "{} ({hidden})",
                    t(lang, "restore hidden tags")
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

        section_label(ui, t(lang, "My tags"));
        let own = self.user_tags.get(&id).cloned().unwrap_or_default();
        if own.is_empty() {
            ui.label(
                egui::RichText::new(t(lang, "none yet"))
                    .size(12.0)
                    .color(dim(ui)),
            );
        }
        let mut remove_own: Option<String> = None;
        for tag in &own {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tag).size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button(t(lang, "remove")).clicked() {
                        remove_own = Some(tag.clone());
                    }
                });
            });
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
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.user_tag_input)
                    .hint_text(t(lang, "your own tag, e.g. furniture"))
                    .desired_width(150.0),
            );
            if ui.button(t(lang, "Add tag")).clicked() {
                let tag = self.user_tag_input.trim().to_string();
                if !tag.is_empty() {
                    match db::add_user_tag(&self.db_path, id, &tag) {
                        Ok(()) => {
                            let tags = self.user_tags.entry(id).or_default();
                            tags.push(tag.clone());
                            tags.sort();
                            self.user_tag_input.clear();
                            self.status =
                                format!("{}: «{tag}»", t(lang, "own tag added"));
                        }
                        Err(e) => {
                            self.status = format!("{}: {e}", t(lang, "tag not added"))
                        }
                    }
                }
            }
        });

        if f.kind == "image" {
            section_label(ui, t(lang, "Text on the photo"));
            let label = if self.ocr_text.get(&id).is_some_and(|t| !t.trim().is_empty()) {
                t(lang, "Recognize again")
            } else {
                t(lang, "Recognize text")
            };
            if ui
                .add_enabled(
                    self.tesseract.is_some() && self.ocr_rx.is_none(),
                    egui::Button::new(label),
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
                    .button(t(lang, "Show the full text"))
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
        self.viewer = Some(Viewer {
            ids,
            idx,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            show_text: false,
            slideshow: false,
            interval_s: 3,
            last_advance: std::time::Instant::now(),
        });
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
        let preview = self.textures.get(&id);
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
        let video_frame = self.video_tex.as_ref().filter(|_| self.video_id == Some(id));
        let paused = video_active && self.video.as_ref().is_some_and(|p| !p.is_playing());
        let current_full = self.viewer_tex.as_ref().filter(|(t, _)| *t == id);
        // A video has no "full frame": there it is the cover frame, not a loading state.
        let stale = self
            .viewer_tex
            .as_ref()
            .filter(|(t, _)| *t != id && f.kind != "video")
            .map(|(_, t)| t);
        let mut new_zoom = v.zoom;
        let mut new_pan = v.pan;
        let mut new_text = v.show_text;
        let mut new_slideshow = v.slideshow;
        let mut new_interval = v.interval_s;

        let screen = ctx.content_rect();
        let resp = egui::Modal::new(egui::Id::unique("photocat-viewer")).show(ctx, |ui| {
            ui.set_min_size(screen.size() - egui::vec2(48.0, 48.0));
            ui.horizontal(|ui| {
                let name = Path::new(&f.path)
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or(&f.path);
                ui.label(egui::RichText::new(name).size(16.0));
                let date = f
                    .taken_at
                    .as_deref()
                    .map(|d| d.replace('T', " ").chars().take(16).collect::<String>())
                    .unwrap_or_else(|| t(lang, "no date").into());
                ui.label(egui::RichText::new(date).size(11.0).color(dim(ui)));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(t(lang, "Close")).clicked() {
                        close = true;
                    }
                    ui.label(
                        egui::RichText::new(i18n::position(lang, v.idx + 1, v.ids.len()))
                            .size(11.0)
                            .color(dim(ui)),
                    );
                });
            });
            ui.add_space(4.0);
            // The playback bar: paging, play and pause, seeking, the volume.
            ui.horizontal_wrapped(|ui| {
                if icon_button(ui, Icon::Prev, 32.0, t(lang, "previous file — ←")).clicked() {
                    step_click = -1;
                }
                let playing = video_active && self.video.as_ref().is_some_and(|p| p.is_playing());
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
                if icon_button(ui, Icon::Next, 32.0, t(lang, "next file — →")).clicked() {
                    step_click = 1;
                }
                if f.kind == "video" {
                    if video_active && let Some(p) = self.video.as_ref() {
                        let dur = p.duration_s().max(0.001);
                        let mut pos = p.position_s().min(dur);
                        ui.label(
                            egui::RichText::new(format!(
                                "{} / {}",
                                db::duration(pos),
                                db::duration(dur)
                            ))
                            .size(11.0)
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
                                egui::Slider::new(&mut vol, 0.0..=1.0).show_value(false),
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
                            .selectable_label(true, t(lang, "Stop the video"))
                            .on_hover_text(t(lang, "stop and go back to the poster frame"))
                            .clicked()
                    {
                        stop_video = true;
                    }
                } else {
                    if ui
                        .selectable_label(
                            v.slideshow,
                            if v.slideshow {
                                t(lang, "Stop the slideshow")
                            } else {
                                t(lang, "Slideshow")
                            },
                        )
                        .on_hover_text(t(lang, "space — start/stop; walks the current list"))
                        .clicked()
                    {
                        new_slideshow = !v.slideshow;
                    }
                    ui.label(egui::RichText::new(t(lang, "interval:")).size(11.0).color(dim(ui)));
                    for secs in [3u64, 5, 10] {
                        if ui
                            .selectable_label(v.interval_s == secs, i18n::seconds(lang, secs))
                            .clicked()
                        {
                            new_interval = secs;
                        }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("−").on_hover_text(t(lang, "zoom out")).clicked() {
                    new_zoom = (v.zoom - 0.25).max(0.5);
                }
                if ui.button(t(lang, "Fit")).clicked() {
                    new_zoom = 1.0;
                }
                if ui.button("+").on_hover_text(t(lang, "zoom in")).clicked() {
                    new_zoom = (v.zoom + 0.25).min(4.0);
                }
                ui.label(
                    egui::RichText::new(format!(
                        "{} {:.0}%",
                        t(lang, "zoom"),
                        v.zoom * 100.0
                    ))
                    .size(11.0)
                    .color(dim(ui)),
                );
                if v.zoom > 1.0 {
                    ui.label(
                        egui::RichText::new(t(lang, "drag to move the frame"))
                            .size(11.0)
                            .color(dim(ui)),
                    );
                }
                if ui
                    .selectable_label(v.show_text, t(lang, "Text on the photo"))
                    .clicked()
                {
                    new_text = !v.show_text;
                }
                if f.kind != "video" && ui.button(t(lang, "In the system viewer")).clicked() {
                    open_video = true;
                }
            });
            ui.separator();
            let avail = ui.available_size();
            let content_h = (avail.y - 8.0).max(120.0);
            let mut image_resp = None;
            if v.show_text {
                ui.horizontal(|ui| {
                    let img_w = (avail.x * 0.62).max(160.0);
                    ui.allocate_ui(egui::vec2(img_w, content_h), |ui| {
                        image_resp = Some(draw_viewer_image(
                            ui,
                            ViewerTextures {
                                full: current_full,
                                preview,
                                stale,
                                video: video_frame,
                                paused,
                            },
                            &f,
                            v.zoom,
                            &mut new_pan,
                            egui::vec2(img_w, content_h),
                            lang,
                        ));
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("viewer-text")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(t(lang, "Text on the photo"))
                                    .size(11.0)
                                    .color(dim(ui)),
                            );
                            let text = self.ocr_text.get(&id).cloned().unwrap_or_default();
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
                image_resp = Some(draw_viewer_image(
                    ui,
                    ViewerTextures {
                        full: current_full,
                        preview,
                        stale,
                        video: video_frame,
                        paused,
                    },
                    &f,
                    v.zoom,
                    &mut new_pan,
                    egui::vec2(avail.x, content_h),
                    lang,
                ));
            }
            // A click on the video frame is like a big ▶ button: play or pause.
            if f.kind == "video" && image_resp.is_some_and(|r| r.clicked()) {
                if video_active {
                    toggle_video = true;
                } else {
                    start_video = true;
                }
            }
        });
        if resp.backdrop_response.clicked() {
            close = true;
        }
        v.zoom = new_zoom;
        v.pan = new_pan;
        v.show_text = new_text;
        if v.slideshow != new_slideshow {
            v.last_advance = std::time::Instant::now();
        }
        v.slideshow = new_slideshow;
        v.interval_s = new_interval;
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
        if close {
            self.stop_video();
            self.viewer = None;
            self.viewer_tex = None;
        } else {
            self.request_around(&v.ids, v.idx);
            self.viewer = Some(v);
        }
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
            tag: self.tag_filter.clone(),
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
        } else if now.tag != self.pick_scope.tag {
            t(lang, "the tag filter changed")
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
            .filter(|(_, r)| match &self.tag_filter {
                None => true,
                Some(want) => {
                    self.user_tags
                        .get(&r.id)
                        .is_some_and(|tags| tags.iter().any(|t| t == want))
                        || self.tags.get(&r.id).is_some_and(|t| {
                            embed::tag_above(t, want, self.tag_threshold(want))
                        })
                }
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

    /// The tags as chips (M22.3): the word with the count of photos after a middle dot, the
    /// picked one in the accent, a click on the picked one clears it. The dictionary does not need
    /// a menu; the count next to a word honestly says "zero" before a click. Returns the counts,
    /// so the model block can warn that no tag passes the threshold.
    fn tag_chips(&mut self, ui: &mut egui::Ui) -> Vec<usize> {
        let lang = self.settings.lang;
        let labels = self.tag_labels();
        let counts = self.tag_counts();
        let mut own_tags: Vec<String> = self.user_tags.values().flatten().cloned().collect();
        own_tags.sort();
        own_tags.dedup();
        let mut picked: Option<Option<String>> = None;
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
            if tag_chip(ui, t(lang, "all tags"), self.tag_filter.is_none()) {
                picked = Some(None);
            }
            for (label, n) in labels.iter().zip(&counts) {
                let chosen = self.tag_filter.as_deref() == Some(label.as_str());
                let text = format!("{} · {n}", i18n::word(lang, label));
                if tag_chip(ui, &text, chosen) {
                    picked = Some(if chosen { None } else { Some(label.clone()) });
                }
            }
            for label in &own_tags {
                let chosen = self.tag_filter.as_deref() == Some(label.as_str());
                let n = self
                    .user_tags
                    .values()
                    .filter(|tags| tags.iter().any(|t| t == label))
                    .count();
                let text = format!("{}: {label} · {n}", t(lang, "mine"));
                if tag_chip(ui, &text, chosen) {
                    picked = Some(if chosen { None } else { Some(label.clone()) });
                }
            }
        });
        if let Some(tag) = picked {
            self.tag_filter = tag;
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

    /// The "all tags" threshold: a percentile over the best weight of every file.
    fn top_threshold(&self) -> f32 {
        if self.tags.len() < 100 {
            return self.settings.tag_show;
        }
        embed::percentile_threshold(&self.top_scores, self.settings.tag_show_pct)
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
) {
    let w = cs.tile.clamp(100.0, 260.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w + 26.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, w));
    let mut button_clicked = false;
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
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
        ui.painter().text(
            egui::pos2(rect.left() + 3.0, img_rect.bottom() + 3.0),
            egui::Align2::LEFT_TOP,
            date,
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(190),
        );
        ui.painter().text(
            egui::pos2(rect.right() - 3.0, img_rect.bottom() + 3.0),
            egui::Align2::RIGHT_TOP,
            format!("{:.2}", f.score),
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(150),
        );
        // A false merge is split right on the tile, without going to another screen.
        let btn = egui::Rect::from_min_size(
            egui::pos2(img_rect.right() - 62.0, img_rect.top() + 4.0),
            egui::vec2(58.0, 20.0),
        );
        if ui
            .place(btn, egui::Button::new(t(lang, "not him")).small())
            .on_hover_text(t(lang, "split this face into a new group"))
            .clicked()
        {
            cs.split_clicked = Some(f.id);
            button_clicked = true;
        }
        // A batch naming: mark several faces and give them one name.
        let pick = egui::Rect::from_min_size(
            egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
            egui::vec2(66.0, 20.0),
        );
        let is_picked = cs.picked.contains(&f.id);
        if ui
            .place(
                pick,
                egui::Button::new(t(lang, if is_picked { "picked" } else { "pick" }))
                    .small(),
            )
            .on_hover_text(t(lang, "mark this face for a shared name"))
            .clicked()
        {
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
                3.0,
                egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 170, 255)),
                egui::StrokeKind::Inside,
            );
        }
    }
    if cs.selected_file == Some(f.file_id) {
        ui.painter().rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 170, 255)),
            egui::StrokeKind::Inside,
        );
    }
    let pointer_on_button = ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| {
        p.x > img_rect.right() - 70.0 && p.y < img_rect.top() + 28.0
            || p.x < img_rect.left() + 74.0 && p.y < img_rect.top() + 28.0
    });
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

/// A tag row in the card: the label, the weight, the percentile and "remove".
fn tag_row(
    ui: &mut egui::Ui,
    tag: &str,
    score: f32,
    pct: Option<u32>,
    confident: Option<&str>,
    hide: &mut Option<String>,
    lang: i18n::Lang,
) {
    ui.horizontal(|ui| {
        let color = if confident == Some(tag) {
            accent(ui)
        } else {
            ui.visuals().text_color()
        };
        ui.label(
            egui::RichText::new(i18n::word(lang, tag))
                .size(12.0)
                .color(color),
        );
        ui.label(
            egui::RichText::new(match pct {
                Some(p) => format!("{score:.2} · {p}%"),
                None => format!("{score:.2}"),
            })
            .size(11.0)
            .color(dim(ui)),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button(t(lang, "remove"))
                .on_hover_text(t(lang, t(lang, "remove the tag from this photo; the original does not change")))
                .clicked()
            {
                *hide = Some(tag.to_string());
            }
        });
    });
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

/// The offset of a zoomed frame: the frame does not leave the edges of the area, and when it
/// is fitted (one of the sides is not bigger than the area) it is centred along that side.
fn clamp_pan(pan: egui::Vec2, image: egui::Vec2, area: egui::Rect) -> egui::Vec2 {
    let room = ((image - area.size()) / 2.0).max(egui::Vec2::ZERO);
    egui::vec2(pan.x.clamp(-room.x, room.x), pan.y.clamp(-room.y, room.y))
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
        ui.painter().image(
            t.id(),
            fit_rect(rect, t.size_vec2()),
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
    let normal = ui.visuals().text_color();
    let accent = egui::Color32::from_rgb(90, 170, 255);
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
                flush(&mut segment, matched, normal, accent, &mut job);
                matched = ch == '«';
            }
            _ => segment.push(ch),
        }
    }
    flush(&mut segment, matched, normal, accent, &mut job);
    job
}

/// A playback bar icon. The shapes are drawn by the painter, not by a font: emoji are
/// forbidden in the interface, and "▶" and "‖" differ in width and height between fonts.
#[derive(Clone, Copy)]
enum Icon {
    Prev,
    Next,
    Play,
    Pause,
    Volume,
}

/// An icon button of the playback bar: 32 px, the tooltip is mandatory.
fn icon_button(ui: &mut egui::Ui, icon: Icon, size: f32, hint: &str) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::click());
    let resp = resp
        .on_hover_text(hint)
        .on_hover_cursor(egui::CursorIcon::PointingHand);
    let visuals = ui.style().interact(&resp);
    ui.painter().rect_filled(rect, 4.0, visuals.bg_fill);
    paint_icon(ui.painter(), rect.center(), size * 0.30, icon, visuals.fg_stroke.color);
    resp
}

/// An icon without an action — a caption next to the slider (the volume).
fn icon_mark(ui: &mut egui::Ui, icon: Icon, size: f32) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
    paint_icon(
        ui.painter(),
        rect.center(),
        size * 0.30,
        icon,
        ui.visuals().text_color(),
    );
}

/// The geometry of an icon: `s` is half the height of the shape, `c` its centre.
fn paint_icon(p: &egui::Painter, c: egui::Pos2, s: f32, icon: Icon, color: egui::Color32) {
    match icon {
        Icon::Play => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - s * 0.7, c.y - s),
                    egui::pos2(c.x - s * 0.7, c.y + s),
                    egui::pos2(c.x + s, c.y),
                ],
                color,
                egui::Stroke::NONE,
            ));
        }
        Icon::Pause => {
            let w = s * 0.44;
            for dx in [-s * 0.42, s * 0.42] {
                p.rect_filled(
                    egui::Rect::from_center_size(
                        egui::pos2(c.x + dx, c.y),
                        egui::vec2(w, s * 2.0),
                    ),
                    1.0,
                    color,
                );
            }
        }
        Icon::Prev | Icon::Next => {
            let dir = if matches!(icon, Icon::Next) { 1.0 } else { -1.0 };
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - dir * s * 0.9, c.y - s),
                    egui::pos2(c.x - dir * s * 0.9, c.y + s),
                    egui::pos2(c.x + dir * s * 0.4, c.y),
                ],
                color,
                egui::Stroke::NONE,
            ));
            p.rect_filled(
                egui::Rect::from_center_size(
                    egui::pos2(c.x + dir * s * 0.95, c.y),
                    egui::vec2(s * 0.36, s * 2.0),
                ),
                1.0,
                color,
            );
        }
        Icon::Volume => {
            p.add(egui::Shape::convex_polygon(
                vec![
                    egui::pos2(c.x - s, c.y - s * 0.35),
                    egui::pos2(c.x - s * 0.5, c.y - s * 0.35),
                    egui::pos2(c.x - s * 0.05, c.y - s),
                    egui::pos2(c.x - s * 0.05, c.y + s),
                    egui::pos2(c.x - s * 0.5, c.y + s * 0.35),
                    egui::pos2(c.x - s, c.y + s * 0.35),
                ],
                color,
                egui::Stroke::NONE,
            ));
            p.add(egui::Shape::line(
                vec![
                    egui::pos2(c.x + s * 0.2, c.y - s * 0.5),
                    egui::pos2(c.x + s * 0.6, c.y),
                    egui::pos2(c.x + s * 0.2, c.y + s * 0.5),
                ],
                egui::Stroke::new(1.5, color),
            ));
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
/// Returns the response of the frame area: a click on a video is play or pause.
fn draw_viewer_image(
    ui: &mut egui::Ui,
    tex: ViewerTextures<'_>,
    f: &FileRow,
    zoom: f32,
    pan: &mut egui::Vec2,
    size: egui::Vec2,
    lang: i18n::Lang,
) -> egui::Response {
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
        return resp;
    }
    if let Some((tid, t)) = tex.full
        && *tid == f.id
    {
        let ts = t.size_vec2();
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
        let img = egui::Rect::from_center_size(area.center() + *pan, size);
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        p.image(
            t.id(),
            img,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else if let Some(t) = tex.stale {
        // While the next frame is decoding the previous sharp one is held — without mush.
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        p.image(
            t.id(),
            fit_rect(area.shrink(1.0), t.size_vec2()),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        ui.painter().text(
            egui::pos2(area.left() + 8.0, area.bottom() - 8.0),
            egui::Align2::LEFT_BOTTOM,
            i18n::t(lang, "loading the next frame…"),
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(235),
        );
    } else if let Some(t) = tex.preview {
        // The native 320 px thumbnail in the centre, without stretching it over the whole screen.
        let size = t.size_vec2();
        let scale = (area.width() / size.x).min(area.height() / size.y).min(1.0);
        let rect = egui::Rect::from_center_size(area.center(), size * scale);
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        p.image(
            t.id(),
            rect,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::from_gray(230),
        );
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
    if f.kind == "video" {
        paint_play_overlay(ui, area, lang);
    }
    resp
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

/// The main view from the mock-up: tiles of equal width, a 4:3 frame, the photo fitted
/// whole (neither stretched nor cropped), a caption strip under the frame. The columns split
/// the width of the area, as `grid-cols-6` does in the mock-up.
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
    // The 4:3 frame is the proportion of the mock-up tile; the photo is fitted into it whole.
    let h = (w * 0.75).round();
    for row in files.chunks(per_row) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = egui::vec2(gap, gap);
            for &f in row {
                draw_cell(ui, ctx, cs, f, w, h);
            }
        });
        ui.add_space(gap);
    }
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
        let right = if cs.show_keep || cs.trash_mode { 96.0 } else { 132.0 };
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
            let btn = egui::Rect::from_min_size(
                egui::pos2(rect.right() - 78.0, rect.center().y - 10.0),
                egui::vec2(70.0, 20.0),
            );
            if ui
                .place(btn, egui::Button::new(t(cs.lang, "restore")).small())
                .clicked()
            {
                cs.restore_clicked = Some(f.id);
            }
        }
        if resp.clicked() {
            let mods = ui.input(|i| i.modifiers);
            cs.clicked = Some((f.id, pick_kind(&mods)));
        }
        if resp.double_clicked() {
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
                draw_cell(ui, ctx, cs, f, w, h);
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
/// card). The frame fills the tile completely: the tile proportion equals the photo proportion.
fn draw_cell(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, f: &FileRow, w: f32, h: f32) {
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
        ui.painter().rect_filled(img_rect, top, th.canvas);
        ui.painter().rect_filled(cap_rect, bottom, th.panel);
        let drawn = paint_thumb(ui, ctx, cs, f, img_rect);
        if drawn {
            round_preview(ui, img_rect, RADIUS_CARD, cs.bg);
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
            let btn = egui::Rect::from_min_size(
                egui::pos2(img_rect.right() - 70.0, img_rect.bottom() - 26.0),
                egui::vec2(66.0, 20.0),
            );
            if ui
                .place(btn, egui::Button::new(t(cs.lang, "restore")).small())
                .on_hover_text(t(
                    cs.lang,
                    "bring the file back into the usual views; on disk it was untouched",
                ))
                .clicked()
            {
                cs.restore_clicked = Some(f.id);
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
    if resp.clicked() {
        let mods = ui.input(|i| i.modifiers);
        cs.clicked = Some((f.id, pick_kind(&mods)));
    }
    if resp.double_clicked() {
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
        // There is no card on "Integrity": it checks folders, not files.
        let details = self.details_open
            && self.view != View::Integrity
            && ui.available_width() >= DETAILS_MIN_W;
        let show_card = self.details_open && self.view != View::Integrity && !details;
        egui::Panel::left("left")
            .exact_size(LEFT_W)
            .resizable(false)
            .show(ui, |ui| self.left_panel(ui, show_card));
        if details {
            egui::Panel::right("details")
                .exact_size(RIGHT_W)
                .resizable(false)
                .show(ui, |ui| self.details_panel(ui));
        }

        self.settings_modal(ctx);
        self.roots_modal(ctx);
        self.picker_modal(ctx);
        self.storage_modal(ctx);
        self.album_modal(ctx);
        self.export_modal(ctx);
        self.trash_modal(ctx);
        self.poll_photo(ctx);
        self.poll_video(ctx);

        // "Integrity" is a form, not a grid: it needs neither the tiles nor their cache nor the order.
        // The screen is drawn before `self.items`/`self.textures` are borrowed and leaves the frame by itself.
        if self.view == View::Integrity {
            egui::CentralPanel::default().show(ui, |ui| self.integrity_panel(ui));
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
        let tag_filter_now = self.tag_filter.clone();
        let tag_filter_raw = tag_filter_now
            .as_deref()
            .map(|t| self.tag_threshold(t))
            .unwrap_or_else(|| self.top_threshold());
        let items = std::mem::take(&mut self.items);
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
            cell_style: self.settings.cell_style,
            time_only: false,
            // The background a tile lies on: the corner cuts of the preview paint exactly it.
            bg: if self.view == View::Dupes {
                ui.visuals().faint_bg_color
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
        let mut tile_save = false;
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
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).size(18.0).strong());
                ui.label(num(ui, count, 12.0).color(dim(ui)));
                // PhotoPrism: the tile size lives in the content panel, not in the left one.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // The size sets the tile, the row height or the list row — but the slider is always
                    // visible (M22.2a): the heading does not jump on a view change, and a list of 2000 rows
                    // has something to scale.
                    {
                        let size = ui.add(
                            egui::Slider::new(&mut self.tile_size, 100.0..=260.0)
                                .step_by(10.0)
                                .show_value(false),
                        );
                        if size.drag_stopped() || size.lost_focus() {
                            tile_save = true;
                        }
                        ui.label(
                            egui::RichText::new(format!(
                                "{} {:.0}",
                                t(lang, "Tiles"),
                                self.tile_size
                            ))
                            .size(11.0)
                            .color(dim(ui)),
                        )
                        .on_hover_text(t(lang, tile_hint(self.settings.cell_style)));
                    }
                    // The cell view (M22): the mock-up tile is the main one, the rows and the list come second.
                    let mut style = self.settings.cell_style;
                    for (value, label, hint) in [
                        (
                            db::CellStyle::Tiles,
                            t(lang, "Tile"),
                            t(lang, "as in the mock-up: a 4:3 frame, the snapshot fitted whole"),
                        ),
                        (
                            db::CellStyle::Rows,
                            t(lang, "Rows"),
                            t(lang, "proportion rows: the tile width follows the frame"),
                        ),
                        (
                            db::CellStyle::List,
                            t(lang, "List"),
                            t(lang, "a list: name, date and size in one row"),
                        ),
                    ] {
                        if ui
                            .selectable_label(
                                style == value,
                                egui::RichText::new(label).size(11.0),
                            )
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
                    // The order is available in the grid and inside an album (M19), but not in the tree.
                    let album_scope_now =
                        view == View::Albums && self.album_selected.is_some();
                    if view == View::Grid || album_scope_now {
                        // M17: how to look (months or a run) and in which order.
                        // While a semantic search runs the search sets the order — the switch is dimmed.
                        let searching = self.semantic_hits.is_some();
                        let mut sort = self.settings.grid_sort;
                        let sort_resp = ui.add_enabled_ui(!searching, |ui| {
                            ui.menu_button(t(lang, "Sort"), |ui| {
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
                            })
                        });
                        sort_resp
                            .inner
                            .response
                            .on_disabled_hover_text(t(lang, "the search sets the order"))
                            .on_hover_text(t(lang, "tile order: date, name or size"));
                        if sort != self.settings.grid_sort {
                            self.settings.grid_sort = sort;
                            view_save = true;
                        }
                        if view == View::Grid {
                            let mut layout = self.settings.grid_layout;
                            for (value, label) in [
                                (db::GridLayout::Flat, t(lang, "Flat")),
                                (db::GridLayout::Months, t(lang, "By months")),
                            ] {
                                let r = ui.add_enabled_ui(!searching, |ui| {
                                    ui.selectable_label(layout == value, label)
                                });
                                if r.inner.clicked() {
                                    layout = value;
                                }
                                r.inner.on_disabled_hover_text(t(lang, "the search sets the order"));
                            }
                            if layout != self.settings.grid_layout {
                                self.settings.grid_layout = layout;
                                view_save = true;
                            }
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if matches!(view, View::Grid | View::Calendar | View::Albums) {
                clear_chips = draw_filter_chips(ui, &chips, (visible.len(), items.len()));
            }
            ui.add_space(2.0);
            ui.separator();
            match view {
            View::Grid => {
                if items.is_empty() {
                    ui.add_space(12.0);
                    ui.label(t(
                        lang,
                        "Add a folder with the “Folders” button or type a path above, then press “Index”.",
                    ));
                    return;
                }
                if visible.is_empty() {
                    ui.add_space(12.0);
                    match &tag_filter_now {
                        Some(tag) => ui.label(i18n::fill(
                            lang,
                            "no photos with the tag “{}” above the threshold {}% ({}) — lower the threshold slider in the left panel",
                            &[
                                tag.clone(),
                                self.settings.tag_show_pct.to_string(),
                                format!("{tag_filter_raw:.2}"),
                            ]
                        )),
                        None => ui.label(t(lang, "nothing found")),
                    };
                    if ui.button(t(lang, "Reset the filter")).clicked() {
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
                // The tree (M19): indents and collapsing. A click opens an album, the triangle expands
                // the sub-albums without changing the selection.
                ui.horizontal_wrapped(|ui| {
                    // The main action of the view carries the accent (M22.6, the mock-up's "+").
                    let new_album = primary_button(ui, t(lang, "+ New album"));
                    if ui.add(new_album).clicked() {
                        album_dialog_request = Some(AlbumDialog::New);
                    }
                    if albums.is_empty() {
                        ui.label(
                            egui::RichText::new(t(
                                lang,
                                "An album is a set of photos with an order and sub-albums. Data only: the files on disk do not change.",
                            ))
                            .size(11.0)
                            .color(dim(ui)),
                        );
                    }
                });
                if !albums.is_empty() {
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
                }
                ui.separator();
                let chosen = album_selected.and_then(|id| albums.iter().find(|n| n.id == id));
                let Some(node) = chosen else {
                    ui.add_space(8.0);
                    ui.label(
                        t(
                            lang,
                            "Pick an album in the tree or create a new one. Inside are the sub-albums and the photos in the order they will lie on disk.",
                        ),
                    );
                    return;
                };
                let node = node.clone();
                // M22.6: the open album is a card header — the name at 14 px and the facts of the
                // card in mono 11 — over the same action row as before.
                ui.add_space(4.0);
                album_header(ui, &node, lang);
                ui.add_space(2.0);
                ui.horizontal_wrapped(|ui| {
                    if ui.button(t(lang, "+ Sub-album")).clicked() {
                        album_dialog_request = Some(AlbumDialog::Sub);
                    }
                    if ui.button(t(lang, "Rename")).clicked() {
                        album_dialog_request = Some(AlbumDialog::Rename);
                    }
                    if ui.button(t(lang, "Move to…")).clicked() {
                        album_dialog_request = Some(AlbumDialog::Move);
                    }
                    if ui
                        .add_enabled(node.position > 0, egui::Button::new(t(lang, "Up")))
                        .on_hover_text(t(lang, "move the album up among its siblings"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::Up);
                    }
                    if ui
                        .add_enabled(
                            node.position + 1 < albums.iter().filter(|n| n.parent_id == node.parent_id).count() as i64,
                            egui::Button::new(t(lang, "Down")),
                        )
                        .on_hover_text(t(lang, "move the album down among its siblings"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::Down);
                    }
                    if ui
                        .button(t(lang, "Delete the album"))
                        .on_hover_text(t(lang, "delete the rows of the album and its sub-albums; the files on disk stay"))
                        .clicked()
                    {
                        album_dialog_request = Some(AlbumDialog::Delete);
                    }
                });
                let children: Vec<db::AlbumNode> = albums
                    .iter()
                    .filter(|n| n.parent_id == node.id)
                    .cloned()
                    .collect();
                if !children.is_empty() {
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new(t(lang, "Sub-albums")).size(11.0).color(dim(ui)));
                    // The cards scale with the "Tiles" slider, so a row of them can outgrow the
                    // window: one row is always whole, two rows at most, the rest scrolls, and the
                    // grid below keeps 220 px — many sub-albums must not hide the album's files.
                    let card_h = album_card_size(self.tile_size).y;
                    let room =
                        (ui.available_height() - 220.0).clamp(card_h, card_h * 2.0 + TILE_GAP);
                    egui::ScrollArea::vertical()
                        .id_salt("album-cards")
                        .max_height(room)
                        .auto_shrink([false, true])
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(TILE_GAP, TILE_GAP);
                                for child in &children {
                                    // The cover is the first snapshot of the sub-album in its order:
                                    // the very preview the grid would show first.
                                    let cover = child.cover.as_ref().and_then(|sha| {
                                        items
                                            .iter()
                                            .find(|f| f.thumb.as_deref() == Some(sha.as_str()))
                                    });
                                    if draw_album_cell(
                                        ui,
                                        ctx,
                                        &mut cells,
                                        child,
                                        cover,
                                        self.tile_size,
                                        lang,
                                    ) {
                                        album_action = Some(AlbumAction::Select(child.id));
                                    }
                                }
                            });
                        });
                }
                ui.add_space(6.0);
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    // "All files" inside an album — a temporary look to gather photos into an album:
                    // otherwise the selection lives only in that grid (M18).
                    if ui
                        .selectable_label(!album_browse, t(lang, "In the album"))
                        .on_hover_text(t(lang, "the photos of this album in its order"))
                        .clicked()
                    {
                        album_browse = false;
                    }
                    if ui
                        .selectable_label(album_browse, "All files")
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
                            egui::Button::new(i18n::fill(lang, "Add the picked ones ({})", &[picked_now.to_string()])),
                        )
                        .on_hover_text(t(lang, "add the marked photos to the end of the album; a repeat does not duplicate rows"))
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
                            egui::Button::new(i18n::fill(lang, "Remove from the album ({})", &[in_album.to_string()])),
                        )
                        .on_hover_text(t(lang, "the album rows go away; the files on disk do not change"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::RemovePicked);
                    }
                    let file_in_album =
                        self.selected.is_some_and(|id| self.album_set.contains(&id));
                    if ui
                        .add_enabled(file_in_album, egui::Button::new(t(lang, "File up")))
                        .on_hover_text(t(lang, "move the selected photo up in the album order"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::FileUp);
                    }
                    if ui
                        .add_enabled(file_in_album, egui::Button::new(t(lang, "File down")))
                        .on_hover_text(t(lang, "move the selected photo down in the album order"))
                        .clicked()
                    {
                        album_action = Some(AlbumAction::FileDown);
                    }
                    if ui
                        .button(t(lang, "Export to disk…"))
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
                            ui.group(|ui| {
                                let reason = g.reason.label(lang);
                                let advice_text = g.advice.label(lang);
                                let head = i18n::fill(lang, "#{} — {} · {} files · {} spare", &[(n + 1).to_string(), reason.to_string(), g.members.len().to_string(), db::human_in(lang, g.waste).to_string()]);
                                ui.label(head);
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
                                ui.label(advice);
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
                            if ui.button(t(lang, "Today")).clicked() {
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
                                .add_enabled(prev_enabled, egui::Button::new("←"))
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
                                .add_enabled(next_enabled, egui::Button::new("→"))
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
                                            .button(format!(
                                                "{} {} {}",
                                                t(lang, "Go to"),
                                                i18n::month_name(lang, m as usize),
                                                y
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
                        ui.heading(t(lang, "Photos with everyone together"));
                        ui.label(
                            egui::RichText::new(names.join(" + "))
                                .size(12.0)
                                .color(dim(ui)),
                        );
                    });
                    let found = i18n::fill(lang, "photos: {}", &[ids.len().to_string()]);
                    ui.label(found);
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
                    ui.horizontal(|ui| {
                        ui.heading(if c.label.is_empty() {
                            t(lang, "no name").to_string()
                        } else {
                            c.label.clone()
                        });
                        ui.label(format!("{} · {}", faces_n(c.faces as usize), photos_n(c.photos as usize)));
                    });
                    ui.horizontal(|ui| {
                        ui.label(t(lang, "Person's name:"));
                        ui.add(
                            egui::TextEdit::singleline(&mut face_label_input)
                                .hint_text(t(lang, "e.g. Lisa"))
                                .desired_width(220.0),
                        );
                        if ui.button(t(lang, "Save the name")).clicked() {
                            face_save_label = Some(face_label_input.trim().to_string());
                        }
                    });
                    ui.label(t(
                        lang,
                        "A name pins the group: rebuilding by threshold will not shuffle it. A wrong merge is split by the “not him” button on the tile.",
                    ));
                } else {
                    ui.label(
                        egui::RichText::new(t(
                            lang,
                            "Pick a group on the left to name it, or split the tiles.",
                        ))
                        .size(12.0)
                        .color(dim(ui)),
                    );
                }
                let shown: Vec<&db::FaceView> = face_views
                    .iter()
                    .filter(|f| face_selected.is_none() || f.cluster_id == face_selected)
                    .collect();
                // A batch naming: mark faces with the "pick" button and give them one name.
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
                            egui::Button::new(t(lang, "Name the picked people")),
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
                            egui::Button::new(t(lang, "Unpick")),
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
                                draw_face_cell(ui, ctx, &mut face_cells, f, lang);
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
                        ui.label(format!(
                            "{} · {}",
                            files_n(trash_items.len()),
                            db::human_in(lang, bytes)
                        ));
                        if ui.button(t(lang, "Restore all")).clicked() {
                            trash_restore_all = true;
                        }
                        let b = primary_button(
                            ui,
                            &format!(
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
        // The tile size is written to the settings when the slider is released, not on every frame.
        if tile_save {
            self.settings.tile_size = self.tile_size;
            self.save_settings();
        }
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
        // A chip removes exactly its own filter; the grid is recomputed on the next frame.
        if clear_chips.query {
            self.query.clear();
        }
        if clear_chips.tag {
            self.tag_filter = None;
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
        App, COAST_DECAY_S, CalScale, IntegrityJob, IntegrityOutcome, IntegrityPlan, KindFilter,
        PickClick, Scroll, SettingsTab, StorageAction, View, Viewer, WHEEL_NOTCH_PTS, clamp_pan,
        album_card_size, days_in_month, db, first_weekday_monday, fit_rect, list_meta_px,
        list_name_px,
        list_row_height, monday_of, month_groups, nearest_file_month, parse_date, pick_kind,
        score_percentile, shift_month, shift_to_file_month,
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
            depth: 0,
            files: ids.len() as i64,
            trashed: 0,
            subalbums: covers.len() as i64,
            bytes: app.items.iter().map(|f| f.size).sum(),
            cover: covers.first().cloned(),
        }];
        for (i, cover) in covers.iter().enumerate() {
            albums.push(db::AlbumNode {
                id: i as i64 + 2,
                parent_id: 1,
                position: i as i64,
                name: format!("Month {}", i + 1),
                note: String::new(),
                depth: 1,
                files: 12,
                trashed: 0,
                subalbums: 0,
                bytes: 12 * 1024 * 1024,
                cover: Some(cover.clone()),
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

    fn viewer_with(ids: Vec<i64>, idx: usize) -> Viewer {
        Viewer {
            ids,
            idx,
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
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
        assert!(texts.iter().any(|t| t == "документ"));
        assert!(texts.iter().any(|t| t == "мебель"));
        assert!(
            !texts.iter().any(|t| t == "дети"),
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
            "Date",
            "Camera",
            "Size",
            "AI tags",
            "My tags",
            "Text on the photo",
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
            "Теги ИИ",
            "Свои теги",
            "Текст на фото",
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
        let texts = window_frame(&mut app, &egui::Context::default());
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
        assert!(
            texts.iter().any(|t| t.starts_with("#1 — ") && t.contains("2 files")),
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
        // A tall window: the group list in the left panel must not be cut off.
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
            "not him",
            "pick",
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
            "restore",
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
            "No date:",
            "No preview:",
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
            "Без даты:",
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
            "Close",
            "Slideshow",
            "Fit",
            "interval:",
            "3 s",
            "Text on the photo",
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
            "Закрыть",
            "Вписать",
            "Слайд-шоу",
            "интервал:",
            "Текст на фото",
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
        app.top_scores = embed::top_score_distribution(&app.tags);
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
                    s.color == accent && (s.width - 1.0).abs() < 0.01 && r.left() > super::LEFT_W
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

    /// M22.2a: the size slider is visible in the heading in all three cell views — it is in the
    /// list too, so the heading does not jump when switching.
    #[test]
    fn the_size_slider_shows_in_all_three_views() {
        let mut app = m17_app();
        app.settings.lang = i18n::Lang::Ru;
        app.tile_size = 200.0;
        for style in [db::CellStyle::Tiles, db::CellStyle::Rows, db::CellStyle::List] {
            app.settings.cell_style = style;
            let texts = window_frame(&mut app, &egui::Context::default());
            assert!(
                texts.iter().any(|t| t == "Плитки 200"),
                "the size slider is gone in the {style:?} view: {texts:?}"
            );
        }
    }

    /// M22.3: the year block of the left panel is a filter, not a decoration. The counts come
    /// from the dates of the index, a click writes the year into the date filter and a second
    /// click (or "all years") clears it. A year without files is not in the list at all.
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
        // At most five rows, however deep the archive is.
        app.items = (0..9)
            .map(|i| dated(i + 1, "x.jpg", &format!("{}-05-01T08:00", 2010 + i)))
            .collect();
        assert_eq!(app.year_counts().len(), 5);
        assert_eq!(app.year_counts()[0].0, "2018");

        app.items = vec![
            dated(1, "a.jpg", "2024-05-01T08:00"),
            dated(2, "b.jpg", "2024-09-09T10:00"),
            dated(3, "c.jpg", "2023-01-01T09:00"),
            undated(5, "undated.jpg"),
        ];
        // A semantic search plus a year is a contradiction: the click drops the words.
        app.semantic_hits = Some(vec![1]);
        app.ocr_snippets.insert(1, "text".into());
        app.pick_year(Some("2024"));
        assert_eq!(app.query, "2024");
        assert_eq!(app.picked_year().as_deref(), Some("2024"));
        assert!(app.semantic_hits.is_none(), "the words are gone with the year");
        assert!(app.ocr_snippets.is_empty());
        let visible: Vec<String> = app
            .filtered()
            .iter()
            .map(|&i| super::file_name(&app.items[i].path))
            .collect();
        assert_eq!(visible, ["a.jpg", "b.jpg"], "only 2024: {visible:?}");
        // The way back: the filter is empty again and every file is visible.
        app.pick_year(None);
        assert!(app.query.is_empty());
        assert!(app.picked_year().is_none());
        assert_eq!(app.filtered().len(), 4, "all years are back");
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
        app.pick_year(Some("2024"));
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
        // Picking a year adds "All years" without growing the block: six cells are still three
        // rows, so the summary does not move and nothing below the years is pushed off.
        app.pick_year(Some("2024"));
        let _ = window_layout(&mut app, &ctx, size);
        let picked = window_layout(&mut app, &ctx, size);
        // The picked year is also written in the query field above (and in the chip of the grid),
        // so the last match inside the left panel is the year row itself.
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
        app.tag_filter = None;
        let _ = chips_layout(&mut app, 236.0);
        let (plain_size, plain) = chips_layout(&mut app, 236.0);
        app.tag_filter = Some("beach".into());
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
            picked.iter().any(|(t, _)| t == "all tags"),
            "no way back to all tags: {picked:?}"
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
            depth: 0,
            files: 3,
            trashed: 0,
            subalbums: 0,
            bytes: 30,
            cover: None,
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
            texts.iter().any(|t| t == "family archive"),
            "no English subtitle: {texts:?}"
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
        assert_eq!(super::LIGHT.accent, egui::Color32::from_rgb(0xd9, 0x77, 0x06));
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
            depth: usize::from(parent != 0),
            files,
            trashed: 0,
            subalbums: subs,
            bytes: 2 * 1024 * 1024,
            cover: None,
        }
    }

    /// M22.6: a sub-album card is one model — the width from the "Tiles" slider, a 16:10 canvas
    /// cover, the name at 13 px and the mono facts at 10 px below it.
    #[test]
    fn the_album_card_follows_the_tiles_slider() {
        let small = album_card_size(100.0);
        let big = album_card_size(260.0);
        assert_eq!(small.x, 100.0);
        assert_eq!(big.x, 260.0);
        // 8 px pad + a 53 px cover (84 × 10/16) + 6 px + the name + the facts + 8 px pad.
        assert!((small.y - 105.0).abs() < 0.01, "{}", small.y);
        // 8 + a 153 px cover (244 × 10/16) + 6 + 16 + 14 + 8.
        assert!((big.y - 205.0).abs() < 0.01, "{}", big.y);
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
            super::draw_album_cell(ui, &ctx, &mut cells, &node, None, tile, i18n::Lang::En);
            used = ui.min_rect().height();
        });
        out.drop_without_applying_deltas();
        used
    }

    /// M22.6: the card grows with the slider: a 105 px card at 100, a 205 px one at 260, and the
    /// drawn height is the model's.
    #[test]
    fn the_album_card_draws_at_the_model_height() {
        let small = album_card_frame_height(100.0);
        let big = album_card_frame_height(260.0);
        assert!((small - 105.0).abs() < 0.01, "a card at 100: {small}");
        assert!((big - 205.0).abs() < 0.01, "a card at 260: {big}");
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
        // The sub-album card: the name, the count of files and the size on the card itself.
        assert!(texts.iter().any(|t| t == "May"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "12 files"), "{texts:?}");
        assert!(texts.iter().any(|t| t == "2.0 MiB"), "{texts:?}");
    }

}
