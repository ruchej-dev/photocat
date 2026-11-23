//! GUI на egui: сетка превью, группы дублей, таймлайн, запуск индексации.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::dupes::{self, Group};
use crate::embed;
use crate::faces;
use crate::index;
use crate::ocr;
use crate::player;
use crate::recycle;
use crate::storage;
use crate::tools::{self, Tool};
use anyhow::Result;
use chrono::Datelike;
use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

type SearchReply = Result<(embed::Model, Vec<(i64, f32)>, f32)>;
type EmbedReply = Result<(embed::EmbedStats, embed::RetagStats)>;
type OcrReply = Result<ocr::Stats>;
type TrashReply = Result<recycle::Report>;
type PhotoReply = (i64, Result<(usize, usize, Vec<u8>)>);

enum FaceReply {
    Scan(Result<faces::Stats>),
    Cluster(Result<faces::ClusterStats>),
}

#[derive(Clone, Copy, PartialEq)]
enum View {
    Grid,
    Dupes,
    Calendar,
    Faces,
    Trash,
}

/// Формат в сетке: всё, только снимки, только видео (M16). Одно понятие «фильтр»
/// живёт в секции «Фильтр» левой панели и повторяется чипом над сеткой.
#[derive(Clone, Copy, PartialEq, Eq)]
enum KindFilter {
    All,
    Images,
    Videos,
}

impl KindFilter {
    fn label(self) -> &'static str {
        match self {
            KindFilter::All => "Все",
            KindFilter::Images => "Фото",
            KindFilter::Videos => "Видео",
        }
    }

    /// Подпись чипа над сеткой; `None` — фильтра нет.
    fn chip(self) -> Option<&'static str> {
        match self {
            KindFilter::All => None,
            KindFilter::Images => Some("только фото"),
            KindFilter::Videos => Some("только видео"),
        }
    }
}

/// Что именно очищаем в «Хранилище»: кэш превью, кропы лиц или строки индекса.
#[derive(Clone, Copy, PartialEq)]
enum StorageAction {
    Thumbs,
    Faces,
    Index,
}

/// Градация календаря: год, месяц, неделя и день. Двойной клик проваливает
/// на слой ниже, вкладки слева от заголовка возвращают наверх (docs/UI.md).
#[derive(Clone, Copy, PartialEq, Debug)]
enum CalScale {
    Year,
    Month,
    Week,
    Day,
}

impl CalScale {
    /// Слой ниже по двойному клику: год → месяц → неделя → день; ниже дня нет.
    fn deeper(self) -> Option<CalScale> {
        match self {
            CalScale::Year => Some(CalScale::Month),
            CalScale::Month => Some(CalScale::Week),
            CalScale::Week => Some(CalScale::Day),
            CalScale::Day => None,
        }
    }
}

/// Текстуры для полного экрана: текущий кадр, миниатюра и предыдущий резкий кадр.
struct ViewerTextures<'a> {
    full: Option<&'a (i64, egui::TextureHandle)>,
    preview: Option<&'a egui::TextureHandle>,
    stale: Option<&'a egui::TextureHandle>,
    /// Кадр встроенного плеера: пока он есть, он важнее статичного снимка.
    video: Option<&'a egui::TextureHandle>,
    /// Видео на паузе: кадр есть, но по центру нужна кнопка ▶.
    paused: bool,
}

/// Полноэкранный просмотр: один снимок поверх всей сетки, со слайд-шоу и текстом.
struct Viewer {
    ids: Vec<i64>,
    idx: usize,
    zoom: f32,
    show_text: bool,
    slideshow: bool,
    interval_s: u64,
    last_advance: std::time::Instant,
}

/// Раздел диалога «Настройки»: редкие настройки не отнимают экран у сетки.
#[derive(Clone, Copy, PartialEq)]
enum SettingsTab {
    Model,
    Index,
    Tags,
    Faces,
    Storage,
    Tools,
}

impl SettingsTab {
    const ALL: [SettingsTab; 6] = [
        SettingsTab::Model,
        SettingsTab::Index,
        SettingsTab::Tags,
        SettingsTab::Faces,
        SettingsTab::Storage,
        SettingsTab::Tools,
    ];

    fn label(self) -> &'static str {
        match self {
            SettingsTab::Model => "Модель ИИ",
            SettingsTab::Index => "Индексация и дубли",
            SettingsTab::Tags => "Теги",
            SettingsTab::Faces => "Лица",
            SettingsTab::Storage => "Хранилище",
            SettingsTab::Tools => "Внешние программы",
        }
    }
}

/// Акцент проекта: значит ровно «выбрано», «уверенный тег», «оставить» (docs/UI.md).
const ACCENT: egui::Color32 = egui::Color32::from_rgb(90, 170, 255);
/// Второстепенный текст и служебные подписи; темнее 150 не опускаемся.
const DIM: egui::Color32 = egui::Color32::from_gray(150);
/// Метрики раскладки: менять только вместе с docs/UI.md.
const LEFT_W: f32 = 260.0;
/// Шаг сетки плиток: в PhotoPrism снимки стоят вплотную, у нас 2 px из шкалы 4 px.
const TILE_GAP: f32 = 2.0;
/// Высота пункта навигации и высота строки фильтров.
const NAV_H: f32 = 28.0;
const RIGHT_W: f32 = 280.0;
/// Ниже этой ширины окна сведения уходят в левую панель: сетке нужны 4 колонки.
const DETAILS_MIN_W: f32 = 1200.0;

/// Варианты порядка плиток и их слова на экране (M17). «Порядок альбома» — M19.
const GRID_SORTS: [(db::GridSort, &str); 4] = [
    (db::GridSort::DateDesc, "Дата: новые сверху"),
    (db::GridSort::DateAsc, "Дата: старые сверху"),
    (db::GridSort::Name, "Имя"),
    (db::GridSort::SizeDesc, "Размер: крупные сверху"),
];

fn sort_label(sort: db::GridSort) -> &'static str {
    GRID_SORTS
        .iter()
        .find(|(value, _)| *value == sort)
        .map(|(_, label)| *label)
        .unwrap_or("Дата: новые сверху")
}

/// Сравнение дат: «без даты» — всегда в конце, в обе стороны (M17).
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

/// Месяцы в порядке выбранной сортировки; «Без даты» — последней группой всегда (M17).
/// Внутри группы порядок берётся из `ordered`, второго места сортировки нет.
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
            .unwrap_or_else(|| "без даты".into());
        by_month.entry(key).or_default().push(i);
    }
    let undated = by_month.remove("без даты");
    let mut groups: Vec<(String, Vec<usize>)> = if ascend {
        by_month.into_iter().collect()
    } else {
        by_month.into_iter().rev().collect()
    };
    if let Some(ids) = undated {
        groups.push(("без даты".into(), ids));
    }
    groups
}

/// Фон выбранного пункта: акцент с прозрачностью, а не сплошная заливка.
fn selected_fill() -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(90, 170, 255, 40)
}

/// Источник даты съёмки: в БД латиницей, на экране по-русски.
fn date_source(src: &str) -> &'static str {
    match src {
        "exif" => "EXIF",
        "filename" => "имя файла",
        "mtime" => "время файла",
        _ => "источник не указан",
    }
}

/// Основная кнопка экрана: единственное место, где фон красится акцентом.
fn primary_button(text: &str) -> egui::Button<'static> {
    egui::Button::new(egui::RichText::new(text).color(ACCENT))
        .fill(egui::Color32::from_rgba_unmultiplied(90, 170, 255, 32))
}

/// Один стиль на всё GUI (docs/UI.md §Вёрстка): плотный ритм, скругление 4 px, без теней.
/// Палитра остаётся из `visuals()`; акцент — единственный собственный цвет.
fn style(ctx: &egui::Context) {
    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(8.0, 6.0);
        s.spacing.button_padding = egui::vec2(10.0, 4.0);
        s.spacing.interact_size = egui::vec2(32.0, 26.0);
        s.spacing.menu_margin = egui::Margin::same(6);
        s.spacing.indent = 16.0;
        s.spacing.slider_width = 120.0;
        s.visuals.selection.bg_fill = selected_fill();
        s.visuals.selection.stroke = egui::Stroke::new(1.0, ACCENT);
        s.visuals.window_corner_radius = egui::CornerRadius::same(4);
        s.visuals.menu_corner_radius = egui::CornerRadius::same(4);
        s.visuals.widgets.noninteractive.corner_radius = egui::CornerRadius::same(4);
        s.visuals.widgets.inactive.corner_radius = egui::CornerRadius::same(4);
        s.visuals.widgets.hovered.corner_radius = egui::CornerRadius::same(4);
        s.visuals.widgets.active.corner_radius = egui::CornerRadius::same(4);
        s.visuals.widgets.open.corner_radius = egui::CornerRadius::same(4);
        s.visuals.window_shadow = egui::Shadow::NONE;
        s.visuals.popup_shadow = egui::Shadow::NONE;
    });
}

/// Заголовок секции в панели: 11 px, второстепенный цвет, без капса.
fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).size(11.0).color(DIM));
    ui.add_space(2.0);
}

/// Строка «подпись слева — число справа»: числа всегда у правого края колонки.
fn stat_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(label).size(12.0).color(DIM));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new(value).size(12.0));
        });
    });
}

/// Строка карточки: подпись 11 px в колонке 72 px, значение 12 px справа с переносом.
/// PhotoPrism-инспектор живёт такими парами, а не столбиком «подпись над значением».
fn meta_row(ui: &mut egui::Ui, label: &str, value: &str) {
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(72.0, 0.0),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                ui.label(egui::RichText::new(label).size(11.0).color(DIM));
            },
        );
        ui.add(egui::Label::new(egui::RichText::new(value).size(12.0)).wrap());
    });
    ui.add_space(4.0);
}

/// Пункт навигации: вся ширина панели, подсветка выбора, слово слева, число справа.
fn nav_item(
    ui: &mut egui::Ui,
    current: &mut View,
    view: View,
    text: &str,
    count: Option<String>,
) {
    let selected = *current == view;
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), NAV_H), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            selected_fill()
        } else if resp.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 4.0, fill);
        let color = if selected {
            ACCENT
        } else if resp.hovered() {
            ui.visuals().text_color()
        } else {
            egui::Color32::from_gray(190)
        };
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(14.0),
            color,
        );
        if let Some(count) = count {
            ui.painter().text(
                rect.right_center() + egui::vec2(-8.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                count,
                egui::FontId::proportional(11.0),
                if selected { ACCENT } else { DIM },
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

/// Заголовок группы навигации: 11 px приглушённым, без рамки (M16).
fn nav_section(ui: &mut egui::Ui, text: &str) {
    ui.add_space(8.0);
    ui.label(egui::RichText::new(text).size(11.0).color(DIM));
    ui.add_space(2.0);
}

/// Пункт левого списка в диалоге «Настройки»: та же геометрия, что у навигации.
fn settings_nav_item(ui: &mut egui::Ui, selected: bool, text: &str) -> bool {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), 26.0), egui::Sense::click());
    if ui.is_rect_visible(rect) {
        let fill = if selected {
            selected_fill()
        } else if resp.hovered() {
            ui.visuals().widgets.hovered.weak_bg_fill
        } else {
            egui::Color32::TRANSPARENT
        };
        ui.painter().rect_filled(rect, 3.0, fill);
        ui.painter().text(
            rect.left_center() + egui::vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            text,
            egui::FontId::proportional(14.0),
            if selected {
                ACCENT
            } else {
                egui::Color32::from_gray(190)
            },
        );
    }
    if resp.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    resp.clicked()
}

/// Имя файла одной строкой: полный путь живёт в карточке, а не в статусе.
fn file_name(path: &str) -> String {
    Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
        .to_string()
}

/// Плоская секция панели: без рамки `Frame::group`, содержимое на всю ширину.
/// Рамка в рамке спорит с фото за внимание (M16, docs/UI.md §Вёрстка).
fn block(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui)) {
    let w = ui.available_width();
    ui.scope(|ui| {
        ui.set_width(w);
        add(ui);
    });
    ui.add_space(8.0);
}

/// Чип активного фильтра: слово с «×»; клик снимает только этот фильтр (M16).
fn filter_chip(ui: &mut egui::Ui, text: &str) -> bool {
    ui.add(
        egui::Button::new(egui::RichText::new(format!("{text} ×")).size(12.0)).small(),
    )
    .on_hover_text("снять фильтр")
    .clicked()
}

/// Снимок активных фильтров для строки чипов.
struct Chips {
    query: String,
    tag: Option<String>,
    kind: KindFilter,
    hits: Option<usize>,
}

/// Какие фильтры просят снять кликом по чипу.
#[derive(Default, Clone, Copy)]
struct FilterClear {
    query: bool,
    tag: bool,
    kind: bool,
    hits: bool,
}

/// Короткая подпись для чипа: длинный путь не распирает строку фильтров.
fn shorten(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        text.to_string()
    } else {
        format!("{}…", text.chars().take(max).collect::<String>())
    }
}

/// Строка чипов над сеткой: видно, что включено, «×» снимает именно это, справа — итог.
/// Строка живёт всегда (при пустом фильтре — «фильтров нет»), поэтому заголовок вида не прыгает.
fn draw_filter_chips(ui: &mut egui::Ui, chips: &Chips, found: (usize, usize)) -> FilterClear {
    let mut clear = FilterClear::default();
    let mut any = false;
    ui.horizontal(|ui| {
        if !chips.query.is_empty() {
            any = true;
            if filter_chip(ui, &format!("путь/дата: {}", shorten(&chips.query, 24))) {
                clear.query = true;
            }
        }
        if let Some(tag) = &chips.tag {
            any = true;
            if filter_chip(ui, &format!("тег: {}", shorten(tag, 24))) {
                clear.tag = true;
            }
        }
        if let Some(label) = chips.kind.chip() {
            any = true;
            if filter_chip(ui, label) {
                clear.kind = true;
            }
        }
        if let Some(n) = chips.hits {
            any = true;
            if filter_chip(ui, &format!("по релевантности: {n} файлов")) {
                clear.hits = true;
            }
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(
                egui::RichText::new(if any {
                    format!("найдено {} из {}", found.0, found.1)
                } else {
                    "фильтров нет".to_string()
                })
                .size(11.0)
                .color(DIM),
            );
        });
    });
    clear
}

/// Заголовок группы сетки: месяц 14 px, число файлов и объём 11 px (как в PhotoPrism).
/// Месяц без даты — отдельная последняя группа, а не мусорный ключ в начале списка.
fn month_header(ui: &mut egui::Ui, key: &str, files: usize, bytes: i64) {
    let title = if key == "без даты" {
        "Без даты".to_string()
    } else {
        match (key.get(0..4), key.get(5..7)) {
            (Some(y), Some(m)) => match (y.parse::<i32>(), m.parse::<usize>()) {
                (Ok(y), Ok(m)) if (1..=12).contains(&m) => {
                    format!("{} {y}", MONTHS[m - 1])
                }
                _ => key.to_string(),
            },
            _ => key.to_string(),
        }
    };
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(title).size(14.0));
        ui.label(
            egui::RichText::new(format!(
                "· файлов {files} · {}",
                db::human(bytes)
            ))
            .size(11.0)
            .color(DIM),
        );
    });
    ui.add_space(4.0);
}

/// Строка прогресса фонового прохода: `done/total`, «Стоп» рядом.
/// Возвращает `true`, если нажали «Стоп».
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
        if running && ui.button("Стоп").clicked() {
            stop = true;
        }
    });
    stop
}

