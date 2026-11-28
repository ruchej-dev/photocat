//! The model catalogue, the ONNX session, the vectors and the frame normalization.

use crate::db;
use super::download::check_model_in;
use super::search::cosine;
use crate::derive;
use anyhow::{bail, Context, Result};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tokenizers::Tokenizer;

/// How the text tower gives a vector: through the 768→512 projection or `text_embeds` directly.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextKind {
    /// Multilingual DistilBERT + Dense 768→512 (Russian search).
    Projected,
    /// The CLIP text tower, `text_embeds` output, no projection (English).
    Direct,
    /// The SigLIP text tower: `input_ids` only, `pooler_output` of 768.
    Pooler,
}

pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

pub struct ModelEntry {
    /// The key of the embeddings in the DB.
    pub id: &'static str,
    /// The subfolder name in the models folder (historically `clip-vit-b32`, not changed).
    pub dir: &'static str,
    pub title: &'static str,
    pub note: &'static str,
    /// The licence of the weights — so the owner sees it before downloading.
    pub license: &'static str,
    pub kind: TextKind,
    /// The embedding dimension of this model (512, 768, …), not a global constant.
    pub dim: usize,
    /// The side of the square at the input of the vision tower.
    pub image_side: u32,
    /// The maximum number of text tokens (64 for SigLIP).
    pub max_tokens: usize,
    /// The padding token; empty means 0. For SigLIP it is `</s>` (id 1): the pooler takes the
    /// last token, so short text is padded with the same EOS, not with a zero.
    pub pad_token: &'static str,
    /// On which side to pad the text to `max_tokens`. On the right — CLIP and SigLIP 1; on the
    /// left — SigLIP 2: its tokenizer pads with `<pad>` on the right itself, while the pooler
    /// reads the last token, and EOS must remain the last one. On the right all texts
    /// collapse into one vector (dog/cat 0.976), and the pictures stop being distinguishable.
    pub pad_left: bool,
    /// Frame normalization: CLIP and SigLIP have different mean/std and filter.
    pub norm: derive::ImageNorm,
    /// How many frames the model sees per photo: CLIP — a centre crop and the whole frame,
    /// SigLIP — one frame stretched into a square (as in its processor).
    pub frames: usize,
    /// The name of the vision tower output in ONNX.
    pub vision_output: &'static str,
    /// Tag weight calibration: cosine → the CLIP confidence scale.
    /// The cosine scale differs between families (CLIP ViT-B/32 — 0.24–0.32, SigLIP 2 —
    /// 0.07–0.10), while `tag_show` and `tag_margin` in the settings are the same for all models.
    /// `score = cos * scale + shift`; `(1.0, 0.0)` is the identity (CLIP).
    /// The coefficients are a property of the model, not of the folder: taken on two sets (M15.6).
    pub score_calib: (f32, f32),
    /// The version of the tag weight calibration. It changes together with `score_calib`: the
    /// weights in `tags` are marked with it in `source`, so old rows do not mix with new ones —
    /// `embed --retag` recomputes them in seconds.
    pub score_version: u8,
    pub files: &'static [ModelFile],
}

impl ModelEntry {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
    /// Whether the model fits the limit from the settings (MiB).
    pub fn fits(&self, max_mb: u64) -> bool {
        self.bytes() <= max_mb.saturating_mul(1024 * 1024)
    }
    /// The tag weight on the CLIP confidence scale (see `score_calib`).
    pub fn score(&self, cos: f32) -> f32 {
        let (scale, shift) = self.score_calib;
        if !scale.is_finite() || scale <= 0.0 || !shift.is_finite() {
            return cos;
        }
        cos * scale + shift
    }
    /// The name of the text tower output in ONNX; for Projected it is not an embedding yet.
    pub fn text_output(&self) -> &'static str {
        match self.kind {
            TextKind::Projected => "last_hidden_state",
            TextKind::Direct => "text_embeds",
            TextKind::Pooler => "pooler_output",
        }
    }
}

pub const DEFAULT_MODEL: &str = "clip-vit-b32-multilingual";

const VISION: ModelFile = ModelFile {
    name: "vision.onnx",
    url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/onnx/vision_model_quantized.onnx",
    sha256: "583fd1110a514667812fee7d684952aaf82a99b959760c8d7dca7e0ab9839299",
    bytes: 89_117_001,
};

