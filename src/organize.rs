//! Раскладка архива по датам: план → подтверждение → применение (hardlink/copy/symlink).
//! Оригиналы не удаляются и не переименовываются: витрина — новые файлы/ссылки рядом, а не поверх
//! архива. Имена в витрине собираются из шаблона (по умолчанию — оригинальные).

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

/// Глубина витрины. `Event` — день плюс место по GPS (ячейка ~1 км): без обратного геокодинга
/// (сеть запрещена) место называется координатами, а снимки без GPS остаются в каталоге дня.
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
                Self::Day => "по дням",
                Self::Month => "по месяцам",
                Self::Event => "по событиям (день + место)",
            },
        )
    }
}

/// Подстановки шаблона имени. `{ext}` пишется без точки; если его нет в шаблоне, расширение
/// исходного файла добавляется само.
pub const PLACEHOLDERS: [&str; 6] = ["{date}", "{time}", "{camera}", "{tag}", "{orig}", "{ext}"];

/// Шаблон по умолчанию для `--rename`: то же, что раньше давал флаг переименования.
pub const RENAME_TEMPLATE: &str = "{date}_{time}_{orig}";

pub struct Options {
    pub out: PathBuf,
    pub layout: Layout,
    /// Шаблон имени без расширения; `None` — оригинальное имя.
    pub name: Option<String>,
    /// Не включать файлы, помеченные лишними в дублях.
    pub exclude_dup: bool,
    /// Уверенный тег для `{tag}`: видимые теги и пороги из настроек.
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
    /// Снимков без даты — в витрину не попадают.
    pub no_date: usize,
    /// Помечены лишними и исключены по `--exclude-dup`.
    pub excluded: usize,
    /// Сколько каталогов витрины получится (дни, месяцы или события).
    pub dirs: usize,
}

/// Проверка шаблона: неизвестная подстановка — ошибка сразу, а не в середине раскладки.
pub fn validate_template(tpl: &str) -> Result<()> {
    validate_tokens(tpl, &PLACEHOLDERS)
}