pub fn run(explicit: Option<PathBuf>) -> Result<()> {
    let db_path = db::resolve_db(explicit.as_deref())?;
    let opts = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1240.0, 840.0])
            .with_title("photocat — каталог медиаархива"),
        ..Default::default()
    };
    eframe::run_native(
        "photocat",
        opts,
        Box::new(move |cc| {
            // Один стиль на всё GUI (docs/UI.md §Вёрстка) и плавная прокрутка:
            // колесо не «шагает» на строку, а догоняет позицию.
            cc.egui_ctx.all_styles_mut(|s| {
                s.scroll_animation.points_per_second = 1400.0;
                s.scroll_animation.duration = egui::Rangef::new(0.08, 0.24);
            });
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
    /// Файлы в корзине приложения: на диске не тронуты, из обычных видов скрыты.
    trash_items: Vec<FileRow>,
    textures: HashMap<i64, egui::TextureHandle>,
    missing: HashSet<i64>,
    selected: Option<i64>,
    view: View,
    groups: Vec<Group>,
    max_dist: u32,
    path_input: String,
    query: String,
    kind_filter: KindFilter,
    progress: Option<Arc<Progress>>,
    rx: Option<mpsc::Receiver<Result<index::Stats>>>,
    /// Теги CLIP: file_id → [(тег, вес)] по убыванию.
    tags: HashMap<i64, Vec<(String, f32)>>,
    /// Распределение весов по каждому тегу (по убыванию) и по лучшему тегу файла:
    /// из него считается процентильный порог фильтра, без сортировки на каждый кадр.
    tag_dists: HashMap<String, Vec<f32>>,
    top_scores: Vec<f32>,
    /// Свои теги владельца: file_id → подписи.
    user_tags: HashMap<i64, Vec<String>>,
    user_tag_input: String,
    tag_filter: Option<String>,
    semantic_query: String,
    /// Результаты смыслового поиска в порядке убывания похожести.
    semantic_hits: Option<Vec<i64>>,
    /// Модель загружена после первого поиска; дальше запросы идут без паузы.
    ml_model: Option<embed::Model>,
    search_rx: Option<mpsc::Receiver<SearchReply>>,
    embed_progress: Option<Arc<Progress>>,
    embed_rx: Option<mpsc::Receiver<EmbedReply>>,
    /// Настройки приложения (модель, путь, лимиты, пороги).
    settings: db::Settings,
    /// Редактируемый путь к моделям: применяется кнопкой, а не на каждый символ.
    models_dir_input: String,
    /// Согласие на скачивание весов (ADR 0015).
    download_consent: bool,
    download_progress: Option<Arc<Progress>>,
    download_rx: Option<mpsc::Receiver<Result<embed::DownloadReport>>>,
    /// Календарь: выбранный месяц и день.
    cal_year: i32,
    cal_month: u32,
    cal_day: Option<String>,
    /// Сколько тегов скрыто владельцем у файла (для «вернуть»).
    hidden_tags: HashMap<i64, usize>,
    /// Текст с фото: загружается по выбранному файлу и кэшируется.
    ocr_text: HashMap<i64, String>,
    /// Фрагменты найденного текста с маркерами «» (для подсветки в карточке).
    ocr_snippets: HashMap<i64, String>,
    /// Искать по тексту на фото (FTS5), а не по смыслу (CLIP).
    search_ocr: bool,
    ocr_files: i64,
    ocr_chars: i64,
    /// Лица: группы, лица для выбранной группы и прогресс прохода.
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
    /// Сколько снимков пойдёт в пакетный проход по эвристике (план, не запуск).
    ocr_candidates: usize,
    /// Из них — ещё без текста; «Продолжить» показывает именно остаток.
    ocr_remaining: usize,
    ocr_progress: Option<Arc<Progress>>,
    ocr_rx: Option<mpsc::Receiver<OcrReply>>,
    /// Версия tesseract (путь из настроек или PATH): `None` — OCR недоступен, кнопка погашена.
    tesseract: Option<String>,
    /// Редактируемые пути к программам: применяются кнопкой после проверки запуском.
    tools_input: tools::Tools,
    /// Проверенное состояние программ (версия или ошибка) — не запускаем их на каждом кадре.
    tool_states: [String; 4],
    /// Диалог «Настройки»: открыт ли и какой раздел показан.
    settings_open: bool,
    settings_tab: SettingsTab,
    /// Правая панель сведений: её можно закрыть, чтобы освободить место календарю и сетке.
    details_open: bool,
    /// Размер плитки и календаря; хранится в настройках.
    tile_size: f32,
    /// Полноэкранный просмотр и загрузка большого кадра в фоне.
    viewer: Option<Viewer>,
    viewer_tex: Option<(i64, egui::TextureHandle)>,
    /// Заранее декодированный кадр выбранного файла: полный экран открывается без ожидания.
    photo_cache: HashMap<i64, (usize, usize, Vec<u8>)>,
    photo_loading: HashSet<i64>,
    photo_jobs: VecDeque<i64>,
    photo_rx: Option<mpsc::Receiver<PhotoReply>>,
    /// Встроенный плеер: видео и звук в окне приложения (ADR 0030).
    video: Option<player::Player>,
    /// Кадр плеера как текстура; обновляется из `poll_video`.
    video_tex: Option<egui::TextureHandle>,
    /// Запасной путь: внешний ffplay, когда встроенный плеер отказался.
    video_child: Option<std::process::Child>,
    /// Когда последний раз обновляли текстуру видео: заливка кадра — самая
    /// дорогая операция в кадре, чаще 30 раз в секунду её делать незачем.
    video_tex_at: Option<std::time::Instant>,
    /// Сообщение «декодер не успевает» показывается один раз на файл.
    video_lag_shown: bool,
    /// Видео доиграло: полноэкранный просмотр переходит к следующему файлу.
    video_ended: bool,
    video_id: Option<i64>,
    /// Календарь: градация и понедельник выбранной недели.
    cal_scale: CalScale,
    cal_week: chrono::NaiveDate,
    /// Выбранные лица для пакетного назначения имени.
    face_picked: HashSet<i64>,
    face_group_input: String,
    /// Группы, отмеченные для поиска «все вместе на одном снимке».
    face_picked_clusters: HashSet<i64>,
    face_together: bool,
    /// Подтверждение очистки корзины: без него в системную корзину ничего не уходит.
    trash_confirm: bool,
    trash_progress: Option<Arc<Progress>>,
    trash_rx: Option<mpsc::Receiver<TrashReply>>,
    /// Каталоги и встроенный выбор каталога (без зависимости от системного диалога).
    roots: Vec<String>,
    roots_open: bool,
    picker_open: bool,
    picker_current: PathBuf,
    picker_entries: Vec<PathBuf>,
    picker_error: String,
    /// Занятое место считается по кнопке, а не каждым кадром.
    storage_usage: Option<storage::Usage>,
    storage_confirm: Option<StorageAction>,
    status: String,
}

impl App {
    fn new(db_path: PathBuf) -> Self {
        let thumb_dir = db::cache_dir().join("thumbs");
        let mut me = Self {
            db_path,
            thumbs_dir: thumb_dir,
            items: Vec::new(),
            trash_items: Vec::new(),
            textures: HashMap::new(),
            missing: HashSet::new(),
            selected: None,
            view: View::Grid,
            groups: Vec::new(),
            max_dist: 6,
            path_input: String::new(),
            query: String::new(),
            kind_filter: KindFilter::All,
            progress: None,
            rx: None,
            tags: HashMap::new(),
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
            tool_states: std::array::from_fn(|_| String::new()),
            settings_open: false,
            settings_tab: SettingsTab::Model,
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
        };
        me.reload();
        me
    }

    fn reload(&mut self) {
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
                self.status = format!(
                    "в индексе {} файлов, {}",
                    rows.len(),
                    db::human(rows.iter().map(|r| r.size).sum())
                );
                self.settings = settings;
                self.tools_input = self.settings.tools.clone();
                // Проверяем tesseract и остальные программы по путям из настроек — один раз.
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
            }
            Err(e) => self.status = format!("ошибка чтения индекса: {e}"),
        }
    }

    /// Открыть календарь на месяце самого свежего файла.
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
        let res = db::open(&self.db_path).and_then(|c| db::save_settings(&c, &self.settings));
        match res {
            Ok(()) => self.status = "настройки сохранены".into(),
            Err(e) => self.status = format!("настройки не сохранены: {e}"),
        }
    }

    /// Диалог «Настройки»: редко нужное, но всегда в один клик с верхней панели.
    fn settings_modal(&mut self, ctx: &egui::Context) {
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
                        ui.label(egui::RichText::new("Настройки").size(16.0));
                    });
                    ui.add_space(8.0);
                    egui::Panel::bottom("settings-footer").show(ui, |ui| {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.add_space(12.0);
                            ui.label(
                                egui::RichText::new(
                                    "Изменения сохраняются сразу; оригиналы не меняются.",
                                )
                                .size(11.0)
                                .color(DIM),
                            );
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.add_space(12.0);
                                    if ui.button("Закрыть").clicked() {
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
                                if settings_nav_item(ui, tab == t, t.label()) {
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

    /// Список каталогов: добавить новый, убрать из индекса (не с диска), индексировать все.
    fn roots_modal(&mut self, ctx: &egui::Context) {
        if !self.roots_open {
            return;
        }
        let mut close = false;
        let mut add = false;
        let mut index_all = false;
        let mut remove: Option<String> = None;
        let roots = self.roots.clone();
        let resp = egui::Modal::new(egui::Id::unique("photocat-roots")).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new("Каталоги архива").size(16.0));
            ui.label(
                egui::RichText::new(
                    "Индексировать можно несколько каталогов. «Добавить каталог…» сразу предлагает \
                     индексацию; «Убрать из индекса» удаляет только строки БД — файлы на диске целы.",
                )
                .size(11.0)
                .color(DIM),
            );
            ui.add_space(8.0);
            if roots.is_empty() {
                ui.label("Каталогов пока нет — добавьте кнопкой ниже.");
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
                                        .small_button("Убрать из индекса")
                                        .on_hover_text("файлы на диске не удаляются")
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
                if ui.button("Добавить каталог…").clicked() {
                    add = true;
                }
                if ui
                    .add_enabled(!roots.is_empty(), primary_button("Индексировать все"))
                    .clicked()
                {
                    index_all = true;
                }
                if ui.button("Закрыть").clicked() {
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
                    self.status =
                        format!("каталог убран из индекса: удалено строк {n}; файлы на диске целы");
                }
                Err(e) => self.status = format!("каталог не убран из индекса: {e}"),
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
                self.picker_error = format!("не прочитать {}: {e}", self.picker_current.display())
            }
        }
    }

    /// Встроенный выбор каталога: без зависимости от системного диалога и без ввода пути руками.
    fn picker_modal(&mut self, ctx: &egui::Context) {
        if !self.picker_open {
            return;
        }
        let mut close = false;
        let mut nav: Option<PathBuf> = None;
        let mut add = false;
        let mut add_and_index = false;
        let current = self.picker_current.clone();
        let entries = self.picker_entries.clone();
        let error = self.picker_error.clone();
        let resp = egui::Modal::new(egui::Id::unique("photocat-picker")).show(ctx, |ui| {
            ui.set_min_width(520.0);
            ui.label(egui::RichText::new("Добавить каталог").size(16.0));
            ui.label(
                egui::RichText::new(current.display().to_string())
                    .size(11.0)
                    .color(DIM),
            );
            if !error.is_empty() {
                ui.label(egui::RichText::new(error).size(12.0).color(DIM));
            }
            ui.horizontal(|ui| {
                if let Some(parent) = current.parent()
                    && ui.button("Вверх").clicked()
                {
                    nav = Some(parent.to_path_buf());
                }
                if ui
                    .add(primary_button("Добавить и индексировать"))
                    .clicked()
                {
                    add = true;
                    add_and_index = true;
                }
                if ui.button("Добавить без индексации").clicked() {
                    add = true;
                }
                if ui.button("Отмена").clicked() {
                    close = true;
                }
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("picker-list")
                .max_height(320.0)
                .show(ui, |ui| {
                    if entries.is_empty() {
                        ui.label("Подкаталогов нет.");
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
                self.status = format!("каталог добавлен: {path}");
            }
            self.picker_open = false;
            if add_and_index {
                self.start_index();
            }
        }
    }

    /// Подтверждение очистки кэша/индекса: архив и модели не трогаются.
    fn storage_modal(&mut self, ctx: &egui::Context) {
        let Some(action) = self.storage_confirm else {
            return;
        };
        let (title, text, button) = match action {
            StorageAction::Thumbs => (
                "Очистить превью?",
                "Будут удалены только файлы <кэш>/photocat/thumbs. Сетка покажет «нет превью», \
                 пока каталог не проиндексируют заново. Оригиналы не меняются.",
                "Очистить превью",
            ),
            StorageAction::Faces => (
                "Очистить кропы лиц?",
                "Будут удалены кропы и строки лиц в БД; группы исчезнут. «Найти лица» заново \
                 создаст их. Фотографии и оригиналы не меняются.",
                "Очистить лица",
            ),
            StorageAction::Index => (
                "Сбросить индекс?",
                "Будут удалены строки БД: файлы, теги, текст, векторы и лица. Настройки, модели \
                 и все файлы на диске остаются. После этого нужна индексация заново.",
                "Сбросить индекс",
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
                if ui.add(primary_button(button)).clicked() {
                    confirmed = true;
                }
                if ui.button("Отмена").clicked() {
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
                        StorageAction::Index => "индекс сброшен; проиндексируйте каталог".into(),
                        _ => format!("кэш очищен: {c}"),
                    };
                }
                Err(e) => self.status = format!("очистка не удалась: {e:#}"),
            }
        }
    }

    /// Подтверждение очистки корзины: единственное действие, которое убирает файл с диска.
    fn trash_modal(&mut self, ctx: &egui::Context) {
        if !self.trash_confirm {
            return;
        }
        let mut confirmed = false;
        let mut cancel = false;
        let n = self.trash_items.len();
        let bytes: i64 = self.trash_items.iter().map(|r| r.size).sum();
        let resp = egui::Modal::new(egui::Id::unique("photocat-trash")).show(ctx, |ui| {
            ui.set_min_width(460.0);
            ui.label(egui::RichText::new("Очистить корзину?").size(16.0));
            ui.add_space(4.0);
            ui.label(format!(
                "Будет перенесено в системную корзину ОС: файлов {n}, {}.",
                db::human(bytes)
            ));
            ui.label(
                egui::RichText::new(
                    "Внутри приложения безвозвратного удаления нет: файлы остаются в системной \
                     корзине, пока вы сами её не очистите. Оригиналы в архиве исчезнут из своих \
                     каталогов — это необратимо средствами приложения.",
                )
                .size(12.0),
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add(primary_button("Переместить в системную корзину"))
                    .clicked()
                {
                    confirmed = true;
                }
                if ui.button("Отмена").clicked() {
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
        self.status = format!(
            "корзина: переношу {} файлов в системную корзину…",
            items.len()
        );
        std::thread::spawn(move || {
            let r = recycle::empty(&db_path, &items, Some(progress));
            let _ = tx.send(r);
        });
    }

    fn settings_section(&mut self, ui: &mut egui::Ui, tab: SettingsTab) {
        ui.label(egui::RichText::new(tab.label()).size(16.0));
        ui.add_space(4.0);
        ui.separator();
        ui.add_space(8.0);
        match tab {
            SettingsTab::Model => self.settings_model(ui),
            SettingsTab::Index => self.settings_index(ui),
            SettingsTab::Tags => self.settings_tags(ui),
            SettingsTab::Faces => self.settings_faces(ui),
            SettingsTab::Storage => self.settings_storage(ui),
            SettingsTab::Tools => self.settings_tools(ui),
        }
    }

    fn settings_model(&mut self, ui: &mut egui::Ui) {
        let current = embed::active_entry(&self.settings);
        let current_id = current.id;
        let mut chosen = current_id;
        ui.label(egui::RichText::new("Модель").size(12.0).color(DIM));
        ui.add_space(2.0);
        egui::ComboBox::from_id_salt("model_pick")
            .width(380.0)
            .selected_text(format!(
                "{} — {}",
                current.title,
                db::human(current.bytes() as i64)
            ))
            .show_ui(ui, |ui| {
                for e in embed::catalog() {
                    ui.selectable_value(
                        &mut chosen,
                        e.id,
                        format!("{} — {} · {}", e.title, db::human(e.bytes() as i64), e.note),
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
            egui::RichText::new("Состояние модели")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        let state = if embed::model_present(&self.settings) {
            format!(
                "Скачана · проверена · {}",
                db::human(embed::model_bytes_in(&self.settings.models_dir, current))
            )
        } else {
            "Не скачана".to_string()
        };
        ui.label(egui::RichText::new(state).size(12.0).color(
            if embed::model_present(&self.settings) {
                ACCENT
            } else {
                DIM
            },
        ));
        if ui.button("Проверить модель").clicked() {
            match embed::check_model(&self.settings) {
                Ok(()) => {
                    self.status = format!(
                        "модель {} проверена: sha256 совпал",
                        embed::active_entry(&self.settings).title
                    )
                }
                Err(e) => self.status = format!("модель не готова: {e:#}"),
            }
        }
        ui.add_space(12.0);

        ui.label(egui::RichText::new("Папка моделей").size(12.0).color(DIM));
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.models_dir_input)
                    .hint_text("путь, куда скачивать веса")
                    .desired_width(380.0),
            );
            if ui.button("Применить путь").clicked() {
                let p = self.models_dir_input.trim();
                if p.is_empty() {
                    self.status = "путь к моделям пуст".into();
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
                .color(DIM),
        );
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new("Лимит на скачивание")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::Slider::new(&mut self.settings.max_model_mb, 50..=2000)
                    .suffix(" МиБ")
                    .step_by(10.0),
            );
            if r.drag_stopped() || r.lost_focus() {
                self.save_settings();
            }
        });
        ui.label(
            egui::RichText::new(
                "Модель сверх лимита не скачивается. Если выбранная недоступна, \
                 загрузчик ищет совместимую в пределах лимита.",
            )
            .size(11.0)
            .color(DIM),
        );
        ui.add_space(8.0);
        for e in embed::catalog() {
            let present = embed::model_present_in(&self.settings.models_dir, e);
            let fits = e.fits(self.settings.max_model_mb);
            let state = if present {
                "на месте"
            } else if fits {
                "не скачана"
            } else {
                "сверх лимита"
            };
            ui.label(
                egui::RichText::new(format!(
                    "{} — {} · {}",
                    e.title,
                    db::human(e.bytes() as i64),
                    state
                ))
                .size(12.0),
            );
        }
        ui.add_space(12.0);

        ui.label(
            egui::RichText::new("Согласие на сеть")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        ui.checkbox(
            &mut self.download_consent,
            "разрешаю скачать веса модели (фото, пути и запросы не отправляются)",
        );
        ui.label(
            egui::RichText::new(
                "Сеть используется только при нажатии «Скачать модель». \
                 Пользовательские данные не отправляются.",
            )
            .size(11.0)
            .color(DIM),
        );
        ui.add_space(8.0);
        let busy = self.download_rx.is_some();
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.download_consent && !busy,
                    primary_button("Скачать модель"),
                )
                .clicked()
            {
                self.start_download();
            }
            if ui
                .add_enabled(
                    self.embed_rx.is_none() && embed::model_present(&self.settings),
                    egui::Button::new("Посчитать теги"),
                )
                .clicked()
            {
                self.start_embed();
            }
        });
        if !self.download_consent {
            ui.label(
                egui::RichText::new("кнопка погашена: нужно согласие на сеть")
                    .size(11.0)
                    .color(DIM),
            );
        }
        if let Some(p) = &self.download_progress {
            let (d, t) = (p.done(), p.total());
            let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(380.0)
                    .text(if t > 0 {
                        format!("файлов: {d}/{t}")
                    } else {
                        p.msg()
                    }),
            );
            ui.label(egui::RichText::new(p.msg()).size(11.0).color(DIM));
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Теги и векторы привязаны к модели: после смены модели их нужно посчитать.",
            )
            .size(11.0)
            .color(DIM),
        );
    }

    fn settings_index(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Потоков на индексацию")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        let r = ui.add(
            egui::DragValue::new(&mut self.settings.index_jobs)
                .range(1..=64)
                .suffix(" потоков"),
        );
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.add_space(12.0);
        ui.label(egui::RichText::new("Порог дублей").size(12.0).color(DIM));
        ui.add_space(2.0);
        let r = ui.add(egui::Slider::new(&mut self.settings.max_dist, 0..=16).text("бит из 64"));
        if r.drag_stopped() || r.lost_focus() {
            self.max_dist = self.settings.max_dist;
            self.save_settings();
        }
        ui.label(
            egui::RichText::new(
                "Больше бит — больше похожих снимков в группе. Точные копии \
                 находятся при любом пороге.",
            )
            .size(11.0)
            .color(DIM),
        );
    }

    fn settings_tags(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new("Порог фильтра по тегу")
                .size(12.0)
                .color(DIM),
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
        ui.label(
            egui::RichText::new(if self.tags.is_empty() {
                "теги ещё не посчитаны".to_string()
            } else {
                format!(
                    "выше порога: файлов {hits} из {} с тегами; процентиль считается отдельно по каждому тегу",
                    self.tags.len()
                )
            })
            .size(11.0)
            .color(DIM),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("Порог уверенного тега (косинус)")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        let r = ui.add(
            egui::Slider::new(&mut self.settings.tag_show, 0.10..=0.50).step_by(0.01),
        );
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.label(
            egui::RichText::new(
                "Это про карточку, OCR и {tag} в витрине, а не про фильтр: тег подсвечивается, \
                 только если он выше порога и отрывается от второго.",
            )
            .size(11.0)
            .color(DIM),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("Отрыв от второго тега")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        let r = ui.add(egui::Slider::new(&mut self.settings.tag_margin, 0.0..=0.10));
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "Ниже порог — больше тегов и больше ошибок. «Убрать» у тега в карточке \
                 скрывает его у одного фото и переживает пересчёт.",
            )
            .size(11.0)
            .color(DIM),
        );
    }

    fn settings_faces(&mut self, ui: &mut egui::Ui) {
        let present = faces::model_present(&self.settings);
        ui.label(
            egui::RichText::new(format!(
                "{} · {} · {}",
                faces::MODEL_ID,
                db::human(faces::model_bytes(&self.settings)),
                if present {
                    "на месте"
                } else {
                    "не скачана"
                }
            ))
            .size(12.0)
            .color(if present { ACCENT } else { DIM }),
        );
        ui.add_space(12.0);
        ui.label(
            egui::RichText::new("Порог склейки лиц (косинус)")
                .size(12.0)
                .color(DIM),
        );
        ui.add_space(2.0);
        let r =
            ui.add(egui::Slider::new(&mut self.settings.face_threshold, 0.20..=0.70).step_by(0.01));
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        ui.label(
            egui::RichText::new(
                "Выше порог — меньше ложных склеек, но один человек может разбиться \
                 на группы. Модели SCRFD и ArcFace — ONNX, локально.",
            )
            .size(11.0)
            .color(DIM),
        );
        ui.add_space(12.0);
        ui.checkbox(
            &mut self.face_download_consent,
            "разрешаю скачать веса лиц (фото не отправляются)",
        );
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    self.face_download_consent && self.face_download_rx.is_none(),
                    primary_button("Скачать модель лиц"),
                )
                .clicked()
            {
                self.start_face_download();
            }
            if ui.button("Проверить модель лиц").clicked() {
                match faces::check_model(&self.settings) {
                    Ok(()) => self.status = "модель лиц проверена: sha256 совпал".into(),
                    Err(e) => self.status = format!("модель лиц не готова: {e:#}"),
                }
            }
        });
        if !self.face_download_consent {
            ui.label(
                egui::RichText::new("кнопка погашена: нужно согласие на сеть")
                    .size(11.0)
                    .color(DIM),
            );
        }
        if let Some(p) = &self.face_download_progress {
            let (d, t) = (p.done(), p.total());
            let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
            ui.add(
                egui::ProgressBar::new(frac)
                    .desired_width(380.0)
                    .text(if t > 0 {
                        format!("файлов лиц: {d}/{t}")
                    } else {
                        p.msg()
                    }),
            );
            ui.label(egui::RichText::new(p.msg()).size(11.0).color(DIM));
        }
    }

    fn settings_storage(&mut self, ui: &mut egui::Ui) {
        let n = self.items.len();
        let bytes: i64 = self.items.iter().map(|r| r.size).sum();
        let trashed = self.trash_items.len();
        meta_row(
            ui,
            "Индекс",
            &format!("файлов {n}, {} · в корзине {trashed}", db::human(bytes)),
        );
        meta_row(ui, "БД", &self.db_path.display().to_string());
        meta_row(ui, "Кэш", &db::cache_dir().display().to_string());
        meta_row(
            ui,
            "Модели",
            &format!(
                "{} · {}",
                self.settings.models_dir.display(),
                db::human(embed::model_bytes_in(
                    &self.settings.models_dir,
                    embed::active_entry(&self.settings)
                ))
            ),
        );
        meta_row(
            ui,
            "Лица",
            &format!(
                "{} · {}",
                faces::MODEL_ID,
                db::human(faces::model_bytes(&self.settings))
            ),
        );
        ui.add_space(8.0);
        if ui.button("Посчитать занятое место").clicked() {
            self.storage_usage = Some(storage::usage(&self.db_path, &self.settings.models_dir));
        }
        if let Some(u) = &self.storage_usage {
            stat_row(ui, "База данных:", &db::human(u.db_bytes as i64));
            stat_row(ui, "Превью:", &db::human(u.thumbs_bytes as i64));
            stat_row(ui, "Кропы лиц:", &db::human(u.faces_bytes as i64));
            stat_row(ui, "Модели:", &db::human(u.models_bytes as i64));
            stat_row(ui, "Весь кэш:", &db::human(u.total_bytes as i64));
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new("Очистка кэша и индекса")
                .size(11.0)
                .color(DIM),
        );
        if ui
            .button("Очистить превью…")
            .on_hover_text("только <кэш>/photocat/thumbs; архив не трогается")
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Thumbs);
        }
        if ui
            .button("Очистить кропы лиц…")
            .on_hover_text("кропы и строки лиц; после этого «Найти лица» пройдёт заново")
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Faces);
        }
        if ui
            .button("Сбросить индекс…")
            .on_hover_text("строки БД: файлы, теги, OCR, векторы, лица; оригиналы целы")
            .clicked()
        {
            self.storage_confirm = Some(StorageAction::Index);
        }
        ui.add_space(8.0);
        ui.label(
            egui::RichText::new(
                "В каталог архива photocat не пишет: только строки в БД и превью в кэше. \
                 Витрина по датам — отдельная команда с явным подтверждением.",
            )
            .size(11.0)
            .color(DIM),
        );
    }

    fn settings_tools(&mut self, ui: &mut egui::Ui) {
        ui.label(
            egui::RichText::new(
                "Пусто — программа ищется в PATH. Путь проверяется запуском при сохранении; \
                 без программы отключается только её работа.",
            )
            .size(11.0)
            .color(DIM),
        );
        ui.add_space(8.0);
        for (i, tool) in tools::ALL.into_iter().enumerate() {
            ui.label(egui::RichText::new(tool.name()).size(12.0).color(DIM));
            ui.add_space(2.0);
            let input = self.tools_input.get_mut(tool);
            ui.add(
                egui::TextEdit::singleline(input)
                    .hint_text("пусто — из PATH")
                    .desired_width(380.0),
            );
            ui.label(
                egui::RichText::new(&self.tool_states[i])
                    .size(11.0)
                    .color(DIM),
            );
            ui.label(
                egui::RichText::new(format!("без неё: {}", tool.without()))
                    .size(11.0)
                    .color(DIM),
            );
            ui.add_space(8.0);
        }
        if ui
            .add(egui::Button::new("Проверить и сохранить программы"))
            .clicked()
        {
            self.apply_tools();
        }
    }

    fn reset_filters(&mut self) {
        self.query.clear();
        self.kind_filter = KindFilter::All;
        self.tag_filter = None;
        self.semantic_hits = None;
        self.ocr_snippets.clear();
        self.status = "фильтры сброшены".into();
    }

    /// Верхняя панель: две строки управления, дальше — только прогресс прохода.
    fn top_bar(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Каталог").size(11.0).color(DIM));
            let w = (ui.available_width() - 470.0).clamp(240.0, 520.0);
            let resp = ui.add(
                egui::TextEdit::singleline(&mut self.path_input)
                    .hint_text("путь к каталогу; несколько — кнопкой «Каталоги»")
                    .desired_width(w),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add(primary_button("Индексировать")).clicked() || enter {
                self.start_index();
            }
            if ui.button("Обновить").clicked() {
                self.reload();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button("Настройки").clicked() {
                    self.settings_open = true;
                }
                if ui
                    .button(format!("Каталоги ({})", self.roots.len()))
                    .on_hover_text("добавить несколько каталогов и убрать каталог из индекса")
                    .clicked()
                {
                    self.roots_open = true;
                }
                if ui
                    .button(if self.details_open {
                        "Скрыть сведения"
                    } else {
                        "Сведения"
                    })
                    .on_hover_text("правая панель с датой, камерой, тегами и текстом")
                    .clicked()
                {
                    self.details_open = !self.details_open;
                }
            });
        });

        ui.add_space(4.0);
        ui.horizontal(|ui| {
            // Режим поиска — сегментированный переключатель перед полем (PhotoPrism).
            let mut text_mode = self.search_ocr;
            if ui
                .selectable_label(!text_mode, egui::RichText::new("Смысловой").size(12.0))
                .clicked()
            {
                text_mode = false;
            }
            if ui
                .selectable_label(text_mode, egui::RichText::new("По тексту").size(12.0))
                .clicked()
            {
                text_mode = true;
            }
            if text_mode != self.search_ocr {
                self.search_ocr = text_mode;
                self.semantic_hits = None;
                self.ocr_snippets.clear();
                self.status = if text_mode {
                    "поиск по тексту, распознанному на снимках".into()
                } else {
                    "смысловой поиск по содержимому снимков".into()
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
                        "справка, квитанция, 2024"
                    } else {
                        "торт, море, мебель, документ"
                    })
                    .desired_width(340.0),
            );
            let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.add_enabled(ready, egui::Button::new("Найти")).clicked() || (enter && ready)
            {
                self.start_search();
            }
            if ui
                .add_enabled(self.semantic_hits.is_some(), egui::Button::new("Сбросить"))
                .clicked()
            {
                self.semantic_hits = None;
                self.ocr_snippets.clear();
                self.status = "поиск сброшен".into();
            }
            if self.search_rx.is_some() {
                ui.label(
                    egui::RichText::new("загрузка модели…")
                        .size(11.0)
                        .color(DIM),
                );
            } else if !ready {
                ui.label(
                    egui::RichText::new(if self.search_ocr {
                        "нет распознанного текста"
                    } else {
                        "модель не скачана"
                    })
                    .size(11.0)
                    .color(DIM),
                );
            }
        });

        if let Some(p) = &self.progress {
            let running = self.rx.is_some();
            if progress_row(ui, "Индексация", p, running) {
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
            if progress_row(ui, "Лица", p, running) {
                p.cancel();
            }
        }
        if let Some(p) = &self.trash_progress {
            let running = self.trash_rx.is_some();
            if progress_row(ui, "Корзина", p, running) {
                p.cancel();
            }
        }
        ui.add_space(4.0);
    }

    /// Строка статуса: факт слева, выбранный файл справа.
    fn status_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(if self.status.is_empty() {
                    "—"
                } else {
                    &self.status
                })
                .size(11.0),
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
                        .unwrap_or_else(|| "без даты".into());
                    ui.label(
                        egui::RichText::new(format!("Выбрано: {} · {date}", file_name(&f.path)))
                            .size(11.0)
                            .color(DIM),
                    );
                }
            });
        });
    }

    /// Снимок фильтров для строки чипов: чипы рисуются без доступа к `self`, потому что
    /// в этот момент сетка уже держит `&mut self.textures`. Снятие применяется после кадра.
    fn chips_snapshot(&self) -> Chips {
        Chips {
            query: self.query.trim().to_string(),
            tag: self.tag_filter.clone(),
            kind: self.kind_filter,
            hits: self.semantic_hits.as_ref().map(|h| h.len()),
        }
    }

    /// Левая панель: карта приложения. Навигация и фильтр закреплены, контекст вида — в прокрутке.
    fn left_panel(&mut self, ui: &mut egui::Ui, show_card: bool) {
        ui.add_space(6.0);
        ui.label(egui::RichText::new("photocat").size(16.0));
        ui.label(egui::RichText::new("семейный архив").size(11.0).color(DIM));
        ui.add_space(8.0);
        ui.separator();

        // Карта приложения — две группы с числами справа (PhotoPrism: Library / Tools).
        // Число объясняет цену пункта до клика: 103 группы, 617 лиц, 3 в корзине.
        nav_section(ui, "БИБЛИОТЕКА");
        nav_item(
            ui,
            &mut self.view,
            View::Grid,
            "Все файлы",
            Some(self.items.len().to_string()),
        );
        nav_item(ui, &mut self.view, View::Calendar, "Календарь", None);
        nav_item(
            ui,
            &mut self.view,
            View::Faces,
            "Лица",
            Some(self.face_clusters.len().to_string()),
        );
        nav_section(ui, "ИНСТРУМЕНТЫ");
        nav_item(
            ui,
            &mut self.view,
            View::Dupes,
            "Дубли и серии",
            Some(self.groups.len().to_string()),
        );
        nav_item(
            ui,
            &mut self.view,
            View::Trash,
            "Корзина",
            Some(self.trash_items.len().to_string()),
        );

        ui.add_space(8.0);
        ui.separator();
        nav_section(ui, "ФИЛЬТР");
        ui.label(
            egui::RichText::new("Поиск по пути или дате")
                .size(11.0)
                .color(DIM),
        );
        ui.add(
            egui::TextEdit::singleline(&mut self.query)
                .hint_text("2024-05, отпуск")
                .desired_width(f32::INFINITY),
        );
        ui.add_space(4.0);
        // Формат — один выбор вместо галочки: все, фото или видео (PhotoPrism: Images/Video).
        ui.horizontal(|ui| {
            for kind in [KindFilter::All, KindFilter::Images, KindFilter::Videos] {
                if ui
                    .selectable_label(
                        self.kind_filter == kind,
                        egui::RichText::new(kind.label()).size(12.0),
                    )
                    .clicked()
                {
                    self.kind_filter = kind;
                }
            }
        });
        ui.add_space(4.0);

        // На широком окне сводка прижата к низу панели. На узком — карточка и сводка
        // идут в общей прокрутке: иначе карточке остаётся полоска в 40 px.
        if !show_card {
            egui::Panel::bottom("left-stats")
                .resizable(false)
                .show(ui, |ui| {
                    ui.add_space(4.0);
                    ui.separator();
                    self.stats_block(ui);
                });
        }
        egui::ScrollArea::vertical()
            .id_salt("left-scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                match self.view {
                    View::Dupes => self.dupes_controls(ui),
                    View::Faces => self.faces_controls(ui),
                    _ => {}
                }
                self.model_block(ui);
                self.ocr_block(ui);
                if show_card {
                    ui.add_space(8.0);
                    ui.separator();
                    self.file_card(ui);
                    ui.add_space(8.0);
                    ui.separator();
                    self.stats_block(ui);
                }
            });
    }

    /// Контекст вида «Дубли и серии»: порог, «Найти группы», сводка и выгрузка плана.
    fn dupes_controls(&mut self, ui: &mut egui::Ui) {
        block(ui, |ui| {
            ui.label(egui::RichText::new("Дубли и серии").size(11.0).color(DIM));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!(
                "Порог похожести: {} бит из 64",
                self.max_dist
            )));
            let resp = ui.add(egui::Slider::new(&mut self.max_dist, 0..=16));
            if resp.changed() {
                self.settings.max_dist = self.max_dist;
                self.save_settings();
            }
            if ui.add(primary_button("Найти группы")).clicked() {
                let t = std::time::Instant::now();
                self.groups = dupes::find_groups(&self.items, self.max_dist);
                let waste: i64 = self.groups.iter().map(|g| g.waste).sum();
                self.status = format!(
                    "групп: {}, дублей на {}, за {:.1} с",
                    self.groups.len(),
                    db::human(waste),
                    t.elapsed().as_secs_f32()
                );
            }
            let sum = dupes::summary(&self.items, &self.groups);
            ui.label(
                egui::RichText::new(format!(
                    "в группах {}, помечено лишними {}, освободится {}",
                    sum.groups,
                    sum.marked_drop,
                    db::human(sum.freed)
                ))
                .size(11.0)
                .color(DIM),
            );
            let has_groups = !self.groups.is_empty();
            ui.add_space(4.0);
            if ui
                .add_enabled(has_groups, egui::Button::new("Экспорт плана"))
                .on_hover_text(
                    "записать CSV в кэш приложения: group, keep_path, drop_path, reason, wasted_bytes; \
                     это список для проверки, а не удаление",
                )
                .clicked()
            {
                let path = db::cache_dir().join("plan.csv");
                let csv = dupes::plan_csv(&self.items, &self.groups);
                match std::fs::write(&path, &csv) {
                    Ok(()) => {
                        self.status = format!(
                            "план записан: {} (строк {})",
                            path.display(),
                            csv.lines().count().saturating_sub(1)
                        )
                    }
                    Err(e) => self.status = format!("план не записан: {e}"),
                }
            }
            if ui
                .add_enabled(has_groups, egui::Button::new("Скопировать список"))
                .on_hover_text("весь план CSV в буфер обмена")
                .clicked()
            {
                ui.ctx()
                    .copy_text(dupes::plan_csv(&self.items, &self.groups));
                self.status = "план скопирован в буфер обмена".into();
            }
            ui.label(
                egui::RichText::new(
                    "«оставить» выбирает лучший кадр группы: он остаётся, остальные помечаются лишними. \
                     Файлы не удаляются; решение хранится в БД и попадает в план.",
                )
                .size(11.0)
                .color(DIM),
            );
            ui.label(
                egui::RichText::new(
                    "«Экспорт плана» пишет CSV в кэш приложения — это отчёт для проверки, не удаление.",
                )
                .size(11.0)
                .color(DIM),
            );
            let no_sharp = self
                .items
                .iter()
                .filter(|r| r.kind == "image" && r.sharpness.is_none())
                .count();
            if no_sharp > 0 {
                ui.label(
                    egui::RichText::new(format!(
                        "резкость не посчитана у {no_sharp} снимков — проиндексируйте каталог"
                    ))
                    .size(11.0)
                    .color(DIM),
                );
            }
        });
    }

    /// Контекст вида «Лица»: порог, проход по снимкам, пересборка групп и их список.
    fn faces_controls(&mut self, ui: &mut egui::Ui) {
        block(ui, |ui| {
            ui.label(egui::RichText::new("Лица").size(11.0).color(DIM));
            ui.add_space(4.0);
            ui.label(egui::RichText::new(format!(
                "Порог склейки: {:.2}",
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
                    egui::RichText::new("модель лиц не скачана")
                        .size(11.0)
                        .color(DIM),
                );
                if ui.button("Настройки лиц").clicked() {
                    self.settings_open = true;
                    self.settings_tab = SettingsTab::Faces;
                }
            }
            ui.label(
                egui::RichText::new(format!(
                    "лиц: {} на {} снимках, групп: {}",
                    self.face_faces,
                    self.face_photos,
                    self.face_clusters.len()
                ))
                .size(11.0)
                .color(DIM),
            );
            ui.label(
                egui::RichText::new(format!(
                    "ещё без лиц: {} из {} снимков",
                    self.face_todo, self.face_images
                ))
                .size(11.0)
                .color(DIM),
            );
            ui.add_space(4.0);
            if self.face_rx.is_some() {
                ui.label("пересобираю группы…");
                if let Some(p) = &self.face_progress
                    && ui.button("Стоп").clicked()
                {
                    p.cancel();
                    self.status = "лица: останавливаю после текущих снимков…".into();
                }
            } else {
                let scan_label = if self.face_todo < self.face_images {
                    format!("Продолжить ({})", self.face_todo)
                } else {
                    format!("Найти лица ({})", self.face_todo)
                };
                if ui
                    .add_enabled(model && self.face_todo > 0, egui::Button::new(scan_label))
                    .on_hover_text(
                        "долгий проход: детекция и эмбеддинги локально, без сети; \
                         уже просканированные снимки пропускаются по sha256",
                    )
                    .clicked()
                {
                    self.start_faces();
                }
                if ui
                    .add_enabled(self.face_faces > 0, egui::Button::new("Пересобрать группы"))
                    .on_hover_text(
                        "порог применяется к лицам; ручные подписи и разъединения сохраняются",
                    )
                    .clicked()
                {
                    self.start_face_clusters();
                }
            }
            ui.add_space(8.0);
            ui.label(egui::RichText::new("Группы").size(11.0).color(DIM));
            let clusters = self.face_clusters.clone();
            egui::ScrollArea::vertical()
                .id_salt("face-clusters")
                .max_height(220.0)
                .show(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.face_selected_cluster.is_none(),
                            format!("Все лица · {}", self.face_faces),
                        )
                        .clicked()
                    {
                        self.face_selected_cluster = None;
                        self.face_label_input.clear();
                    }
                    for c in clusters {
                        let label = if c.label.is_empty() {
                            "без имени".to_string()
                        } else {
                            c.label.clone()
                        };
                        ui.horizontal(|ui| {
                            let mut mark = self.face_picked_clusters.contains(&c.id);
                            if ui
                                .checkbox(&mut mark, "")
                                .on_hover_text("отметить группу для поиска «все вместе»")
                                .changed()
                            {
                                if mark {
                                    self.face_picked_clusters.insert(c.id);
                                } else {
                                    self.face_picked_clusters.remove(&c.id);
                                }
                            }
                            if ui
                                .selectable_label(
                                    self.face_selected_cluster == Some(c.id),
                                    format!("{label} · {} фото", c.photos),
                                )
                                .clicked()
                            {
                                self.face_selected_cluster = Some(c.id);
                                self.face_label_input = c.label;
                            }
                        });
                    }
                });
            // «Все вместе»: снимки, где есть хотя бы по одному лицу из каждой отмеченной группы.
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
                            format!("Показать лица (вместе: {n})")
                        } else {
                            format!("Фото, где все вместе ({n})")
                        }),
                    )
                    .on_hover_text("показать только снимки, где есть лица из всех отмеченных групп")
                    .clicked()
                {
                    self.face_together = !self.face_together;
                }
            } else {
                self.face_together = false;
            }
        });
    }

    /// Блок модели: состояние, «Посчитать теги», фильтр по тегу.
    fn model_block(&mut self, ui: &mut egui::Ui) {
        block(ui, |ui| {
            ui.label(egui::RichText::new("Модель ИИ").size(11.0).color(DIM));
            ui.add_space(2.0);
            if embed::model_present(&self.settings) {
                let entry = embed::active_entry(&self.settings);
                ui.label(
                    egui::RichText::new(format!(
                        "{} · {}",
                        entry.title,
                        db::human(embed::model_bytes_in(&self.settings.models_dir, entry))
                    ))
                    .size(11.0)
                    .color(DIM),
                );
                if ui
                    .add_enabled(self.embed_rx.is_none(), egui::Button::new("Посчитать теги"))
                    .on_hover_text(
                        "локально прогнать CLIP по превью: 14 приблизительных категорий; \
                         нужный предмет ищите смысловым поиском словами; ничего не отправляется",
                    )
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
                    egui::RichText::new(format!("с уверенным тегом: {tagged}"))
                        .size(11.0)
                        .color(DIM),
                );
                ui.label(
                    egui::RichText::new(
                        "свободный смысловой поиск понимает любые слова, не только 14 тегов",
                    )
                    .size(11.0)
                    .color(DIM),
                );
                let mut own_tags: Vec<String> =
                    self.user_tags.values().flatten().cloned().collect();
                own_tags.sort();
                own_tags.dedup();
                // Число справа от тега — сколько снимков с ним при текущем пороге.
                let counts = self.tag_counts();
                egui::ComboBox::from_id_salt("tag_filter")
                    .width(ui.available_width())
                    .selected_text(
                        self.tag_filter
                            .clone()
                            .map(|t| format!("тег: {t}"))
                            .unwrap_or_else(|| "все теги".into()),
                    )
                    .show_ui(ui, |ui| {
                        if ui
                            .selectable_value(&mut self.tag_filter, None, "все теги")
                            .clicked()
                        {
                            ui.close();
                        }
                        for (label, n) in embed::TAG_LABELS.iter().zip(&counts) {
                            if ui
                                .selectable_value(
                                    &mut self.tag_filter,
                                    Some(label.to_string()),
                                    format!("{label} · {n}"),
                                )
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                        for label in &own_tags {
                            let n = self
                                .user_tags
                                .values()
                                .filter(|tags| tags.iter().any(|t| t == label))
                                .count();
                            if ui
                                .selectable_value(
                                    &mut self.tag_filter,
                                    Some(label.clone()),
                                    format!("свой: {label} · {n}"),
                                )
                                .clicked()
                            {
                                ui.close();
                            }
                        }
                    });
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new("Порог фильтра")
                        .size(11.0)
                        .color(DIM),
                );
                self.tag_filter_slider(ui);
                if !self.tags.is_empty() && counts.iter().all(|n| *n == 0) {
                    ui.label(
                        egui::RichText::new(
                            "ни один тег не проходит порог — опустите ползунок",
                        )
                        .size(11.0)
                        .color(DIM),
                    );
                }
                if let Some(hits) = &self.semantic_hits {
                    ui.label(
                        egui::RichText::new(format!("поиск: {} файлов", hits.len()))
                            .size(11.0)
                            .color(DIM),
                    );
                }
            } else {
                ui.label(
                    egui::RichText::new("модель не скачана")
                        .size(11.0)
                        .color(DIM),
                );
            }
            if ui.button("Настройки модели").clicked() {
                self.settings_open = true;
                self.settings_tab = SettingsTab::Model;
            }
        });
    }

    /// Блок текста на фото: сколько уже распознано и сколько осталось.
    fn ocr_block(&mut self, ui: &mut egui::Ui) {
        block(ui, |ui| {
            ui.label(egui::RichText::new("Текст на фото").size(11.0).color(DIM));
            ui.add_space(2.0);
            if self.tesseract.is_none() {
                ui.label(
                    egui::RichText::new("tesseract не запускается — путь в «Настройках»")
                        .size(11.0)
                        .color(DIM),
                );
            }
            let images = self.items.iter().filter(|r| r.kind == "image").count();
            ui.label(
                egui::RichText::new(format!("с текстом: {} из {images} снимков", self.ocr_files))
                    .size(11.0)
                    .color(DIM),
            );
            ui.label(
                egui::RichText::new(format!(
                    "кандидатов: {}, осталось {}",
                    self.ocr_candidates, self.ocr_remaining
                ))
                .size(11.0)
                .color(DIM),
            );
            ui.add_space(4.0);
            if self.ocr_rx.is_some() {
                if ui.button("Стоп").clicked()
                    && let Some(p) = &self.ocr_progress
                {
                    p.cancel();
                    self.status = "OCR: останавливаю после текущих файлов…".into();
                }
            } else {
                let label = if self.ocr_remaining < self.ocr_candidates {
                    format!("Продолжить ({})", self.ocr_remaining)
                } else {
                    format!("Распознать документы ({})", self.ocr_candidates)
                };
                if ui
                    .add_enabled(
                        self.tesseract.is_some() && self.ocr_remaining > 0,
                        egui::Button::new(label),
                    )
                    .on_hover_text(
                        "долгий проход: документы и скриншоты без камеры и по уверенному тегу; \
                         один снимок — кнопкой в карточке файла",
                    )
                    .clicked()
                {
                    self.start_ocr(None);
                }
            }
        });
    }

    /// Сводка внизу левой панели: только числа с подписями, без действий.
    fn stats_block(&mut self, ui: &mut egui::Ui) {
        ui.label(egui::RichText::new("Сводка").size(11.0).color(DIM));
        ui.add_space(2.0);
        let n = self.items.len();
        let bytes: i64 = self.items.iter().map(|r| r.size).sum();
        let no_date = self.items.iter().filter(|r| r.taken_at.is_none()).count();
        let no_thumb = self.items.iter().filter(|r| r.thumb.is_none()).count();
        stat_row(ui, "Файлов:", &n.to_string());
        stat_row(ui, "Объём:", &db::human(bytes));
        stat_row(ui, "Без даты:", &no_date.to_string());
        stat_row(ui, "Без превью:", &no_thumb.to_string());
        let no_cover = self
            .items
            .iter()
            .filter(|r| r.kind == "video" && r.thumb.is_none())
            .count();
        if no_cover > 0 {
            stat_row(ui, "Видео без обложки:", &no_cover.to_string());
            ui.label(
                egui::RichText::new("нажмите «Индексировать» — обложки создаст ffmpeg")
                    .size(11.0)
                    .color(DIM),
            );
        }
        ui.add_space(4.0);
    }

    /// Правая панель: сведения и действия по выбранному файлу.
    fn details_panel(&mut self, ui: &mut egui::Ui) {
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("Сведения о файле").size(14.0));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button("Скрыть")
                    .on_hover_text("освободить место сетке и календарю")
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

    /// Карточка файла: превью, метаданные, теги и текст. Одна на весь интерфейс.
    fn file_card(&mut self, ui: &mut egui::Ui) {
        let Some(id) = self.selected else {
            ui.add_space(8.0);
            ui.label(
                egui::RichText::new(
                    "Выберите снимок в сетке — здесь появятся дата, камера, теги и текст.",
                )
                .size(12.0)
                .color(DIM),
            );
            return;
        };
        let Some(f) = self.items.iter().find(|r| r.id == id).cloned() else {
            return;
        };

        // Текст с фото читается из БД один раз на файл, а не каждый кадр.
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
            clicked: None,
            open_clicked: None,
            show_keep: false,
            keep_clicked: None,
            trash_mode: false,
            restore_clicked: None,
            tile: self.tile_size,
            budget: 4,
        };
        if !paint_thumb(ui, &ctx, &mut cells, &f, rect.shrink(1.0)) {
            let txt = match f.kind.as_str() {
                "video" => "▶ видео".to_string(),
                _ => "нет превью".to_string(),
            };
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                txt,
                egui::FontId::proportional(12.0),
                DIM,
            );
        }
        ui.add_space(8.0);
        // Имя файла — главное, путь — подпись под ним (PhotoPrism-инспектор).
        ui.label(egui::RichText::new(file_name(&f.path)).size(14.0));
        ui.label(egui::RichText::new(&f.path).size(11.0).color(DIM));
        ui.add_space(6.0);
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Открыть полный экран")
                .on_hover_text("двойной клик по плитке делает то же")
                .clicked()
            {
                self.open_viewer(id);
            }
            if ui
                .button("В корзину")
                .on_hover_text(
                    "убрать из обычных видов в корзину приложения; на диске файл не меняется",
                )
                .clicked()
            {
                match db::mark_trashed(&self.db_path, id) {
                    Ok(()) => {
                        self.selected = None;
                        self.reload();
                        self.status = "файл в корзине приложения; на диске не тронут".into();
                    }
                    Err(e) => self.status = format!("файл не убран в корзину: {e}"),
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
            date_source(&f.taken_src)
        );
        meta_row(ui, "Дата", &date);
        if let Some(c) = &f.camera {
            meta_row(ui, "Камера", c);
        }
        if let (Some(a), Some(b)) = (f.lat, f.lon) {
            meta_row(ui, "GPS", &format!("{a:.5}, {b:.5}"));
        }
        let mut size = format!("{}×{} · {}", f.width, f.height, db::human(f.size));
        if f.kind == "video"
            && let Some(d) = f.duration_s
        {
            size.push_str(&format!(" · ▶ {}", db::duration(d)));
        }
        meta_row(ui, "Размер", &size);
        meta_row(
            ui,
            "SHA256",
            &format!("{}…", &f.sha256[..16.min(f.sha256.len())]),
        );

        section_label(ui, "Теги ИИ");
        // Уверенность — по отрыву первого тега от второго (порог и отрыв в настройках).
        // Если отрыва нет, «документ» не подсвечивается: честное «не уверен» и топ-3,
        // остальные веса — под раскрывающейся строкой, чтобы не выдавать шум за ответ.
        let confident = self.tags.get(&id).and_then(|tags| {
            embed::confident_tag_with(tags, self.settings.tag_show, self.settings.tag_margin)
                .map(|(tag, _)| tag.to_string())
        });
        let mut hide: Option<String> = None;
        if let Some(tags) = self.tags.get(&id).cloned() {
            match &confident {
                Some(tag) => {
                    ui.label(
                        egui::RichText::new(format!("тег: {tag}"))
                            .size(12.0)
                            .color(ACCENT),
                    );
                }
                None if !tags.is_empty() => {
                    ui.label(
                        egui::RichText::new("тег: не уверен — первые веса близки")
                            .size(12.0)
                            .color(DIM),
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
                tag_row(ui, tag, *score, pct, confident.as_deref(), &mut hide);
            }
            if confident.is_none() && tags.len() > top {
                egui::CollapsingHeader::new(format!("все теги ({})", tags.len()))
                    .id_salt(("card_tags", id))
                    .show(ui, |ui| {
                        for (tag, score) in tags.iter().skip(top) {
                            let pct = self.tag_percentile(tag, *score);
                            tag_row(ui, tag, *score, pct, None, &mut hide);
                        }
                    });
            }
            ui.spacing_mut().item_spacing.y = spacing;
        } else {
            ui.label(
                egui::RichText::new("тег: не посчитан")
                    .size(12.0)
                    .color(DIM),
            );
        }
        if let Some(tag) = hide {
            match db::hide_tag(&self.db_path, id, &tag) {
                Ok(()) => {
                    if let Some(tags) = self.tags.get_mut(&id) {
                        tags.retain(|(t, _)| t != &tag);
                    }
                    *self.hidden_tags.entry(id).or_insert(0) += 1;
                    self.status = format!("тег «{tag}» убран у этого фото");
                }
                Err(e) => self.status = format!("тег не убран: {e}"),
            }
        }
        let hidden = self.hidden_tags.get(&id).copied().unwrap_or(0);
        if hidden > 0
            && ui
                .button(format!("вернуть скрытые теги ({hidden})"))
                .clicked()
        {
            match db::unhide_tags(&self.db_path, id) {
                Ok(_) => {
                    self.reload();
                    self.status = "скрытые теги возвращены".into();
                }
                Err(e) => self.status = format!("теги не вернуть: {e}"),
            }
        }

        section_label(ui, "Свои теги");
        let own = self.user_tags.get(&id).cloned().unwrap_or_default();
        if own.is_empty() {
            ui.label(egui::RichText::new("пока нет").size(12.0).color(DIM));
        }
        let mut remove_own: Option<String> = None;
        for tag in &own {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(tag).size(12.0));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("убрать").clicked() {
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
                    self.status = format!("свой тег «{tag}» убран");
                }
                Err(e) => self.status = format!("тег не убран: {e}"),
            }
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.user_tag_input)
                    .hint_text("свой тег, например мебель")
                    .desired_width(150.0),
            );
            if ui.button("Добавить тег").clicked() {
                let tag = self.user_tag_input.trim().to_string();
                if !tag.is_empty() {
                    match db::add_user_tag(&self.db_path, id, &tag) {
                        Ok(()) => {
                            let tags = self.user_tags.entry(id).or_default();
                            tags.push(tag.clone());
                            tags.sort();
                            self.user_tag_input.clear();
                            self.status = format!("свой тег добавлен: «{tag}»");
                        }
                        Err(e) => self.status = format!("тег не добавлен: {e}"),
                    }
                }
            }
        });

        if f.kind == "image" {
            section_label(ui, "Текст на фото");
            let label = if self.ocr_text.get(&id).is_some_and(|t| !t.trim().is_empty()) {
                "Распознать заново"
            } else {
                "Распознать текст"
            };
            if ui
                .add_enabled(
                    self.tesseract.is_some() && self.ocr_rx.is_none(),
                    egui::Button::new(label),
                )
                .on_hover_text("только для этого снимка; оригинал не меняется")
                .clicked()
            {
                self.start_ocr(Some(id));
            }
            if self.tesseract.is_none() {
                ui.label(
                    egui::RichText::new("кнопка погашена: tesseract не запускается")
                        .size(11.0)
                        .color(DIM),
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
                    .button("Показать текст целиком")
                    .on_hover_text("полный экран: кадр и весь распознанный текст")
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

    /// Переход из лица к снимку: не в общую сетку, а в день календаря, где он лежит.
    /// Если даты нет — общая сетка с выделением этого файла.
    fn open_from_faces(&mut self, file_id: i64) {
        self.selected = Some(file_id);
        // Старый поиск/фильтр не должен прятать снимок в дне календаря.
        self.reset_filters();
        self.selected = Some(file_id);
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
                self.status = format!("снимок открыт в календаре: {}", d.format("%Y-%m-%d"));
            }
            None => {
                self.view = View::Grid;
                self.status = "у снимка нет даты — открыт в общих файлах с выделением".into();
            }
        }
    }

    /// Поставить кадр в очередь фонового декодирования. `front` — текущий кадр важнее соседей.
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

    /// Один фоновый декодер за раз: очередь не даёт десяткам потоков съесть память.
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

    /// Текущий кадр, следующий, предыдущий и один за следующим — для листания и слайд-шоу.
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

    /// Предзагрузка выбранного файла (клик по плитке до двойного клика).
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
                self.status = format!("кадр не декодирован: {e:#}");
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

    /// Видео играет встроенный плеер: кадр в текстуру, звук через cpal.
    /// Если файл ему не по зубам (кодек вне области, битый контейнер) —
    /// честно уступаем внешнему `ffplay`, а не показываем чёрный экран.
    fn start_video_playback(&mut self, f: &FileRow) {
        self.stop_video();
        self.video_lag_shown = false;
        self.video_id = Some(f.id);
        match player::Player::open(Path::new(&f.path), player::Options::default()) {
            Ok(p) => {
                self.status = format!(
                    "видео: {}{}",
                    f.path,
                    if p.has_audio() { " (со звуком)" } else { " (без звука)" }
                );
                self.video = Some(p);
            }
            Err(e) => {
                self.status = format!("встроенный плеер не взялся ({e:#}); открываю ffplay");
                let tools = self.settings.tools.clone();
                match tools::ffplay(&tools, Path::new(&f.path)) {
                    Ok(child) => {
                        self.video_child = Some(child);
                    }
                    Err(e2) => {
                        self.status = format!("ffplay не запустился ({e2:#}); системный проигрыватель");
                        match tools::open_path(Path::new(&f.path)) {
                            Ok(()) => {
                                self.status = format!("открываю системным плеером: {}", f.path)
                            }
                            Err(e3) => self.status = format!("видео не открыть: {e3:#}"),
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

    /// Кадр плеера → текстура. Пока видео играет, окно перерисовывается 30 раз в секунду.
    fn poll_video(&mut self, ctx: &egui::Context) {
        if let Some(child) = &mut self.video_child
            && child.try_wait().ok().flatten().is_some()
        {
            self.video_child = None;
            self.video_id = None;
            self.status = "ffplay: видео закрыто".into();
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
            self.status =
                "декодер не успевает за звуком: звук выключен, видео идёт медленнее".into();
        }
        if let Some(e) = p.error() {
            self.status = format!("видео остановилось: {e}");
            self.video = None;
            self.video_id = None;
            self.video_tex = None;
            return;
        }
        if p.is_finished() {
            self.status = "видео доиграло".into();
            // Доигравший файл сам уступает место следующему (см. `viewer_modal`).
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

    /// Список для листания в полноэкранном просмотре: текущий вид и текущий фильтр.
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
            show_text: false,
            slideshow: false,
            interval_s: 3,
            last_advance: std::time::Instant::now(),
        });
        self.viewer_tex = None;
    }

    /// Смена файла в полноэкранном просмотре.
    ///
    /// Видео обязательно останавливается: иначе кадр старого видео остаётся на
    /// экране поверх нового снимка, и листание выглядит как «ничего не показывает».
    fn viewer_step(&mut self, v: &mut Viewer, step: i32) {
        if step == 0 || v.ids.len() <= 1 {
            return;
        }
        self.stop_video();
        v.idx = (v.idx as i64 + step as i64).rem_euclid(v.ids.len() as i64) as usize;
        v.last_advance = std::time::Instant::now();
    }

    fn viewer_modal(&mut self, ctx: &egui::Context) {
        let Some(mut v) = self.viewer.take() else {
            return;
        };
        if v.ids.is_empty() {
            return;
        }
        v.idx = v.idx.min(v.ids.len() - 1);
        // Видео доиграло — показываем следующий файл сами: иначе показ стоит на
        // последнем кадре, и кажется, что листание не работает.
        let ended = std::mem::take(&mut self.video_ended);
        let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        let mut step = 0i32;
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            step = 1;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            step = -1;
        }
        // Нажатая в тот же кадр стрелка главнее: за один кадр — один шаг.
        if step == 0 && ended && v.idx + 1 < v.ids.len() {
            step = 1;
        }
        self.viewer_step(&mut v, step);
        if v.slideshow
            && v.last_advance.elapsed() >= std::time::Duration::from_secs(v.interval_s)
        {
            let next = v.ids[(v.idx + 1) % v.ids.len()];
            // Видео ждать не нужно: показывается кадр-обложка, полного декода нет.
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
                // Не показываем мыло: ждём готовый следующий кадр.
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
            self.status = "файл не найден в индексе — просмотр закрыт".into();
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
        // Пробел — то же, что ▶: у видео пуск/пауза, у снимка слайд-шоу.
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
        // Кадр плеера принадлежит конкретному файлу: без проверки старое видео
        // рисуется поверх нового снимка и смена файла не видна.
        let video_frame = self.video_tex.as_ref().filter(|_| self.video_id == Some(id));
        let paused = video_active && self.video.as_ref().is_some_and(|p| !p.is_playing());
        let current_full = self.viewer_tex.as_ref().filter(|(t, _)| *t == id);
        // У видео нет «полного кадра»: там кадр-обложка, а не загрузка.
        let stale = self
            .viewer_tex
            .as_ref()
            .filter(|(t, _)| *t != id && f.kind != "video")
            .map(|(_, t)| t);
        let mut new_zoom = v.zoom;
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
                    .unwrap_or_else(|| "без даты".into());
                ui.label(egui::RichText::new(date).size(11.0).color(DIM));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Закрыть").clicked() {
                        close = true;
                    }
                    ui.label(
                        egui::RichText::new(format!("{} из {}", v.idx + 1, v.ids.len()))
                            .size(11.0)
                            .color(DIM),
                    );
                });
            });
            ui.add_space(4.0);
            // Панель воспроизведения: листание, пуск/пауза, перемотка, звук.
            ui.horizontal_wrapped(|ui| {
                if icon_button(ui, Icon::Prev, 32.0, "предыдущий файл — ←").clicked() {
                    step_click = -1;
                }
                let playing = video_active && self.video.as_ref().is_some_and(|p| p.is_playing());
                if f.kind == "video" {
                    if video_active && self.video.is_none() {
                        ui.label(
                            egui::RichText::new("ffplay отдельным окном")
                                .size(11.0)
                                .color(DIM),
                        );
                    } else if icon_button(
                        ui,
                        if playing { Icon::Pause } else { Icon::Play },
                        32.0,
                        if video_active {
                            "пауза и продолжение — пробел"
                        } else {
                            "смотреть видео — пробел"
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
                    "слайд-шоу — пробел",
                )
                .clicked()
                {
                    new_slideshow = !v.slideshow;
                }
                if icon_button(ui, Icon::Next, 32.0, "следующий файл — →").clicked() {
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
                            .color(DIM),
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
                                egui::RichText::new("нет устройства").size(11.0).color(DIM),
                            );
                        }
                    }
                    if video_active
                        && ui
                            .selectable_label(true, "Стоп видео")
                            .on_hover_text("остановить и вернуться к кадру")
                            .clicked()
                    {
                        stop_video = true;
                    }
                } else {
                    if ui
                        .selectable_label(
                            v.slideshow,
                            if v.slideshow {
                                "Стоп слайд-шоу"
                            } else {
                                "Слайд-шоу"
                            },
                        )
                        .on_hover_text("пробел — пуск/стоп; листает текущий список")
                        .clicked()
                    {
                        new_slideshow = !v.slideshow;
                    }
                    ui.label(egui::RichText::new("интервал:").size(11.0).color(DIM));
                    for secs in [3u64, 5, 10] {
                        if ui
                            .selectable_label(v.interval_s == secs, format!("{secs} с"))
                            .clicked()
                        {
                            new_interval = secs;
                        }
                    }
                }
            });
            ui.horizontal_wrapped(|ui| {
                if ui.button("−").on_hover_text("уменьшить").clicked() {
                    new_zoom = (v.zoom - 0.25).max(0.5);
                }
                if ui.button("Вписать").clicked() {
                    new_zoom = 1.0;
                }
                if ui.button("+").on_hover_text("увеличить").clicked() {
                    new_zoom = (v.zoom + 0.25).min(4.0);
                }
                ui.label(
                    egui::RichText::new(format!("масштаб {:.0}%", v.zoom * 100.0))
                        .size(11.0)
                        .color(DIM),
                );
                if ui
                    .selectable_label(v.show_text, "Текст на фото")
                    .clicked()
                {
                    new_text = !v.show_text;
                }
                if f.kind != "video" && ui.button("В системном просмотрщике").clicked() {
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
                            img_w,
                            content_h,
                        ));
                    });
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("viewer-text")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new("Текст на фото").size(11.0).color(DIM));
                            let text = self.ocr_text.get(&id).cloned().unwrap_or_default();
                            if text.trim().is_empty() {
                                ui.label(
                                    egui::RichText::new(
                                        "Текст не распознан. Кнопка «Распознать текст» — в карточке файла.",
                                    )
                                    .size(12.0)
                                    .color(DIM),
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
                    avail.x,
                    content_h,
                ));
            }
            // Клик по кадру видео — как большая кнопка ▶: пуск или пауза.
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
        v.show_text = new_text;
        if v.slideshow != new_slideshow {
            v.last_advance = std::time::Instant::now();
        }
        v.slideshow = new_slideshow;
        v.interval_s = new_interval;
        if open_video {
            match tools::open_path(Path::new(&f.path)) {
                Ok(()) => self.status = format!("открываю системным проигрывателем: {}", f.path),
                Err(e) => self.status = format!("не открыть: {e:#}"),
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
        // Кнопки ←/→ листают после отрисовки: кадр нового файла — следующим кадром.
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

    /// Один проход по программам: версия, найденный файл, источник. Запускать в reload/сохранении.
    fn refresh_tool_states(&mut self) {
        self.tool_states = std::array::from_fn(|i| {
            let tool = tools::ALL[i];
            let where_ = match self.settings.tools.locate(tool) {
                Some(p) => format!("{} ({})", p.display(), self.settings.tools.source(tool)),
                None => format!(
                    "{} ({})",
                    self.settings.tools.program(tool),
                    self.settings.tools.source(tool)
                ),
            };
            match self.settings.tools.version(tool) {
                Ok(v) => format!("{} · {where_} · {}", short_version(&v), tool.purpose()),
                Err(e) => format!("не работает: {e:#} · {where_} · {}", tool.purpose()),
            }
        });
    }

    /// Путь к программе сохраняется только после успешного запуска версии.
    /// Ошибка остаётся в статусе (внизу окна), а не уходит в консоль.
    fn apply_tools(&mut self) {
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
            self.status = format!("программы не сохранены: {}", problems.join("; "));
            return;
        }
        self.settings.tools = self.tools_input.clone();
        self.save_settings();
        self.tesseract = self.settings.tools.probe(Tool::Tesseract);
        self.refresh_tool_states();
        self.status = if checked.is_empty() {
            "программы: все берутся из PATH".into()
        } else {
            format!("программы сохранены и проверены: {}", checked.join(", "))
        };
    }

    fn start_download(&mut self) {
        if self.download_rx.is_some() || !self.download_consent {
            return;
        }
        let settings = self.settings.clone();
        let requested = settings.model_id.clone();
        let progress = Arc::new(Progress::default());
        self.download_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.download_rx = Some(rx);
        self.status = "скачивание модели: только веса, без пользовательских данных".into();
        std::thread::spawn(move || {
            let r = embed::download_model(&settings, &requested, true, Some(progress));
            let _ = tx.send(r);
        });
    }

    /// Единственное место, где считается порядок плиток (M17).
    /// При смысловом поиске порядок задаёт поиск, иначе — `settings.grid_sort`.
    /// Ключ тотальный и устойчивый: дата (или имя/размер), затем `id`, поэтому
    /// одинаковые даты не переставляются между кадрами. Сетка, полный экран и
    /// выделение (M18) берут порядок только отсюда.
    fn ordered(&self, visible: &[usize]) -> Vec<usize> {
        let mut out = visible.to_vec();
        if self.semantic_hits.is_some() {
            // `filtered()` уже отдал индексы по убыванию похожести.
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
        }
        out
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        let mut out: Vec<usize> = self
            .items
            .iter()
            .enumerate()
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

    /// Порог отбора в фильтре — процентиль по выбранному тегу (ADR 0032).
    /// 100% — самый сильный снимок этого тега, 0% — все, 70% — верхние 30%.
    fn tag_filter_slider(&mut self, ui: &mut egui::Ui) {
        if self.tags.is_empty() {
            ui.label(
                egui::RichText::new("теги ещё не посчитаны — нажмите «Посчитать теги»")
                    .size(11.0)
                    .color(DIM),
            );
            return;
        }
        let mut pct = self.settings.tag_show_pct;
        let r = ui
            .add(
                egui::Slider::new(&mut pct, 0..=100)
                    .custom_formatter(|v, _| format!("{v:.0}%")),
            )
            .on_hover_text(
                "100% — самый сильный снимок этого тега, 0% — все; 70% — верхние 30%",
            );
        if r.changed() {
            self.settings.tag_show_pct = pct;
        }
        if r.drag_stopped() || r.lost_focus() {
            self.save_settings();
        }
        if self.tags.len() < 100 {
            ui.label(
                egui::RichText::new(format!(
                    "мало тегов ({} файлов) — процентиль шумный, берётся сырой порог {:.2}",
                    self.tags.len(),
                    self.settings.tag_show
                ))
                .size(11.0)
                .color(DIM),
            );
        }
    }

    /// Сырой порог для тега при текущем процентиле. На маленьком каталоге
    /// процентиль шумный, поэтому там действует сырой порог уверенного тега.
    fn tag_threshold(&self, tag: &str) -> f32 {
        if self.tags.len() < 100 {
            return self.settings.tag_show;
        }
        self.tag_dists
            .get(tag)
            .and_then(|d| embed::percentile_threshold(d, self.settings.tag_show_pct))
            .unwrap_or(self.settings.tag_show)
    }

    /// Порог «все теги»: процентиль по лучшему весу каждого файла.
    fn top_threshold(&self) -> f32 {
        if self.tags.len() < 100 {
            return self.settings.tag_show;
        }
        embed::percentile_threshold(&self.top_scores, self.settings.tag_show_pct)
            .unwrap_or(self.settings.tag_show)
    }

    /// Процентиль веса тега в индексе: 100 — самый сильный снимок этого тега.
    /// На маленьком каталоге не показывается (там шумно), остаётся только вес.
    fn tag_percentile(&self, tag: &str, score: f32) -> Option<u32> {
        if self.tags.len() < 100 {
            return None;
        }
        let d = self.tag_dists.get(tag)?;
        score_percentile(d, score)
    }

    /// Снимков на каждый тег из `TAG_LABELS` при его процентильном пороге: число рядом
    /// с тегом честно говорит «ноль», а не удивляет после нажатия.
    fn tag_counts(&self) -> Vec<usize> {
        let mut counts = vec![0usize; embed::TAG_LABELS.len()];
        for scores in self.tags.values() {
            for (tag, score) in scores {
                if *score < self.tag_threshold(tag) {
                    continue;
                }
                if let Some(i) = embed::TAG_LABELS.iter().position(|l| *l == tag.as_str()) {
                    counts[i] += 1;
                }
            }
        }
        counts
    }

    fn start_embed(&mut self) {
        if self.embed_rx.is_some() {
            return;
        }
        let db_path = self.db_path.clone();
        let progress = Arc::new(Progress::default());
        self.embed_progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.embed_rx = Some(rx);
        self.status = "CLIP: считаю эмбеддинги и теги…".into();
        std::thread::spawn(move || {
            let r = embed::embed_and_retag(&db_path, 8, 8, Some(progress));
            let _ = tx.send(r);
        });
    }



    /// `Some(id)` — только этот снимок (главное действие), `None` — пакетный проход по кандидатам.
    fn start_ocr(&mut self, file: Option<i64>) {
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
                    .unwrap_or_else(|| "выбранного снимка".into());
                format!("OCR: распознаю текст с {name}…")
            }
            None => format!(
                "OCR: пакетный проход по {} кандидатам…",
                self.ocr_candidates
            ),
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
                    self.status = format!(
                        "поиск по тексту «{q}»: файлов {} за {} мс",
                        hits.len(),
                        t.elapsed().as_millis()
                    );
                    self.ocr_snippets = hits.iter().cloned().collect();
                    self.semantic_hits = Some(hits.into_iter().map(|(id, _)| id).collect());
                }
                Err(e) => self.status = format!("поиск по тексту не удался: {e}"),
            }
            return;
        }
        if !embed::model_present(&self.settings) {
            self.status = "смысловой поиск недоступен: модель не скачана (см. «Настройки»)".into();
            return;
        }
        if let Some(model) = self.ml_model.as_mut() {
            let t = std::time::Instant::now();
            match embed::search(&self.db_path, model, &q, 200) {
                Ok(hits) => {
                    self.status = format!(
                        "смысловой поиск «{q}»: {} файлов за {} мс",
                        hits.len(),
                        t.elapsed().as_millis()
                    );
                    self.semantic_hits = Some(hits.into_iter().map(|(id, _)| id).collect());
                }
                Err(e) => self.status = format!("поиск не удался: {e}"),
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
        self.status = format!("CLIP: загружаю модель для запроса «{q}»…");
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
        self.status = format!("лица: моделей {}, порог {threshold:.2}…", faces::MODEL_ID);
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
        self.status = "лица: пересобираю группы…".into();
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
        self.status = "скачивание модели лиц: только веса, без фотографий".into();
        std::thread::spawn(move || {
            let r = faces::download_model(&settings, true, Some(progress));
            let _ = tx.send(r);
        });
    }

    fn start_index(&mut self) {
        let mut roots: Vec<PathBuf> = self.roots.iter().map(PathBuf::from).collect();
        let typed = self.path_input.trim();
        if !typed.is_empty() && !roots.iter().any(|r| r == Path::new(typed)) {
            roots.push(PathBuf::from(typed));
        }
        if roots.is_empty() {
            self.status =
                "укажите каталог в верхней панели или добавьте его кнопкой «Каталоги»".into();
            return;
        }
        let missing: Vec<String> = roots
            .iter()
            .filter(|r| !r.is_dir())
            .map(|r| r.display().to_string())
            .collect();
        roots.retain(|r| r.is_dir());
        if roots.is_empty() {
            self.status = format!("не каталог: {}", missing.join(", "));
            return;
        }
        let db_path = self.db_path.clone();
        let tools = self.settings.tools.clone();
        let jobs = self.settings.index_jobs;
        let progress = Arc::new(Progress::default());
        self.progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.status = if roots.len() == 1 {
            format!("индексация {}…", roots[0].display())
        } else {
            format!("индексация {} каталогов…", roots.len())
        };
        if !missing.is_empty() {
            self.status.push_str(&format!(" (пропущено: {})", missing.join(", ")));
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
                match index::run(&root, jobs, &db_path, &tools, Some(progress.clone())) {
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
                        eprintln!("индексация {}: {e}", root.display());
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
            }));
        });
    }
}