const MULTILINGUAL_FILES: [ModelFile; 4] = [
    VISION,
    ModelFile {
        name: "text.onnx",
        url: "https://huggingface.co/sentence-transformers/clip-ViT-B-32-multilingual-v1/resolve/main/onnx/model_quint8_avx2.onnx",
        sha256: "fbc8fbeaa5237d96bd1bf430057c70d34de8334a463e933a5faa305f6caeed9c",
        bytes: 135_377_779,
    },
    ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/sentence-transformers/clip-ViT-B-32-multilingual-v1/resolve/main/tokenizer.json",
        sha256: "5b4e1a8171c81dfd666ae40265b9530c6e0b3d53923fe8ac493dcc84229adf81",
        bytes: 1_961_847,
    },
    ModelFile {
        name: "text_dense.safetensors",
        url: "https://huggingface.co/sentence-transformers/clip-ViT-B-32-multilingual-v1/resolve/main/2_Dense/model.safetensors",
        sha256: "d12568dc7300970a4d3dbb49068ad16cd89b99840b74b026f8e48071e9414f74",
        bytes: 1_572_984,
    },
];

const ENGLISH_FILES: [ModelFile; 3] = [
    VISION,
    ModelFile {
        name: "text.onnx",
        url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/onnx/text_model_quantized.onnx",
        sha256: "73baab855d406190da9faa498cfedf65f15cf309f4cc7385b7b032e6d08e5c3a",
        bytes: 64_504_507,
    },
    ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/Xenova/clip-vit-base-patch32/resolve/main/tokenizer.json",
        sha256: "f7f3b7af117d467b58374797691a6438d3e6b9e9cef800dfd5dced7f697a90cd",
        bytes: 2_224_119,
    },
];

const SIGLIP_FILES: [ModelFile; 3] = [
    ModelFile {
        name: "vision.onnx",
        url: "https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/main/onnx/vision_model_quantized.onnx",
        sha256: "ef14a954f3d57e1806666432bd9785004c1dc27100aa260eee0cb0f10a5de058",
        bytes: 99_499_129,
    },
    ModelFile {
        name: "text.onnx",
        url: "https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/main/onnx/text_model_quantized.onnx",
        sha256: "ad0329b1f35acc66d8953ff2559ce358da8eb0a7011794cf951523d63a4dbce2",
        bytes: 111_475_220,
    },
    ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/Xenova/siglip-base-patch16-224/resolve/main/tokenizer.json",
        sha256: "4a17c975210be5ab4c36b47d8dae4eefb866dbfb1e676e394aad85dc30a3ae08",
        bytes: 2_398_744,
    },
];

const SIGLIP2_FILES: [ModelFile; 3] = [
    ModelFile {
        name: "vision.onnx",
        url: "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/main/onnx/vision_model_quantized.onnx",
        sha256: "5f2b401c1a4fc095702a5d45348e17ad46c4f87064085365b43c6e8eaa5c0070",
        bytes: 94_553_333,
    },
    ModelFile {
        name: "text.onnx",
        url: "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/main/onnx/text_model_quantized.onnx",
        sha256: "3a0603d3a00c05a80a6ded4743c16aaac7b1e62cdcc7e362e7ce418659b96400",
        bytes: 283_438_275,
    },
    ModelFile {
        name: "tokenizer.json",
        url: "https://huggingface.co/onnx-community/siglip2-base-patch16-224-ONNX/resolve/main/tokenizer.json",
        sha256: "cb9140fae3ac5122c972d37adf83e1248471a38147ad76f8215c8872c6fd8322",
        bytes: 34_363_039,
    },
];

