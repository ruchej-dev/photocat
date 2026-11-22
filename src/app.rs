//! GUI на egui: сетка превью, группы дублей, таймлайн, запуск индексации.

use crate::db::{self, FileRow, Progress};
use crate::dupes::{self, Group};
use crate::embed;
use crate::index;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

type SearchReply = Result<(embed::Model, Vec<(i64, f32)>, f32)>;
type EmbedReply = Result<embed::EmbedStats>;

#[derive(Clone, Copy, PartialEq)]
enum View {
    Grid,
    Dupes,
    Timeline,
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
        Box::new(move |_cc| Ok(Box::new(App::new(db_path.clone())))),
    )
    .map_err(|e| anyhow::anyhow!("{e}"))
}

struct App {
    db_path: PathBuf,
    thumbs_dir: PathBuf,
    items: Vec<FileRow>,
    textures: HashMap<i64, egui::TextureHandle>,
    missing: HashSet<i64>,
    selected: Option<i64>,
    view: View,
    groups: Vec<Group>,
    max_dist: u32,
    path_input: String,
    query: String,
    only_images: bool,
    progress: Option<Arc<Progress>>,
    rx: Option<mpsc::Receiver<Result<index::Stats>>>,
    /// Теги CLIP: file_id → [(тег, вес)] по убыванию.
    tags: HashMap<i64, Vec<(String, f32)>>,
    tag_filter: Option<String>,
    semantic_query: String,
    /// Результаты смыслового поиска в порядке убывания похожести.
    semantic_hits: Option<Vec<i64>>,
    /// Модель загружена после первого поиска; дальше запросы идут без паузы.
    ml_model: Option<embed::Model>,
    search_rx: Option<mpsc::Receiver<SearchReply>>,
    embed_progress: Option<Arc<Progress>>,
    embed_rx: Option<mpsc::Receiver<EmbedReply>>,
    status: String,
}

impl App {
    fn new(db_path: PathBuf) -> Self {
        let thumb_dir = db::cache_dir().join("thumbs");
        let mut me = Self {
            db_path,
            thumbs_dir: thumb_dir,
            items: Vec::new(),
            textures: HashMap::new(),
            missing: HashSet::new(),
            selected: None,
            view: View::Grid,
            groups: Vec::new(),
            max_dist: 6,
            path_input: String::new(),
            query: String::new(),
            only_images: true,
            progress: None,
            rx: None,
            tags: HashMap::new(),
            tag_filter: None,
            semantic_query: String::new(),
            semantic_hits: None,
            ml_model: None,
            search_rx: None,
            embed_progress: None,
            embed_rx: None,
            status: String::new(),
        };
        me.reload();
        me
    }

    fn reload(&mut self) {
        self.textures.clear();
        self.missing.clear();
        self.semantic_hits = None;
        match db::open(&self.db_path).and_then(|c| {
            let rows = db::load_all(&c)?;
            let tags = db::load_tags(&c)?;
            Ok((rows, tags))
        }) {
            Ok((rows, tags)) => {
                self.status = format!(
                    "в индексе {} файлов, {}",
                    rows.len(),
                    db::human(rows.iter().map(|r| r.size).sum())
                );
                self.items = rows;
                self.tags = tags;
                self.groups.clear();
            }
            Err(e) => self.status = format!("ошибка чтения индекса: {e}"),
        }
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        let mut out: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, r)| !self.only_images || r.kind == "image")
            .filter(|(_, r)| {
                q.is_empty()
                    || r.path.to_lowercase().contains(&q)
                    || r.taken_at.clone().unwrap_or_default().contains(&q)
            })
            .filter(|(_, r)| match &self.tag_filter {
                None => true,
                Some(want) => self
                    .tags
                    .get(&r.id)
                    .and_then(|t| embed::confident_tag(t))
                    .map(|(tag, _)| tag == want)
                    .unwrap_or(false),
            })
            .map(|(i, _)| i)
            .collect();
        if let Some(hits) = &self.semantic_hits {
            let rank: HashMap<i64, usize> = hits
                .iter()
                .enumerate()
                .map(|(n, id)| (*id, n))
                .collect();
            out.retain(|&i| rank.contains_key(&self.items[i].id));
            out.sort_by_key(|&i| rank.get(&self.items[i].id).copied().unwrap_or(usize::MAX));
        }
        out
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
            let r = embed::embed_missing(&db_path, 8, 8, Some(progress));
            let _ = tx.send(r);
        });
    }

    fn start_search(&mut self) {
        let q = self.semantic_query.trim().to_string();
        if q.is_empty() {
            self.semantic_hits = None;
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
        let (tx, rx) = mpsc::channel();
        self.search_rx = Some(rx);
        self.status = format!("CLIP: загружаю модель для запроса «{q}»…");
        std::thread::spawn(move || {
            let t = std::time::Instant::now();
            let res = embed::Model::load(8).and_then(|mut model| {
                let hits = embed::search(&db_path, &mut model, &q, 200)?;
                let ms = t.elapsed().as_secs_f32() * 1000.0;
                Ok((model, hits, ms))
            });
            let _ = tx.send(res);
        });
    }

    fn start_index(&mut self) {
        let root = PathBuf::from(self.path_input.trim());
        if !root.is_dir() {
            self.status = format!("не каталог: {}", root.display());
            return;
        }
        let db_path = self.db_path.clone();
        let progress = Arc::new(Progress::default());
        self.progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.status = format!("индексация {}…", root.display());
        std::thread::spawn(move || {
            let r = index::run(&root, 8, &db_path, Some(progress));
            let _ = tx.send(r);
        });
    }
}