/// Плитка лица: та же метрика, что у файла (150×150 + подпись), но кадр — выровненное лицо,
/// а не превью файла. Поэтому у неё своя загрузка текстуры, а не `draw_cell`.
struct FaceCells {
    textures: HashMap<i64, egui::TextureHandle>,
    missing: HashSet<i64>,
    dir: PathBuf,
    selected_file: Option<i64>,
    split_clicked: Option<i64>,
    open_clicked: Option<i64>,
    viewer_clicked: Option<i64>,
    /// Выбранные лица: имя можно назначить сразу нескольким.
    picked: HashSet<i64>,
    tile: f32,
}

fn draw_face_cell(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut FaceCells, f: &db::FaceView) {
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
                "нет кадра",
                egui::FontId::proportional(12.0),
                DIM,
            );
        }
        let date = f
            .taken_at
            .as_deref()
            .map(|d| d.chars().take(10).collect::<String>())
            .unwrap_or_else(|| "без даты".into());
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
        // Ложная склейка разъединяется прямо на плитке, без перехода в другой экран.
        let btn = egui::Rect::from_min_size(
            egui::pos2(img_rect.right() - 62.0, img_rect.top() + 4.0),
            egui::vec2(58.0, 20.0),
        );
        if ui
            .place(btn, egui::Button::new("не он").small())
            .on_hover_text("отделить это лицо в новую группу")
            .clicked()
        {
            cs.split_clicked = Some(f.id);
            button_clicked = true;
        }
        // Пакетное назначение: отметить несколько лиц и дать им одно имя.
        let pick = egui::Rect::from_min_size(
            egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
            egui::vec2(66.0, 20.0),
        );
        let is_picked = cs.picked.contains(&f.id);
        if ui
            .place(
                pick,
                egui::Button::new(if is_picked {
                    "выбрано"
                } else {
                    "выбрать"
                })
                .small(),
            )
            .on_hover_text("отметить это лицо для общего имени")
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

