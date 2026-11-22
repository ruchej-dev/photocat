//! Поиск дублей: точные (sha256) и похожие снимки (расстояние Хэмминга по dHash).

use crate::db::{self, FileRow};
use crate::{flag_num, flag_str, has_flag};
use anyhow::Result;
use std::collections::HashMap;
use std::path::PathBuf;

pub struct Group {
    pub members: Vec<usize>,
    pub waste: i64,
    pub reason: &'static str,
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
            Group {
                waste: total - biggest,
                members: v,
                reason: if same_content { "точная копия" } else { "похожий снимок" },
            }
        })
        .collect();
    groups.sort_by_key(|g| std::cmp::Reverse(g.waste));
    groups
}

pub fn cli(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let max_dist = flag_num(args, "--max-dist", 6) as u32;
    let csv = flag_str(args, "--csv");
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
        for (n, g) in groups.iter().enumerate().take(200) {
            println!(
                "\n#{} [{}] файлов: {}, дублей на {}",
                n + 1,
                g.reason,
                g.members.len(),
                db::human(g.waste)
            );
            let base = rows[g.members[0]].dhash.map(|v| v as u64);
            for &i in &g.members {
                let r = &rows[i];
                let dist = match (base, r.dhash) {
                    (Some(a), Some(b)) => crate::derive::hamming(a, b as u64),
                    _ => 0,
                };
                println!(
                    "   {:>10}  {}  Δ{:<2}  {}",
                    db::human(r.size),
                    r.taken_at.clone().unwrap_or_else(|| "-".into()),
                    dist,
                    r.path
                );
            }
        }
        if groups.len() > 200 {
            println!("\n… и ещё {} групп (полный список: --csv)", groups.len() - 200);
        }
    }
    if let Some(path) = csv {
        let mut s = String::from("group,reason,hamming,wasted,size,path\n");
        for (n, g) in groups.iter().enumerate() {
            let base = rows[g.members[0]].dhash.map(|v| v as u64);
            for &i in &g.members {
                let r = &rows[i];
                let d = match (base, r.dhash) {
                    (Some(a), Some(b)) => crate::derive::hamming(a, b as u64).to_string(),
                    _ => "".into(),
                };
                s.push_str(&format!(
                    "{},{},{},{},{},{}\n",
                    n + 1,
                    g.reason,
                    d,
                    g.waste,
                    r.size,
                    csv_escape(&r.path)
                ));
            }
        }
        std::fs::write(&path, s)?;
        println!("полный список: {path}");
    }
    Ok(())
}

fn csv_escape(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}