static CATALOG: [ModelEntry; 4] = [
    ModelEntry {
        id: DEFAULT_MODEL,
        dir: "clip-vit-b32",
        title: "CLIP ViT-B/32, multilingual",
        note: "Russian search and tags, the best quality",
        license: "MIT + Apache-2.0",
        kind: TextKind::Projected,
        dim: 512,
        image_side: 224,
        max_tokens: 128,
        pad_token: "",
        pad_left: false,
        norm: derive::CLIP_NORM,
        frames: 2,
        vision_output: "image_embeds",
        score_calib: (1.0, 0.0),
        score_version: 1,
        files: &MULTILINGUAL_FILES,
    },
    ModelEntry {
        id: "clip-vit-b32-en",
        dir: "clip-vit-b32-en",
        title: "CLIP ViT-B/32, English",
        note: "smaller, English queries only",
        license: "MIT",
        kind: TextKind::Direct,
        dim: 512,
        image_side: 224,
        max_tokens: 128,
        pad_token: "",
        pad_left: false,
        norm: derive::CLIP_NORM,
        frames: 2,
        vision_output: "image_embeds",
        score_calib: (1.0, 0.0),
        score_version: 1,
        files: &ENGLISH_FILES,
    },
    ModelEntry {
        id: "siglip-base-224-en",
        dir: "siglip-base-224-en",
        title: "SigLIP base 224, English",
        note: "768 features, SigLIP architecture; English queries",
        license: "Apache-2.0",
        kind: TextKind::Pooler,
        dim: 768,
        image_side: 224,
        max_tokens: 64,
        pad_token: "</s>",
        pad_left: false,
        norm: derive::SIGLIP_NORM,
        frames: 1,
        vision_output: "pooler_output",
        // The SigLIP cosine is below the CLIP scale; the coefficients come from M15.6 (two sets,
        // 2340 files): the medians of the weight and of the margin match CLIP.
        score_calib: (0.55, 0.23),
        score_version: 2,
        files: &SIGLIP_FILES,
    },
    ModelEntry {
        id: "siglip2-base-224-multi",
        dir: "siglip2-base-224-multi",
        title: "SigLIP 2 base 224, multilingual",
        note: "768 features, 100+ languages; needs a 400 MiB limit",
        license: "Apache-2.0",
        kind: TextKind::Pooler,
        dim: 768,
        image_side: 224,
        max_tokens: 64,
        pad_token: "<pad>",
        pad_left: true,
        norm: derive::SIGLIP_NORM,
        frames: 1,
        vision_output: "pooler_output",
        score_calib: (1.42, 0.14),
        score_version: 2,
        files: &SIGLIP2_FILES,
    },
];

pub fn catalog() -> &'static [ModelEntry] {
    &CATALOG
}

/// A model by id; an empty id means the first in the catalogue.
pub fn entry(id: &str) -> Option<&'static ModelEntry> {
    if id.is_empty() {
        CATALOG.first()
    } else {
        CATALOG.iter().find(|e| e.id == id)
    }
}

/// The active model from the settings; if the id is stale — the first compatible one.
pub fn active_entry(settings: &db::Settings) -> &'static ModelEntry {
    entry(&settings.model_id).unwrap_or(&CATALOG[0])
}


pub fn tag_source(e: &ModelEntry) -> String {
    if e.score_version <= 1 {
        format!("clip:{}", e.id)
    } else {
        format!("clip:{}@v{}", e.id, e.score_version)
    }
}


pub fn model_dir(models_dir: &Path, e: &ModelEntry) -> PathBuf {
    models_dir.join(e.dir)
}

pub(super) fn file_paths(dir: &Path, e: &ModelEntry) -> Vec<PathBuf> {
    e.files.iter().map(|f| dir.join(f.name)).collect()
}


/// Lay the token rows into one batch: pad to `max_len` on the needed side.
/// The tail of pads the tokenizer put there itself is removed: for SigLIP 2 it is on the
/// right, while the pooler reads the last token — EOS must remain the last one.
pub(super) fn pad_rows(rows: &[Vec<i64>], max_len: usize, pad: i64, pad_left: bool) -> (Vec<i64>, Vec<i64>) {
    let batch = rows.len();
    let mut ids = vec![pad; batch * max_len];
    let mut mask = vec![0i64; batch * max_len];
    for (b, row) in rows.iter().enumerate() {
        let mut row = &row[..row.len().min(max_len)];
        while row.len() > 1 && row.last() == Some(&pad) {
            row = &row[..row.len() - 1];
        }
        let start = if pad_left { max_len - row.len() } else { 0 };
        for (i, &id) in row.iter().enumerate() {
            ids[b * max_len + start + i] = id;
            mask[b * max_len + start + i] = 1;
        }
    }
    (ids, mask)
}

pub struct Model {
    _env: Environment,
    vision: Session,
    text: Session,
    tokenizer: Tokenizer,
    dense: Vec<f32>,
    /// Label → the vectors of its prompts (one per template); enabled tags only.
    tag_vectors: Vec<(String, Vec<f32>)>,
    pub entry: &'static ModelEntry,
}