/// Короткая подпись версии: первая строка и первые три слова («ffmpeg version 9.0.2»).
fn short_version(line: &str) -> String {
    line.split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ")
}

/// Процентиль веса по убывающему распределению: доля снимков с меньшим весом.
/// Самый сильный снимок тега — 100 (после округления), слабейший — 0.
fn score_percentile(sorted_desc: &[f32], score: f32) -> Option<u32> {
    if sorted_desc.is_empty() {
        return None;
    }
    let below = sorted_desc.iter().filter(|s| **s < score).count();
    Some((below as f64 * 100.0 / sorted_desc.len() as f64).round() as u32)
}

/// Строка тега в карточке: подпись, вес, процентиль и «убрать».
fn tag_row(
    ui: &mut egui::Ui,
    tag: &str,
    score: f32,
    pct: Option<u32>,
    confident: Option<&str>,
    hide: &mut Option<String>,
) {
    ui.horizontal(|ui| {
        let color = if confident == Some(tag) {
            ACCENT
        } else {
            ui.visuals().text_color()
        };
        ui.label(egui::RichText::new(tag).size(12.0).color(color));
        ui.label(
            egui::RichText::new(match pct {
                Some(p) => format!("{score:.2} · {p}%"),
                None => format!("{score:.2}"),
            })
            .size(11.0)
            .color(DIM),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .small_button("убрать")
                .on_hover_text("убрать тег у этого фото; оригинал не меняется")
                .clicked()
            {
                *hide = Some(tag.to_string());
            }
        });
    });
}

