//! Поиск дублей: точные (sha256) и похожие снимки (расстояние Хэмминга по dHash),
//! совет «что оставить» и экспорт плана решения. Оригиналы не трогаем.

use crate::db::{self, FileRow};
use crate::i18n::{self, Lang};
use crate::{flag_num, flag_str, has_flag};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

/// Почему файлы попали в одну группу. Метка переводится на месте (M21.6):
/// сам признак — данные, слова — отчёт.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Reason {
    Exact,
    Similar,
}

impl Reason {
    pub fn label(self, lang: Lang) -> &'static str {
        i18n::t(
            lang,
            match self {
                Reason::Exact => "точная копия",
                Reason::Similar => "похожий снимок",
            },
        )
    }
}

/// Из чего сложился совет «что оставить»: числа и признаки без слов, чтобы отчёт
/// и интерфейс переводились, а не хранили готовую русскую фразу (M21.6).
#[derive(Clone, PartialEq, Debug)]
pub enum AdvicePart {
    SharperBy(f64),
    Sharper,
    MorePixels,
    BiggerFile,
}

#[derive(Clone, PartialEq, Debug, Default)]
pub struct Advice {
    /// Все файлы — копии одного содержимого: выбирать нечего.
    pub same_content: bool,
    pub parts: Vec<AdvicePart>,
}

impl Advice {
    /// Совет словами на языке отчёта: части соединяются точкой-разделителем.
    pub fn label(&self, lang: Lang) -> String {
        if self.same_content {
            return i18n::t(lang, "точные копии — разницы нет").to_string();
        }
        if self.parts.is_empty() {
            return i18n::t(lang, "отличия небольшие — выбирайте глазами").to_string();
        }
        self.parts
            .iter()
            .map(|p| match p {
                AdvicePart::SharperBy(pct) => i18n::fill(
                    lang,
                    "резче остальных на {}%",
                    &[format!("{pct:.0}")],
                ),
                AdvicePart::Sharper => i18n::t(lang, "резче остальных").to_string(),
                AdvicePart::MorePixels => i18n::t(lang, "больше по разрешению").to_string(),
                AdvicePart::BiggerFile => i18n::t(lang, "крупнее файл").to_string(),
            })
            .collect::<Vec<_>>()
            .join(" · ")
    }
}

pub struct Group {
    pub members: Vec<usize>,
    pub waste: i64,
    pub reason: Reason,
    /// Индекс в `rows`: кадр, который советуем оставить.
    pub suggested: usize,
    /// Почему именно он — признаки, слова собираются на языке отчёта.
    pub advice: Advice,
}

/// Что уже решено по найденным группам.
#[derive(Default)]
pub struct Summary {
    pub groups: usize,
    pub marked_drop: usize,
    pub freed: i64,
}

struct Uf {
    p: Vec<usize>,
}

impl Uf {
    fn new(n: usize) -> Self {
        Self { p: (0..n).collect() }
    }
    fn find(&mut self, mut a: usize) -> usize {
        while self.p[a] != a {
            self.p[a] = self.p[self.p[a]];
            a = self.p[a];
        }
        a
    }
    fn union(&mut self, a: usize, b: usize) {
        let (ra, rb) = (self.find(a), self.find(b));
        if ra != rb {
            self.p[ra] = rb;
        }
    }
}

/// Выше этого числа хешей попарный перебор заменяется индексом по блокам: на 2230
/// файлах перебор — 0.03 с, а на 20 тыс. это уже ~200 млн сравнений. Порог — не
/// магия в цикле, а именованная константа.
const INDEX_MIN: usize = 20_000;

/// Блоков по 8 бит. Если расстояние Хэмминга меньше числа блоков, хотя бы один
/// блок совпадает целиком — это и есть точный фильтр кандидатов (принцип ящиков).
/// Восемь блоков дают гарантию для радиуса 0–7; больше — попарный перебор.
const INDEX_BLOCKS: usize = 8;
const INDEX_MAX_RADIUS: u32 = INDEX_BLOCKS as u32 - 1;

fn chunk(value: u64, block: usize) -> u8 {
    (value >> (8 * block)) as u8
}

/// Кластеры: сначала склеиваем точные копии по sha256, затем изображения по dHash.
pub fn find_groups(rows: &[FileRow], max_dist: u32) -> Vec<Group> {
    find_groups_with(rows, max_dist, INDEX_MIN)
}