impl Model {
    pub fn load(settings: &db::Settings, threads: usize) -> Result<Self> {
        let entry = active_entry(settings);
        check_model_in(&settings.models_dir, entry)?;
        let dir = model_dir(&settings.models_dir, entry);
        let f = |name: &str| dir.join(name);
        let threads = threads.max(1);
        let env = shared_environment()?;
        let vision = Session::builder(&env).map_err(oe)?
            .with_intra_threads(threads).map_err(oe)?
            .commit_from_file(f("vision.onnx"))
            .map_err(oe)?;
        let text = Session::builder(&env).map_err(oe)?
            .with_intra_threads(threads).map_err(oe)?
            .commit_from_file(f("text.onnx"))
            .map_err(oe)?;
        let tokenizer = Tokenizer::from_file(f("tokenizer.json"))
            .map_err(|e| anyhow::anyhow!("cannot load the tokenizer: {e}"))?;
        // The dimension is a property of the model, not a global constant. Where the ONNX metadata
        // is fixed, it is checked at load; where dynamic — on the first pass.
        if let Some(dim) = output_dim(&vision, entry.vision_output, "vision")? {
            ensure_dim("vision", dim, entry.dim)?;
        }
        if entry.kind != TextKind::Projected
            && let Some(dim) = output_dim(&text, entry.text_output(), "text")?
        {
            ensure_dim("text", dim, entry.dim)?;
        }
        let dense = if entry.kind == TextKind::Projected {
            read_dense(&f("text_dense.safetensors"), entry.dim)?
        } else {
            Vec::new()
        };
        let mut me = Self {
            _env: env,
            vision,
            text,
            tokenizer,
            dense,
            tag_vectors: Vec::new(),
            entry,
        };
        me.tag_vectors = Vec::new();
        Ok(me)
    }

    /// Prepare the prompt vectors from the dictionary: 2–3 templates per label.
    /// A tag weight is the best of its templates, not the average: a description of an object may
    /// match only one wording, and the average blurs such a guess.
    /// The photo vectors are not recomputed — editing the dictionary costs one text tower pass.
    pub fn set_tag_defs(&mut self, defs: &[db::TagDef]) -> Result<()> {
        let mut out: Vec<(String, Vec<f32>)> = Vec::new();
        for d in defs.iter().filter(|d| d.enabled) {
            let prompts: Vec<&str> = d
                .prompts
                .iter()
                .map(|p| p.trim())
                .filter(|p| !p.is_empty())
                .collect();
            if prompts.is_empty() {
                continue;
            }
            let vectors = self.encode_texts(&prompts)?;
            for v in vectors {
                out.push((d.label.clone(), v));
            }
        }
        self.tag_vectors = out;
        Ok(())
    }

    fn tokenize(&self, texts: &[&str]) -> Result<(Vec<i64>, Vec<i64>, usize, usize)> {
        let mut rows: Vec<Vec<i64>> = Vec::with_capacity(texts.len());
        let mut max_len = 1usize;
        for t in texts {
            let enc = self
                .tokenizer
                .encode(*t, true)
                .map_err(|e| anyhow::anyhow!("tokenizing “{t}”: {e}"))?;
            let mut ids: Vec<i64> = enc.get_ids().iter().map(|&v| v as i64).collect();
            ids.truncate(self.entry.max_tokens);
            if ids.is_empty() {
                ids.push(0);
            }
            max_len = max_len.max(ids.len());
            rows.push(ids);
        }
        // SigLIP was trained on fixed 64 tokens with pad=EOS: short text without padding to 64
        // gives the pooler the wrong last token.
        let max_len = if self.entry.kind == TextKind::Pooler {
            self.entry.max_tokens
        } else {
            max_len
        };
        let pad = if self.entry.pad_token.is_empty() {
            0
        } else {
            self.tokenizer
                .token_to_id(self.entry.pad_token)
                .map(|v| v as i64)
                .unwrap_or(0)
        };
        let (ids, mask) = pad_rows(&rows, max_len, pad, self.entry.pad_left);
        Ok((ids, mask, rows.len(), max_len))
    }