struct Cells<'a> {
    textures: &'a mut HashMap<i64, egui::TextureHandle>,
    missing: &'a mut HashSet<i64>,
    thumbs_dir: &'a Path,
    selected: Option<i64>,
    clicked: Option<i64>,
    /// Двойной клик — полноэкранный просмотр.
    open_clicked: Option<i64>,
    /// Показать на плитке кнопку «оставить» (вид «Дубли»).
    show_keep: bool,
    /// Нажата «оставить» у файла — применить решение к его группе.
    keep_clicked: Option<i64>,
    /// Вид «Корзина»: на плитке кнопка «вернуть» и пометка «в корзине».
    trash_mode: bool,
    restore_clicked: Option<i64>,
    /// Размер плитки, px: задаётся ползунком в левой панели.
    tile: f32,
    /// Сколько новых текстур декодировать за кадр: остальное — следующим кадром,
    /// иначе быстрая прокрутка упирается в декодирование JPEG в UI-потоке.
    budget: usize,
}

fn load_texture(ctx: &egui::Context, path: &Path, name: &str) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, &img.into_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

/// Вписать картинку в квадрат плитки, сохранив её пропорции: по краям остаются поля.
fn fit_rect(bounds: egui::Rect, size: egui::Vec2) -> egui::Rect {
    if size.x <= 0.0 || size.y <= 0.0 {
        return bounds;
    }
    let scale = (bounds.width() / size.x).min(bounds.height() / size.y);
    egui::Rect::from_center_size(bounds.center(), size * scale)
}

