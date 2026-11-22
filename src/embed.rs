//! CLIP-эмбеддинги: локальная ONNX-модель, векторы в БД, теги, косинусный поиск, k-means.
//!
//! Модель лежит файлами в `~/.cache/photocat/models/clip-vit-b32/` и проверяется по sha256.
//! Приложение никогда не ходит в сеть: веса скачивает `scripts/fetch-model.sh` по явной команде.

use crate::db::{self, FileRow, Progress};
use crate::derive;
use anyhow::{Context, Result, bail};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokenizers::Tokenizer;

pub const MODEL: &str = "clip-vit-b32-multilingual";
pub const DIM: usize = 512;
pub const IMAGE_SIDE: u32 = 224;
/// Максимальная длина текста (sentence_bert_config.json: 128).
pub const MAX_TOKENS: usize = 128;
pub const TAG_SOURCE: &str = "clip";
/// Порог, ниже которого тег не показываем и не фильтруем (калиброван на корпусе).
pub const TAG_SHOW: f32 = 0.22;
/// Насколько первый тег должен отрываться от второго, чтобы ему можно было верить.
pub const TAG_MARGIN: f32 = 0.01;

/// Русские подписи и промпты для мультиязычного текстового энкодера.
pub const TAGS: [(&str, &str); 7] = [
    ("пляж", "фотография пляжа и моря"),
    ("документ", "фотография документа, лист бумаги с текстом"),
    ("скриншот", "снимок экрана, интерфейс программы"),
    ("еда", "фотография еды, тарелка с едой"),
    ("животные", "фотография животного, кошка или собака"),
    ("дети", "фотография детей, ребёнок"),
    ("праздник", "праздник, торт со свечами, вечеринка"),
];

const VISION_SHA: &str = "583fd1110a514667812fee7d684952aaf82a99b959760c8d7dca7e0ab9839299";
const TEXT_SHA: &str = "fbc8fbeaa5237d96bd1bf430057c70d34de8334a463e933a5faa305f6caeed9c";
const TOKENIZER_SHA: &str = "5b4e1a8171c81dfd666ae40265b9530c6e0b3d53923fe8ac493dcc84229adf81";
const DENSE_SHA: &str = "d12568dc7300970a4d3dbb49068ad16cd89b99840b74b026f8e48071e9414f74";

pub fn model_dir() -> PathBuf {
    db::cache_dir().join("models/clip-vit-b32")
}

struct Files {
    vision: PathBuf,
    text: PathBuf,
    tokenizer: PathBuf,
    dense: PathBuf,
}

fn files() -> Files {
    let d = model_dir();
    Files {
        vision: d.join("vision.onnx"),
        text: d.join("text.onnx"),
        tokenizer: d.join("tokenizer.json"),
        dense: d.join("text_dense.safetensors"),
    }
}

/// Файлы модели на месте (без проверки sha256 — она при загрузке).
pub fn model_present() -> bool {
    let f = files();
    f.vision.is_file() && f.text.is_file() && f.tokenizer.is_file() && f.dense.is_file()
}

/// Проверить наличие и sha256 всех файлов модели.
pub fn check_model() -> Result<()> {
    let f = files();
    for (path, want, what) in [
        (&f.vision, VISION_SHA, "зрительная часть"),
        (&f.text, TEXT_SHA, "текстовая часть"),
        (&f.tokenizer, TOKENIZER_SHA, "токенизатор"),
        (&f.dense, DENSE_SHA, "проекция текста"),
    ] {
        if !path.is_file() {
            bail!(
                "нет модели CLIP: {}\nскачайте веса: bash scripts/fetch-model.sh",
                path.display()
            );
        }
        let got = derive::sha256_file(path)
            .with_context(|| format!("не прочитать {}", path.display()))?;
        if got != want {
            bail!(
                "файл модели повреждён ({what}): {} \nsha256 {got}, ожидался {want}\nперекачайте: bash scripts/fetch-model.sh",
                path.display()
            );
        }
    }
    Ok(())
}