    /// Text embeddings (for a query and for prompts). The vectors are normalized.
    pub fn encode_texts(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let (ids, mask, batch, max_len) = self.tokenize(texts)?;
        let mut out = Vec::with_capacity(batch);
        if self.entry.kind != TextKind::Projected {
            let outputs = self.text.run(ort::inputs![
                "input_ids" => Tensor::from_array((vec![batch as i64, max_len as i64], ids)).map_err(oe)?,
            ]).map_err(oe)?;
            let name = self.entry.text_output();
            let (shape, data) = outputs[name]
                .try_extract_tensor::<f32>()
                .map_err(oe)?;
            let dim = *shape.last().unwrap_or(&0) as usize;
            ensure_dim("text", dim, self.entry.dim)?;
            for row in data.chunks(dim) {
                let mut v = row.to_vec();
                normalize(&mut v);
                out.push(v);
            }
            if out.len() != batch {
                bail!("the text model returned {} vectors for {batch} requests", out.len());
            }
            return Ok(out);
        }
        let outputs = self.text.run(ort::inputs![
            "input_ids" => Tensor::from_array((vec![batch as i64, max_len as i64], ids)).map_err(oe)?,
            "attention_mask" => Tensor::from_array((vec![batch as i64, max_len as i64], mask.clone())).map_err(oe)?,
        ]).map_err(oe)?;
        let (shape, data) = outputs["last_hidden_state"].try_extract_tensor::<f32>().map_err(oe)?;
        let hidden = *shape.last().unwrap_or(&0) as usize;
        let want_hidden = self.dense.len() / self.entry.dim;
        if hidden == 0 || hidden != want_hidden {
            bail!(
                "the text model returned {hidden} features, {want_hidden} expected; \
                 the projection does not match the vector of {}",
                self.entry.dim
            );
        }
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
            let mut y = vec![0f32; self.entry.dim];
            for (o, out) in y.iter_mut().enumerate() {
                let w = &self.dense[o * hidden..(o + 1) * hidden];
                *out = w.iter().zip(&pooled).map(|(a, b)| a * b).sum();
            }
            normalize(&mut y);
            out.push(y);
        }
        Ok(out)
    }

    /// Image embeddings by paths to 320 px previews. The vectors are normalized.
    ///
    /// Per photo the model sees two frames — a centre crop and the whole frame with margins;
    /// the vectors are averaged and normalized again. One vector per photo in the DB.
    pub fn embed_thumbs(&mut self, thumbs: &[PathBuf]) -> Result<Vec<Vec<f32>>> {
        if thumbs.is_empty() {
            return Ok(Vec::new());
        }
        let side = self.entry.image_side;
        let frames = self.entry.frames.max(1);
        let plane = (side * side) as usize;
        let mut data = Vec::with_capacity(thumbs.len() * frames * plane * 3);
        for p in thumbs {
            if frames == 2 {
                let (center, whole) = derive::clip_inputs(p, side, &self.entry.norm)?;
                data.extend_from_slice(&center);
                data.extend_from_slice(&whole);
            } else {
                data.extend_from_slice(&derive::squash_input(p, side, &self.entry.norm)?);
            }
        }
        let values = self.run_vision(data, thumbs.len() * frames)?;
        let out = if frames == 2 {
            average_pairs(&values, thumbs.len(), self.entry.dim)
        } else {
            values.chunks(self.entry.dim).map(<[f32]>::to_vec).collect()
        };
        if out.len() != thumbs.len() {
            bail!("the vision model returned {} vectors for {} images", out.len(), thumbs.len());
        }
        Ok(out)
    }

    /// Run the vision model over ready CHW inputs; the vectors are normalized.
    /// `n` is the number of frames the data was prepared for (not photos).
    pub(super) fn run_vision(&mut self, data: Vec<f32>, n: usize) -> Result<Vec<f32>> {
        let side = self.entry.image_side;
        let outputs = self.vision.run(ort::inputs![
            "pixel_values" => Tensor::from_array((
                vec![n as i64, 3, side as i64, side as i64],
                data
            )).map_err(oe)?
        ]).map_err(oe)?;
        let name = self.entry.vision_output;
        let (shape, values) = outputs[name].try_extract_tensor::<f32>().map_err(oe)?;
        let dim = *shape.last().unwrap_or(&0) as usize;
        ensure_dim("vision", dim, self.entry.dim)?;
        let count = values.len() / dim;
        if count != n {
            bail!("the vision model returned {count} vectors for {n} frames");
        }
        let mut out = values.to_vec();
        for chunk in out.chunks_mut(dim) {
            normalize(chunk);
        }
        Ok(out)
    }

    /// Tags for one vector: every enabled tag of the dictionary, weight descending.
    /// A label has several templates — its best weight is taken.
    pub fn tag_scores(&self, image: &[f32]) -> Vec<(String, f32)> {
        let mut best: Vec<(String, f32)> = Vec::new();
        for (label, v) in &self.tag_vectors {
            let c = self.entry.score(cosine(image, v));
            match best.iter_mut().find(|(l, _)| l == label) {
                Some((_, s)) => *s = s.max(c),
                None => best.push((label.clone(), c)),
            }
        }
        best.sort_by(|a, b| b.1.total_cmp(&a.1));
        best
    }

    /// How many enabled tags are in the dictionary now.
    pub fn tag_count(&self) -> usize {
        let mut labels: Vec<&str> = self.tag_vectors.iter().map(|(l, _)| l.as_str()).collect();
        labels.sort_unstable();
        labels.dedup();
        labels.len()
    }
}

