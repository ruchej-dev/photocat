//! k-means over the vectors and the cluster report.

use super::model::{active_entry, normalize};
use super::search::cosine;
use crate::db::{self, FileRow};
use crate::i18n;
use anyhow::{bail, Result};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// A deterministic k-means++ over normalized vectors.
pub fn kmeans(xs: &[Vec<f32>], k: usize, iters: usize, seed: u64) -> Vec<usize> {
    let n = xs.len();
    if n == 0 {
        return Vec::new();
    }
    let k = k.clamp(1, n);
    let dim = xs[0].len();
    let mut cent: Vec<Vec<f32>> = Vec::with_capacity(k);
    let mut rng = seed | 1;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    cent.push(xs[(next() as usize) % n].clone());
    let mut dist2 = vec![f32::INFINITY; n];
    while cent.len() < k {
        let last = cent.last().unwrap();
        let mut sum = 0f64;
        for (i, x) in xs.iter().enumerate() {
            let d = 1.0 - cosine(x, last);
            if d < dist2[i] {
                dist2[i] = d;
            }
            sum += dist2[i] as f64;
        }
        let mut pick = 0usize;
        if sum > 0.0 {
            let mut target = (next() as f64 / u64::MAX as f64) * sum;
            for (i, d) in dist2.iter().enumerate() {
                target -= *d as f64;
                if target <= 0.0 {
                    pick = i;
                    break;
                }
            }
        } else {
            pick = (next() as usize) % n;
        }
        cent.push(xs[pick].clone());
    }
    let mut labels = vec![usize::MAX; n];
    for _ in 0..iters.max(1) {
        let mut changed = false;
        for (i, x) in xs.iter().enumerate() {
            let mut best = 0;
            let mut best_d = f32::INFINITY;
            for (c, cv) in cent.iter().enumerate() {
                let d = 1.0 - cosine(x, cv);
                if d < best_d {
                    best_d = d;
                    best = c;
                }
            }
            if labels[i] != best {
                labels[i] = best;
                changed = true;
            }
        }
        if !changed {
            break;
        }
        let mut sums = vec![vec![0f64; dim]; k];
        let mut counts = vec![0usize; k];
        for (i, x) in xs.iter().enumerate() {
            counts[labels[i]] += 1;
            for (s, v) in sums[labels[i]].iter_mut().zip(x) {
                *s += *v as f64;
            }
        }
        for c in 0..k {
            if counts[c] == 0 {
                // an empty cluster takes the farthest point
                let far = labels
                    .iter()
                    .enumerate()
                    .max_by(|a, b| {
                        let da = 1.0 - cosine(&xs[a.0], &cent[labels[a.0]]);
                        let db = 1.0 - cosine(&xs[b.0], &cent[labels[b.0]]);
                        da.total_cmp(&db)
                    })
                    .map(|(i, _)| i)
                    .unwrap_or(0);
                cent[c] = xs[far].clone();
                labels[far] = c;
                continue;
            }
            let mut v: Vec<f32> = sums[c].iter().map(|s| (*s / counts[c] as f64) as f32).collect();
            normalize(&mut v);
            cent[c] = v;
        }
    }
    labels
}

pub struct ClusterStats {
    pub photos: usize,
    pub clusters: usize,
    pub out: PathBuf,
    pub seconds: f64,
}

/// Build the clusters and write the report "cluster → 12 examples".
pub fn write_clusters(db_path: &Path, out: &Path, k: usize) -> Result<ClusterStats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let entry = active_entry(&settings);
    let rows = db::load_all(&conn)?;
    let by_id: HashMap<i64, &FileRow> = rows.iter().map(|r| (r.id, r)).collect();
    let vectors = db::load_embeddings(&conn, entry.id)?;
    if vectors.len() < 2 {
        bail!("no embeddings: run `photocat embed` first");
    }
    let xs: Vec<Vec<f32>> = vectors.iter().map(|(_, v)| v.clone()).collect();
    let labels = kmeans(&xs, k, 30, 0x5eed_2026);
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); k.clamp(1, xs.len())];
    for (i, c) in labels.iter().enumerate() {
        buckets[*c].push(i);
    }
    buckets.retain(|b| !b.is_empty());
    buckets.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let lang = settings.lang;
    let mut md = String::from(i18n::t(settings.lang, "# CLIP clusters (M3)\n\n"));
    md.push_str(&format!(
        "{}\n\n",
        i18n::fill(
            lang,
            "Model `{}`, files with embeddings: {}, clusters: {}, k={}.",
            &[
                entry.id.to_string(),
                xs.len().to_string(),
                buckets.len().to_string(),
                k.to_string(),
            ],
        )
    ));
    md.push_str(i18n::t(
        lang,
        "A report for the owner: 12 examples per cluster.\n",
    ));
    for (n, b) in buckets.iter().enumerate() {
        md.push_str(&format!(
            "\n{}\n\n",
            i18n::fill(
                lang,
                "## Cluster {} — {} photos",
                &[(n + 1).to_string(), b.len().to_string()],
            )
        ));
        for &i in b.iter().take(12) {
            let id = vectors[i].0;
            if let Some(r) = by_id.get(&id) {
                md.push_str(&format!("- `{}`\n", r.path));
            }
        }
        if b.len() > 12 {
            md.push_str(&format!(
                "- {}\n",
                i18n::fill(settings.lang, "… and {} more", &[(b.len() - 12).to_string()])
            ));
        }
    }
    if let Some(dir) = out.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(out, md)?;
    Ok(ClusterStats {
        photos: xs.len(),
        clusters: buckets.len(),
        out: out.to_path_buf(),
        seconds: t0.elapsed().as_secs_f64(),
    })
}
