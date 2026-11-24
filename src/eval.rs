//! M15.7 — a labelled check of the tag dictionary on a real folder.
//!
//! The tags are hints, so the only honest way to know whether a change helped is a sample
//! labelled by a human. This module does the mechanical half: it picks a reproducible sample,
//! writes the labels file and the contact sheets, and turns the filled labels back into numbers
//! (top-1/top-3, how often a confident tag is right, how many "furniture" shots the percentile
//! filter finds).
//!
//! Nothing here reads the photos for the model or writes into the archive: the sample goes by
//! stored tags, the sheets are built from the previews in the cache.
//!
//! Layer L2: `db`, `embed` and the `image` crate; no egui, no SQL strings.

use crate::{db, embed};
use anyhow::{Context, Result};
use image::{Rgb, RgbImage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Сколько снимков в выборке по умолчанию (критерий M15.7 — не меньше ста).
pub const SAMPLE_DEFAULT: u64 = 120;
/// Сколько плиток на контактном листе: 4×4 по 320 px — читается человеком и не режется.
pub const SHEET_PER: usize = 16;
const TILE: u32 = 320;
const PAD: u32 = 6;
const HEADER: u32 = 26;

/// Отобранный файл выборки.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub id: i64,
    pub path: String,
    pub sha256: String,
    /// Путь превью относительно `<кэш>/photocat/thumbs` — из индекса, не из sha256 в коде.
    pub thumb: Option<String>,
}

/// Что получилось при выгрузке выборки.
#[derive(Debug)]
pub struct Export {
    pub labels: PathBuf,
    pub guesses: PathBuf,
    pub sheets: Vec<PathBuf>,
    pub rows: usize,
}

/// Разложить файлы по sha256 и взять каждый k-й: выборка не зависит от имён и дат и
/// повторяется один в один. Меньше `n` файлов — берётся всё, что есть.
pub fn pick(candidates: &[Row], n: usize) -> Vec<Row> {
    if n == 0 || candidates.is_empty() {
        return Vec::new();
    }
    if candidates.len() <= n {
        return candidates.to_vec();
    }
    (0..n)
        .map(|i| candidates[i * candidates.len() / n].clone())
        .collect()
}

/// Кандидаты в выборку из каталога `dir`: изображения, не в корзине, с превью и с тегами
/// активной модели. Порядок — по sha256 (см. `db::tagged_files`).
pub fn candidates(conn: &rusqlite::Connection, dir: &Path, source: &str) -> Result<Vec<Row>> {
    Ok(db::tagged_files(conn, source)?
        .into_iter()
        .filter(|f| Path::new(&f.path).starts_with(dir))
        .map(|f| Row {
            id: f.id,
            path: f.path,
            sha256: f.sha256,
            thumb: f.thumb,
        })
        .collect())
}

/// Выгрузить выборку: `labels.csv` для разметки, `guesses.csv` с подсказками модели и
/// контактные листы с номерами. Метки из уже существующего `labels.csv` переносятся —
/// так выборку можно перевыгрузить, не потеряв работу.
pub fn export(out_dir: &Path, rows: &[(Row, Vec<(String, f32)>)], thumbs: &Path) -> Result<Export> {
    std::fs::create_dir_all(out_dir)
        .with_context(|| format!("не создать каталог {}", out_dir.display()))?;
    let labels = out_dir.join("labels.csv");
    let guesses = out_dir.join("guesses.csv");
    let old = read_labels(&labels).unwrap_or_default();

    let mut lab = vec![vec!["n".into(), "path".into(), "label".into()]];
    let mut gue = vec![vec![
        "n".into(),
        "path".into(),
        "top1".into(),
        "score1".into(),
        "top2".into(),
        "score2".into(),
        "top3".into(),
        "score3".into(),
    ]];
    for (i, (row, tags)) in rows.iter().enumerate() {
        let n = (i + 1).to_string();
        let kept = old
            .iter()
            .find(|o| o.path == row.path)
            .map(|o| o.as_text())
            .unwrap_or_default();
        lab.push(vec![n.clone(), row.path.clone(), kept]);
        let mut g = vec![n, row.path.clone()];
        for k in 0..3 {
            match tags.get(k) {
                Some((tag, score)) => {
                    g.push(tag.clone());
                    g.push(format!("{score:.3}"));
                }
                None => {
                    g.push(String::new());
                    g.push(String::new());
                }
            }
        }
        gue.push(g);
    }
    write_csv(&labels, &lab)?;
    write_csv(&guesses, &gue)?;

    let mut sheets = Vec::new();
    for (k, chunk) in rows.chunks(SHEET_PER).enumerate() {
        let path = out_dir.join(format!("sheet-{:02}.png", k + 1));
        render_sheet(&path, chunk, k * SHEET_PER, thumbs)?;
        sheets.push(path);
    }
    Ok(Export {
        labels,
        guesses,
        sheets,
        rows: rows.len(),
    })
}