fn validate_tokens(tpl: &str, allowed: &[&str]) -> Result<()> {
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        let after = &rest[start..];
        let Some(end) = after.find('}') else {
            anyhow::bail!("в шаблоне имени не закрыта скобка: {after}");
        };
        let token = &after[..=end];
        if !allowed.contains(&token) {
            anyhow::bail!(
                "неизвестная подстановка {token}; допустимы: {}",
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
    // Сначала порядок по будущим путям (при равенстве — по источнику), потом нумерация коллизий:
    // так `-2` гарантированно идёт после базового имени.
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

/// Ячейка GPS ~1 км (два знака после запятой): соседние кадры одной прогулки попадают вместе,
/// кадры из разных мест — в разные каталоги. Без обратного геокодинга имя — координаты.
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

/// Уверенный верхний тег — та же формула, что `embed::confident_tag_with`.
/// Продублирована нарочно: `organize` не зависит от модели и её модуля (правило слоёв).
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

/// Одна подстановка: только безопасные для любой ФС символы, не длиннее 40 знаков.
fn component(s: &str) -> String {
    tidy(s).chars().take(40).collect::<String>()
        .trim_end_matches(['_', '-', '.'])
        .to_string()
}

/// Пробелы → `_`, запрещённые символы выброшены, повторы и хвосты разделителей убраны.
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

/// `file.jpg` + 2 → `file-2.jpg`; файл без расширения не получает точку на конце.
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

/// Символическая ссылка на файл. На Windows нужен режим разработчика или права администратора;
/// без них ошибка попадает в счётчик `fail`, а не роняет применение плана.
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

// ---------------------------------------------------------------- M20: выгрузка альбома

/// Шаблон имени файла при выгрузке: позиция в альбоме и оригинальное имя.
pub const EXPORT_TEMPLATE: &str = "{n}_{orig}";

/// Подстановки выгрузки. `{n}` — позиция в альбоме, `{hash8}` — первые 8 знаков sha256.
pub const EXPORT_PLACEHOLDERS: [&str; 8] = [
    "{n}", "{date}", "{time}", "{camera}", "{tag}", "{orig}", "{ext}", "{hash8}",
];

/// Предел длины одного компонента пути и полного пути на диске (запас до 260 в Windows).
pub const MAX_NAME: usize = 120;
pub const MAX_PATH: usize = 240;

/// Альбом, готовый к выгрузке: узел дерева и его файлы в порядке альбома.
#[derive(Clone, Debug)]
pub struct ExportAlbum {
    pub id: i64,
    pub parent_id: i64,
    pub name: String,
    pub files: Vec<FileRow>,
}

/// Данные из БД (`db::album_export_data`) в вид, который понимает `plan_album`.
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
    /// Каталог, внутри которого создаётся корневой каталог альбома.
    pub out: PathBuf,
    /// Шаблон имени файла; по умолчанию `{n}_{orig}`.
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
    /// Путь относительно `out`; включает имя корневого каталога альбома.
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

/// План выгрузки: дерево каталогов и копии файлов. На диск ничего не пишется.
#[derive(Clone, Debug, Default)]
pub struct ExportPlan {
    pub root_id: i64,
    pub out: PathBuf,
    /// Имя корневого каталога альбома (относительно `out`).
    pub root: PathBuf,
    pub dirs: Vec<ExportDir>,
    pub items: Vec<ExportItem>,
    /// Файлы в корзине приложения: остаются в альбоме, но на диск не выгружаются.
    pub trashed: usize,
    /// Файлы, чей полный путь длиннее `MAX_PATH`: пропущены с причиной.
    pub too_long: usize,
    /// Имена, которым коллизия добавила `-2` уже на этапе плана.
    pub renamed: usize,
    pub bytes: i64,
}

impl ExportPlan {
    pub fn files(&self) -> usize {
        self.items.len()
    }

    /// Строка сводки для GUI и CLI; числа с единицами — через `db::human_in`.
    pub fn summary(&self, lang: Lang) -> String {
        let mut s = i18n::fill(
            lang,
            "каталогов {} · файлов {} · {}; в корзине и не выгружаются: {}; имён с коллизией: {}; слишком длинных путей: {}",
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
                "; пустых каталогов: {}",
                &[empty.to_string()],
            ));
        }
        if self.too_long > 0 {
            s.push_str(i18n::t(lang, "; такие файлы пропущены"));
        }
        s
    }

    /// План-дерево: каталоги с отступами и имена файлов. `limit` строк —
    /// для окна; CLI печатает всё.
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

/// Отчёт выгрузки. Ни один файл в цели не перезаписан и не удалён.
#[derive(Clone, Debug, Default)]
pub struct ExportReport {
    pub copied: usize,
    /// Уже выгруженные файлы, в том числе под старым номером (`stale`).
    pub skipped: usize,
    /// Сколько пропущенных лежит в цели под другим именем: порядок устарел.
    pub stale: usize,
    /// Скопировано под именем с `-2` из-за занятого имени.
    pub renamed: usize,
    pub failed: usize,
    pub bytes: u64,
    pub dirs_created: usize,
    pub cancelled: bool,
    /// Что именно создано — для отчёта и теста «отмена в середине».
    pub created: Vec<PathBuf>,
    pub errors: Vec<String>,
    /// Язык отчёта (M21.6): ставит вызывающий до печати; английский по умолчанию.
    pub lang: Lang,
}

impl fmt::Display for ExportReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let text = i18n::fill(
            self.lang,
            "выгружено {} файлов · {}; пропущено уже выгруженных {} (порядок в цели устарел {}); переименовано из-за коллизии {}; каталогов создано {}; ошибок {}",
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
            i18n::t(self.lang, "; остановлено")
        } else {
            ""
        };
        write!(f, "{text}{tail}")
    }
}

/// Проверка шаблона выгрузки: `{n}` и `{hash8}` здесь есть, а в витрине их нет.
pub fn validate_export_template(tpl: &str) -> Result<()> {
    validate_tokens(tpl, &EXPORT_PLACEHOLDERS)
}

/// `--out` должен существовать, быть каталогом и не лежать внутри каталога архива.
pub fn check_out_dir(out: &Path, roots: &[String]) -> Result<()> {
    if !out.exists() {
        anyhow::bail!(
            "каталог назначения не существует: {} — создайте его сами",
            out.display()
        );
    }
    if !out.is_dir() {
        anyhow::bail!("каталог назначения — не каталог: {}", out.display());
    }
    let canon = out.canonicalize().unwrap_or_else(|_| out.to_path_buf());
    for root in roots {
        if let Ok(root_canon) = Path::new(root).canonicalize()
            && (canon == root_canon || canon.starts_with(&root_canon))
        {
            anyhow::bail!(
                "выгрузка внутри каталога архива запрещена: {} — выберите каталог вне {}",
                out.display(),
                root
            );
        }
    }
    Ok(())
}

