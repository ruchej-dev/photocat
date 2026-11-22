//! Раскладка архива по датам: план → подтверждение → применение (hardlink/copy/symlink).
//! Оригиналы не удаляются и не переименовываются: витрина — новые файлы/ссылки рядом, а не поверх
//! архива. Имена в витрине собираются из шаблона (по умолчанию — оригинальные).

use crate::db::{self, FileRow};
use crate::{flag_str, has_flag};
use anyhow::{Context, Result};
use std::collections::{BTreeMap, HashMap};
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

    pub fn title(self) -> &'static str {
        match self {
            Self::Day => "по дням",
            Self::Month => "по месяцам",
            Self::Event => "по событиям (день + место)",
        }
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
    let mut rest = tpl;
    while let Some(start) = rest.find('{') {
        let after = &rest[start..];
        let Some(end) = after.find('}') else {
            anyhow::bail!("в шаблоне имени не закрыта скобка: {after}");
        };
        let token = &after[..=end];
        if !PLACEHOLDERS.contains(&token) {
            anyhow::bail!(
                "неизвестная подстановка {token}; допустимы: {}",
                PLACEHOLDERS.join(" ")
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

pub fn cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let out = match flag_str(args, "--out") {
        Some(o) => PathBuf::from(o),
        None => {
            eprintln!("нужен каталог назначения: organize --out DIR [--apply] [--by day|month|event]");
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
                eprintln!("--by: ожидается day, month или event, получено «{v}»");
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

    let conn = db::open(&db_path)?;
    let rows = db::load_all(&conn)?;
    let settings = db::load_settings(&conn);
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
    println!("витрина: {} → {}", layout.title(), out.display());
    println!(
        "файлов с датой: {}, без даты (не попадут в план): {}, помечено лишними и не включено: {}",
        plan.items.len(),
        plan.no_date,
        plan.excluded
    );
    println!("каталогов витрины: {}", plan.dirs);
    for (top, n) in &by_top {
        println!("  {top}: {n}");
    }
    println!("\nпримеры:");
    for it in plan.items.iter().take(5) {
        println!("  {}  →  {}", it.src.display(), it.dst.display());
    }
    if !apply_it {
        println!(
            "\nэто план (dry-run). Добавьте --apply, чтобы создать ссылки/копии в {}",
            out.display()
        );
        return Ok(());
    }
    let (ok, fail) = apply(&plan.items, mode)?;
    println!("создано: {ok}, ошибок: {fail}");
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
        }
    }

    fn plan_of(rows: &[FileRow], opts: &Options) -> Plan {
        build_plan(rows, opts)
    }

    fn dst(plan: &Plan, i: usize) -> String {
        plan.items[i].dst.to_string_lossy().to_string()
    }

    #[test]
    fn по_дням_и_по_месяцам_разная_глубина() {
        let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
        let mut opts = Options::new("/out");
        let day = plan_of(&rows, &opts);
        assert_eq!(dst(&day, 0), "/out/2024/2024-05-17/IMG_1.jpg");
        opts.layout = Layout::Month;
        let month = plan_of(&rows, &opts);
        assert_eq!(dst(&month, 0), "/out/2024/2024-05/IMG_1.jpg");
    }

    #[test]
    fn событие_делит_день_по_месту_а_без_gps_оставляет_в_дне() {
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
    fn гпс_ячейка_знает_юг_и_запад() {
        assert_eq!(gps_cell(-33.868_8, -151.209_3), "33.87S_151.21W");
        assert_eq!(gps_cell(-0.001, 0.000_1), "0.00N_0.00E");
    }

    #[test]
    fn шаблон_имени_подставляет_дату_время_камеру_и_расширение() {
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
    fn тег_в_имени_только_если_он_уверенный() {
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
    fn пустые_подстановки_не_оставляют_разделителей() {
        let r = row(1, "/a/x.jpg", Some("2024-05-17T14:42:43"));
        let mut opts = Options::new("/out");
        opts.name = Some("{tag}_{date}_{camera}_{orig}".into());
        let plan = plan_of(&[r], &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_x.jpg");
    }

    #[test]
    fn недопустимые_символы_заменяются_а_коллизии_нумеруются() {
        let a = row(1, "/a/photo 1.jpg", Some("2024-05-17T14:42:43"));
        let b = row(2, "/b/photo 1.jpg", Some("2024-05-17T15:00:00"));
        let mut opts = Options::new("/out");
        opts.name = Some("{date}_{orig}".into());
        let plan = plan_of(&[a, b], &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_photo_1.jpg");
        assert_eq!(dst(&plan, 1), "/out/2024/2024-05-17/20240517_photo_1-2.jpg");
    }

    #[test]
    fn без_шаблона_имя_остаётся_оригинальным() {
        let rows = [row(1, "/a/IMG_20240517_144243.jpg", Some("2024-05-17T14:42:43"))];
        let plan = plan_of(&rows, &Options::new("/out"));
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/IMG_20240517_144243.jpg");
    }

    #[test]
    fn переименование_совместимо_со_старым_форматом() {
        let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
        let mut opts = Options::new("/out");
        opts.name = Some(RENAME_TEMPLATE.into());
        let plan = plan_of(&rows, &opts);
        assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_144243_IMG_1.jpg");
    }

    #[test]
    fn лишние_исключаются_а_без_даты_считаются() {
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
    fn шаблон_с_неизвестной_подстановкой_отвергается() {
        assert!(validate_template("{date}_{foo}").is_err());
        assert!(validate_template("{date}_{").is_err());
        assert!(validate_template(RENAME_TEMPLATE).is_ok());
        assert!(validate_template(&PLACEHOLDERS.join("")).is_ok());
    }

    #[test]
    fn раскладка_без_apply_ничего_не_создаёт() {
        let dir = std::env::temp_dir().join(format!("photocat-organize-{}", std::process::id()));
        let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
        let _ = build_plan(&rows, &Options::new(&dir));
        assert!(!dir.exists());
    }

    #[test]
    fn номер_коллизии_не_ломает_файл_без_расширения() {
        let dst = PathBuf::from("/out/2024/2024-05-17/file");
        assert_eq!(numbered(&dst, 2), PathBuf::from("/out/2024/2024-05-17/file-2"));
        let dst = PathBuf::from("/out/file.jpg");
        assert_eq!(numbered(&dst, 3), PathBuf::from("/out/file-3.jpg"));
    }
}