struct Cells<'a> {
    textures: &'a mut HashMap<i64, egui::TextureHandle>,
    missing: &'a mut HashSet<i64>,
    thumbs_dir: &'a Path,
    selected: Option<i64>,
    clicked: Option<i64>,
    /// Показать на плитке кнопку «оставить» (вид «Дубли»).
    show_keep: bool,
    /// Нажата «оставить» у файла — применить решение к его группе.
    keep_clicked: Option<i64>,
}

fn load_texture(ctx: &egui::Context, path: &Path, name: &str) -> Option<egui::TextureHandle> {
    let img = image::open(path).ok()?.to_rgba8();
    let size = [img.width() as usize, img.height() as usize];
    let color = egui::ColorImage::from_rgba_unmultiplied(size, &img.into_raw());
    Some(ctx.load_texture(name, color, egui::TextureOptions::LINEAR))
}

fn draw_cell(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, f: &FileRow) {
    const W: f32 = 150.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, W + 26.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(W, W));
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        let mut drawn = false;
        if let Some(t) = cs.textures.get(&f.id) {
            ui.painter().image(
                t.id(),
                img_rect,
                egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                egui::Color32::WHITE,
            );
            drawn = true;
        } else if !cs.missing.contains(&f.id) {
            let loaded = f.thumb.as_ref().and_then(|rel| {
                load_texture(ctx, &cs.thumbs_dir.join(rel), &format!("t{}", f.id))
            });
            match loaded {
                Some(t) => {
                    cs.textures.insert(f.id, t);
                }
                None => {
                    cs.missing.insert(f.id);
                }
            }
        }
        if !drawn {
            let txt = match f.kind.as_str() {
                "video" => "▶ видео",
                _ => "нет превью",
            };
            ui.painter().text(
                img_rect.center(),
                egui::Align2::CENTER_CENTER,
                txt,
                egui::FontId::proportional(12.0),
                egui::Color32::GRAY,
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
                ui.painter().rect_filled(
                    img_rect,
                    3.0,
                    egui::Color32::from_black_alpha(150),
                );
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
                    3.0,
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
    }
    if cs.selected == Some(f.id) {
        ui.painter().rect_stroke(
            rect,
            3.0,
            egui::Stroke::new(2.0, egui::Color32::from_rgb(90, 170, 255)),
            egui::StrokeKind::Inside,
        );
    }
    if resp.clicked() {
        cs.clicked = Some(f.id);
    }
    if resp.hovered()
        && let Some(t) = cs.textures.get(&f.id).cloned()
    {
        resp.on_hover_ui_at_pointer(|ui| {
            let big = egui::load::SizedTexture::new(t.id(), egui::vec2(400.0, 400.0));
            ui.add(egui::Image::new(big).maintain_aspect_ratio(true));
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
                        Ok(stats) => {
                            self.reload();
                            self.status = stats.to_string();
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
                            self.status = format!(
                                "смысловой поиск: {} файлов за {ms:.0} мс",
                                hits.len()
                            );
                            self.semantic_hits =
                                Some(hits.into_iter().map(|(id, _)| id).collect());
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

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Каталог:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("/home/alx/Documents/nasty/mail_media")
                        .desired_width(520.0),
                );
                if ui.button("Индексировать").clicked() {
                    self.start_index();
                }
                if ui.button("Обновить").clicked() {
                    self.reload();
                }
                if let Some(p) = &self.progress {
                    let (d, t) = (p.done(), p.total());
                    let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                    let text = if t > 0 {
                        format!("{d}/{t}")
                    } else {
                        p.msg()
                    };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(220.0)
                            .text(text),
                    );
                }
            });
            if embed::model_present() {
                ui.horizontal(|ui| {
                    ui.label("Смысловой поиск:");
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.semantic_query)
                            .hint_text("торт, море, документ")
                            .desired_width(300.0),
                    );
                    let enter =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    if ui.button("Найти").clicked() || enter {
                        self.start_search();
                    }
                    if self.semantic_hits.is_some() && ui.button("Сбросить").clicked() {
                        self.semantic_hits = None;
                        self.status = "смысловой поиск сброшен".into();
                    }
                    if self.search_rx.is_some() {
                        ui.label("загрузка модели CLIP…");
                    }
                });
            }
            if let Some(p) = &self.embed_progress {
                let (d, t) = (p.done(), p.total());
                let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                ui.add(
                    egui::ProgressBar::new(frac)
                        .desired_width(320.0)
                        .text(if t > 0 {
                            format!("CLIP: {d}/{t}")
                        } else {
                            p.msg()
                        }),
                );
            }
            ui.add_space(4.0);
        });

        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "—" } else { &self.status });
                if let Some(id) = self.selected
                    && let Some(f) = self.items.iter().find(|r| r.id == id)
                {
                    {
                        ui.separator();
                        ui.label(format!(
                            "{} · {} · {}×{} · {}",
                            f.kind,
                            db::human(f.size),
                            f.width,
                            f.height,
                            f.taken_src
                        ));
                    }
                }
            });
        });

        egui::Panel::left("left").default_size(260.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("Вид");
            ui.selectable_value(&mut self.view, View::Grid, "Все файлы");
            ui.selectable_value(&mut self.view, View::Dupes, "Дубли и серии");
            ui.selectable_value(&mut self.view, View::Timeline, "По датам");
            ui.separator();
            ui.label("Поиск по пути/дате:");
            ui.add(egui::TextEdit::singleline(&mut self.query).desired_width(f32::INFINITY));
            ui.checkbox(&mut self.only_images, "только изображения");
            ui.separator();

            if self.view == View::Dupes {
                ui.label("Порог похожести (бит из 64):");
                ui.add(egui::Slider::new(&mut self.max_dist, 0..=16));
                if ui.button("Найти группы").clicked() {
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
                ui.label(format!(
                    "в группах {}, помечено лишними {}, освободится {}",
                    sum.groups,
                    sum.marked_drop,
                    db::human(sum.freed)
                ));
                let has_groups = !self.groups.is_empty();
                if ui
                    .add_enabled(has_groups, egui::Button::new("Экспорт плана"))
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
                    .add_enabled(has_groups, egui::Button::new("Скопировать список в буфер"))
                    .clicked()
                {
                    ctx.copy_text(dupes::plan_csv(&self.items, &self.groups));
                    self.status = "план скопирован в буфер обмена".into();
                }
                ui.label(format!("найдено групп: {}", self.groups.len()));
                let no_sharp = self
                    .items
                    .iter()
                    .filter(|r| r.kind == "image" && r.sharpness.is_none())
                    .count();
                if no_sharp > 0 {
                    ui.label(format!(
                        "резкость не посчитана у {no_sharp} снимков — проиндексируйте каталог"
                    ));
                }
            }

            ui.separator();
            ui.heading("Смысловые теги");
            if embed::model_present() {
                ui.label(format!(
                    "CLIP: {} · {} признаков",
                    db::human(embed::model_bytes()),
                    embed::DIM
                ));
                if ui
                    .add_enabled(
                        self.embed_rx.is_none(),
                        egui::Button::new("Посчитать теги"),
                    )
                    .clicked()
                {
                    self.start_embed();
                }
                egui::ComboBox::from_label("Тег")
                    .selected_text(self.tag_filter.clone().unwrap_or_else(|| "все".into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.tag_filter, None, "все");
                        for (label, _) in embed::TAGS {
                            ui.selectable_value(
                                &mut self.tag_filter,
                                Some(label.to_string()),
                                label,
                            );
                        }
                    });
                let tagged = self
                    .items
                    .iter()
                    .filter(|r| {
                        self.tags
                            .get(&r.id)
                            .and_then(|t| embed::confident_tag(t))
                            .is_some()
                    })
                    .count();
                ui.label(format!("с уверенным тегом: {tagged}"));
                if let Some(hits) = &self.semantic_hits {
                    ui.label(format!("смысловой поиск: {} файлов", hits.len()));
                }
            } else {
                ui.label("CLIP: модель не найдена");
                ui.label("bash scripts/fetch-model.sh");
            }

            ui.separator();
            let n = self.items.len();
            let bytes: i64 = self.items.iter().map(|r| r.size).sum();
            let no_date = self.items.iter().filter(|r| r.taken_at.is_none()).count();
            let no_thumb = self.items.iter().filter(|r| r.thumb.is_none()).count();
            ui.label(format!("файлов: {n}"));
            ui.label(format!("объём: {}", db::human(bytes)));
            ui.label(format!("без даты: {no_date}"));
            ui.label(format!("без превью (видео/битые): {no_thumb}"));
            ui.label(format!("БД: {}", self.db_path.display()));

            if let Some(id) = self.selected
                && let Some(f) = self.items.iter().find(|r| r.id == id)
            {
                {
                    ui.separator();
                    ui.heading("Файл");
                    ui.label(format!("путь: {}", f.path));
                    ui.label(format!("дата: {}", f.taken_at.clone().unwrap_or_else(|| "—".into())));
                    if let Some(c) = &f.camera {
                        ui.label(format!("камера: {c}"));
                    }
                    if let (Some(a), Some(b)) = (f.lat, f.lon) {
                        ui.label(format!("GPS: {a:.5}, {b:.5}"));
                    }
                    match self.tags.get(&id) {
                        Some(tags) => match embed::confident_tag(tags) {
                            Some((tag, score)) => {
                                ui.label(format!("тег: {tag} ({score:.2})"));
                            }
                            None => {
                                ui.label("тег: не уверен");
                            }
                        },
                        None => {
                            ui.label("тег: не посчитан");
                        }
                    }
                    ui.label(format!("sha256: {}…", &f.sha256[..16.min(f.sha256.len())]));
                }
            }
        });

        let visible = self.filtered();
        let items = std::mem::take(&mut self.items);
        let mut cells = Cells {
            textures: &mut self.textures,
            missing: &mut self.missing,
            thumbs_dir: &self.thumbs_dir,
            selected: self.selected,
            clicked: None,
            show_keep: self.view == View::Dupes,
            keep_clicked: None,
        };
        let groups = std::mem::take(&mut self.groups);
        let view = self.view;

        egui::CentralPanel::default().show(ui, |ui| match view {
            View::Grid => {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            for &i in &visible {
                                draw_cell(ui, ctx, &mut cells, &items[i]);
                            }
                        });
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
            View::Timeline => {
                let mut by_day: std::collections::BTreeMap<String, Vec<usize>> =
                    std::collections::BTreeMap::new();
                for &i in &visible {
                    let key = items[i]
                        .taken_at
                        .as_deref()
                        .map(|d| d.chars().take(10).collect::<String>())
                        .unwrap_or_else(|| "без даты".into());
                    by_day.entry(key).or_default().push(i);
                }
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for (day, idxs) in by_day.iter().rev() {
                            let bytes: i64 = idxs.iter().map(|&i| items[i].size).sum();
                            egui::CollapsingHeader::new(format!(
                                "{day} — {} файлов, {}",
                                idxs.len(),
                                db::human(bytes)
                            ))
                            .id_salt(day)
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
        });

        self.items = items;
        self.groups = groups;
        if let Some(id) = cells.clicked {
            self.selected = Some(id);
        }
        if let Some(keep_id) = cells.keep_clicked {
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
                        self.status = format!(
                            "оставлен {name}, помечено лишними {}",
                            drops.len()
                        )
                    }
                    Err(e) => self.status = format!("решение не сохранено: {e}"),
                }
            }
        }
    }
}