/// Общая реализация с явным порогом: тесты сравнивают индекс и перебор на одних
/// данных, передавая 0 или `usize::MAX`.
fn find_groups_with(rows: &[FileRow], max_dist: u32, index_from: usize) -> Vec<Group> {
    let mut uf = Uf::new(rows.len());

    let mut by_sha: HashMap<&str, usize> = HashMap::new();
    for (i, r) in rows.iter().enumerate() {
        if r.sha256.is_empty() {
            continue;
        }
        if let Some(&j) = by_sha.get(r.sha256.as_str()) {
            uf.union(i, j);
        } else {
            by_sha.insert(&r.sha256, i);
        }
    }

    let hashed: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, r)| r.dhash.is_some())
        .map(|(i, _)| i)
        .collect();
    let use_index = hashed.len() >= index_from && max_dist <= INDEX_MAX_RADIUS;
    if use_index {
        union_similar_indexed(rows, &hashed, max_dist, &mut uf);
    } else {
        union_similar_pairwise(rows, &hashed, max_dist, &mut uf);
    }

    let mut buckets: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..rows.len() {
        let r = uf.find(i);
        buckets.entry(r).or_default().push(i);
    }

    let mut groups: Vec<Group> = buckets
        .into_values()
        .filter(|v| v.len() > 1)
        .map(|mut v| {
            v.sort_by_key(|&i| std::cmp::Reverse(rows[i].size));
            let total: i64 = v.iter().map(|&i| rows[i].size).sum();
            let biggest = rows[v[0]].size;
            let same_content = v.iter().all(|&i| rows[i].sha256 == rows[v[0]].sha256);
            let suggested = pick_best(rows, &v);
            let advice = advice_of(rows, &v, suggested);
            Group {
                waste: total - biggest,
                members: v,
                reason: if same_content {
                    Reason::Exact
                } else {
                    Reason::Similar
                },
                suggested,
                advice,
            }
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.waste));
    groups
}

/// Прежний попарный перебор: O(n²), ниже порога он дешевле индекса.
fn union_similar_pairwise(rows: &[FileRow], hashed: &[usize], max_dist: u32, uf: &mut Uf) {
    for (a, &i) in hashed.iter().enumerate() {
        let hi = rows[i].dhash.unwrap() as u64;
        for &j in &hashed[a + 1..] {
            let hj = rows[j].dhash.unwrap() as u64;
            if crate::derive::hamming(hi, hj) <= max_dist {
                uf.union(i, j);
            }
        }
    }
}

/// То же через многоблочный индекс по 8-битным кускам dHash. Для радиуса ≤ 7
/// совпадение хотя бы одного блока обязательно, поэтому кандидатов в разы меньше,
/// чем файлов; полное расстояние Хэмминга всё равно проверяется по u64.
/// Метрическое дерево (BK-tree) здесь не спасает: на 64-битных хешах при радиусе 6
/// почти все точки лежат на расстоянии ~32, и неравенство треугольника не отсекает
/// ничего — на 50 тыс. это 17.2 с вместо долей секунды (ADR 0038).
fn union_similar_indexed(rows: &[FileRow], hashed: &[usize], max_dist: u32, uf: &mut Uf) {
    // Один запрос на уникальный dHash: одинаковые кадры (чёрные кадры видео,
    // серии) не должны давать n² проверок сами с собой. Уникальным значениям
    // даём плотные номера — дальше дедупликация кандидатов массивом меток.
    let mut index: HashMap<u64, usize> = HashMap::new();
    let mut values: Vec<u64> = Vec::new();
    let mut groups: Vec<Vec<usize>> = Vec::new();
    for &i in hashed {
        let value = rows[i].dhash.unwrap() as u64;
        let uid = *index.entry(value).or_insert_with(|| {
            values.push(value);
            groups.push(Vec::new());
            values.len() - 1
        });
        groups[uid].push(i);
    }
    for ids in &groups {
        for &i in &ids[1..] {
            uf.union(ids[0], i);
        }
    }
    let mut buckets: Vec<HashMap<u8, Vec<usize>>> =
        (0..INDEX_BLOCKS).map(|_| HashMap::new()).collect();
    for (uid, &value) in values.iter().enumerate() {
        for (block, map) in buckets.iter_mut().enumerate() {
            map.entry(chunk(value, block)).or_default().push(uid);
        }
    }
    let mut seen = vec![0u32; values.len()];
    let mut stamp = 0u32;
    let mut candidates: Vec<usize> = Vec::new();
    for (uid, &value) in values.iter().enumerate() {
        stamp += 1;
        candidates.clear();
        for (block, map) in buckets.iter().enumerate() {
            let Some(list) = map.get(&chunk(value, block)) else {
                continue;
            };
            for &other in list {
                if seen[other] != stamp {
                    seen[other] = stamp;
                    candidates.push(other);
                }
            }
        }
        for &other in &candidates {
            if other != uid && crate::derive::hamming(value, values[other]) <= max_dist {
                for &j in &groups[other] {
                    uf.union(groups[uid][0], j);
                }
            }
        }
    }
}

