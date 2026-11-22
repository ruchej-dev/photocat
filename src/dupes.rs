//! Поиск дублей: точные (sha256) и похожие снимки (расстояние Хэмминга по dHash),
//! совет «что оставить» и экспорт плана решения. Оригиналы не трогаем.

use crate::db::{self, FileRow};
use crate::{flag_num, flag_str, has_flag};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Group {
    pub members: Vec<usize>,
    pub waste: i64,
    pub reason: &'static str,
    /// Индекс в `rows`: кадр, который советуем оставить.
    pub suggested: usize,
    /// Почему именно он — словами, для интерфейса и отчёта.
    pub advice: String,
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

/// Кластеры: сначала склеиваем точные копии по sha256, затем изображения по dHash.
pub fn find_groups(rows: &[FileRow], max_dist: u32) -> Vec<Group> {
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
    for (a, &i) in hashed.iter().enumerate() {
        let hi = rows[i].dhash.unwrap() as u64;
        for &j in &hashed[a + 1..] {
            let hj = rows[j].dhash.unwrap() as u64;
            if crate::derive::hamming(hi, hj) <= max_dist {
                uf.union(i, j);
            }
        }
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
                reason: if same_content { "точная копия" } else { "похожий снимок" },
                suggested,
                advice,
            }
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.waste));
    groups
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

/// Совет словами: почему этот кадр лучше остальных.
fn advice_of(rows: &[FileRow], members: &[usize], best: usize) -> String {
    let same_content = !rows[best].sha256.is_empty()
        && members.iter().all(|&i| rows[i].sha256 == rows[best].sha256);
    if same_content {
        return "точные копии — разницы нет".into();
    }
    let mut parts: Vec<String> = Vec::new();
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
            parts.push(format!("резче остальных на {pct:.0}%"));
        } else if pct > 0.0 {
            parts.push("резче остальных".into());
        }
    }
    let best_px = pixels_of(&rows[best]);
    let max_px = members.iter().map(|&i| pixels_of(&rows[i])).max().unwrap_or(0);
    if best_px > 0
        && best_px == max_px
        && members.iter().filter(|&&i| pixels_of(&rows[i]) == max_px).count() == 1
    {
        parts.push("больше по разрешению".into());
    }
    let max_size = members.iter().map(|&i| rows[i].size).max().unwrap_or(0);
    if max_size > 0
        && rows[best].size == max_size
        && members.iter().filter(|&&i| rows[i].size == max_size).count() == 1
    {
        parts.push("крупнее файл".into());
    }
    if parts.is_empty() {
        "отличия небольшие — выбирайте глазами".into()
    } else {
        parts.join(" · ")
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
pub fn plan_csv(rows: &[FileRow], groups: &[Group]) -> String {
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
                csv_escape(g.reason),
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
    let rows = db::load_all(&conn)?;
    let groups = find_groups(&rows, max_dist);

    let waste: i64 = groups.iter().map(|g| g.waste).sum();
    let files: usize = groups.iter().map(|g| g.members.len()).sum();
    println!(
        "групп: {}, файлов в них: {}, потенциально лишнего: {}",
        groups.len(),
        files,
        db::human(waste)
    );
    if !quiet {
        let sum = summary(&rows, &groups);
        if sum.marked_drop > 0 {
            println!(
                "помечено лишними: {}, освободится {}",
                sum.marked_drop,
                db::human(sum.freed)
            );
        }
        for (n, g) in groups.iter().enumerate().take(200) {
            println!(
                "\n#{} [{}] файлов: {}, дублей на {} · совет: {}{}",
                n + 1,
                g.reason,
                g.members.len(),
                db::human(g.waste),
                short_name(&rows[keep_of(&rows, g)].path),
                if g.advice.is_empty() {
                    String::new()
                } else {
                    format!(" — {}", g.advice)
                }
            );
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
                    db::human(r.size),
                    r.taken_at.clone().unwrap_or_else(|| "-".into()),
                    dist,
                    match r.decision.as_deref() {
                        Some("keep") => "оставить",
                        Some("drop") => "лишний",
                        _ if i == keep => "оставить",
                        _ => "",
                    },
                    r.path
                );
            }
        }
        if groups.len() > 200 {
            println!("\n… и ещё {} групп (полный список: --csv)", groups.len() - 200);
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
                    g.reason,
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
        println!("полный список: {path}");
    }
    if let Some(path) = plan {
        std::fs::write(&path, plan_csv(&rows, &groups))?;
        println!("план решения: {path}");
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
        }
    }

    fn one_group(rows: &[FileRow]) -> Group {
        let members: Vec<usize> = (0..rows.len()).collect();
        let suggested = pick_best(rows, &members);
        Group {
            waste: 0,
            advice: advice_of(rows, &members, suggested),
            members,
            reason: "похожий снимок",
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
        assert!(g.advice.contains("резче остальных на 150%"), "{}", g.advice);
    }

    #[test]
    fn exact_copies_have_no_winner() {
        let rows = vec![
            row("/a.jpg", "same", 100, 10.0, 100),
            row("/b.jpg", "same", 100, 10.0, 100),
        ];
        let g = one_group(&rows);
        assert_eq!(g.advice, "точные копии — разницы нет");
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
        let plan = plan_csv(&rows, &[g]);
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
