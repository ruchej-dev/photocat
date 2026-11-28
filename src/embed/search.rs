//! Cosine similarity and the semantic search over the stored vectors.

use super::model::{normalize, Model};
use crate::db;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len());
    let mut dot = 0f32;
    let mut na = 0f32;
    let mut nb = 0f32;
    for i in 0..n {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

/// Vectors from the DB, sorted by the cosine with the query.
pub fn search(
    db_path: &Path,
    model: &mut Model,
    query: &str,
    top: usize,
) -> Result<Vec<(i64, f32)>> {
    let mut q = model
        .encode_texts(&[query])?
        .pop()
        .context("an empty text embedding")?;
    normalize(&mut q);
    let conn = db::open(db_path)?;
    let vectors = db::load_embeddings(&conn, model.entry.id)?;
    let mut scored: Vec<(i64, f32)> = vectors
        .into_iter()
        .map(|(id, v)| (id, cosine(&q, &v)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(top);
    Ok(scored)
}

/// A filter by tag: any weight above the threshold fits, not only the top one.
/// So 14 hints do not "eat" each other: a photo with children shows up in both "children" and "clothes".
pub fn tag_above(scores: &[(String, f32)], want: &str, show: f32) -> bool {
    scores
        .iter()
        .any(|(tag, score)| tag == want && *score >= show)
}

/// The distribution of the weight of each tag over the folder, descending — for the percentile threshold.
/// Computed once when the tags are loaded, not on every interface frame.
pub fn tag_distributions(
    tags: &HashMap<i64, Vec<(String, f32)>>,
) -> HashMap<String, Vec<f32>> {
    let mut out: HashMap<String, Vec<f32>> = HashMap::new();
    for scores in tags.values() {
        for (tag, score) in scores {
            out.entry(tag.clone()).or_default().push(*score);
        }
    }
    for v in out.values_mut() {
        v.sort_by(|a, b| b.total_cmp(a));
    }
    out
}