fn sharpness_of(r: &FileRow) -> f64 {
    r.sharpness.unwrap_or(0.0)
}

fn pixels_of(r: &FileRow) -> i64 {
    r.width.max(0) * r.height.max(0)
}

/// Лучший кадр: резкость → разрешение → размер файла. При равенстве — любой.
fn pick_best(rows: &[FileRow], members: &[usize]) -> usize {
    *members
        .iter()
        .max_by(|a, b| {
            let a = **a;
            let b = **b;
            sharpness_of(&rows[a])
                .partial_cmp(&sharpness_of(&rows[b]))
                .unwrap_or(std::cmp::Ordering::Equal)
                .then(pixels_of(&rows[a]).cmp(&pixels_of(&rows[b])))
                .then(rows[a].size.cmp(&rows[b].size))
        })
        .unwrap_or(&members[0])
}

/// Совет: почему этот кадр лучше остальных (без слов — слова собирает `Advice::label`).
fn advice_of(rows: &[FileRow], members: &[usize], best: usize) -> Advice {
    let same_content = !rows[best].sha256.is_empty()
        && members.iter().all(|&i| rows[i].sha256 == rows[best].sha256);
    if same_content {
        return Advice {
            same_content: true,
            parts: Vec::new(),
        };
    }
    let mut parts: Vec<AdvicePart> = Vec::new();
    let best_sharp = sharpness_of(&rows[best]);
    let other_sharp = members
        .iter()
        .copied()
        .filter(|&i| i != best)
        .map(|i| sharpness_of(&rows[i]))
        .fold(0.0_f64, f64::max);
    if best_sharp > 0.0 && other_sharp > 0.0 {
        let pct = (best_sharp / other_sharp - 1.0) * 100.0;
        if pct >= 3.0 {
            parts.push(AdvicePart::SharperBy(pct));
        } else if pct > 0.0 {
            parts.push(AdvicePart::Sharper);
        }
    }
    let best_px = pixels_of(&rows[best]);
    let max_px = members.iter().map(|&i| pixels_of(&rows[i])).max().unwrap_or(0);
    if best_px > 0
        && best_px == max_px
        && members.iter().filter(|&&i| pixels_of(&rows[i]) == max_px).count() == 1
    {
        parts.push(AdvicePart::MorePixels);
    }
    let max_size = members.iter().map(|&i| rows[i].size).max().unwrap_or(0);
    if max_size > 0
        && rows[best].size == max_size
        && members.iter().filter(|&&i| rows[i].size == max_size).count() == 1
    {
        parts.push(AdvicePart::BiggerFile);
    }
    Advice {
        same_content: false,
        parts,
    }
}

/// Кадр, который считается оставленным: решение владельца, иначе совет.
pub fn keep_of(rows: &[FileRow], g: &Group) -> usize {
    g.members
        .iter()
        .copied()
        .find(|&i| rows[i].decision.as_deref() == Some("keep"))
        .unwrap_or(g.suggested)
}

/// Счётчик для панели: сколько помечено лишним и сколько байт это освободит.
pub fn summary(rows: &[FileRow], groups: &[Group]) -> Summary {
    let mut s = Summary {
        groups: groups.len(),
        ..Default::default()
    };
    for g in groups {
        for &i in &g.members {
            if rows[i].decision.as_deref() == Some("drop") {
                s.marked_drop += 1;
                s.freed += rows[i].size;
            }
        }
    }
    s
}

