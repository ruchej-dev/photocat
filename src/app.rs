//! GUI на egui: сетка превью, группы дублей, таймлайн, запуск индексации.

use crate::db::{self, FileRow, Progress};
use crate::dupes::{self, Group};
use crate::index;
use anyhow::Result;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc;

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
            status: String::new(),
        };
        me.reload();
        me
    }

    fn reload(&mut self) {
        self.textures.clear();
        self.missing.clear();
        match db::open(&self.db_path).and_then(|c| db::load_all(&c)) {
            Ok(rows) => {
                self.status = format!(
                    "в индексе {} файлов, {}",
                    rows.len(),
                    db::human(rows.iter().map(|r| r.size).sum())
                );
                self.items = rows;
                self.groups.clear();
            }
            Err(e) => self.status = format!("ошибка чтения индекса: {e}"),
        }
    }

    fn filtered(&self) -> Vec<usize> {
        let q = self.query.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, r)| !self.only_images || r.kind == "image")
            .filter(|(_, r)| {
                q.is_empty()
                    || r.path.to_lowercase().contains(&q)
                    || r.taken_at.clone().unwrap_or_default().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
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
    if resp.hovered() {
        if let Some(t) = cs.textures.get(&f.id).cloned() {
            resp.on_hover_ui_at_pointer(|ui| {
                let big = egui::load::SizedTexture::new(t.id(), egui::vec2(400.0, 400.0));
                ui.add(egui::Image::new(big).maintain_aspect_ratio(true));
                ui.label(&f.path);
            });
        }
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
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(220.0)
                            .text(format!("{d}/{t}")),
                    );
                }
            });
            ui.add_space(4.0);
        });

        egui::Panel::bottom("bottom").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(if self.status.is_empty() { "—" } else { &self.status });
                if let Some(id) = self.selected {
                    if let Some(f) = self.items.iter().find(|r| r.id == id) {
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
                ui.label(format!("найдено групп: {}", self.groups.len()));
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

            if let Some(id) = self.selected {
                if let Some(f) = self.items.iter().find(|r| r.id == id) {
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
    }
}