/// Один способ нарисовать превью: сетка, календарь и миниатюры дней используют его же.
/// Возвращает `true`, если картинка нарисована.
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

const MONTHS: [&str; 12] = [
    "январь",
    "февраль",
    "март",
    "апрель",
    "май",
    "июнь",
    "июль",
    "август",
    "сентябрь",
    "октябрь",
    "ноябрь",
    "декабрь",
];
const WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];

fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let m = month as i32 - 1 + delta;
    let y = year + m.div_euclid(12);
    let m = m.rem_euclid(12) as u32 + 1;
    (y.clamp(1900, 2199), m)
}

/// Дата из `taken_at` (`ГГГГ-ММ-ДД[THH:MM]`) без паники на мусоре.
fn parse_date(s: &str) -> Option<chrono::NaiveDate> {
    chrono::NaiveDate::parse_from_str(s.get(0..10)?, "%Y-%m-%d").ok()
}

/// Понедельник недели, в которую попадает дата.
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

/// Ближайший месяц в направлении `delta`, в котором есть файлы.
/// Пустые месяцы не показываем: листать их бессмысленно.
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

/// Ближайший месяц с файлами к указанному (сначала назад, потом вперёд).
#[allow(dead_code)]
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

/// Фрагмент текста с совпадениями: FTS5 отмечает их « », UI красит их акцентом.
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

/// Значок панели воспроизведения. Фигуры рисует painter, а не шрифт: эмодзи в
/// интерфейсе запрещены, а «▶» и «‖» в разных шрифтах разной ширины и высоты.
#[derive(Clone, Copy)]
enum Icon {
    Prev,
    Next,
    Play,
    Pause,
    Volume,
}

/// Кнопка-иконка панели воспроизведения: 32 px, подсказка обязательна.
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

/// Значок без действия — подпись рядом с ползунком (звук).
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

/// Геометрия значка: `s` — половина высоты фигуры, `c` — её центр.
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

/// Большая кнопка воспроизведения по центру кадра видео: до запуска видно, что
/// это видео, а не снимок, и один клик запускает его.
fn paint_play_overlay(ui: &egui::Ui, area: egui::Rect) {
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
    // Подпись на подложке: поверх светлого кадра текст иначе не читается.
    let galley = p.layout_no_wrap(
        "видео · нажмите ▶, чтобы смотреть".into(),
        egui::FontId::proportional(12.0),
        egui::Color32::from_gray(235),
    );
    let pos = egui::pos2(area.left() + 8.0, area.bottom() - 8.0 - galley.size().y);
    let bg = egui::Rect::from_min_size(pos, galley.size()).expand2(egui::vec2(4.0, 2.0));
    p.rect_filled(bg, 4.0, egui::Color32::from_black_alpha(130));
    p.galley(pos, galley, egui::Color32::from_gray(235));
}

/// Один способ нарисовать кадр в полноэкранном просмотре: 100% = вписать,
/// больше — увеличить и обрезать по области (прокрутка колесом не листает файл).
/// Возвращает отклик области кадра: клик по видео — пуск или пауза.
fn draw_viewer_image(
    ui: &mut egui::Ui,
    tex: ViewerTextures<'_>,
    f: &FileRow,
    zoom: f32,
    w: f32,
    h: f32,
) -> egui::Response {
    let (area, resp) = ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
    let resp = if f.kind == "video" {
        resp.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        resp
    };
    ui.painter()
        .rect_filled(area, 4.0, ui.visuals().extreme_bg_color);
    if let Some(t) = tex.video {
        // Играющий кадр: вписываем в область, без «мыла» и без ожидания.
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        p.image(
            t.id(),
            fit_rect(area.shrink(1.0), t.size_vec2()),
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
        if tex.paused {
            paint_play_overlay(ui, area);
        }
        return resp;
    }
    if let Some((tid, t)) = tex.full
        && *tid == f.id
    {
        let ts = t.size_vec2();
        let fit = (area.width() / ts.x).min(area.height() / ts.y);
        let size = ts * (fit * zoom);
        let img = egui::Rect::from_center_size(area.center(), size);
        let p = ui.painter().with_clip_rect(area.shrink(1.0));
        p.image(
            t.id(),
            img,
            egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            egui::Color32::WHITE,
        );
    } else if let Some(t) = tex.stale {
        // Пока следующий кадр декодируется, держим предыдущий резкий — без мыла.
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
            "загрузка следующего кадра…",
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(235),
        );
    } else if let Some(t) = tex.preview {
        // Нативная 320 px миниатюра по центру, без растягивания на весь экран.
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
        // У видео «полного кадра» не бывает: это кадр-обложка, ждать нечего.
        if f.kind != "video" {
            ui.painter().text(
                egui::pos2(area.left() + 8.0, area.bottom() - 8.0),
                egui::Align2::LEFT_BOTTOM,
                "полный кадр загружается…",
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(235),
            );
        }
    } else if f.kind != "video" {
        ui.painter().text(
            area.center(),
            egui::Align2::CENTER_CENTER,
            "полный кадр загружается…",
            egui::FontId::proportional(14.0),
            DIM,
        );
    }
    if f.kind == "video" {
        paint_play_overlay(ui, area);
    }
    resp
}