/// Одна метка из `labels.csv`.
#[derive(Debug, Clone, PartialEq)]
pub struct LabelRow {
    pub path: String,
    pub labels: Vec<String>,
    pub other: bool,
    pub skip: bool,
}

impl LabelRow {
    /// Пустая метка — снимок ещё не размечен.
    pub fn unlabelled(&self) -> bool {
        !self.skip && !self.other && self.labels.is_empty()
    }

    /// Обратно в текст для `labels.csv`: нормализованный вид, порядок меток сохранён.
    pub fn as_text(&self) -> String {
        if self.skip {
            return "skip".into();
        }
        if self.other {
            return "other".into();
        }
        self.labels.join("|")
    }
}

/// Разметка одного снимка из строки CSV. Пусто — не размечено, `skip` — пропустить,
/// `other` — ни один из тегов словаря не подходит; несколько допустимых меток — `a|b`.
pub fn parse_label(s: &str) -> LabelRow {
    let raw = s.trim().to_ascii_lowercase();
    let mut row = LabelRow {
        path: String::new(),
        labels: Vec::new(),
        other: false,
        skip: false,
    };
    if raw.is_empty() {
        return row;
    }
    for part in raw.split('|') {
        let p = part.trim();
        match p {
            "" => {}
            "skip" | "пропуск" => row.skip = true,
            "other" | "другое" => row.other = true,
            _ => row.labels.push(p.to_string()),
        }
    }
    if row.other || row.skip {
        // `other|furniture` — противоречие; сильнее пропуск, затем «другое».
        row.labels.clear();
    }
    row
}

/// Прочитать `labels.csv`: `path` во втором столбце, `label` — в последнем.
pub fn read_labels(path: &Path) -> Result<Vec<LabelRow>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("не прочитать {}", path.display()))?;
    let mut rows = Vec::new();
    for (i, fields) in csv_parse(&text).into_iter().enumerate() {
        if i == 0 {
            continue; // шапка
        }
        let Some(file) = fields.get(1) else { continue };
        let mut row = parse_label(fields.get(2).map(String::as_str).unwrap_or(""));
        row.path = file.clone();
        rows.push(row);
    }
    Ok(rows)
}

/// Один размеченный снимок в цифрах.
#[derive(Debug, Default, Clone)]
pub struct TagScore {
    pub label: String,
    pub n: usize,
    pub top1: usize,
    pub top3: usize,
}

/// Сколько размеченных «мебельных» снимков нашёл процентильный фильтр этого тега.
#[derive(Debug, Clone, Copy)]
pub struct PercentileRecall {
    pub pct: i32,
    pub threshold: f32,
    /// Сколько размеченных снимков этого тега всего.
    pub n: usize,
    pub found: usize,
}

/// Итог размеченной проверки.
#[derive(Debug, Default)]
pub struct Score {
    pub rows: usize,
    pub unlabelled: usize,
    pub skipped: usize,
    pub missing: usize,
    pub labelled: usize,
    pub other: usize,
    /// Сколько размеченных снимков допускают больше одной метки словаря.
    pub multi: usize,
    pub top1: usize,
    pub top3: usize,
    /// Снимки, где модель сказала «уверенно» (вес ≥ порога и отрыв от второго).
    pub confident: usize,
    /// Из них с меткой словаря — первый тег угадан.
    pub confident_ok: usize,
    /// Из них без метки словаря («другое») — уверенно, но мимо.
    pub confident_other: usize,
    pub per_tag: Vec<TagScore>,
    pub furniture: Vec<PercentileRecall>,
}