/// Размер файлов модели в байтах (для интерфейса и отчётов).
pub fn model_bytes() -> i64 {
    let f = files();
    [f.vision, f.text, f.tokenizer, f.dense]
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as i64)
        .sum()
}

pub struct Model {
    _env: Environment,
    vision: Session,
    text: Session,
    tokenizer: Tokenizer,
    dense: Vec<f32>,
    tag_vectors: Vec<Vec<f32>>,
}

impl Model {
    pub fn load(threads: usize) -> Result<Self> {
        check_model()?;
        let f = files();
        let threads = threads.max(1);
        let env = ort::init()
            .with_name("photocat")
            .build()
            .map_err(oe)?;
        let vision = Session::builder(&env).map_err(oe)?
            .with_intra_threads(threads).map_err(oe)?
            .commit_from_file(&f.vision)
            .map_err(oe)?;
        let text = Session::builder(&env).map_err(oe)?
            .with_intra_threads(threads).map_err(oe)?
            .commit_from_file(&f.text)
            .map_err(oe)?;
        let tokenizer = Tokenizer::from_file(&f.tokenizer)
            .map_err(|e| anyhow::anyhow!("не загрузить токенизатор: {e}"))?;
        let dense = read_dense(&f.dense)?;
        let mut me = Self {
            _env: env,
            vision,
            text,
            tokenizer,
            dense,
            tag_vectors: Vec::new(),
        };
        let prompts: Vec<&str> = TAGS.iter().map(|(_, p)| *p).collect();
        let tag_vectors = me.encode_texts(&prompts)?;
        me.tag_vectors = tag_vectors;
        Ok(me)
    }

    /// Текстовые эмбеддинги (для запроса и промптов). Векторы нормализованы.
    pub fn encode_texts(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let mut rows: Vec<Vec<i64>> = Vec::with_capacity(texts.len());
        let mut max_len = 1usize;
        for t in texts {
            let enc = self
                .tokenizer
                .encode(*t, true)
                .map_err(|e| anyhow::anyhow!("токенизация «{t}»: {e}"))?;
            let mut ids: Vec<i64> = enc.get_ids().iter().map(|&v| v as i64).collect();
            ids.truncate(MAX_TOKENS);
            if ids.is_empty() {
                ids.push(0);
            }
            max_len = max_len.max(ids.len());
            rows.push(ids);
        }
        let batch = rows.len();
        let mut ids = vec![0i64; batch * max_len];
        let mut mask = vec![0i64; batch * max_len];
        for (b, row) in rows.iter().enumerate() {
            for (i, &id) in row.iter().enumerate() {
                ids[b * max_len + i] = id;
                mask[b * max_len + i] = 1;
            }
        }
        let outputs = self.text.run(ort::inputs![
            "input_ids" => Tensor::from_array((vec![batch as i64, max_len as i64], ids)).map_err(oe)?,
            "attention_mask" => Tensor::from_array((vec![batch as i64, max_len as i64], mask.clone())).map_err(oe)?,
        ]).map_err(oe)?;
        let (shape, data) = outputs["last_hidden_state"].try_extract_tensor::<f32>().map_err(oe)?;
        let hidden = *shape.last().unwrap_or(&0) as usize;
        if hidden == 0 || hidden != self.dense.len() / DIM {
            bail!(
                "текстовая модель вернула {hidden} признаков, ожидалось {}",
                self.dense.len() / DIM
            );
        }
        let mut out = Vec::with_capacity(batch);
        for b in 0..batch {
            let mut pooled = vec![0f32; hidden];
            let mut n = 0f32;
            for t in 0..max_len {
                if mask[b * max_len + t] == 0 {
                    continue;
                }
                let base = (b * max_len + t) * hidden;
                for (i, v) in pooled.iter_mut().enumerate() {
                    *v += data[base + i];
                }
                n += 1.0;
            }
            if n > 0.0 {
                for v in pooled.iter_mut() {
                    *v /= n;
                }
            }
            let mut y = vec![0f32; DIM];
            for (o, out) in y.iter_mut().enumerate() {
                let w = &self.dense[o * hidden..(o + 1) * hidden];
                *out = w.iter().zip(&pooled).map(|(a, b)| a * b).sum();
            }
            normalize(&mut y);
            out.push(y);
        }
        Ok(out)
    }