/// Average the frame vectors of one photo (two per photo) and normalize.
/// Without normalization the mean of two unit vectors is shorter than one, and the cosine with prompts lies.
pub(super) fn average_pairs(values: &[f32], images: usize, dim: usize) -> Vec<Vec<f32>> {
    let mut out = Vec::with_capacity(images);
    for i in 0..images {
        let mut v = vec![0f32; dim];
        for part in 0..2 {
            let base = (i * 2 + part) * dim;
            let Some(chunk) = values.get(base..base + dim) else {
                break;
            };
            for (a, b) in v.iter_mut().zip(chunk) {
                *a += *b;
            }
        }
        normalize(&mut v);
        out.push(v);
    }
    out
}

/// A dimension check with text that shows both the obtained and the expected number.
pub(super) fn ensure_dim(what: &str, got: usize, want: usize) -> Result<()> {
    if got == want {
        return Ok(());
    }
    bail!(
        "the {what} model returned {got} features while the catalogue says {want}; \
         a model and its vectors do not mix — pick the same model or recompute the tags"
    )
}

/// The last dimension of the ONNX output, if it is fixed in the metadata.
fn output_dim(session: &Session, name: &str, what: &str) -> Result<Option<usize>> {
    let Some(outlet) = session.outputs().iter().find(|o| o.name() == name) else {
        bail!("the {what} model has no output “{name}”; the catalogue does not match the files");
    };
    if let ort::value::ValueType::Tensor { shape, .. } = outlet.dtype()
        && let Some(&last) = shape.last()
        && last > 0
    {
        return Ok(Some(last as usize));
    }
    Ok(None)
}

fn oe<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("ONNX Runtime: {e}")
}

/// One ONNX environment per process: ORT does not allow a second one, and CLIP and faces live side by side.
static ORT_ENV: OnceLock<Environment> = OnceLock::new();

pub(crate) fn shared_environment() -> Result<Environment> {
    if let Some(env) = ORT_ENV.get() {
        return Ok(env.clone());
    }
    let env = ort::init().with_name("photocat").build().map_err(oe)?;
    let _ = ORT_ENV.set(env.clone());
    Ok(env)
}

fn read_dense(path: &Path, dim: usize) -> Result<Vec<f32>> {
    let bytes = std::fs::read(path).with_context(|| format!("cannot read {}", path.display()))?;
    if bytes.len() < 8 {
        bail!("the dense layer is damaged: {}", path.display());
    }
    let header_len = u64::from_le_bytes(bytes[..8].try_into().unwrap()) as usize;
    let header: serde_json::Value = serde_json::from_slice(&bytes[8..8 + header_len])
        .context("cannot parse the safetensors header")?;
    let info = header
        .get("linear.weight")
        .context("the safetensors hold no linear.weight")?;
    let start = info["data_offsets"][0].as_u64().unwrap_or(0) as usize + 8 + header_len;
    let end = info["data_offsets"][1].as_u64().unwrap_or(0) as usize + 8 + header_len;
    let raw = bytes
        .get(start..end)
        .context("the safetensors are shorter than declared")?;
    let w: Vec<f32> = raw
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| f32::from_le_bytes(*b))
        .collect();
    if dim == 0 || !w.len().is_multiple_of(dim) {
        bail!("the text projection of {} numbers does not divide by the dimension {dim}", w.len());
    }
    Ok(w)
}

pub(super) fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x /= n;
        }
    }
}