/// План решения без удаления: одна строка на каждый «лишний» файл.
/// `wasted_bytes` — сколько освободится, если убрать все помеченные файлы группы.
pub fn plan_csv(rows: &[FileRow], groups: &[Group], lang: Lang) -> String {
    let mut s = String::from("group,keep_path,drop_path,reason,wasted_bytes\n");
    for (n, g) in groups.iter().enumerate() {
        let keep = keep_of(rows, g);
        let wasted: i64 = g
            .members
            .iter()
            .filter(|&&i| i != keep)
            .map(|&i| rows[i].size)
            .sum();
        for &i in &g.members {
            if i == keep {
                continue;
            }
            s.push_str(&format!(
                "{},{},{},{},{}\n",
                n + 1,
                csv_escape(&rows[keep].path),
                csv_escape(&rows[i].path),
                csv_escape(g.reason.label(lang)),
                wasted
            ));
        }
    }
    s
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn short_name(path: &str) -> &str {
    std::path::Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or(path)
}

pub fn cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let max_dist = flag_num(args, "--max-dist", 6) as u32;
    let csv = flag_str(args, "--csv");
    let plan = flag_str(args, "--plan");
    let quiet = has_flag(args, "--quiet");

    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    let lang = db::load_settings(&conn).lang;
    let rows = db::load_all(&conn)?;
    let groups = find_groups(&rows, max_dist);

    let waste: i64 = groups.iter().map(|g| g.waste).sum();
    let files: usize = groups.iter().map(|g| g.members.len()).sum();
    println!(
        "{}",
        i18n::fill(
            lang,
            "групп: {}, файлов в них: {}, потенциально лишнего: {}",
            &[
                groups.len().to_string(),
                files.to_string(),
                db::human_in(lang, waste),
            ]
        )
    );
    if !quiet {
        let sum = summary(&rows, &groups);
        if sum.marked_drop > 0 {
            println!(
                "{}",
                i18n::fill(
                    lang,
                    "помечено лишними: {}, освободится {}",
                    &[sum.marked_drop.to_string(), db::human_in(lang, sum.freed)]
                )
            );
        }
        for (n, g) in groups.iter().enumerate().take(200) {
            let header = i18n::fill(
                lang,
                "#{} [{}] файлов: {}, дублей на {} · совет: {} — {}",
                &[
                    (n + 1).to_string(),
                    g.reason.label(lang).to_string(),
                    g.members.len().to_string(),
                    db::human_in(lang, g.waste),
                    short_name(&rows[keep_of(&rows, g)].path).to_string(),
                    g.advice.label(lang),
                ],
            );
            println!("\n{header}");
            let base = rows[g.members[0]].dhash.map(|v| v as u64);
            let keep = keep_of(&rows, g);
            for &i in &g.members {
                let r = &rows[i];
                let dist = match (base, r.dhash) {
                    (Some(a), Some(b)) => crate::derive::hamming(a, b as u64),
                    _ => 0,
                };
                println!(
                    "   {:>10}  {}  Δ{:<2}  {:<7} {}",
                    db::human_in(lang, r.size),
                    r.taken_at.clone().unwrap_or_else(|| "-".into()),
                    dist,
                    match r.decision.as_deref() {
                        Some("keep") => i18n::t(lang, "оставить"),
                        Some("drop") => i18n::t(lang, "лишний"),
                        _ if i == keep => i18n::t(lang, "оставить"),
                        _ => "",
                    },
                    r.path
                );
            }
        }
        if groups.len() > 200 {
            println!(
                "\n{}",
                i18n::fill(
                    lang,
                    "… и ещё {} групп (полный список: --csv)",
                    &[(groups.len() - 200).to_string()]
                )
            );
        }
    }
    if let Some(path) = csv {
        let mut s =
            String::from("group,reason,hamming,wasted,size,sharpness,decision,path\n");
        for (n, g) in groups.iter().enumerate() {
            let base = rows[g.members[0]].dhash.map(|v| v as u64);
            for &i in &g.members {
                let r = &rows[i];
                let d = match (base, r.dhash) {
                    (Some(a), Some(b)) => crate::derive::hamming(a, b as u64).to_string(),
                    _ => "".into(),
                };
                s.push_str(&format!(
                    "{},{},{},{},{},{},{},{}\n",
                    n + 1,
                    g.reason.label(lang),
                    d,
                    g.waste,
                    r.size,
                    r.sharpness.map(|v| format!("{v:.1}")).unwrap_or_default(),
                    r.decision.as_deref().unwrap_or(""),
                    csv_escape(&r.path)
                ));
            }
        }
        std::fs::write(&path, s)?;
        println!(
            "{}",
            i18n::fill(lang, "полный список: {}", std::slice::from_ref(&path))
        );
    }
    if let Some(path) = plan {
        std::fs::write(&path, plan_csv(&rows, &groups, lang))?;
        println!(
            "{}",
            i18n::fill(lang, "план решения: {}", std::slice::from_ref(&path))
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(path: &str, sha: &str, size: i64, sharp: f64, px: i64) -> FileRow {
        FileRow {
            id: 0,
            path: path.into(),
            size,
            mtime: 0,
            sha256: sha.into(),
            kind: "image".into(),
            width: px,
            height: px,
            taken_at: None,
            taken_src: "exif".into(),
            lat: None,
            lon: None,
            camera: None,
            dhash: Some(1),
            sharpness: Some(sharp),
            duration_s: None,
            thumb: None,
            decision: None,
            trashed_at: None,
        }
    }

    fn one_group(rows: &[FileRow]) -> Group {
        let members: Vec<usize> = (0..rows.len()).collect();
        let suggested = pick_best(rows, &members);
        Group {
            waste: 0,
            advice: advice_of(rows, &members, suggested),
            members,
            reason: Reason::Similar,
            suggested,
        }
    }

    #[test]
    fn advice_prefers_sharpest_frame() {
        let rows = vec![
            row("/a.jpg", "aa", 100, 10.0, 100),
            row("/b.jpg", "bb", 100, 50.0, 100),
            row("/c.jpg", "cc", 100, 20.0, 100),
        ];
        let g = one_group(&rows);
        assert_eq!(rows[g.suggested].path, "/b.jpg");
        let advice = g.advice.label(Lang::Ru);
        assert!(advice.contains("резче остальных на 150%"), "{advice}");
    }

    #[test]
    fn exact_copies_have_no_winner() {
        let rows = vec![
            row("/a.jpg", "same", 100, 10.0, 100),
            row("/b.jpg", "same", 100, 10.0, 100),
        ];
        let g = one_group(&rows);
        assert!(g.advice.same_content);
        assert_eq!(g.advice.label(Lang::Ru), "точные копии — разницы нет");
    }

    /// Нормализованное разбиение на группы: сравнение «тот же набор групп».
    fn partition(groups: &[Group]) -> Vec<Vec<usize>> {
        let mut p: Vec<Vec<usize>> = groups
            .iter()
            .map(|g| {
                let mut members = g.members.clone();
                members.sort_unstable();
                members
            })
            .collect();
        p.sort();
        p
    }

    /// Детерминированный xorshift: тесты не зависят от внешнего `rand`.
    fn random_rows(n: usize, seed: u64) -> Vec<FileRow> {
        let mut x = seed | 1;
        (0..n)
            .map(|i| {
                x ^= x << 13;
                x ^= x >> 7;
                x ^= x << 17;
                let mut r = row(&format!("/r{i}.jpg"), &format!("sha{i}"), 100, 1.0, 100);
                r.dhash = Some(x as i64);
                r
            })
            .collect()
    }

    /// M11: индекс даёт ровно те же группы, что и попарный перебор, на случайных
    /// наборах и на краевых случаях. Порог не влияет на результат, только на способ.
    #[test]
    fn индекс_и_перебор_дают_одинаковые_группы() {
        // Пусто, один элемент, все одинаковые, все далёкие.
        for rows in [
            Vec::new(),
            random_rows(1, 1),
            (0..25)
                .map(|i| row(&format!("/s{i}.jpg"), &format!("s{i}"), 100, 1.0, 100))
                .collect::<Vec<_>>(),
            (0..25)
                .map(|i| {
                    let mut r = row(&format!("/f{i}.jpg"), &format!("f{i}"), 100, 1.0, 100);
                    r.dhash = Some((i as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15) as i64);
                    r
                })
                .collect::<Vec<_>>(),
        ] {
            for max_dist in [0, 1, 6, 10] {
                let tree = partition(&find_groups_with(&rows, max_dist, 0));
                let brute = partition(&find_groups_with(&rows, max_dist, usize::MAX));
                assert_eq!(tree, brute, "max_dist {max_dist}, {} строк", rows.len());
            }
        }
        // Случайные наборы: разных размеров и порогов, с одинаковыми хешами тоже.
        for (seed, n) in [(1, 8), (2, 40), (3, 80), (4, 160)] {
            let mut rows = random_rows(n, seed);
            for k in (0..n).step_by(17) {
                rows[k].dhash = rows[0].dhash;
            }
            for max_dist in 0..=10 {
                let tree = partition(&find_groups_with(&rows, max_dist, 0));
                let brute = partition(&find_groups_with(&rows, max_dist, usize::MAX));
                assert_eq!(tree, brute, "seed {seed}, max_dist {max_dist}");
            }
        }
    }

    /// M11: точные копии по sha256 склеиваются независимо от dHash и способа поиска.
    #[test]
    fn sha_копии_склеиваются_и_через_индекс() {
        let mut rows = random_rows(4, 7);
        rows[0].sha256 = "same".into();
        rows[3].sha256 = "same".into();
        rows[0].dhash = Some(0);
        rows[3].dhash = Some(i64::MAX);
        for threshold in [0, usize::MAX] {
            let groups = find_groups_with(&rows, 6, threshold);
            assert_eq!(groups.len(), 1, "threshold {threshold}");
            let mut members = groups[0].members.clone();
            members.sort_unstable();
            assert_eq!(members, vec![0, 3]);
        }
    }

    /// M11: замер на синтетике — 20 тыс. и 50 тыс. хешей, `max_dist 6`. Гонять
    /// release-сборкой: `cargo test --release --bin photocat синтетике -- --ignored --nocapture`.
    #[test]
    #[ignore = "замер: 20k/50k синтетических хешей, нужен release"]
    fn индекс_замер_на_синтетике() {
        fn peak_rss() -> String {
            std::fs::read_to_string("/proc/self/status")
                .ok()
                .and_then(|s| {
                    s.lines()
                        .find(|l| l.starts_with("VmHWM:"))
                        .map(str::to_string)
                })
                .unwrap_or_else(|| "VmHWM недоступен".into())
        }
        for n in [20_000usize, 50_000] {
            // Половина хешей — почти копия соседа: так в наборе есть группы, а не
            // только далёкие пары, и запрос не вырождается в пустой.
            let mut rows = random_rows(n, 42);
            for i in (1..n).step_by(2) {
                let base = rows[i - 1].dhash.unwrap() as u64;
                rows[i].dhash = Some((base ^ (1 << (i % 64))) as i64);
            }
            let t = std::time::Instant::now();
            let groups = find_groups(&rows, 6);
            let elapsed = t.elapsed();
            let files: usize = groups.iter().map(|g| g.members.len()).sum();
            println!(
                "{n} хешей: групп {}, файлов в них {files}, {:.3} с; {}",
                groups.len(),
                elapsed.as_secs_f64(),
                peak_rss()
            );
            assert!(elapsed.as_secs_f64() < 5.0, "50k должны укладываться в 5 с");
        }
    }

    #[test]
    fn plan_uses_owner_decision_and_counts_waste() {
        let mut rows = vec![
            row("/a.jpg", "aa", 100, 10.0, 100),
            row("/b.jpg", "bb", 200, 50.0, 100),
            row("/c.jpg", "cc", 300, 20.0, 100),
        ];
        let g = one_group(&rows);
        rows[0].decision = Some("keep".into());
        rows[1].decision = Some("drop".into());
        rows[2].decision = Some("drop".into());
        let plan = plan_csv(&rows, &[g], Lang::Ru);
        let lines: Vec<&str> = plan.lines().collect();
        assert_eq!(lines[0], "group,keep_path,drop_path,reason,wasted_bytes");
        assert_eq!(lines.len(), 3);
        assert!(lines[1].contains("/a.jpg"));
        assert!(lines[1].contains("/b.jpg"));
        assert!(lines[1].ends_with(",500"), "{}", lines[1]);
        let sum = summary(&rows, &[one_group(&rows)]);
        assert_eq!(sum.marked_drop, 2);
        assert_eq!(sum.freed, 500);
    }
}