    /// Эмбеддинги изображений по путям к превью 320 px. Векторы нормализованы.
    pub fn embed_thumbs(&mut self, thumbs: &[PathBuf]) -> Result<Vec<Vec<f32>>> {
        if thumbs.is_empty() {
            return Ok(Vec::new());
        }
        let plane = (IMAGE_SIDE * IMAGE_SIDE) as usize;
        let mut data = Vec::with_capacity(thumbs.len() * plane * 3);
        for p in thumbs {
            data.extend_from_slice(&derive::clip_input(p, IMAGE_SIDE)?);
        }
        let n = thumbs.len();
        let outputs = self.vision.run(ort::inputs![
            "pixel_values" => Tensor::from_array((
                vec![n as i64, 3, IMAGE_SIDE as i64, IMAGE_SIDE as i64],
                data
            )).map_err(oe)?
        ]).map_err(oe)?;
        let (shape, values) = outputs["image_embeds"].try_extract_tensor::<f32>().map_err(oe)?;
        let dim = *shape.last().unwrap_or(&0) as usize;
        if dim != DIM {
            bail!("зрительная модель вернула {dim} признаков, ожидалось {DIM}");
        }
        let mut out = Vec::with_capacity(n);
        for chunk in values.chunks(DIM) {
            let mut v = chunk.to_vec();
            normalize(&mut v);
            out.push(v);
        }
        if out.len() != n {
            bail!("зрительная модель вернула {} векторов на {n} картинок", out.len());
        }
        Ok(out)
    }

    /// Теги для одного вектора: все промпты с весом по убыванию.
    pub fn tag_scores(&self, image: &[f32]) -> Vec<(String, f32)> {
        let mut out: Vec<(String, f32)> = TAGS
            .iter()
            .zip(&self.tag_vectors)
            .map(|((label, _), v)| ((*label).to_string(), cosine(image, v)))
            .collect();
        out.sort_by(|a, b| b.1.total_cmp(&a.1));
        out
    }
}

/// Уверенный тег — только первый, если он выше порога и заметно отрывается от второго.
pub fn confident_tag(scores: &[(String, f32)]) -> Option<(&str, f32)> {
    let (tag, top) = scores.first()?;
    if *top < TAG_SHOW {
        return None;
    }
    if let Some((_, second)) = scores.get(1)
        && top - second < TAG_MARGIN
    {
        return None;
    }
    Some((tag.as_str(), *top))
}

fn oe<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("ONNX Runtime: {e}")
}

fn read_dense(path: &Path) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("не прочитать {}", path.display()))?;
    if bytes.len() < 8 {
        bail!("плотный слой повреждён: {}", path.display());
    }
    let header_len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&bytes[8..8 + header_len])
        .context("не разобрать safetensors-заголовок")?;
    let info = header
        .get("linear.weight")
        .context("в safetensors нет linear.weight")?;
    let start = info["data_offsets"][0].as_u64().unwrap_or(0) as usize + 8 + header_len;
    let end = info["data_offsets"][1].as_u64().unwrap_or(0) as usize + 8 + header_len;
    let raw = bytes
        .get(start..end)
        .context("safetensors короче заявленного")?;
    let w: Vec<f32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    if w.len() != DIM * 768 {
        bail!("проекция текста {} чисел, ожидалось {}", w.len(), DIM * 768);
    }
    Ok(w)
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}

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

/// Векторы из БД, отсортированные по косинусу с запросом.
pub fn search(
    db_path: &Path,
    model: &mut Model,
    query: &str,
    top: usize,
) -> Result<Vec<(i64, f32)>> {
    let mut q = model
        .encode_texts(&[query])?
        .pop()
        .context("пустой текстовый эмбеддинг")?;
    normalize(&mut q);
    let conn = db::open(db_path)?;
    let vectors = db::load_embeddings(&conn, MODEL)?;
    let mut scored: Vec<(i64, f32)> = vectors
        .into_iter()
        .map(|(id, v)| (id, cosine(&q, &v)))
        .collect();
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    scored.truncate(top);
    Ok(scored)
}