impl Score {
    pub fn top1_share(&self) -> f32 {
        share(self.top1, self.labelled)
    }
    pub fn top3_share(&self) -> f32 {
        share(self.top3, self.labelled)
    }
    /// Точность уверенного тега: доля правильных среди тех, где модель была уверена.
    pub fn confident_precision(&self) -> f32 {
        share(self.confident_ok, self.confident_ok + self.confident_other)
    }
}

fn share(a: usize, b: usize) -> f32 {
    if b == 0 { 0.0 } else { a as f32 / b as f32 }
}

/// Посчитать метрики по разметке и тегам из БД. `tags` — путь → [(метка, вес)] по убыванию.
/// Всё, что нужно для отчёта, считается здесь, без интерфейса и без SQL.
pub fn score(
    labels: &[LabelRow],
    tags: &HashMap<String, Vec<(String, f32)>>,
    defs: &[db::TagDef],
    show: f32,
    margin: f32,
    dists: &HashMap<String, Vec<f32>>,
    percentiles: &[i32],
) -> Result<Score> {
    let mut by_id: HashMap<&str, &str> = HashMap::new();
    for d in defs {
        by_id.insert(d.id.as_str(), d.label.as_str());
    }
    let mut out = Score {
        rows: labels.len(),
        per_tag: defs
            .iter()
            .map(|d| TagScore {
                label: d.label.clone(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    };
    for row in labels {
        if row.skip {
            out.skipped += 1;
            continue;
        }
        if row.unlabelled() {
            out.unlabelled += 1;
            continue;
        }
        let Some(scores) = tags.get(&row.path) else {
            out.missing += 1;
            continue;
        };
        let want: Vec<&str> = row
            .labels
            .iter()
            .map(|id| {
                by_id.get(id.as_str()).copied().ok_or_else(|| {
                    let mut known: Vec<&str> = by_id.keys().copied().collect();
                    known.sort_unstable();
                    anyhow::anyhow!(
                        "unknown label «{id}» in {}; the dictionary has: {}",
                        row.path,
                        known.join(", ")
                    )
                })
            })
            .collect::<Result<_>>()?;
        let top1 = scores.first().map(|(t, _)| t.as_str());
        let top3: Vec<&str> = scores.iter().take(3).map(|(t, _)| t.as_str()).collect();
        let confident = embed::confident_tag_with(scores, show, margin).is_some();
        if row.other {
            out.other += 1;
            if confident {
                out.confident += 1;
                out.confident_other += 1;
            }
            continue;
        }
        out.labelled += 1;
        if want.len() > 1 {
            out.multi += 1;
        }
        let hit1 = top1.is_some_and(|t| want.contains(&t));
        let hit3 = top3.iter().any(|t| want.contains(t));
        if hit1 {
            out.top1 += 1;
        }
        if hit3 {
            out.top3 += 1;
        }
        if confident {
            out.confident += 1;
            if hit1 {
                out.confident_ok += 1;
            }
        }
        for t in &mut out.per_tag {
            if !want.contains(&t.label.as_str()) {
                continue;
            }
            t.n += 1;
            if hit1 {
                t.top1 += 1;
            }
            if hit3 {
                t.top3 += 1;
            }
        }
    }
    // Фильтр по «мебели»: сколько размеченных мебельных снимков он находит.
    let furniture = by_id.get("furniture").copied();
    if let (Some(label), Some(dist)) = (furniture, furniture.and_then(|l| dists.get(l))) {
        let mut found_rows: Vec<(&str, f32)> = Vec::new();
        for row in labels {
            if row.other || row.skip || row.unlabelled() {
                continue;
            }
            let is_furniture = row
                .labels
                .iter()
                .any(|id| by_id.get(id.as_str()).copied() == Some(label));
            if !is_furniture {
                continue;
            }
            let s = tags
                .get(&row.path)
                .and_then(|list| list.iter().find(|(t, _)| t == label))
                .map(|(_, s)| *s)
                .unwrap_or(0.0);
            found_rows.push((row.path.as_str(), s));
        }
        for pct in percentiles {
            if let Some(threshold) = embed::percentile_threshold(dist, *pct) {
                let found = found_rows.iter().filter(|(_, s)| *s >= threshold).count();
                out.furniture.push(PercentileRecall {
                    pct: *pct,
                    threshold,
                    n: found_rows.len(),
                    found,
                });
            }
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ CSV

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

pub fn write_csv(path: &Path, rows: &[Vec<String>]) -> Result<()> {
    let mut out = String::new();
    for r in rows {
        let line: Vec<String> = r.iter().map(|f| csv_field(f)).collect();
        out.push_str(&line.join(","));
        out.push('\n');
    }
    std::fs::write(path, out).with_context(|| format!("не записать {}", path.display()))
}

/// Разбор CSV с кавычками: запятая и перевод строки внутри кавычек — часть поля.
pub fn csv_parse(text: &str) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut field = String::new();
    let mut quoted = false;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' => {
                    if chars.peek() == Some(&'"') {
                        chars.next();
                        field.push('"');
                    } else {
                        quoted = false;
                    }
                }
                _ => field.push(c),
            }
            continue;
        }
        match c {
            '"' => quoted = true,
            ',' => row.push(std::mem::take(&mut field)),
            '\r' => {}
            '\n' => {
                row.push(std::mem::take(&mut field));
                if !(row.len() == 1 && row[0].is_empty()) {
                    rows.push(std::mem::take(&mut row));
                } else {
                    row.clear();
                }
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        rows.push(row);
    }
    rows
}

// ------------------------------------------------------------------ sheets

/// Контактный лист: плитки 320 px в сетке 4×4 с номером снимка в рамке.
/// Подсказок модели на листе нет: разметка должна быть слепой.
fn render_sheet(
    out: &Path,
    rows: &[(Row, Vec<(String, f32)>)],
    first: usize,
    thumbs: &Path,
) -> Result<()> {
    let cols = 4u32;
    let lines = (rows.len().div_ceil(cols as usize)) as u32;
    let w = cols * (TILE + PAD) + PAD;
    let h = lines * (TILE + PAD + HEADER) + PAD;
    let mut sheet = RgbImage::from_pixel(w, h, Rgb([24, 24, 28]));
    for (k, (row, _)) in rows.iter().enumerate() {
        let c = k as u32 % cols;
        let r = k as u32 / cols;
        let x = PAD + c * (TILE + PAD);
        let y = PAD + r * (TILE + PAD + HEADER);
        draw_digits(
            &mut sheet,
            x + 4,
            y + 5,
            first + k + 1,
            3,
            Rgb([255, 220, 120]),
        );
        // Само превью: лежит в кэше, к архиву не обращаемся.
        if let Some(rel) = &row.thumb {
            let p = thumbs.join(rel);
            match image::open(&p) {
                Ok(img) => {
                    let mut img = img.to_rgb8();
                    if img.width() > TILE || img.height() > TILE {
                        img = image::imageops::resize(
                            &img,
                            TILE.min(img.width()),
                            TILE.min(img.height()),
                            image::imageops::FilterType::Triangle,
                        );
                    }
                    let dx = x + (TILE.saturating_sub(img.width())) / 2;
                    let dy = y + HEADER + (TILE.saturating_sub(img.height())) / 2;
                    image::imageops::overlay(&mut sheet, &img, dx as i64, dy as i64);
                }
                Err(e) => {
                    // Битая плитка не рушит лист: номер остаётся, превью — пустое.
                    eprintln!("{}: {e}", p.display());
                }
            }
        }
    }
    sheet
        .save_with_format(out, image::ImageFormat::Png)
        .with_context(|| format!("не записать {}", out.display()))
}

/// Цифры 3×5, увеличенные в `scale` раз: номер на листе должен читаться без шрифта.
fn draw_digits(img: &mut RgbImage, x: u32, y: u32, mut n: usize, scale: u32, color: Rgb<u8>) {
    const GLYPHS: [[u8; 5]; 10] = [
        [0b111, 0b101, 0b101, 0b101, 0b111],
        [0b010, 0b110, 0b010, 0b010, 0b111],
        [0b111, 0b001, 0b111, 0b100, 0b111],
        [0b111, 0b001, 0b111, 0b001, 0b111],
        [0b101, 0b101, 0b111, 0b001, 0b001],
        [0b111, 0b100, 0b111, 0b001, 0b111],
        [0b111, 0b100, 0b111, 0b101, 0b111],
        [0b111, 0b001, 0b010, 0b010, 0b010],
        [0b111, 0b101, 0b111, 0b101, 0b111],
        [0b111, 0b101, 0b111, 0b001, 0b111],
    ];
    let mut digits = Vec::new();
    loop {
        digits.push(n % 10);
        n /= 10;
        if n == 0 {
            break;
        }
    }
    digits.reverse();
    let mut cx = x;
    for d in digits {
        let glyph = GLYPHS[d];
        for (row, bits) in glyph.iter().enumerate() {
            for col in 0..3u32 {
                if bits & (0b100 >> col) == 0 {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        let px = cx + col * scale + dx;
                        let py = y + row as u32 * scale + dy;
                        if px < img.width() && py < img.height() {
                            img.put_pixel(px, py, color);
                        }
                    }
                }
            }
        }
        cx += 4 * scale;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defs() -> Vec<db::TagDef> {
        ["furniture", "interior", "document", "food"]
            .iter()
            .enumerate()
            .map(|(i, id)| db::TagDef {
                model: "m".into(),
                id: (*id).into(),
                label: format!("{id}-label"),
                prompts: Vec::new(),
                enabled: true,
                position: i as i64,
                min_percentile: 0,
            })
            .collect()
    }

    fn tags(list: &[(&str, &[(&str, f32)])]) -> HashMap<String, Vec<(String, f32)>> {
        list.iter()
            .map(|(p, t)| {
                (
                    (*p).to_string(),
                    t.iter().map(|(a, b)| ((*a).to_string(), *b)).collect(),
                )
            })
            .collect()
    }

    fn label(path: &str, s: &str) -> LabelRow {
        let mut r = parse_label(s);
        r.path = path.into();
        r
    }

    fn row(n: usize) -> Row {
        Row {
            id: n as i64,
            path: format!("d/{n}.jpg"),
            sha256: format!("{n:064x}"),
            thumb: None,
        }
    }

    #[test]
    fn the_sample_is_even_and_repeatable() {
        let all: Vec<Row> = (0..100).map(row).collect();
        let picked = pick(&all, 10);
        assert_eq!(picked.len(), 10);
        let ids: Vec<i64> = picked.iter().map(|r| r.id).collect();
        assert_eq!(ids, vec![0, 10, 20, 30, 40, 50, 60, 70, 80, 90]);
        assert_eq!(pick(&all, 10), pick(&all, 10));
        assert_eq!(pick(&all, 0).len(), 0);
        assert_eq!(pick(&all[..5], 10).len(), 5);
        assert_eq!(pick(&[], 10).len(), 0);
    }

    #[test]
    fn the_labels_read_empty_skip_and_several_labels() {
        assert!(parse_label("").unlabelled());
        assert!(parse_label("  ").unlabelled());
        assert!(parse_label("skip").skip);
        assert!(parse_label("other").other);
        let two = parse_label("Furniture | interior");
        assert_eq!(two.labels, vec!["furniture", "interior"]);
        assert!(!two.other && !two.skip);
        // Противоречие «другое + метка» решается в пользу «другого».
        let odd = parse_label("other|furniture");
        assert!(odd.other);
        assert!(odd.labels.is_empty());
    }

    #[test]
    fn the_csv_survives_commas_and_quotes() {
        let rows = vec![
            vec!["n".to_string(), "path".to_string(), "label".to_string()],
            vec!["1".into(), "a,\"b\".jpg".into(), "other".into()],
        ];
        let dir = std::env::temp_dir().join("photocat-eval-csv");
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("labels.csv");
        write_csv(&p, &rows).unwrap();
        let back = read_labels(&p).unwrap();
        assert_eq!(back.len(), 1);
        assert_eq!(back[0].path, "a,\"b\".jpg");
        assert!(back[0].other);
    }

    #[test]
    fn the_metrics_count_top1_top3_and_the_confident_mistake() {
        let d = defs();
        let t = tags(&[
            (
                "ok.jpg",
                &[("furniture-label", 0.30), ("interior-label", 0.10)],
            ),
            (
                "near.jpg",
                &[("interior-label", 0.29), ("furniture-label", 0.28)],
            ),
            ("bad.jpg", &[("document-label", 0.31), ("food-label", 0.10)]),
            ("odd.jpg", &[("food-label", 0.25), ("document-label", 0.10)]),
        ]);
        let labels = vec![
            label("ok.jpg", "furniture"),
            label("near.jpg", "furniture"),
            label("bad.jpg", "other"),
            label("odd.jpg", "other"),
            label("skipped.jpg", "skip"),
            label("empty.jpg", ""),
            label("gone.jpg", "furniture"),
        ];
        let s = score(&labels, &t, &d, 0.22, 0.01, &HashMap::new(), &[]).unwrap();
        assert_eq!(s.rows, 7);
        assert_eq!(s.labelled, 2);
        assert_eq!(s.other, 2);
        assert_eq!(s.skipped, 1);
        assert_eq!(s.unlabelled, 1);
        assert_eq!(s.missing, 1);
        assert_eq!(s.multi, 0);
        assert_eq!(s.top1, 1, "near.jpg is top-2, not top-1");
        assert_eq!(s.top3, 2);
        // ok.jpg — уверенно и в цель; bad.jpg и odd.jpg — уверенно мимо («другое»);
        // near.jpg при отрыве 0.01 не считается уверенным — ровно на пороге.
        assert_eq!(s.confident, 3);
        assert_eq!(s.confident_ok, 1);
        assert_eq!(s.confident_other, 2);
        assert!((s.confident_precision() - 1.0 / 3.0).abs() < 1e-6);
        let fur = s
            .per_tag
            .iter()
            .find(|t| t.label == "furniture-label")
            .unwrap();
        assert_eq!((fur.n, fur.top1, fur.top3), (2, 1, 2));
    }

    #[test]
    fn several_allowed_labels_count_as_a_hit() {
        let d = defs();
        let t = tags(&[("a.jpg", &[("interior-label", 0.30), ("food-label", 0.10)])]);
        let s = score(
            &[label("a.jpg", "furniture|interior")],
            &t,
            &d,
            0.22,
            0.01,
            &HashMap::new(),
            &[],
        )
        .unwrap();
        assert_eq!((s.labelled, s.top1, s.top3), (1, 1, 1));
        assert_eq!(s.multi, 1);
        assert_eq!(s.per_tag.iter().filter(|t| t.n > 0).count(), 2);
    }

    #[test]
    fn an_unknown_label_stops_the_count() {
        let err = score(
            &[label("a.jpg", "unicorn")],
            &tags(&[("a.jpg", &[("food-label", 0.3)])]),
            &defs(),
            0.22,
            0.01,
            &HashMap::new(),
            &[],
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("unicorn"), "{err}");
        assert!(err.contains("furniture"), "{err}");
    }

    #[test]
    fn the_furniture_percentile_counts_the_found_shots() {
        let d = defs();
        let t = tags(&[
            ("a.jpg", &[("furniture-label", 0.40)]),
            ("b.jpg", &[("furniture-label", 0.20)]),
        ]);
        let mut dist: Vec<f32> = vec![0.4, 0.35, 0.3, 0.25, 0.2];
        dist.sort_by(|a, b| b.total_cmp(a));
        let mut dists = HashMap::new();
        dists.insert("furniture-label".to_string(), dist);
        let s = score(
            &[label("a.jpg", "furniture"), label("b.jpg", "furniture")],
            &t,
            &d,
            0.22,
            0.01,
            &dists,
            &[100, 70],
        )
        .unwrap();
        assert_eq!(s.furniture.len(), 2);
        assert_eq!(s.furniture[0].n, 2);
        // 100 % — только самый сильный снимок каталога (0.40): a.jpg.
        assert_eq!(s.furniture[0].pct, 100);
        assert_eq!(s.furniture[0].found, 1);
        // 70 % — верхние 30 % (ceil(1.5)=2) → порог 0.35, по-прежнему a.jpg.
        assert_eq!(s.furniture[1].pct, 70);
        assert_eq!(s.furniture[1].found, 1);
    }

    #[test]
    fn the_number_is_drawn_within_the_sheet() {
        let mut img = RgbImage::from_pixel(20, 20, Rgb([0, 0, 0]));
        draw_digits(&mut img, 0, 0, 101, 3, Rgb([255, 255, 255]));
        // Три цифры по 4*3 px — 36 px: за границей листа пиксель не появляется.
        assert_eq!(img.width(), 20);
        let lit = img.pixels().filter(|p| p[0] > 0).count();
        assert!(lit > 0 && lit < 20 * 20);
    }
}