fn draw_cell(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, f: &FileRow) {
    let w = cs.tile.clamp(100.0, 260.0);
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(w, w + 26.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(w, w));
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, 4.0, ui.visuals().extreme_bg_color);
        let drawn = paint_thumb(ui, ctx, cs, f, img_rect);
        // Отклик на наведение: тонкая обводка и курсор — плитка читается как цель (M16).
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            ui.painter().rect_stroke(
                img_rect,
                4.0,
                egui::Stroke::new(1.0, ui.visuals().widgets.hovered.bg_stroke.color),
                egui::StrokeKind::Inside,
            );
        }
        if !drawn {
            let txt = match f.kind.as_str() {
                "video" => match f.duration_s {
                    Some(d) => format!("▶ видео · {}", db::duration(d)),
                    None => "▶ видео".into(),
                },
                _ => "нет превью".into(),
            };
            ui.painter().text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                txt,
                egui::FontId::proportional(12.0),
                DIM,
            );
        }
        // Видео видно по кадру-обложке, но подпись с длительностью не даёт спутать его с фото.
        if drawn && f.kind == "video" {
            let strip = egui::Rect::from_min_max(
                egui::pos2(img_rect.left(), img_rect.bottom() - 16.0),
                img_rect.right_bottom(),
            );
            ui.painter()
                .rect_filled(strip, 0.0, egui::Color32::from_black_alpha(130));
            let txt = match f.duration_s {
                Some(d) => format!("▶ {}", db::duration(d)),
                None => "▶ видео".into(),
            };
            ui.painter().text(
                egui::pos2(img_rect.left() + 4.0, img_rect.bottom() - 8.0),
                egui::Align2::LEFT_CENTER,
                txt,
                egui::FontId::proportional(11.0),
                egui::Color32::from_gray(235),
            );
        }
        let date = f
            .taken_at
            .as_deref()
            .map(|d| d.replace('T', " ").chars().take(16).collect::<String>())
            .unwrap_or_else(|| "без даты".into());
        ui.painter().text(
            egui::pos2(rect.left() + 3.0, img_rect.bottom() + 3.0),
            egui::Align2::LEFT_TOP,
            date,
            egui::FontId::proportional(11.0),
            egui::Color32::from_gray(190),
        );
        // Решение по дублю видно на плитке в любом виде, а не только по наведению.
        match f.decision.as_deref() {
            Some("drop") => {
                ui.painter()
                    .rect_filled(img_rect, 4.0, egui::Color32::from_black_alpha(150));
                ui.painter().text(
                    img_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "лишний",
                    egui::FontId::proportional(16.0),
                    egui::Color32::from_gray(210),
                );
            }
            Some("keep") => {
                ui.painter().rect_stroke(
                    img_rect,
                    4.0,
                    egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 170, 255)),
                    egui::StrokeKind::Inside,
                );
                ui.painter().text(
                    egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
                    egui::Align2::LEFT_TOP,
                    "оставить",
                    egui::FontId::proportional(12.0),
                    egui::Color32::from_rgb(90, 170, 255),
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
                .place(btn, egui::Button::new("оставить").small())
                .clicked()
            {
                cs.keep_clicked = Some(f.id);
            }
        }
        if cs.trash_mode {
            ui.painter()
                .rect_filled(img_rect, 4.0, egui::Color32::from_black_alpha(90));
            ui.painter().text(
                egui::pos2(img_rect.left() + 4.0, img_rect.top() + 4.0),
                egui::Align2::LEFT_TOP,
                "в корзине",
                egui::FontId::proportional(12.0),
                egui::Color32::from_gray(235),
            );
            let btn = egui::Rect::from_min_size(
                egui::pos2(img_rect.right() - 70.0, img_rect.bottom() - 26.0),
                egui::vec2(66.0, 20.0),
            );
            if ui
                .place(btn, egui::Button::new("вернуть").small())
                .on_hover_text("вернуть файл в обычные виды; на диске он не менялся")
                .clicked()
            {
                cs.restore_clicked = Some(f.id);
            }
        }
    }
    if cs.selected == Some(f.id) {
        ui.painter().rect_stroke(
            rect,
            4.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 170, 255)),
            egui::StrokeKind::Inside,
        );
    }
    if resp.clicked() {
        cs.clicked = Some(f.id);
    }
    if resp.double_clicked() {
        cs.open_clicked = Some(f.id);
    }
    if resp.hovered()
        && let Some(t) = cs.textures.get(&f.id).cloned()
    {
        resp.on_hover_ui_at_pointer(|ui| {
            // Тултип тоже не растягивает кадр: 400 px по длинной стороне, пропорции исходные.
            let big = egui::load::SizedTexture::new(t.id(), t.size_vec2());
            ui.add(egui::Image::new(big).max_size(egui::vec2(400.0, 400.0)));
            ui.label(&f.path);
        });
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let ctx = &ctx;
        // завершение фоновой индексации
        if let Some(rx) = &self.rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.status = match res {
                        Ok(s) => s.to_string(),
                        Err(e) => format!("индексация не удалась: {e}"),
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
                        Ok((embed, tags)) => {
                            self.reload();
                            self.status = format!("{embed}; {tags}");
                        }
                        Err(e) => self.status = format!("CLIP не посчитан: {e}"),
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

        if let Some(rx) = &self.search_rx {
            match rx.try_recv() {
                Ok(res) => {
                    self.search_rx = None;
                    match res {
                        Ok((model, hits, ms)) => {
                            self.ml_model = Some(model);
                            self.status =
                                format!("смысловой поиск: {} файлов за {ms:.0} мс", hits.len());
                            self.semantic_hits = Some(hits.into_iter().map(|(id, _)| id).collect());
                        }
                        Err(e) => self.status = format!("поиск не удался: {e}"),
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
                        Ok(report) => {
                            self.settings.model_id = report.model.to_string();
                            self.ml_model = None;
                            self.semantic_hits = None;
                            self.save_settings();
                            self.reload();
                            self.status = report.to_string();
                        }
                        Err(e) => self.status = format!("модель не скачана: {e}"),
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
                        Ok(stats) => {
                            self.reload();
                            self.status = if stats.cancelled {
                                stats.to_string()
                            } else if stats.candidates == 1 {
                                if stats.chars > 0 {
                                    format!("текст распознан: знаков {}", stats.chars)
                                } else {
                                    "на этом снимке текста не найдено".into()
                                }
                            } else {
                                stats.to_string()
                            };
                        }
                        Err(e) => self.status = format!("текст не распознан: {e:#}"),
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
                        FaceReply::Scan(Ok(stats)) => {
                            self.reload();
                            self.status = stats.to_string();
                        }
                        FaceReply::Scan(Err(e)) => self.status = format!("лица не найдены: {e:#}"),
                        FaceReply::Cluster(Ok(stats)) => {
                            self.reload();
                            self.status = stats.to_string();
                        }
                        FaceReply::Cluster(Err(e)) => {
                            self.status = format!("группы не собраны: {e:#}")
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
                        Err(e) => self.status = format!("корзина не очищена: {e:#}"),
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
                        Ok(report) => {
                            self.status = report.to_string();
                            self.reload();
                        }
                        Err(e) => self.status = format!("модель лиц не скачана: {e:#}"),
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

        egui::Panel::top("top")
            .resizable(false)
            .show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status")
            .resizable(false)
            .show(ui, |ui| self.status_bar(ui));

        // Сведения живут в правой панели, пока в сетке остаются 4 колонки;
        // на узком окне карточка уходит в левую, а сетка не «съёживется».
        let details = self.details_open && ui.available_width() >= DETAILS_MIN_W;
        let show_card = self.details_open && !details;
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
        self.trash_modal(ctx);
        self.poll_photo(ctx);
        self.poll_video(ctx);

        let visible = self.filtered();
        // Порядок плиток считается здесь и больше нигде (M17): сетка, полный экран
        // и будущее выделение берут один и тот же список.
        let ordered = self.ordered(&visible);
        // Снимок фильтров снимается до того, как сетка заберёт `&mut self.textures`.
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
            clicked: None,
            open_clicked: None,
            show_keep: self.view == View::Dupes,
            keep_clicked: None,
            trash_mode: self.view == View::Trash,
            restore_clicked: None,
            tile: self.tile_size,
            budget: 8,
        };
        let groups = std::mem::take(&mut self.groups);
        let trash_items = std::mem::take(&mut self.trash_items);
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
        let mut tile_save = false;
        let mut view_save = false;
        let mut clear_chips = FilterClear::default();
        let view = self.view;
        let mut cal_year = self.cal_year;
        let mut cal_month = self.cal_month;
        let mut cal_day = self.cal_day.clone();
        let mut cal_scale = self.cal_scale;
        let mut cal_week = self.cal_week;

        let (title, count) = match view {
            View::Grid => (
                "Все файлы".to_string(),
                if visible.len() == items.len() {
                    format!("файлов {}", items.len())
                } else {
                    format!("файлов {} из {}", visible.len(), items.len())
                },
            ),
            View::Dupes => {
                let waste: i64 = groups.iter().map(|g| g.waste).sum();
                (
                    "Дубли и серии".to_string(),
                    format!("групп {}, зря {}", groups.len(), db::human(waste)),
                )
            }
            View::Calendar => (
                "Календарь".to_string(),
                format!(
                    "{} {}",
                    MONTHS[(cal_month.clamp(1, 12) - 1) as usize],
                    cal_year
                ),
            ),
            View::Faces => (
                "Лица".to_string(),
                format!("лиц {} на {} снимках", face_total, face_photos),
            ),
            View::Trash => (
                "Корзина".to_string(),
                format!(
                    "файлов {}, {}",
                    self.trash_items.len(),
                    db::human(self.trash_items.iter().map(|r| r.size).sum())
                ),
            ),
        };

        egui::CentralPanel::default().show(ui, |ui| {
            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(title).size(18.0));
                ui.label(egui::RichText::new(count).size(12.0).color(DIM));
                // PhotoPrism: размер плиток живёт в панели содержимого, а не в левой панели.
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let size = ui.add(
                        egui::Slider::new(&mut self.tile_size, 100.0..=260.0)
                            .step_by(10.0)
                            .show_value(false),
                    );
                    if size.drag_stopped() || size.lost_focus() {
                        tile_save = true;
                    }
                    ui.label(
                        egui::RichText::new(format!("Плитки {:.0}", self.tile_size))
                            .size(11.0)
                            .color(DIM),
                    )
                    .on_hover_text("размер плиток, 100–260 px");
                    if view == View::Grid {
                        // M17: как смотреть (месяцы/подряд) и в каком порядке.
                        // Пока идёт смысловой поиск, порядок задаёт поиск — переключатель погашен.
                        let searching = self.semantic_hits.is_some();
                        let mut sort = self.settings.grid_sort;
                        let sort_resp = ui.add_enabled_ui(!searching, |ui| {
                            ui.menu_button("Сортировка", |ui| {
                                for (value, label) in GRID_SORTS {
                                    ui.selectable_value(&mut sort, value, label);
                                }
                                ui.label(
                                    egui::RichText::new(format!(
                                        "сейчас: {}",
                                        sort_label(sort)
                                    ))
                                    .size(11.0)
                                    .color(DIM),
                                );
                            })
                        });
                        sort_resp
                            .inner
                            .response
                            .on_disabled_hover_text("порядок задаёт поиск")
                            .on_hover_text("порядок плиток: дата, имя или размер");
                        if sort != self.settings.grid_sort {
                            self.settings.grid_sort = sort;
                            view_save = true;
                        }
                        let mut layout = self.settings.grid_layout;
                        for (value, label) in [
                            (db::GridLayout::Flat, "Подряд"),
                            (db::GridLayout::Months, "По месяцам"),
                        ] {
                            let r = ui.add_enabled_ui(!searching, |ui| {
                                ui.selectable_label(layout == value, label)
                            });
                            if r.inner.clicked() {
                                layout = value;
                            }
                            r.inner.on_disabled_hover_text("порядок задаёт поиск");
                        }
                        if layout != self.settings.grid_layout {
                            self.settings.grid_layout = layout;
                            view_save = true;
                        }
                    }
                });
            });
            ui.add_space(4.0);
            if matches!(view, View::Grid | View::Calendar) {
                clear_chips = draw_filter_chips(ui, &chips, (visible.len(), items.len()));
            }
            ui.add_space(2.0);
            ui.separator();
            match view {
            View::Grid => {
                if items.is_empty() {
                    ui.add_space(12.0);
                    ui.label(
                        "Добавьте каталог кнопкой «Каталоги» или введите путь вверху, затем \
                         нажмите «Индексировать».",
                    );
                    return;
                }
                if visible.is_empty() {
                    ui.add_space(12.0);
                    match &tag_filter_now {
                        Some(tag) => ui.label(format!(
                            "нет снимков с тегом «{tag}» выше порога {}% ({tag_filter_raw:.2}) — \
                             опустите ползунок порога в левой панели",
                            self.settings.tag_show_pct
                        )),
                        None => ui.label("ничего не найдено"),
                    };
                    if ui.button("Сбросить фильтр").clicked() {
                        reset = true;
                    }
                    return;
                }
                egui::ScrollArea::vertical()
                    .id_salt("grid")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Порядок пришёл из `App::ordered`; здесь только раскладка.
                        // «Подряд» — одна непрерывная сетка без заголовков месяцев.
                        // При смысловом поиске порядок задаёт поиск, поэтому список плоский.
                        if self.semantic_hits.is_some()
                            || self.settings.grid_layout == db::GridLayout::Flat
                        {
                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(TILE_GAP, TILE_GAP);
                                ui.horizontal_wrapped(|ui| {
                                    for &i in &ordered {
                                        draw_cell(ui, ctx, &mut cells, &items[i]);
                                    }
                                });
                            });
                            return;
                        }
                        // «По месяцам»: месяцы в порядке сортировки, «Без даты» — последней
                        // группой в обе стороны; внутри месяца — порядок `ordered`.
                        let ascend = self.settings.grid_sort == db::GridSort::DateAsc;
                        for (key, ids) in month_groups(&ordered, &items, ascend) {
                            let bytes: i64 = ids.iter().map(|&i| items[i].size).sum();
                            month_header(ui, &key, ids.len(), bytes);
                            ui.scope(|ui| {
                                ui.spacing_mut().item_spacing = egui::vec2(TILE_GAP, TILE_GAP);
                                ui.horizontal_wrapped(|ui| {
                                    for &i in &ids {
                                        draw_cell(ui, ctx, &mut cells, &items[i]);
                                    }
                                });
                            });
                            ui.add_space(16.0);
                        }
                    });
            }
            View::Dupes => {
                if groups.is_empty() {
                    ui.label("Нажмите «Найти группы» слева.");
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (n, g) in groups.iter().enumerate() {
                            ui.group(|ui| {
                                ui.label(format!(
                                    "#{} — {} · файлов {} · зря {}",
                                    n + 1,
                                    g.reason,
                                    g.members.len(),
                                    db::human(g.waste)
                                ));
                                let keep = dupes::keep_of(&items, g);
                                let name = Path::new(&items[keep].path)
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or(&items[keep].path);
                                let decided = g
                                    .members
                                    .iter()
                                    .any(|&i| items[i].decision.as_deref() == Some("keep"));
                                ui.label(format!(
                                    "{}: оставить {} — {}",
                                    if decided { "решение" } else { "совет" },
                                    name,
                                    g.advice
                                ));
                                ui.horizontal_wrapped(|ui| {
                                    for &i in &g.members {
                                        draw_cell(ui, ctx, &mut cells, &items[i]);
                                    }
                                });
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
                                .unwrap_or_else(|| "без даты".into());
                            by_day.entry(k).or_default().push(i);
                        }
                        let months: BTreeSet<String> = by_day
                            .keys()
                            .filter(|k| k.as_str() != "без даты")
                            .map(|d| d.chars().take(7).collect())
                            .collect();

                        // Слои календаря: год → месяц → неделя → день. Вкладки возвращают наверх,
                        // двойной клик по ячейке проваливает на слой ниже.
                        ui.horizontal(|ui| {
                            for (scale, label) in [
                                (CalScale::Year, "Год"),
                                (CalScale::Month, "Месяц"),
                                (CalScale::Week, "Неделя"),
                                (CalScale::Day, "День"),
                            ] {
                                if ui
                                    .selectable_label(cal_scale == scale, label)
                                    .clicked()
                                {
                                    // Выбранный день — точка опоры: месяц и неделя
                                    // открываются там, где он лежит.
                                    if let Some(d) = cal_day.as_deref().and_then(parse_date) {
                                        cal_year = d.year();
                                        cal_month = d.month();
                                        cal_week = monday_of(d);
                                    }
                                    // В «День» без выбранного дня показываем сегодняшний,
                                    // если он в открытом месяце, иначе первое число.
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
                            if ui.button("Сегодня").clicked() {
                                let today = chrono::Local::now().date_naive();
                                cal_year = today.year();
                                cal_month = today.month();
                                cal_week = monday_of(today);
                                // На слое «День» сегодня — выбор, а не сброс.
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
                                CalScale::Year => "предыдущий год",
                                CalScale::Month => "предыдущий месяц с файлами",
                                CalScale::Week => "предыдущая неделя",
                                CalScale::Day => "предыдущий день",
                            };
                            let next_label = match cal_scale {
                                CalScale::Year => "следующий год",
                                CalScale::Month => "следующий месяц с файлами",
                                CalScale::Week => "следующая неделя",
                                CalScale::Day => "следующий день",
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
                                .on_hover_text(prev_label)
                                .clicked()
                            {
                                nav = -1;
                            }
                            let title = match cal_scale {
                                CalScale::Year => format!("{cal_year}"),
                                CalScale::Month => format!(
                                    "{} {}",
                                    MONTHS[(cal_month.clamp(1, 12) - 1) as usize],
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
                                            WEEKDAYS
                                                [d.weekday().num_days_from_monday() as usize],
                                            d.format("%d.%m.%Y")
                                        )
                                    })
                                    .unwrap_or_else(|| "день не выбран".into()),
                            };
                            ui.heading(title);
                            if ui
                                .add_enabled(next_enabled, egui::Button::new("→"))
                                .on_hover_text(next_label)
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
                                let cell_w = ((ui.available_width() - 14.0) / 4.0).max(40.0);
                                for row in 0..3u32 {
                                    ui.horizontal(|ui| {
                                        for col in 0..4u32 {
                                            let m = row * 4 + col + 1;
                                            let key = format!("{cal_year:04}-{m:02}");
                                            let count: usize = by_day
                                                .iter()
                                                .filter(|(d, _)| d.starts_with(&key))
                                                .map(|(_, v)| v.len())
                                                .sum();
                                            let (rect, resp) = ui.allocate_exact_size(
                                                egui::vec2(cell_w, 92.0),
                                                egui::Sense::click(),
                                            );
                                            let resp = resp
                                                .on_hover_text("двойной клик — открыть месяц");
                                            if ui.is_rect_visible(rect) {
                                                let selected = cal_month == m;
                                                let fill = if selected {
                                                    ui.visuals()
                                                        .selection
                                                        .bg_fill
                                                        .gamma_multiply(0.25)
                                                } else {
                                                    ui.visuals().faint_bg_color
                                                };
                                                ui.painter().rect_filled(
                                                    rect.shrink(1.0),
                                                    4.0,
                                                    fill,
                                                );
                                                ui.painter().text(
                                                    rect.left_top() + egui::vec2(8.0, 6.0),
                                                    egui::Align2::LEFT_TOP,
                                                    MONTHS[(m - 1) as usize],
                                                    egui::FontId::proportional(13.0),
                                                    egui::Color32::from_gray(190),
                                                );
                                                ui.painter().text(
                                                    rect.right_top() + egui::vec2(-8.0, 6.0),
                                                    egui::Align2::RIGHT_TOP,
                                                    format!("{count}"),
                                                    egui::FontId::proportional(11.0),
                                                    DIM,
                                                );
                                                let ids: Vec<usize> = by_day
                                                    .iter()
                                                    .filter(|(d, _)| d.starts_with(&key))
                                                    .flat_map(|(_, v)| v.iter().copied())
                                                    .take(4)
                                                    .collect();
                                                if !ids.is_empty() {
                                                    let area = egui::Rect::from_min_max(
                                                        rect.left_top() + egui::vec2(6.0, 26.0),
                                                        rect.right_bottom() - egui::vec2(6.0, 6.0),
                                                    );
                                                    let tw = ((area.width() - 6.0) / 4.0)
                                                        .max(8.0);
                                                    for (k, &i) in ids.iter().enumerate() {
                                                        let r = egui::Rect::from_min_size(
                                                            egui::pos2(
                                                                area.left()
                                                                    + k as f32 * (tw + 2.0),
                                                                area.top(),
                                                            ),
                                                            egui::vec2(tw, area.height()),
                                                        );
                                                        paint_thumb(
                                                            ui, ctx, &mut cells, &items[i], r,
                                                        );
                                                    }
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
                                    ui.add_space(2.0);
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
                                ui.label(format!(
                                    "{month_n} файлов за месяц, {}",
                                    db::human(month_bytes)
                                ));
                                if month_n == 0 {
                                    ui.add_space(12.0);
                                    ui.label("В этом месяце файлов нет.");
                                    if let Some(last) = months.iter().next_back()
                                        && let (Some(y), Some(m)) =
                                            (last.get(0..4), last.get(5..7))
                                        && let (Ok(y), Ok(m)) =
                                            (y.parse::<i32>(), m.parse::<u32>())
                                        && ui.button(format!("Перейти к {last}")).clicked()
                                    {
                                        cal_year = y;
                                        cal_month = m;
                                        cal_day = None;
                                    }
                                    return;
                                }

                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    let wd_w = (ui.available_width() - 6.0 * 4.0) / 7.0;
                                    for wd in WEEKDAYS {
                                        ui.allocate_ui(
                                            egui::vec2(wd_w, 18.0),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(wd)
                                                        .small()
                                                        .color(egui::Color32::from_gray(150)),
                                                );
                                            },
                                        );
                                    }
                                });
                                let cell_w = ((ui.available_width() - 6.0 * 4.0 - 2.0) / 7.0)
                                    .max(32.0);
                                let cell_h = 96.0;
                                let first = first_weekday_monday(cal_year, cal_month);
                                let n_days = days_in_month(cal_year, cal_month);
                                let mut day = 1u32;
                                for _week in 0..6 {
                                    if day > n_days {
                                        break;
                                    }
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = 4.0;
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
                                            let idxs = by_day.get(&date);
                                            let (rect, resp) = ui.allocate_exact_size(
                                                egui::vec2(cell_w, cell_h),
                                                egui::Sense::click(),
                                            );
                                            let resp = resp
                                                .on_hover_text("двойной клик — открыть неделю");
                                            if ui.is_rect_visible(rect) {
                                                let selected = cal_day.as_deref()
                                                    == Some(date.as_str());
                                                let fill = if selected {
                                                    ui.visuals()
                                                        .selection
                                                        .bg_fill
                                                        .gamma_multiply(0.25)
                                                } else {
                                                    ui.visuals().faint_bg_color
                                                };
                                                ui.painter().rect_filled(
                                                    rect.shrink(1.0),
                                                    4.0,
                                                    fill,
                                                );
                                                if selected {
                                                    ui.painter().rect_stroke(
                                                        rect.shrink(1.0),
                                                        4.0,
                                                        egui::Stroke::new(
                                                            2.0,
                                                            egui::Color32::from_rgb(90, 170, 255),
                                                        ),
                                                        egui::StrokeKind::Inside,
                                                    );
                                                }
                                                ui.painter().text(
                                                    rect.left_top() + egui::vec2(6.0, 4.0),
                                                    egui::Align2::LEFT_TOP,
                                                    day.to_string(),
                                                    egui::FontId::proportional(12.0),
                                                    egui::Color32::from_gray(190),
                                                );
                                                if let Some(ids) = idxs {
                                                    ui.painter().text(
                                                        rect.right_top()
                                                            + egui::vec2(-6.0, 4.0),
                                                        egui::Align2::RIGHT_TOP,
                                                        ids.len().to_string(),
                                                        egui::FontId::proportional(11.0),
                                                        DIM,
                                                    );
                                                    let area = egui::Rect::from_min_max(
                                                        rect.left_top()
                                                            + egui::vec2(4.0, 22.0),
                                                        rect.right_bottom()
                                                            - egui::vec2(4.0, 4.0),
                                                    );
                                                    let n = ids.len().min(3);
                                                    let tw = ((area.width()
                                                        - (n as f32 - 1.0) * 2.0)
                                                        / n as f32)
                                                        .max(8.0);
                                                    for (k, &i) in
                                                        ids.iter().take(n).enumerate()
                                                    {
                                                        let r = egui::Rect::from_min_size(
                                                            egui::pos2(
                                                                area.left()
                                                                    + k as f32
                                                                        * (tw + 2.0),
                                                                area.top(),
                                                            ),
                                                            egui::vec2(tw, area.height()),
                                                        );
                                                        ui.painter().rect_filled(
                                                            r,
                                                            2.0,
                                                            ui.visuals().extreme_bg_color,
                                                        );
                                                        paint_thumb(
                                                            ui, ctx, &mut cells, &items[i], r,
                                                        );
                                                    }
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
                                    ui.add_space(2.0);
                                }
                            }
                            CalScale::Week => {
                                let cell_w = ((ui.available_width() - 6.0 * 4.0 - 2.0) / 7.0)
                                    .max(32.0);
                                let cell_h = 190.0;
                                ui.add_space(4.0);
                                ui.horizontal(|ui| {
                                    ui.spacing_mut().item_spacing.x = 4.0;
                                    for d in 0..7i64 {
                                        let date = cal_week + chrono::Duration::days(d);
                                        let key = date.format("%Y-%m-%d").to_string();
                                        let idxs = by_day.get(&key);
                                        let (rect, resp) = ui.allocate_exact_size(
                                            egui::vec2(cell_w, cell_h),
                                            egui::Sense::click(),
                                        );
                                        let resp = resp
                                            .on_hover_text("двойной клик — открыть день");
                                        if ui.is_rect_visible(rect) {
                                            let selected = cal_day.as_deref()
                                                == Some(key.as_str());
                                            let fill = if selected {
                                                ui.visuals()
                                                    .selection
                                                    .bg_fill
                                                    .gamma_multiply(0.25)
                                            } else {
                                                ui.visuals().faint_bg_color
                                            };
                                            ui.painter().rect_filled(
                                                rect.shrink(1.0),
                                                4.0,
                                                fill,
                                            );
                                            if selected {
                                                ui.painter().rect_stroke(
                                                    rect.shrink(1.0),
                                                    4.0,
                                                    egui::Stroke::new(
                                                        2.0,
                                                        egui::Color32::from_rgb(90, 170, 255),
                                                    ),
                                                    egui::StrokeKind::Inside,
                                                );
                                            }
                                            ui.painter().text(
                                                rect.left_top() + egui::vec2(6.0, 4.0),
                                                egui::Align2::LEFT_TOP,
                                                format!(
                                                    "{} {}",
                                                    WEEKDAYS[d as usize],
                                                    date.format("%d.%m")
                                                ),
                                                egui::FontId::proportional(12.0),
                                                egui::Color32::from_gray(190),
                                            );
                                            if let Some(ids) = idxs {
                                                ui.painter().text(
                                                    rect.right_top()
                                                        + egui::vec2(-6.0, 4.0),
                                                    egui::Align2::RIGHT_TOP,
                                                    ids.len().to_string(),
                                                    egui::FontId::proportional(11.0),
                                                    DIM,
                                                );
                                                let area = egui::Rect::from_min_max(
                                                    rect.left_top() + egui::vec2(4.0, 24.0),
                                                    rect.right_bottom()
                                                        - egui::vec2(4.0, 4.0),
                                                );
                                                let rows = 3usize;
                                                let cols = 2usize;
                                                let tw = ((area.width()
                                                    - (cols as f32 - 1.0) * 2.0)
                                                    / cols as f32)
                                                    .max(8.0);
                                                let th = ((area.height()
                                                    - (rows as f32 - 1.0) * 2.0)
                                                    / rows as f32)
                                                    .max(8.0);
                                                for (k, &i) in ids
                                                    .iter()
                                                    .take(rows * cols)
                                                    .enumerate()
                                                {
                                                    let r = egui::Rect::from_min_size(
                                                        egui::pos2(
                                                            area.left()
                                                                + (k % cols) as f32
                                                                    * (tw + 2.0),
                                                            area.top()
                                                                + (k / cols) as f32
                                                                    * (th + 2.0),
                                                        ),
                                                        egui::vec2(tw, th),
                                                    );
                                                    paint_thumb(
                                                        ui, ctx, &mut cells, &items[i], r,
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
                                        ui.heading(format!(
                                            "{} файлов, {}",
                                            idxs.len(),
                                            db::human(bytes)
                                        ));
                                        ui.horizontal_wrapped(|ui| {
                                            for &i in idxs {
                                                draw_cell(ui, ctx, &mut cells, &items[i]);
                                            }
                                        });
                                    }
                                    _ => {
                                        ui.label("В этот день файлов нет.");
                                    }
                                }
                            }
                        }
                        if cal_scale != CalScale::Day {
                            ui.separator();
                            if let Some(day) = &cal_day {
                                match by_day.get(day) {
                                    Some(idxs) => {
                                        let bytes: i64 =
                                            idxs.iter().map(|&i| items[i].size).sum();
                                        ui.heading(format!(
                                            "{day} — {} файлов, {}",
                                            idxs.len(),
                                            db::human(bytes)
                                        ));
                                        ui.horizontal_wrapped(|ui| {
                                            for &i in idxs {
                                                draw_cell(ui, ctx, &mut cells, &items[i]);
                                            }
                                        });
                                    }
                                    None => {
                                        ui.label("В этот день файлов нет.");
                                    }
                                }
                            } else {
                                ui.label(
                                    "Выберите день в календаре — ниже появятся снимки.",
                                );
                            }
                        }
                        if let Some(idxs) = by_day.get("без даты") {
                            ui.separator();
                            egui::CollapsingHeader::new(format!(
                                "Без даты — {} файлов",
                                idxs.len()
                            ))
                            .id_salt("no-date")
                            .show(ui, |ui| {
                                ui.horizontal_wrapped(|ui| {
                                    for &i in idxs {
                                        draw_cell(ui, ctx, &mut cells, &items[i]);
                                    }
                                });
                            });
                        }
                    });
            }
            View::Faces => {
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
                                "без имени".to_string()
                            } else {
                                c.label.clone()
                            }
                        })
                        .collect();
                    ui.horizontal(|ui| {
                        ui.heading("Фото, где все вместе");
                        ui.label(
                            egui::RichText::new(names.join(" + "))
                                .size(12.0)
                                .color(DIM),
                        );
                    });
                    ui.label(format!("снимков: {}", ids.len()));
                    if ids.is_empty() {
                        ui.label("Нет снимков, где есть лица из всех отмеченных групп.");
                    }
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for f in &items {
                                    if ids.contains(&f.id) {
                                        draw_cell(ui, ctx, &mut cells, f);
                                    }
                                }
                            });
                        });
                } else {
                let current = face_clusters
                    .iter()
                    .find(|c| Some(c.id) == face_selected)
                    .cloned();
                if let Some(c) = &current {
                    ui.horizontal(|ui| {
                        ui.heading(if c.label.is_empty() {
                            "без имени".to_string()
                        } else {
                            c.label.clone()
                        });
                        ui.label(format!("лиц {} · фото {}", c.faces, c.photos));
                    });
                    ui.horizontal(|ui| {
                        ui.label("Имя человека:");
                        ui.add(
                            egui::TextEdit::singleline(&mut face_label_input)
                                .hint_text("например, Лиза")
                                .desired_width(220.0),
                        );
                        if ui.button("Сохранить имя").clicked() {
                            face_save_label = Some(face_label_input.trim().to_string());
                        }
                    });
                    ui.label(
                        "Подпись закрепляет группу: пересборка по порогу её не перемешает. \
                         Ложную склейку разъединяет кнопка «не он» на плитке.",
                    );
                } else {
                    ui.label(
                        egui::RichText::new(
                            "Выберите группу слева, чтобы дать ей имя, или разъединяйте плитки.",
                        )
                        .size(12.0)
                        .color(DIM),
                    );
                }
                let shown: Vec<&db::FaceView> = face_views
                    .iter()
                    .filter(|f| face_selected.is_none() || f.cluster_id == face_selected)
                    .collect();
                // Пакетное назначение: отметить лица кнопкой «выбрать» и дать одно имя.
                ui.separator();
                ui.horizontal_wrapped(|ui| {
                    ui.label(format!("Выбрано лиц: {}", face_picked.len()));
                    ui.add(
                        egui::TextEdit::singleline(&mut face_group_input)
                            .hint_text("имя для выбранных")
                            .desired_width(220.0),
                    );
                    if ui
                        .add_enabled(
                            !face_picked.is_empty(),
                            egui::Button::new("Назначить имя выбранным"),
                        )
                        .on_hover_text(
                            "отмеченные лица уедут в одну группу; пустое имя создаст группу без подписи",
                        )
                        .clicked()
                    {
                        face_assign = Some(face_group_input.trim().to_string());
                    }
                    if ui
                        .add_enabled(!face_picked.is_empty(), egui::Button::new("Снять выбор"))
                        .clicked()
                    {
                        face_picked.clear();
                        clear_picked = true;
                    }
                });
                if face_total == 0 {
                    ui.add_space(12.0);
                    ui.label("Лиц пока нет: нажмите «Найти лица» слева (нужна модель лиц).");
                } else if shown.is_empty() {
                    ui.label("В этой группе нет лиц.");
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for f in &shown {
                                draw_face_cell(ui, ctx, &mut face_cells, f);
                            }
                        });
                    });
                }
            }
            View::Trash => {
                ui.label(
                    "Файлы в корзине приложения: на диске не тронуты. «Вернуть» — снова в архив,                      «Очистить корзину» — перенос в системную корзину только после подтверждения.",
                );
                if trash_items.is_empty() {
                    ui.add_space(12.0);
                    ui.label("Корзина пуста.");
                } else {
                    ui.horizontal(|ui| {
                        let bytes: i64 = trash_items.iter().map(|r| r.size).sum();
                        ui.label(format!(
                            "файлов {}, {}",
                            trash_items.len(),
                            db::human(bytes)
                        ));
                        if ui.button("Вернуть всё").clicked() {
                            trash_restore_all = true;
                        }
                        if ui
                            .add(primary_button(&format!(
                                "Очистить корзину ({})",
                                trash_items.len()
                            )))
                            .on_hover_text(
                                "перенос в системную корзину ОС; безвозвратного удаления нет",
                            )
                            .clicked()
                        {
                            trash_clear = true;
                        }
                    });
                    egui::ScrollArea::vertical()
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            ui.horizontal_wrapped(|ui| {
                                for r in &trash_items {
                                    draw_cell(ui, ctx, &mut cells, r);
                                }
                            });
                        });
                }
            }
            }
        });

        self.items = items;
        self.groups = groups;
        self.trash_items = trash_items;
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
        // Освобождаем заимствования self.textures/missing до open_viewer/reload.
        #[allow(clippy::drop_non_drop)]
        drop(cells);
        // Размер плиток пишется в настройки после отпускания ползунка, а не каждый кадр.
        if tile_save {
            self.settings.tile_size = self.tile_size;
            self.save_settings();
        }
        if view_save {
            self.save_settings();
        }
        // Чип снимает ровно свой фильтр; сетка пересчитается следующим кадром.
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
        if let Some(id) = cell_open {
            self.open_viewer(id);
        } else if let Some(id) = cell_click {
            self.selected = Some(id);
            self.prefetch_photo(id);
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
            self.status = format!("возвращено из корзины: {n}");
        }
        if let Some(id) = restore_click {
            match db::untrash(&self.db_path, id) {
                Ok(()) => {
                    self.reload();
                    self.status = "файл возвращён из корзины".into();
                }
                Err(e) => self.status = format!("файл не вернуть: {e}"),
            }
        }
        if trash_clear {
            self.trash_confirm = true;
        }
        if let Some(keep_id) = keep_click {
            self.selected = Some(keep_id);
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
                    .unwrap_or("файл")
                    .to_string();
                match db::mark_keep(&self.db_path, keep_id, &drops) {
                    Ok(()) => {
                        self.status = format!("оставлен {name}, помечено лишними {}", drops.len())
                    }
                    Err(e) => self.status = format!("решение не сохранено: {e}"),
                }
            }
        }
        if let Some(label) = face_save_label
            && let Some(id) = face_selected
        {
            match faces::label(&self.db_path, id, &label) {
                Ok(()) => {
                    self.status = if label.is_empty() {
                        "подпись группы убрана".into()
                    } else {
                        format!("группа подписана: «{label}»")
                    };
                    self.reload();
                }
                Err(e) => self.status = format!("подпись не сохранена: {e:#}"),
            }
        }
        if let Some(face_id) = face_split {
            match faces::split(&self.db_path, face_id) {
                Ok(cid) => {
                    self.face_selected_cluster = Some(cid);
                    self.reload();
                    self.status = "лицо отделено в новую группу; дайте ей имя".into();
                }
                Err(e) => self.status = format!("лицо не отделено: {e:#}"),
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
                match faces::group_picked(&self.db_path, &ids, &label) {
                    Ok(cid) => {
                        self.face_selected_cluster = Some(cid);
                        self.face_picked.clear();
                        self.reload();
                        self.status = if label.trim().is_empty() {
                            format!("{} лиц собраны в новую группу; дайте ей имя", ids.len())
                        } else {
                            format!("{} лиц назначены группе «{}»", ids.len(), label.trim())
                        };
                    }
                    Err(e) => self.status = format!("лица не назначены: {e:#}"),
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
        App, CalScale, Viewer, days_in_month, db, first_weekday_monday, fit_rect, monday_of,
        month_groups, nearest_file_month, parse_date, score_percentile, shift_month,
        shift_to_file_month,
    };
    use crate::embed;
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

    /// Один кадр полноэкранного просмотра без окна: возвращает весь нарисованный текст.
    /// Единственный способ проверить «листание показывает следующий файл» без GUI.
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
        // Модальное окно (anchored area) на первом кадре ещё не знает своего размера
        // и рисует только фон. Поэтому кадр прогоняется дважды: событие — в первый,
        // готовые фигуры — из второго.
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

    /// Один кадр карточки файла без окна: проверяет текст «не уверен» и топ-3.
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

    /// Один кадр всего окна без окна: проверяет группы по месяцам и их порядок.
    /// `eframe::Frame::_new_kittest` — штатный тестовый кадр eframe, окно не открывается.
    fn window_frame(app: &mut App, ctx: &egui::Context) -> Vec<String> {
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
        let mut texts = Vec::new();
        for c in &out.shapes {
            collect_text(&c.shape, &mut texts);
        }
        out.drop_without_applying_deltas();
        texts
    }

    fn viewer_with(ids: Vec<i64>, idx: usize) -> Viewer {
        Viewer {
            ids,
            idx,
            zoom: 1.0,
            show_text: false,
            slideshow: false,
            interval_s: 3,
            last_advance: std::time::Instant::now(),
        }
    }

    /// Листание в полном экране обязано менять файл в ту же секунду, а видео —
    /// останавливаться. Иначе кадр старого видео рисуется поверх нового файла.
    /// Владелец: «сверху документ, ниже скриншот, а мебель потом». При близких весах
    /// карточка обязана сказать «не уверен», показать топ-3 и ничего не подсвечивать.
    #[test]
    fn карточка_пишет_не_уверен_при_близких_тегах() {
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
        let texts = card_frame(&mut app, &egui::Context::default());
        assert!(
            texts.iter().any(|t| t.contains("тег: не уверен")),
            "нет честного «не уверен»: {texts:?}"
        );
        assert!(texts.iter().any(|t| t == "документ"));
        assert!(texts.iter().any(|t| t == "мебель"));
        assert!(
            !texts.iter().any(|t| t == "дети"),
            "четвёртый тег спрятан под «все теги»: {texts:?}"
        );
    }

    #[test]
    fn карточка_подсвечивает_уверенный_тег() {
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
        let texts = card_frame(&mut app, &egui::Context::default());
        assert!(texts.iter().any(|t| t == "тег: мебель"), "{texts:?}");
        assert!(!texts.iter().any(|t| t.contains("не уверен")), "{texts:?}");
    }

    #[test]
    fn листание_идёт_по_кругу_и_сбрасывает_таймер_слайдшоу() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        let mut v = viewer_with(vec![7, 8, 9], 0);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 1);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 2);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 0, "последний файл уступает первому, как в слайд-шоу");
        app.viewer_step(&mut v, -1);
        assert_eq!(v.idx, 2);
        assert!(v.last_advance.elapsed() < std::time::Duration::from_millis(100));
    }

    /// Смена файла останавливает плеер: без этого `video_id` остаётся от старого
    /// кадра, и `poll_video` продолжает рисовать его поверх нового снимка.
    #[test]
    fn смена_файла_останавливает_видео() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.video_id = Some(7);
        let mut v = viewer_with(vec![7, 8], 0);
        app.viewer_step(&mut v, 1);
        assert_eq!(app.video_id, None);
        assert_eq!(v.idx, 1);
    }

    /// Владелец: «листание вперёд-назад ничего не показывает». Видео должно
    /// показываться кадром-обложкой с кнопкой ▶, а стрелка — открывать следующий файл.
    #[test]
    fn просмотр_видео_рисует_кнопку_и_листается_стрелкой() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![
            file_row(1, "image", "a.jpg"),
            file_row(2, "video", "b.mp4"),
            file_row(3, "image", "c.jpg"),
        ];
        app.open_viewer(2);
        let ctx = egui::Context::default();
        let texts = viewer_frame(&mut app, &ctx, None);
        assert!(
            texts.iter().any(|t| t.contains("нажмите ▶")),
            "на кадре видео должна быть большая кнопка ▶: {texts:?}"
        );
        let texts = viewer_frame(&mut app, &ctx, Some(egui::Key::ArrowRight));
        assert_eq!(app.viewer.as_ref().expect("просмотр открыт").idx, 2);
        assert!(
            texts.iter().any(|t| t == "c.jpg"),
            "в заголовке должен быть следующий файл: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("нажмите ▶")),
            "на снимке нет кнопки ▶: {texts:?}"
        );
    }

    /// Прошлый снимок держится на экране, пока грузится следующий кадр. У видео
    /// «следующего кадра» нет — иначе после листания на видео оставался старый снимок,
    /// а владелец видел «ничего не показывает».
    #[test]
    fn переход_на_видео_не_оставляет_прошлый_снимок() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        app.items = vec![file_row(1, "image", "a.jpg"), file_row(2, "video", "b.mp4")];
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
            "должен открыться следующий файл: {texts:?}"
        );
        assert!(
            !texts.iter().any(|t| t.contains("загрузка следующего кадра")),
            "кадр прошлого снимка не должен оставаться на видео: {texts:?}"
        );
        assert!(
            texts.iter().any(|t| t.contains("нажмите ▶")),
            "на видео должна быть кнопка ▶: {texts:?}"
        );
    }

    #[test]
    fn листание_одного_файла_ничего_не_делает() {
        let mut app = App::new(std::path::PathBuf::from("/tmp/photocat-test.db"));
        let mut v = viewer_with(vec![7], 0);
        app.viewer_step(&mut v, 1);
        assert_eq!(v.idx, 0);
    }

    fn bounds() -> egui::Rect {
        egui::Rect::from_min_size(egui::pos2(10.0, 20.0), egui::vec2(150.0, 150.0))
    }

    #[test]
    fn портрет_вписан_по_высоте_и_по_центру() {
        let r = fit_rect(bounds(), egui::vec2(320.0, 640.0));
        assert!((r.height() - 150.0).abs() < 0.01);
        assert!((r.width() - 75.0).abs() < 0.01);
        assert!((r.center().x - bounds().center().x).abs() < 0.01);
        assert!((r.center().y - bounds().center().y).abs() < 0.01);
    }

    #[test]
    fn панорама_вписана_по_ширине() {
        let r = fit_rect(bounds(), egui::vec2(1200.0, 400.0));
        assert!((r.width() - 150.0).abs() < 0.01);
        assert!((r.height() - 50.0).abs() < 0.01);
    }

    #[test]
    fn квадрат_занимает_всю_плитку() {
        let r = fit_rect(bounds(), egui::vec2(320.0, 320.0));
        assert!((r.width() - 150.0).abs() < 0.01);
        assert!((r.height() - 150.0).abs() < 0.01);
    }

    #[test]
    fn нулевой_размер_не_ломает_раскладку() {
        let r = fit_rect(bounds(), egui::vec2(0.0, 0.0));
        assert_eq!(r, bounds());
    }

    #[test]
    fn месяц_листается_через_границы_года() {
        assert_eq!(shift_month(2024, 12, 1), (2025, 1));
        assert_eq!(shift_month(2024, 1, -1), (2023, 12));
        assert_eq!(shift_month(2024, 5, 0), (2024, 5));
    }

    #[test]
    fn длина_месяца_учитывает_високосный_год() {
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(2024, 5), 31);
    }

    fn months(paths: &[&str]) -> BTreeSet<String> {
        paths.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn пустые_месяцы_пропускаются_при_листании() {
        let m = months(&["2024-01", "2024-05", "2024-09"]);
        assert_eq!(shift_to_file_month(&m, 2024, 1, 1), Some((2024, 5)));
        assert_eq!(shift_to_file_month(&m, 2024, 5, -1), Some((2024, 1)));
        assert_eq!(shift_to_file_month(&m, 2024, 5, 1), Some((2024, 9)));
        assert_eq!(shift_to_file_month(&m, 2024, 9, 1), None);
        assert_eq!(shift_to_file_month(&m, 2024, 1, -1), None);
        assert_eq!(shift_to_file_month(&months(&[]), 2024, 5, 1), None);
    }

    #[test]
    fn сегодня_уходит_в_ближайший_месяц_с_файлами() {
        let m = months(&["2024-01", "2024-07"]);
        assert_eq!(nearest_file_month(&m, 2024, 3), Some((2024, 1)));
        assert_eq!(nearest_file_month(&m, 2024, 6), Some((2024, 7)));
        assert_eq!(nearest_file_month(&months(&[]), 2024, 3), None);
    }

    #[test]
    fn дата_из_строки_и_понедельник_недели() {
        let d = parse_date("2024-05-15T12:30:00").unwrap();
        assert_eq!(d.to_string(), "2024-05-15");
        assert_eq!(monday_of(d).to_string(), "2024-05-13");
        assert!(parse_date("без даты").is_none());
        assert!(parse_date("2024-13-40").is_none());
    }

    #[test]
    fn неделя_календаря_начинается_с_понедельника() {
        assert_eq!(first_weekday_monday(2024, 1), 0); // 1 января 2024 — понедельник
        assert_eq!(first_weekday_monday(2024, 5), 2); // 1 мая 2024 — среда
    }

    /// Порог фильтра — процентиль своего тега, а не глобальный максимум:
    /// на 100% у «мебели» остаётся её самый сильный снимок, на 0% — все.
    #[test]
    fn фильтр_тега_идёт_по_процентилю_своего_тега() {
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
        assert_eq!(app.tag_counts()[7], 120, "«мебель» — седьмая в TAG_LABELS");
        // Маленький каталог: процентиль шумный, действует сырой порог.
        app.tags.retain(|id, _| *id < 50);
        app.settings.tag_show = 0.22;
        assert!((app.tag_threshold("мебель") - 0.22).abs() < 1e-6);
    }

    #[test]
    fn процентиль_веса_считает_долю_слабейших() {
        let d = [0.30, 0.20, 0.10];
        assert_eq!(score_percentile(&d, 0.30), Some(67));
        assert_eq!(score_percentile(&d, 0.20), Some(33));
        assert_eq!(score_percentile(&d, 0.10), Some(0));
        assert_eq!(score_percentile(&d, 0.40), Some(100));
        assert_eq!(score_percentile(&[], 0.2), None);
    }

    #[test]
    fn календарь_проваливается_до_дня() {
        assert_eq!(CalScale::Year.deeper(), Some(CalScale::Month));
        assert_eq!(CalScale::Month.deeper(), Some(CalScale::Week));
        assert_eq!(CalScale::Week.deeper(), Some(CalScale::Day));
        assert_eq!(CalScale::Day.deeper(), None);
    }

    /// Сетка — «моменты»: свежий месяц первым, внутри месяца свежие сверху,
    /// файлы без даты — последней группой (M16, ADR 0033).
    #[test]
    fn сетка_группирует_по_месяцам_от_свежих() {
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
                .unwrap_or_else(|| panic!("нет заголовка «{needle}»: {texts:?}"))
        };
        let (may, april_pos, no_date) = (pos("май 2024"), pos("апрель 2024"), pos("Без даты"));
        assert!(may < april_pos, "май свежее апреля: {texts:?}");
        assert!(april_pos < no_date, "«Без даты» — последняя группа: {texts:?}");
        // Внутри месяца подписи плиток идут от свежих к старым.
        let may_new = texts.iter().position(|t| t == "2024-05-09 10:00").unwrap();
        let may_old = texts.iter().position(|t| t == "2024-05-01 08:00").unwrap();
        assert!(may_new < may_old, "внутри месяца свежие сверху: {texts:?}");
    }

    /// Файлы для M17: два майских, апрельский и снимок без даты.
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
    fn порядок_по_дате_и_без_даты_в_конце_в_обе_стороны() {
        let mut app = m17_app();
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.settings.grid_sort = db::GridSort::DateDesc;
        assert_eq!(app.ordered(&visible), vec![1, 0, 2, 3], "новые сверху, без даты в конце");
        app.settings.grid_sort = db::GridSort::DateAsc;
        assert_eq!(app.ordered(&visible), vec![2, 0, 1, 3], "старые сверху, без даты всё равно в конце");
    }

    #[test]
    fn месяцы_переворачиваются_а_без_даты_остаётся_последней() {
        let mut app = m17_app();
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.settings.grid_sort = db::GridSort::DateDesc;
        let desc = app.ordered(&visible);
        let keys: Vec<String> = month_groups(&desc, &app.items, false)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["2024-05", "2024-04", "без даты"]);
        app.settings.grid_sort = db::GridSort::DateAsc;
        let asc = app.ordered(&visible);
        let keys: Vec<String> = month_groups(&asc, &app.items, true)
            .into_iter()
            .map(|(k, _)| k)
            .collect();
        assert_eq!(keys, ["2024-04", "2024-05", "без даты"]);
    }

    #[test]
    fn имя_и_размер_сортируются() {
        let mut app = m17_app();
        app.items[0].path = "/tmp/b.jpg".into();
        app.items[1].path = "/tmp/A.jpg".into();
        app.items[2].path = "/tmp/c.jpg".into();
        let visible: Vec<usize> = (0..app.items.len()).collect();
        app.settings.grid_sort = db::GridSort::Name;
        // Имя без учёта регистра: A, b, c, undated.
        assert_eq!(app.ordered(&visible), vec![1, 0, 2, 3]);
        app.settings.grid_sort = db::GridSort::SizeDesc;
        assert_eq!(
            app.ordered(&visible),
            vec![3, 1, 2, 0],
            "крупные сверху: 40, 30, 20, 10"
        );
    }

    #[test]
    fn при_смысловом_поиске_порядок_задаёт_поиск() {
        let mut app = m17_app();
        app.settings.grid_sort = db::GridSort::SizeDesc;
        app.semantic_hits = Some(vec![4, 1, 3]);
        assert_eq!(app.ordered(&app.filtered()), vec![3, 0, 2]);
        // Результаты поиска — плоский список по релевантности, без групп месяцев.
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            !texts.iter().any(|t| t == "май 2024"),
            "при поиске месяцы не группируются: {texts:?}"
        );
    }

    #[test]
    fn подряд_рисует_сетку_без_заголовков_месяцев() {
        let mut app = m17_app();
        app.settings.grid_layout = db::GridLayout::Flat;
        let texts = window_frame(&mut app, &egui::Context::default());
        assert!(
            !texts.iter().any(|t| t == "май 2024"),
            "в «Подряд» заголовков месяцев быть не должно: {texts:?}"
        );
        assert!(texts.iter().any(|t| t.contains("2024-05-09")), "{texts:?}");
    }
}