pub struct EmbedStats {
    pub total: usize,
    pub computed: usize,
    pub skipped: usize,
    pub errors: usize,
    pub seconds: f64,
    pub model_bytes: i64,
}

impl std::fmt::Display for EmbedStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CLIP: посчитано {}/{}, пропущено {}, ошибок {}, за {:.1} с, модель {}",
            self.computed,
            self.total,
            self.skipped,
            self.errors,
            self.seconds,
            db::human(self.model_bytes)
        )
    }
}

/// Посчитать эмбеддинги и теги для изображений, у которых их ещё нет (ключ — sha256).
pub fn embed_missing(
    db_path: &Path,
    threads: usize,
    batch: usize,
    progress: Option<Arc<Progress>>,
) -> Result<EmbedStats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let rows = db::load_all(&conn)?;
    let done = db::embedding_shas(&conn, MODEL)?;
    let todo: Vec<&FileRow> = rows
        .iter()
        .filter(|r| r.kind == "image" && r.thumb.is_some())
        .filter(|r| done.get(&r.id) != Some(&r.sha256))
        .collect();
    let total = rows.iter().filter(|r| r.kind == "image").count();
    let skipped = total.saturating_sub(todo.len());
    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg("загрузка модели CLIP…");
    }
    if todo.is_empty() {
        if let Some(p) = &progress {
            p.set_msg("CLIP: всё уже посчитано");
            p.finish();
        }
        return Ok(EmbedStats {
            total,
            computed: 0,
            skipped,
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            model_bytes: model_bytes(),
        });
    }
    let mut model = Model::load(threads)?;
    if let Some(p) = &progress {
        p.set_msg("CLIP: эмбеддинги…");
    }
    let thumbs_dir = db::cache_dir().join("thumbs");
    let mut computed = 0usize;
    let mut errors = 0usize;
    let batch = batch.clamp(1, 64);
    for part in todo.chunks(batch) {
        let mut paths = Vec::with_capacity(part.len());
        let mut valid = Vec::with_capacity(part.len());
        for r in part {
            let rel = r.thumb.as_deref().unwrap_or_default();
            paths.push(thumbs_dir.join(rel));
            valid.push(*r);
        }
        let vectors = match model.embed_thumbs(&paths) {
            Ok(v) => v,
            Err(e) => {
                // пакет может упасть из-за одного битого превью — откатываемся на поштучно
                if let Some(p) = &progress {
                    p.set_msg(format!("CLIP: пакет не прошёл ({e}), по одному"));
                }
                let mut single = Vec::with_capacity(paths.len());
                for (r, path) in valid.iter().zip(&paths) {
                    match model.embed_thumbs(std::slice::from_ref(path)) {
                        Ok(v) => single.push(Some(v.into_iter().next().unwrap_or_default())),
                        Err(err) => {
                            eprintln!("\nCLIP: не декодировать {}: {err}", path.display());
                            let _ = r;
                            single.push(None);
                        }
                    }
                }
                single
                    .into_iter()
                    .map(|v| v.unwrap_or_default())
                    .collect()
            }
        };
        let tx = conn.unchecked_transaction()?;
        for (r, vec) in valid.iter().zip(vectors) {
            if vec.is_empty() {
                errors += 1;
                if let Some(p) = &progress {
                    p.error();
                    p.tick();
                }
                continue;
            }
            db::save_embedding(&tx, r.id, MODEL, &r.sha256, &vec)?;
            db::replace_tags(&tx, r.id, TAG_SOURCE, &model.tag_scores(&vec))?;
            computed += 1;
            if let Some(p) = &progress {
                p.tick();
            }
        }
        tx.commit()?;
    }
    if let Some(p) = &progress {
        p.set_msg("CLIP: готово");
        p.finish();
    }
    Ok(EmbedStats {
        total,
        computed,
        skipped,
        errors,
        seconds: t0.elapsed().as_secs_f64(),
        model_bytes: model_bytes(),
    })
}

