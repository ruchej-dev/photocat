//! GUI на egui: сетка превью, группы дублей, таймлайн, запуск индексации.

use crate::db::{self, FileRow, Progress};
use crate::dupes::{self, Group};
use crate::embed;
use crate::faces;
use crate::index;
use crate::ocr;
use crate::tools::{self, Tool};
use anyhow::Result;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use chrono::Datelike;
use std::sync::Arc;
use std::sync::mpsc;

type SearchReply = Result<(embed::Model, Vec<(i64, f32)>, f32)>;
type EmbedReply = Result<embed::EmbedStats>;
type OcrReply = Result<ocr::Stats>;

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
    Settings,
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
            status: String::new(),
        };
        me.reload();
        me
    }

    fn reload(&mut self) {
        self.textures.clear();
        self.missing.clear();
        self.face_textures.clear();
        self.face_missing.clear();
        self.semantic_hits = None;
        self.ocr_text.clear();
        self.ocr_snippets.clear();
        match db::open(&self.db_path).and_then(|c| {
            let settings = db::load_settings(&c);
            let source = embed::tag_source(embed::active_entry(&settings));
            let rows = db::load_all(&c)?;
            let tags = db::load_tags(&c, &source)?;
            let hidden = db::load_hidden_counts(&c)?;
            let (ocr_files, ocr_chars) = db::ocr_summary(&c)?;
            let face_clusters = db::load_face_clusters(&c)?;
            let face_views = db::load_face_views(&c, None)?;
            let (face_faces, face_photos, _) = db::face_summary(&c)?;
            let (face_images, face_todo) = db::face_scan_plan(&c)?;
            Ok((
                settings, rows, tags, hidden, ocr_files, ocr_chars, face_clusters, face_views,
                face_faces, face_photos, face_images, face_todo,
            ))
        }) {
            Ok((
                settings, rows, tags, hidden, ocr_files, ocr_chars, face_clusters, face_views,
                face_faces, face_photos, face_images, face_todo,
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
                self.items = rows;
                self.tags = tags;
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
        if let Some(date) = self
            .items
            .iter()
            .filter_map(|r| r.taken_at.as_deref())
            .max()
        {
            let y = date.get(0..4).and_then(|v| v.parse().ok()).unwrap_or(2024);
            let m = date.get(5..7).and_then(|v| v.parse().ok()).unwrap_or(1);
            self.cal_year = y;
            self.cal_month = m;
        } else {
            self.cal_year = 2024;
            self.cal_month = 1;
        }
    }

    fn save_settings(&mut self) {
        let res = db::open(&self.db_path)
            .and_then(|c| db::save_settings(&c, &self.settings));
        match res {
            Ok(()) => self.status = "настройки сохранены".into(),
            Err(e) => self.status = format!("настройки не сохранены: {e}"),
        }
    }

    /// Экран настроек: модель, путь, лимит, пороги, индексация.
    fn settings_view(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.heading("Настройки");
                ui.label(
                    "Всё хранится в БД рядом с индексом; оригиналы фото и видео не меняются.",
                );
                ui.add_space(8.0);

                ui.heading("Модель ИИ");
                let current = embed::active_entry(&self.settings);
                let current_id = current.id;
                let mut chosen = current_id;
                egui::ComboBox::from_label("Модель")
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
                                format!(
                                    "{} — {} · {}",
                                    e.title,
                                    db::human(e.bytes() as i64),
                                    e.note
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

                ui.horizontal(|ui| {
                    ui.label("Папка моделей:");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.models_dir_input)
                            .hint_text("путь, куда скачивать веса")
                            .desired_width(420.0),
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
                ui.label(format!(
                    "сейчас: {}",
                    self.settings.models_dir.display()
                ));

                let r = ui.add(
                    egui::DragValue::new(&mut self.settings.max_model_mb)
                        .range(50..=2000)
                        .suffix(" МиБ")
                        .speed(10),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.save_settings();
                }
                ui.label(
                    "Лимит на скачивание: модель сверх него не скачивается. Если выбранная                      недоступна, загрузчик ищет совместимую в пределах лимита.",
                );

                ui.add_space(4.0);
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
                    ui.label(format!(
                        "{} — {} · {}",
                        e.title,
                        db::human(e.bytes() as i64),
                        state
                    ));
                }

                ui.checkbox(
                    &mut self.download_consent,
                    "разрешаю скачать веса модели (фото, пути и запросы не отправляются)",
                );
                let busy = self.download_rx.is_some();
                if ui
                    .add_enabled(
                        self.download_consent && !busy,
                        egui::Button::new("Скачать модель"),
                    )
                    .clicked()
                {
                    self.start_download();
                }
                if let Some(p) = &self.download_progress {
                    let (d, t) = (p.done(), p.total());
                    let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(360.0)
                            .text(if t > 0 {
                                format!("файлов: {d}/{t}")
                            } else {
                                p.msg()
                            }),
                    );
                    ui.label(p.msg());
                }
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
                if ui
                    .add_enabled(
                        self.embed_rx.is_none() && embed::model_present(&self.settings),
                        egui::Button::new("Посчитать теги"),
                    )
                    .clicked()
                {
                    self.start_embed();
                }
                ui.label(
                    "Теги и векторы привязаны к модели: после смены модели их нужно посчитать.",
                );

                ui.add_space(8.0);
                ui.heading("Индексация и дубли");
                let r = ui.add(
                    egui::DragValue::new(&mut self.settings.index_jobs)
                        .range(1..=64)
                        .suffix(" потоков"),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.save_settings();
                }
                let r = ui.add(
                    egui::Slider::new(&mut self.settings.max_dist, 0..=16)
                        .text("порог дублей, бит из 64"),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.max_dist = self.settings.max_dist;
                    self.save_settings();
                }

                ui.add_space(8.0);
                ui.heading("Теги");
                let r = ui.add(
                    egui::Slider::new(&mut self.settings.tag_show, 0.10..=0.50)
                        .text("порог уверенного тега"),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.save_settings();
                }
                let r = ui.add(
                    egui::Slider::new(&mut self.settings.tag_margin, 0.0..=0.10)
                        .text("отрыв от второго тега"),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.save_settings();
                }
                ui.label(
                    "Ниже порог — больше тегов и больше ошибок. «Убрать» у тега в карточке                      скрывает его у одного фото и переживает пересчёт.",
                );

                ui.add_space(8.0);
                ui.heading("Лица");
                let present = faces::model_present(&self.settings);
                ui.label(format!(
                    "{} · {} · {}",
                    faces::MODEL_ID,
                    db::human(faces::model_bytes(&self.settings)),
                    if present { "на месте" } else { "не скачана" }
                ));
                let r = ui.add(
                    egui::Slider::new(&mut self.settings.face_threshold, 0.20..=0.70)
                        .step_by(0.01)
                        .text("порог склейки лиц (косинус)"),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.save_settings();
                }
                ui.label(
                    "Модели SCRFD и ArcFace — ONNX, локально; лица, пути и эмбеддинги никуда не \
                     отправляются.",
                );
                ui.checkbox(
                    &mut self.face_download_consent,
                    "разрешаю скачать веса лиц (фото не отправляются)",
                );
                if ui
                    .add_enabled(
                        self.face_download_consent && self.face_download_rx.is_none(),
                        egui::Button::new("Скачать модель лиц"),
                    )
                    .clicked()
                {
                    self.start_face_download();
                }
                if let Some(p) = &self.face_download_progress {
                    let (d, t) = (p.done(), p.total());
                    let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(360.0)
                            .text(if t > 0 {
                                format!("файлов лиц: {d}/{t}")
                            } else {
                                p.msg()
                            }),
                    );
                    ui.label(p.msg());
                }
                if ui.button("Проверить модель лиц").clicked() {
                    match faces::check_model(&self.settings) {
                        Ok(()) => self.status = "модель лиц проверена: sha256 совпал".into(),
                        Err(e) => self.status = format!("модель лиц не готова: {e:#}"),
                    }
                }

                ui.add_space(8.0);
                ui.heading("Хранилище");
                ui.label(format!("БД: {}", self.db_path.display()));
                ui.label(format!("кэш: {}", db::cache_dir().display()));
                ui.label(format!("превью: {}", self.thumbs_dir.display()));
                ui.label(format!(
                    "модели: {} · {}",
                    self.settings.models_dir.display(),
                    db::human(embed::model_bytes_in(
                        &self.settings.models_dir,
                        embed::active_entry(&self.settings)
                    ))
                ));

                ui.add_space(8.0);
                ui.heading("Внешние программы");
                ui.label(
                    "Пусто — программа ищется в PATH. Путь проверяется запуском при сохранении; \
                     без программы отключается только её работа.",
                );
                for (i, tool) in tools::ALL.into_iter().enumerate() {
                    ui.horizontal(|ui| {
                        ui.label(format!("{:<10}", tool.name()));
                        let input = self.tools_input.get_mut(tool);
                        ui.add(
                            egui::TextEdit::singleline(input)
                                .hint_text("пусто — из PATH")
                                .desired_width(380.0),
                        );
                    });
                    ui.label(format!("      {}", self.tool_states[i]));
                    ui.label(format!("      без неё: {}", tool.without()));
                }
                if ui.button("Проверить и сохранить программы").clicked() {
                    self.apply_tools();
                }
            });
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
                    .and_then(|t| {
                        embed::confident_tag_with(
                            t,
                            self.settings.tag_show,
                            self.settings.tag_margin,
                        )
                    })
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
        self.status = format!(
            "лица: моделей {}, порог {threshold:.2}…",
            faces::MODEL_ID
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
        let root = PathBuf::from(self.path_input.trim());
        if !root.is_dir() {
            self.status = format!("не каталог: {}", root.display());
            return;
        }
        let db_path = self.db_path.clone();
        let tools = self.settings.tools.clone();
        let progress = Arc::new(Progress::default());
        self.progress = Some(progress.clone());
        let (tx, rx) = mpsc::channel();
        self.rx = Some(rx);
        self.status = format!("индексация {}…", root.display());
        std::thread::spawn(move || {
            let r = index::run(&root, 8, &db_path, &tools, Some(progress));
            let _ = tx.send(r);
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
}

fn draw_face_cell(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut FaceCells, f: &db::FaceView) {
    const W: f32 = 150.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, W + 26.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(W, W));
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
                egui::Color32::GRAY,
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
    if resp.clicked() {
        cs.open_clicked = Some(f.file_id);
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
    line.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
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
        match f.thumb.as_ref().and_then(|rel| {
            load_texture(ctx, &cs.thumbs_dir.join(rel), &format!("t{}", f.id))
        }) {
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
    "январь", "февраль", "март", "апрель", "май", "июнь", "июль", "август", "сентябрь",
    "октябрь", "ноябрь", "декабрь",
];
const WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];

fn shift_month(year: i32, month: u32, delta: i32) -> (i32, u32) {
    let m = month as i32 - 1 + delta;
    let y = year + m.div_euclid(12);
    let m = m.rem_euclid(12) as u32 + 1;
    (y.clamp(1900, 2199), m)
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
fn nearest_file_month(
    months: &BTreeSet<String>,
    year: i32,
    month: u32,
) -> Option<(i32, u32)> {
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
    fn flush(segment: &mut String, matched: bool, normal: egui::Color32, accent: egui::Color32, job: &mut egui::text::LayoutJob) {
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

fn draw_cell(ui: &mut egui::Ui, ctx: &egui::Context, cs: &mut Cells, f: &FileRow) {
    const W: f32 = 150.0;
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(W, W + 26.0), egui::Sense::click());
    let img_rect = egui::Rect::from_min_size(rect.min, egui::vec2(W, W));
    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, 3.0, ui.visuals().extreme_bg_color);
        let drawn = paint_thumb(ui, ctx, cs, f, img_rect);
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
                egui::Color32::GRAY,
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

        egui::Panel::top("top").show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.label("Каталог:");
                ui.add(
                    egui::TextEdit::singleline(&mut self.path_input)
                        .hint_text("путь к каталогу с фото")
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
                    if self.rx.is_some() && ui.button("Стоп").clicked() {
                        p.cancel();
                    }
                }
            });
            let semantic_ready = embed::model_present(&self.settings);
            if semantic_ready || self.ocr_files > 0 {
                ui.horizontal(|ui| {
                    ui.label(if self.search_ocr {
                        "Текст на фото:"
                    } else {
                        "Смысловой поиск:"
                    });
                    let resp = ui.add(
                        egui::TextEdit::singleline(&mut self.semantic_query)
                            .hint_text(if self.search_ocr {
                                "справка, квитанция, 2024"
                            } else {
                                "торт, море, документ"
                            })
                            .desired_width(300.0),
                    );
                    let enter =
                        resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let ready = self.search_ocr || semantic_ready;
                    if ui
                        .add_enabled(ready, egui::Button::new("Найти"))
                        .clicked()
                        || (enter && ready)
                    {
                        self.start_search();
                    }
                    ui.checkbox(&mut self.search_ocr, "по тексту на фото")
                        .on_hover_text(
                            "искать по тексту, распознанному на документе или скриншоте",
                        );
                    if self.semantic_hits.is_some() && ui.button("Сбросить").clicked() {
                        self.semantic_hits = None;
                        self.ocr_snippets.clear();
                        self.status = "поиск сброшен".into();
                    }
                    if self.search_rx.is_some() {
                        ui.label("загрузка модели CLIP…");
                    }
                    if !semantic_ready && !self.search_ocr {
                        ui.label("модель не скачана");
                    }
                });
            }
            if let Some(p) = &self.embed_progress {
                let (d, t) = (p.done(), p.total());
                let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(320.0)
                            .text(if t > 0 {
                                format!("CLIP: {d}/{t}")
                            } else {
                                p.msg()
                            }),
                    );
                    if self.embed_rx.is_some() && ui.button("Стоп").clicked() {
                        p.cancel();
                    }
                });
            }
            if let Some(p) = &self.ocr_progress {
                let (d, t) = (p.done(), p.total());
                let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(320.0)
                            .text(if t > 0 {
                                format!("OCR: {d}/{t}")
                            } else {
                                p.msg()
                            }),
                    );
                    if self.ocr_rx.is_some() && ui.button("Стоп").clicked() {
                        p.cancel();
                    }
                });
            }
            if let Some(p) = &self.face_progress {
                let (d, t) = (p.done(), p.total());
                let frac = if t > 0 { d as f32 / t as f32 } else { 0.0 };
                ui.horizontal(|ui| {
                    ui.add(
                        egui::ProgressBar::new(frac)
                            .desired_width(320.0)
                            .text(if t > 0 {
                                format!("Лица: {d}/{t}")
                            } else {
                                p.msg()
                            }),
                    );
                    if self.face_rx.is_some() && ui.button("Стоп").clicked() {
                        p.cancel();
                    }
                });
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

        // Текст с фото читается из БД один раз на файл, а не каждый кадр.
        if let Some(id) = self.selected
            && !self.ocr_text.contains_key(&id)
            && !self.ocr_snippets.contains_key(&id)
            && let Ok(conn) = db::open(&self.db_path)
            && let Ok(text) = db::load_ocr(&conn, id)
        {
            self.ocr_text.insert(id, text.unwrap_or_default());
        }

        let mut unhide_file: Option<i64> = None;
        let mut ocr_file: Option<i64> = None;
        egui::Panel::left("left").default_size(260.0).show(ui, |ui| {
            ui.add_space(6.0);
            ui.heading("photocat");
            ui.label(
                egui::RichText::new("семейный архив")
                    .small()
                    .color(egui::Color32::from_gray(150)),
            );
            ui.separator();
            ui.label(egui::RichText::new("БИБЛИОТЕКА").small().weak());
            ui.selectable_value(&mut self.view, View::Grid, "Все файлы");
            ui.selectable_value(&mut self.view, View::Dupes, "Дубли и серии");
            ui.selectable_value(&mut self.view, View::Calendar, "Календарь");
            ui.selectable_value(&mut self.view, View::Faces, "Лица");
            ui.selectable_value(&mut self.view, View::Settings, "Настройки");
            ui.separator();
            ui.label("Поиск по пути/дате:");
            ui.add(egui::TextEdit::singleline(&mut self.query).desired_width(f32::INFINITY));
            ui.checkbox(&mut self.only_images, "только изображения");
            ui.separator();

            if self.view == View::Dupes {
                ui.label("Порог похожести (бит из 64):");
                let resp = ui.add(egui::Slider::new(&mut self.max_dist, 0..=16));
                if resp.changed() {
                    self.settings.max_dist = self.max_dist;
                    self.save_settings();
                }
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

            if self.view == View::Faces {
                ui.separator();
                ui.label(format!(
                    "Порог склейки лиц: {:.2}",
                    self.settings.face_threshold
                ));
                let r = ui.add(
                    egui::Slider::new(&mut self.settings.face_threshold, 0.20..=0.70)
                        .step_by(0.01),
                );
                if r.drag_stopped() || r.lost_focus() {
                    self.save_settings();
                }
                ui.label(
                    "Выше порог — меньше ложных склеек, но один человек может разбиться на группы.",
                );
                let model = faces::model_present(&self.settings);
                if model {
                    ui.label(format!(
                        "модель лиц {} · {}",
                        faces::MODEL_ID,
                        db::human(faces::model_bytes(&self.settings))
                    ));
                } else {
                    ui.label("модель лиц не скачана");
                    if ui.button("Настройки лиц").clicked() {
                        self.view = View::Settings;
                    }
                }
                ui.label(format!(
                    "лиц: {} на {} снимках, групп: {}",
                    self.face_faces,
                    self.face_photos,
                    self.face_clusters.len()
                ));
                ui.label(format!(
                    "ещё без лиц: {} из {} снимков",
                    self.face_todo, self.face_images
                ));
                if self.face_rx.is_some() {
                    if let Some(p) = &self.face_progress {
                        if ui.button("Стоп").clicked() {
                            p.cancel();
                            self.status = "лица: останавливаю после текущих снимков…".into();
                        }
                    } else {
                        ui.label("пересобираю группы…");
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
                        .on_hover_text("порог применяется к лицам; ручные подписи и разъединения сохраняются")
                        .clicked()
                    {
                        self.start_face_clusters();
                    }
                }
                ui.separator();
                ui.label(egui::RichText::new("ГРУППЫ").small().weak());
                let clusters = self.face_clusters.clone();
                egui::ScrollArea::vertical()
                    .id_salt("face-clusters")
                    .max_height(250.0)
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
                        }
                    });
            }

            ui.separator();
            ui.heading("Модель ИИ");
            if embed::model_present(&self.settings) {
                let entry = embed::active_entry(&self.settings);
                ui.label(format!(
                    "{}: {} · {} признаков",
                    entry.title,
                    db::human(embed::model_bytes_in(&self.settings.models_dir, entry)),
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
                ui.label("Фильтр по уверенному тегу:");
                egui::ComboBox::from_id_salt("tag_filter")
                    .selected_text(self.tag_filter.clone().unwrap_or_else(|| "все".into()))
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut self.tag_filter, None, "все");
                        for label in embed::TAG_LABELS {
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
                ui.label(format!("с уверенным тегом: {tagged}"));
                if let Some(hits) = &self.semantic_hits {
                    ui.label(format!("смысловой поиск: {} файлов", hits.len()));
                }
            } else {
                ui.label("модель не скачана");
            }
            if ui.button("Настройки модели").clicked() {
                self.view = View::Settings;
            }

            ui.separator();
            ui.label(egui::RichText::new("ТЕКСТ НА ФОТО").small().weak());
            match &self.tesseract {
                Some(v) => {
                    ui.label(egui::RichText::new(v).small().color(egui::Color32::from_gray(150)));
                }
                None => {
                    ui.label(
                        "tesseract не запускается — укажите путь в «Настройках» \
                         или установите tesseract и языки rus, eng",
                    );
                }
            }
            let images = self.items.iter().filter(|r| r.kind == "image").count();
            ui.label(format!("с текстом: {} из {} снимков", self.ocr_files, images));
            if self.ocr_chars > 0 {
                ui.label(format!("знаков: {}", self.ocr_chars));
            }
            ui.label(format!(
                "кандидатов на проход: {}, осталось {}",
                self.ocr_candidates, self.ocr_remaining
            ));
            if self.ocr_rx.is_some() {
                if ui.button("Стоп").clicked()
                    && let Some(p) = &self.ocr_progress
                {
                    p.cancel();
                    self.status = "OCR: останавливаю после текущих файлов…".into();
                }
            } else if ui
                .add_enabled(
                    self.tesseract.is_some() && self.ocr_remaining > 0,
                    egui::Button::new(if self.ocr_remaining < self.ocr_candidates {
                        format!("Продолжить ({})", self.ocr_remaining)
                    } else {
                        format!("Распознать документы ({})", self.ocr_candidates)
                    }),
                )
                .on_hover_text(
                    "долгий проход: документы и скриншоты без камеры и по уверенному тегу; \
                     один снимок — кнопкой в карточке файла",
                )
                .clicked()
            {
                self.start_ocr(None);
            }

            ui.separator();
            let n = self.items.len();
            let bytes: i64 = self.items.iter().map(|r| r.size).sum();
            let no_date = self.items.iter().filter(|r| r.taken_at.is_none()).count();
            let no_thumb = self.items.iter().filter(|r| r.thumb.is_none()).count();
            ui.label(format!("файлов: {n}"));
            ui.label(format!("объём: {}", db::human(bytes)));
            ui.label(format!("без даты: {no_date}"));
            ui.label(format!("без превью: {no_thumb}"));

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
                    if f.kind == "video" {
                        ui.label(match f.duration_s {
                            Some(d) => format!("длительность: {}", db::duration(d)),
                            None => "длительность: —".into(),
                        });
                    }
                    let confident = self.tags.get(&id).and_then(|tags| {
                        embed::confident_tag_with(
                            tags,
                            self.settings.tag_show,
                            self.settings.tag_margin,
                        )
                        .map(|(tag, _)| tag.to_string())
                    });
                    let mut hide: Option<String> = None;
                    if let Some(tags) = self.tags.get(&id) {
                        ui.label("теги:");
                        for (tag, score) in tags {
                            ui.horizontal(|ui| {
                                let color = if confident.as_deref() == Some(tag.as_str()) {
                                    egui::Color32::from_rgb(90, 170, 255)
                                } else {
                                    egui::Color32::from_gray(190)
                                };
                                ui.label(
                                    egui::RichText::new(format!("{tag} ({score:.2})"))
                                        .color(color),
                                );
                                if ui
                                    .small_button("убрать")
                                    .on_hover_text(
                                        "убрать тег у этого фото; оригинал не меняется",
                                    )
                                    .clicked()
                                {
                                    hide = Some(tag.clone());
                                }
                            });
                        }
                    } else {
                        ui.label("тег: не посчитан");
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
                        && ui.button(format!("вернуть скрытые теги ({hidden})")).clicked()
                    {
                        unhide_file = Some(id);
                    }
                    if f.kind == "image" {
                        let label = if self
                            .ocr_text
                            .get(&id)
                            .is_some_and(|t| !t.trim().is_empty())
                        {
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
                            ocr_file = Some(id);
                        }
                    }
                    let shown = self
                        .ocr_snippets
                        .get(&id)
                        .cloned()
                        .or_else(|| {
                            self.ocr_text.get(&id).map(|t| {
                                t.chars().take(400).collect::<String>()
                            })
                        })
                        .filter(|t| !t.trim().is_empty());
                    if let Some(text) = shown {
                        ui.separator();
                        ui.label("текст с фото:");
                        ui.label(highlight_job(ui, &text));
                    }
                    ui.label(format!("sha256: {}…", &f.sha256[..16.min(f.sha256.len())]));
                }
            }
        });

        if let Some(id) = ocr_file {
            self.start_ocr(Some(id));
        }

        if let Some(id) = unhide_file {
            match db::unhide_tags(&self.db_path, id) {
                Ok(_) => {
                    self.reload();
                    self.status = "скрытые теги возвращены".into();
                }
                Err(e) => self.status = format!("теги не вернуть: {e}"),
            }
        }

        if self.view == View::Settings {
            egui::CentralPanel::default().show(ui, |ui| self.settings_view(ui));
            return;
        }

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
        let face_clusters = std::mem::take(&mut self.face_clusters);
        let face_views = std::mem::take(&mut self.face_views);
        let mut face_cells = FaceCells {
            textures: std::mem::take(&mut self.face_textures),
            missing: std::mem::take(&mut self.face_missing),
            dir: db::cache_dir().join("faces"),
            selected_file: self.selected,
            split_clicked: None,
            open_clicked: None,
        };
        let face_selected = self.face_selected_cluster;
        let mut face_label_input = self.face_label_input.clone();
        let face_total = self.face_faces;
        let mut face_save_label: Option<String> = None;
        let view = self.view;
        let mut cal_year = self.cal_year;
        let mut cal_month = self.cal_month;
        let mut cal_day = self.cal_day.clone();

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
            View::Calendar => {
                egui::ScrollArea::vertical()
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
                        let prev_month = shift_to_file_month(&months, cal_year, cal_month, -1);
                        let next_month = shift_to_file_month(&months, cal_year, cal_month, 1);

                        let mut nav = 0i32;
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(prev_month.is_some(), egui::Button::new("←"))
                                .on_hover_text("предыдущий месяц с файлами")
                                .clicked()
                            {
                                nav = -1;
                            }
                            ui.heading(format!(
                                "{} {}",
                                MONTHS[(cal_month.clamp(1, 12) - 1) as usize],
                                cal_year
                            ));
                            if ui
                                .add_enabled(next_month.is_some(), egui::Button::new("→"))
                                .on_hover_text("следующий месяц с файлами")
                                .clicked()
                            {
                                nav = 1;
                            }
                            if ui.button("Сегодня").clicked() {
                                let today = chrono::Local::now().date_naive();
                                let (y, m) = (today.year(), today.month());
                                if months.contains(&month_key(y, m)) {
                                    cal_year = y;
                                    cal_month = m;
                                } else if let Some((y, m)) =
                                    nearest_file_month(&months, y, m)
                                {
                                    cal_year = y;
                                    cal_month = m;
                                } else {
                                    cal_year = y;
                                    cal_month = m;
                                }
                                cal_day = None;
                            }
                        });
                        if nav != 0 {
                            let target = if nav < 0 { prev_month } else { next_month };
                            if let Some((y, m)) = target {
                                cal_year = y;
                                cal_month = m;
                                cal_day = None;
                            }
                        }
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
                            for wd in WEEKDAYS {
                                ui.allocate_ui(egui::vec2(
                                    (ui.available_width() - 12.0) / 7.0,
                                    18.0,
                                ), |ui| {
                                    ui.label(
                                        egui::RichText::new(wd)
                                            .small()
                                            .color(egui::Color32::from_gray(150)),
                                    );
                                });
                            }
                        });
                        let cell_w = ((ui.available_width() - 6.0 * 4.0) / 7.0).max(82.0);
                        let cell_h = 96.0;
                        let first = first_weekday_monday(cal_year, cal_month);
                        let n_days = days_in_month(cal_year, cal_month);
                        let mut day = 1u32;
                        for _week in 0..6 {
                            if day > n_days {
                                break;
                            }
                            ui.horizontal(|ui| {
                                for wd in 0..7u32 {
                                    if day > n_days {
                                        ui.allocate_space(egui::vec2(cell_w, cell_h));
                                        continue;
                                    }
                                    if wd < first && day == 1 {
                                        ui.allocate_space(egui::vec2(cell_w, cell_h));
                                        continue;
                                    }
                                    let date = format!("{cal_year:04}-{cal_month:02}-{day:02}");
                                    let idxs = by_day.get(&date);
                                    let (rect, resp) = ui.allocate_exact_size(
                                        egui::vec2(cell_w, cell_h),
                                        egui::Sense::click(),
                                    );
                                    if ui.is_rect_visible(rect) {
                                        let selected = cal_day.as_deref() == Some(date.as_str());
                                        let fill = if selected {
                                            ui.visuals().selection.bg_fill.gamma_multiply(0.25)
                                        } else {
                                            ui.visuals().faint_bg_color
                                        };
                                        ui.painter().rect_filled(rect.shrink(1.0), 4.0, fill);
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
                                                rect.right_top() + egui::vec2(-6.0, 4.0),
                                                egui::Align2::RIGHT_TOP,
                                                ids.len().to_string(),
                                                egui::FontId::proportional(11.0),
                                                egui::Color32::from_gray(150),
                                            );
                                            let area = egui::Rect::from_min_max(
                                                rect.left_top() + egui::vec2(4.0, 22.0),
                                                rect.right_bottom() - egui::vec2(4.0, 4.0),
                                            );
                                            let n = ids.len().min(3);
                                            let tw = ((area.width() - (n as f32 - 1.0) * 2.0)
                                                / n as f32)
                                                .max(8.0);
                                            for (k, &i) in ids.iter().take(n).enumerate() {
                                                let r = egui::Rect::from_min_size(
                                                    egui::pos2(
                                                        area.left()
                                                            + k as f32 * (tw + 2.0),
                                                        area.top(),
                                                    ),
                                                    egui::vec2(tw, area.height()),
                                                );
                                                ui.painter().rect_filled(
                                                    r,
                                                    2.0,
                                                    ui.visuals().extreme_bg_color,
                                                );
                                                paint_thumb(ui, ctx, &mut cells, &items[i], r);
                                            }
                                            if resp.clicked() {
                                                cal_day = Some(date.clone());
                                            }
                                        }
                                    }
                                    day += 1;
                                }
                            });
                            ui.add_space(2.0);
                        }
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
                            ui.label("Выберите день в календаре — ниже появятся снимки.");
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
                    ui.heading(format!("Все лица · {face_total}"));
                    ui.label("Выберите группу слева, чтобы дать ей имя, или разъединяйте плитки.");
                }
                let shown: Vec<&db::FaceView> = face_views
                    .iter()
                    .filter(|f| face_selected.is_none() || f.cluster_id == face_selected)
                    .collect();
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
            View::Settings => unreachable!("настройки рисуются до сетки"),
        });

        self.items = items;
        self.groups = groups;
        self.face_clusters = face_clusters;
        self.face_views = face_views;
        let face_split = face_cells.split_clicked;
        let face_open = face_cells.open_clicked;
        self.face_textures = std::mem::take(&mut face_cells.textures);
        self.face_missing = std::mem::take(&mut face_cells.missing);
        self.face_label_input = face_label_input;
        self.cal_year = cal_year;
        self.cal_month = cal_month;
        self.cal_day = cal_day;
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
            && let Some(file_id) = face_open
        {
            self.selected = Some(file_id);
            self.view = View::Grid;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        days_in_month, first_weekday_monday, fit_rect, nearest_file_month, shift_month,
        shift_to_file_month,
    };
    use std::collections::BTreeSet;

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
    fn неделя_календаря_начинается_с_понедельника() {
        assert_eq!(first_weekday_monday(2024, 1), 0); // 1 января 2024 — понедельник
        assert_eq!(first_weekday_monday(2024, 5), 2); // 1 мая 2024 — среда
    }
}