/// План выгрузки: дерево каталогов альбома и копии файлов. Ничего не создаёт.
pub fn plan_album(albums: &[ExportAlbum], opts: &ExportOptions) -> Result<ExportPlan> {
    let Some(root) = albums.first() else {
        anyhow::bail!("альбом пуст — выгружать нечего");
    };
    let root_name = dir_component(&root.name, root.id);
    let root_rel = PathBuf::from(&root_name);
    let root_path = opts.out.join(&root_rel);
    if root_path.exists() && !root_path.is_dir() {
        anyhow::bail!(
            "в {} уже лежит файл «{root_name}», а нужен каталог",
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
    // Имена среди соседей: сравнение без учёта регистра везде, а не только на
    // Windows/macOS. Альбомы в БД уже не различаются регистром, поэтому
    // строже — значит предсказуемее.
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

/// Выгрузка плана на диск: только копии, ничего не перезаписывается.
/// Существующий файл с тем же sha256 — пропуск (прогон идемпотентен);
/// с другим содержимым — новое имя `-2`; файл с тем же содержимым под старым
/// номером — «порядок в цели устарел».
pub fn apply_export(plan: &ExportPlan, progress: Option<&Progress>) -> ExportReport {
    apply_export_in(plan, progress, &db::cache_dir().join("exports"))
}

/// То же, но с явным каталогом кэша: тесты не пишут в кэш владельца.
fn apply_export_in(
    plan: &ExportPlan,
    progress: Option<&Progress>,
    cache_dir: &Path,
) -> ExportReport {
    let mut report = ExportReport::default();
    if let Some(p) = progress {
        p.set_total(plan.items.len());
        p.set_msg("выгрузка альбома");
    }
    let cache_key = format!("{}-{:08x}", plan.root_id, path_hash(&plan.out));
    let mut cache = load_export_cache(cache_dir, &cache_key);
    let mut hashes: HashMap<PathBuf, String> = HashMap::new();
    let mut created: HashSet<PathBuf> = HashSet::new();
    let mut entries: HashMap<PathBuf, Vec<(String, u64)>> = HashMap::new();

    // Структура важнее содержимого: пустой альбом тоже даёт пустой каталог.
    for dir in &plan.dirs {
        let path = plan.out.join(&dir.rel);
        if path.is_dir() {
            continue;
        }
        match std::fs::create_dir_all(&path) {
            Ok(()) => report.dirs_created += 1,
            Err(e) => note_export_error(
                &mut report,
                format!("каталог {}: {e}", path.display()),
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
                    format!("нет файла: {}", item.src.display()),
                );
                break 'item;
            };
            if !meta.is_file() {
                note_export_error(
                    &mut report,
                    format!("не обычный файл: {}", item.src.display()),
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
                        format!("копия {}: {e}", dst.display()),
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

    // Кэш «что уже выгружено» — в кэше приложения, не в `--out`: цель остаётся чистой.
    cache.retain(|rel, _| plan.out.join(rel).is_file());
    save_export_cache(cache_dir, &cache_key, &cache);
    if let Some(p) = progress {
        p.finish();
    }
    report
}

/// Решение по одному файлу: что делать с уже существующим именем в цели.
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
                    "не прочитать содержимое цели: {}",
                    planned.display()
                ));
            }
        }
    }
    // Тот же снимок лежит в каталоге под другим номером: порядок в цели устарел.
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

/// Есть ли в этом каталоге тот же sha256 под другим именем (и это не файл,
/// созданный в этом же прогоне: две одинаковые копии в альбоме — две строки).
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

/// Кэш выгрузки: относительный путь → (размер, mtime мс, sha256).
type ExportCache = HashMap<String, (u64, u64, String)>;

/// sha256 файла в цели: сначала кэш выгрузки (если совпали размер и mtime),
/// иначе честное чтение. Подмена содержимого с сохранением mtime — та же цена,
/// что у индексации (AGENTS §8).
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

/// Первое свободное `name-2.ext`, `name-3.ext`, … в этом каталоге.
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

/// `stem` и `ext` без точки; расширение — последняя точка, как у `Path`.
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

/// Ширина номера позиции: 2 знака до 99, 3 до 999, дальше по числу знаков.
fn number_width(count: usize) -> usize {
    if count <= 99 {
        2
    } else if count <= 999 {
        3
    } else {
        count.to_string().len()
    }
}

/// Имя файла по шаблону выгрузки. Пустое имя заменяется на `{hash8}`
/// (а без хеша — на `file`), длина компонента — не больше `MAX_NAME`.
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

/// Дата и время из `taken_at` для подстановок: `20240517` и `144243`.
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

/// Имя каталога альбома: безопасные символы, ≤ `MAX_NAME`, пустое — `album-<id>`.
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

/// Как `tidy`, но запрещённый символ становится разделителем, а не исчезает:
/// «Дача/2024» и «Дача:2024» дают одно имя `Дача_2024`, и коллизия видна, а не
/// превращается в другое имя без следа.
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

/// Имена устройств Windows нельзя использовать как имя файла или каталога.
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

/// Ключ сравнения имён: без учёта регистра на всех ОС.
fn name_key(s: &str) -> String {
    s.to_lowercase()
}

/// Сравнение имени с записью в цели: на Windows и macOS файловая система
/// не различает регистр, на Linux — различает.
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

/// Кэш привязан к альбому и каталогу цели: другой `--out` — другой кэш.
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
            // Старый формат (sha, size, rel) не доверяем: файл перечитается один раз.
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

/// Кэш ускоряет повтор: известный sha256 не читается с диска, если размер совпал.
/// Ошибка записи кэша не валит выгрузку — цель уже создана.
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

/// CLI: `photocat export --album N --out DIR [--name ШАБЛОН] [--apply]`.
/// Без `--apply` — только план-дерево; на диске не создаётся ничего.
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
                    "нужен номер альбома: export --album N --out DIR [--name ШАБЛОН] [--apply]"
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
                    "нужен каталог назначения: export --album N --out DIR [--apply]"
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
            "выгрузка альбома: {} → {}",
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
                "это план (dry-run). Добавьте --apply, чтобы скопировать файлы (не ссылки)."
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
                    "выгрузка: {}/{} файлов, ошибок {}",
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
    let mut report = apply_export(&plan, Some(&progress));
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
                    "нужен каталог назначения: organize --out DIR [--apply] [--by day|month|event]"
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
                        "--by: ожидается day, month или event, получено «{}»",
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

    // Считаем по верхнему каталогу витрины (год или «без даты»), а не по глубине пути:
    // у событий внутри дня есть ещё каталог места.
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
            "витрина: {} → {}",
            &[layout.title(lang).to_string(), out.display().to_string()]
        )
    );
    println!(
        "{}",
        i18n::fill(
            lang,
            "файлов с датой: {}, без даты (не попадут в план): {}, помечено лишними и не включено: {}",
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
            "каталогов витрины: {}",
            &[plan.dirs.to_string()]
        )
    );
    for (top, n) in &by_top {
        println!("  {top}: {n}");
    }
    println!("\n{}", i18n::t(lang, "примеры:"));
    for it in plan.items.iter().take(5) {
        println!("  {}  →  {}", it.src.display(), it.dst.display());
    }
    if !apply_it {
        println!(
            "\n{}",
            i18n::fill(
                lang,
                "это план (dry-run). Добавьте --apply, чтобы создать ссылки/копии в {}",
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
            "создано: {}, ошибок: {}",
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

    // ------------------------------------------------------------ M20: выгрузка

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
        assert_eq!(plan.dirs[2].files, 0, "пустой альбом остаётся в плане");
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
        // Шаблон витрины `{n}` не знает, и наоборот.
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

        // 240 символов пути: 120 у корня + 120 у файла — уже за пределом.
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
        let first = apply_export_in(&plan, None, &dir.join("cache"));
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
                "выгрузка — копия, а не hardlink"
            );
        }
        let second = apply_export_in(&plan, None, &dir.join("cache"));
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
        let report = apply_export_in(&plan, None, &dir.join("cache"));
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
        assert_eq!(apply_export_in(&first, None, &dir.join("cache")).copied, 2);
        // Меняем порядок: имена с номерами другие, но содержимое уже лежит в цели.
        let second = plan_album(
            &[album(1, 0, "Порядок", vec![rb, ra])],
            &ExportOptions::new(&out),
        )
        .unwrap();
        let report = apply_export_in(&second, None, &dir.join("cache"));
        assert_eq!(report.copied, 0);
        assert_eq!(report.stale, 2);
        assert_eq!(report.renamed, 0);
        let files = std::fs::read_dir(out.join("Порядок")).unwrap().count();
        assert_eq!(files, 2, "дублей в цели не появилось");
    }

    #[test]
    fn an_empty_album_creates_an_empty_folder() {
        let dir = tmp_dir("empty");
        let out = dir.join("out");
        std::fs::create_dir_all(&out).unwrap();
        let planets = [album(1, 0, "Пустой", Vec::new())];
        let plan = plan_album(&planets, &ExportOptions::new(&out)).unwrap();
        let report = apply_export_in(&plan, None, &dir.join("cache"));
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
        let report = apply_export_in(&plan, None, &dir.join("cache"));
        assert_eq!(report.copied, 1);
        assert_eq!(report.failed, 1);
        assert!(report.errors.iter().any(|e| e.contains("нет файла")));
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
        let report = apply_export_in(&plan, Some(&progress), &dir.join("cache"));
        assert!(report.cancelled);
        assert_eq!(report.created.len(), 0);
        assert!(!out.join("Стоп/01_a.jpg").exists());

        // Обычный прогон перечисляет созданные файлы (для отчёта и теста).
        let report = apply_export_in(&plan, None, &dir.join("cache"));
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