/// Детерминированный k-means++ над нормализованными векторами.
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
                // пустой кластер забирает самую дальнюю точку
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

/// Собрать кластеры и записать отчёт «кластер → 12 примеров».
pub fn write_clusters(db_path: &Path, out: &Path, k: usize) -> Result<ClusterStats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let rows = db::load_all(&conn)?;
    let by_id: HashMap<i64, &FileRow> = rows.iter().map(|r| (r.id, r)).collect();
    let vectors = db::load_embeddings(&conn, MODEL)?;
    if vectors.len() < 2 {
        bail!("нет эмбеддингов: сначала запустите `photocat embed`");
    }
    let xs: Vec<Vec<f32>> = vectors.iter().map(|(_, v)| v.clone()).collect();
    let labels = kmeans(&xs, k, 30, 0x5eed_2026);
    let mut buckets: Vec<Vec<usize>> = vec![Vec::new(); k.clamp(1, xs.len())];
    for (i, c) in labels.iter().enumerate() {
        buckets[*c].push(i);
    }
    buckets.retain(|b| !b.is_empty());
    buckets.sort_by_key(|b| std::cmp::Reverse(b.len()));

    let mut md = String::from("# Кластеры CLIP (M3)\n\n");
    md.push_str(&format!(
        "Модель `{MODEL}`, фото с эмбеддингами: {}, кластеров: {}, k={k}.\n\n",
        xs.len(),
        buckets.len()
    ));
    md.push_str("Отчёт для просмотра владельцем: 12 примеров на кластер.\n");
    for (n, b) in buckets.iter().enumerate() {
        md.push_str(&format!("\n## Кластер {} — {} фото\n\n", n + 1, b.len()));
        for &i in b.iter().take(12) {
            let id = vectors[i].0;
            if let Some(r) = by_id.get(&id) {
                md.push_str(&format!("- `{}`\n", r.path));
            }
        }
        if b.len() > 12 {
            md.push_str(&format!("- … и ещё {}\n", b.len() - 12));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_of_normalized_vectors() {
        let a = {
            let mut v = vec![1.0, 0.0, 0.0, 0.0];
            normalize(&mut v);
            v
        };
        let b = {
            let mut v = vec![0.0, 2.0, 0.0, 0.0];
            normalize(&mut v);
            v
        };
        assert!(cosine(&a, &a) > 0.999);
        assert!(cosine(&a, &b).abs() < 1e-6);
    }

    #[test]
    fn kmeans_separates_two_groups() {
        let mut xs = Vec::new();
        for i in 0..20 {
            xs.push(vec![1.0, i as f32 * 0.001, 0.0, 0.0]);
            xs.push(vec![0.0, 0.0, 1.0, i as f32 * 0.001]);
        }
        let labels = kmeans(&xs, 2, 20, 7);
        let a = labels[0];
        let b = labels[1];
        assert_ne!(a, b);
        assert!(labels.iter().step_by(2).all(|&l| l == a));
        assert!(labels.iter().skip(1).step_by(2).all(|&l| l == b));
    }

    #[test]
    #[ignore = "нужны веса модели и проиндексированный корпус"]
    fn model_roundtrip_on_real_photo() {
        let db_path = db::resolve_db(None).unwrap();
        let conn = db::open(&db_path).unwrap();
        let rows = db::load_all(&conn).unwrap();
        let r = rows.iter().find(|r| r.thumb.is_some()).expect("нет фото с превью");
        let mut m = Model::load(4).unwrap();
        let thumb = db::cache_dir().join("thumbs").join(r.thumb.as_ref().unwrap());
        let v = m.embed_thumbs(&[thumb]).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), DIM);
        assert!((v[0].iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-3);
        let tags = m.tag_scores(&v[0]);
        println!("{}, теги: {:?}", r.path, tags);
        let t = m.encode_texts(&["пляж", "документ"]).unwrap();
        println!(
            "косинус: пляж {:.3}, документ {:.3}",
            cosine(&v[0], &t[0]),
            cosine(&v[0], &t[1])
        );
    }

}
