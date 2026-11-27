//! CLIP embeddings: a local ONNX model, vectors in the DB, tags, cosine search, k-means.
//!
//! The models live as files in the folder from the settings (`~/.cache/photocat/models/` by
//! default) and are checked by sha256. The network is used only for weights from the catalogue, with the owner's consent (ADR 0015).

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::i18n::{self, Lang};
use anyhow::{Context, Result, bail};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokenizers::Tokenizer;

/// The default thresholds (the settings keep the same ones; calibrated on the corpus).
#[allow(dead_code)]
pub const TAG_SHOW: f32 = 0.22;
/// How far the first tag must lead the second for it to be trusted.
#[allow(dead_code)]
pub const TAG_MARGIN: f32 = 0.01;

/// A default tag: a label and general prompt templates in two languages.
/// The labels and prompts live in the DB (`tag_defs`): the default only seeds an empty dictionary.
pub struct TagDefault {
    pub id: &'static str,
    pub label: &'static str,
    pub prompts_ru: &'static [&'static str],
    pub prompts_en: &'static [&'static str],
}

/// The version of the default set: written to `settings.tag_defs_version`.
/// Updating the default does not overwrite the owner's edits — it only records what was seeded.
pub const DEFAULT_TAG_VERSION: i64 = 2;

/// 14 general hints, 2–3 templates each. No words from a particular archive: the set must
/// work on any folder (M15.4/M15.5, plan/M15-tags.md §4).
pub const DEFAULT_TAGS: [TagDefault; 14] = [
    TagDefault {
        id: "beach",
        label: "beach",
        prompts_ru: &["фотография пляжа и моря", "на снимке песчаный пляж и вода", "пляж, море и песок"],
        prompts_en: &["a photo of a beach and the sea", "a sandy beach by the water", "beach, sea and sand"],
    },
    TagDefault {
        id: "document",
        label: "document",
        prompts_ru: &["фотография документа, лист бумаги с текстом", "на снимке лист бумаги с напечатанным текстом", "страница документа крупным планом"],
        prompts_en: &["a photo of a document, a sheet of paper with text", "a sheet of paper with printed text", "a close-up of a document page"],
    },
    TagDefault {
        id: "screenshot",
        label: "screenshot",
        prompts_ru: &["снимок экрана, интерфейс программы", "на снимке экран компьютера или телефона", "скриншот интерфейса приложения"],
        prompts_en: &["a screenshot of a computer screen", "a screen of a computer or a phone", "a screenshot of an app interface"],
    },
    TagDefault {
        id: "food",
        label: "food",
        prompts_ru: &["фотография еды, тарелка с едой", "на снимке еда на тарелке", "блюдо на столе крупным планом"],
        prompts_en: &["a photo of food, a plate with food", "food on a plate", "a close-up of a dish on the table"],
    },
    TagDefault {
        id: "animals",
        label: "animals",
        prompts_ru: &["фотография животного, кошка или собака", "на снимке домашнее животное", "кошка или собака крупным планом"],
        prompts_en: &["a photo of an animal, a cat or a dog", "a pet at home", "a close-up of a cat or a dog"],
    },
    TagDefault {
        id: "children",
        label: "children",
        prompts_ru: &["фотография детей, ребёнок", "на снимке ребёнок", "дети на фотографии"],
        prompts_en: &["a photo of children, a child", "a child in the picture", "children on a photo"],
    },
    TagDefault {
        id: "holiday",
        label: "celebration",
        prompts_ru: &["праздник, торт со свечами, вечеринка", "на снимке праздник и торт", "люди празднуют день рождения"],
        prompts_en: &["a party, a birthday cake with candles", "a celebration with a cake", "people celebrating a birthday"],
    },
    TagDefault {
        id: "furniture",
        label: "furniture",
        prompts_ru: &["фотография мебели, шкаф, стол, диван", "на снимке мебель в комнате", "шкаф, стол или диван крупным планом"],
        prompts_en: &["a photo of furniture, a cupboard, table or sofa", "furniture inside a room", "a close-up of a cupboard, table or sofa"],
    },
    TagDefault {
        id: "interior",
        label: "interior",
        prompts_ru: &["фотография комнаты, интерьер квартиры", "на снимке интерьер комнаты", "вид комнаты внутри"],
        prompts_en: &["a photo of a room interior", "the interior of a room", "a view inside a room"],
    },
    TagDefault {
        id: "clothes",
        label: "clothes",
        prompts_ru: &["фотография одежды, куртка, платье", "на снимке одежда", "куртка или платье крупным планом"],
        prompts_en: &["a photo of clothes, a jacket or a dress", "clothes in the picture", "a close-up of a jacket or a dress"],
    },
    TagDefault {
        id: "nature",
        label: "nature",
        prompts_ru: &["фотография природы, лес, поле, небо", "на снимке лес, поле или небо", "пейзаж с лесом и полем"],
        prompts_en: &["a photo of nature, forest, field or sky", "forest, field or sky", "a landscape with forest and field"],
    },
    TagDefault {
        id: "city",
        label: "city",
        prompts_ru: &["фотография улицы и домов, город", "на снимке городская улица", "дома и улица города"],
        prompts_en: &["a photo of a city street and buildings", "a city street", "buildings and a street in a city"],
    },
    TagDefault {
        id: "transport",
        label: "transport",
        prompts_ru: &["фотография транспорта, автомобиль, автобус", "на снимке автомобиль или автобус", "транспорт на дороге"],
        prompts_en: &["a photo of a vehicle, car or bus", "a car or a bus", "vehicles on the road"],
    },
    TagDefault {
        id: "plants",
        label: "plants",
        prompts_ru: &["фотография растений, цветы и деревья", "на снимке цветы и деревья", "растения крупным планом"],
        prompts_en: &["a photo of plants, flowers and trees", "flowers and trees", "a close-up of plants"],
    },
];

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

/// The default dictionary of a model: labels are translated, prompts are in the language of its text tower.
pub fn default_defs(model: &str, kind: TextKind) -> Vec<db::TagDef> {
    DEFAULT_TAGS
        .iter()
        .enumerate()
        .map(|(i, d)| db::TagDef {
            model: model.to_string(),
            id: d.id.to_string(),
            label: d.label.to_string(),
            prompts: match kind {
                TextKind::Projected => d.prompts_ru,
                TextKind::Direct | TextKind::Pooler => d.prompts_en,
            }
            .iter()
            .map(|p| p.to_string())
            .collect(),
            enabled: true,
            position: i as i64,
            min_percentile: 0,
        })
        .collect()
}

/// The model dictionary from the DB; an empty one is seeded with the default once.
/// The owner's edits are not overwritten: the default is added to an empty table and
/// updated with a new version only if the dictionary has not been edited since the seed
/// (the `tag_defs_sig` fingerprint matches).
pub fn ensure_tag_defs(db_path: &Path, e: &ModelEntry) -> Result<Vec<db::TagDef>> {
    let conn = db::open(db_path)?;
    let defs = db::load_tag_defs(&conn, e.id)?;
    let version = db::get_setting(&conn, "tag_defs_version")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(0);
    let sig = db::get_setting(&conn, "tag_defs_sig").unwrap_or_default();
    let untouched = !defs.is_empty()
        && version > 0
        && version < DEFAULT_TAG_VERSION
        && !sig.is_empty()
        && sig == defs_signature(&defs);
    if !defs.is_empty() && !untouched {
        return Ok(defs);
    }
    let defs = default_defs(e.id, e.kind);
    db::replace_tag_defs(&conn, e.id, &defs)?;
    write_defs_meta(db_path, DEFAULT_TAG_VERSION, &defs)?;
    Ok(defs)
}

/// The dictionary fingerprint: it tells "the standard set, nobody edited it" from edits.
/// Not cryptography but a change detector: FNV-1a over the sorted strings.
fn defs_signature(defs: &[db::TagDef]) -> String {
    let mut rows: Vec<String> = defs
        .iter()
        .map(|d| format!("{}\t{}\t{}", d.id, d.label, d.prompts.join("|")))
        .collect();
    rows.sort();
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in rows.join("\n").as_bytes() {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100_0000_01b3);
    }
    format!("{h:016x}")
}

fn write_defs_meta(db_path: &Path, version: i64, defs: &[db::TagDef]) -> Result<()> {
    let conn = db::open(db_path)?;
    db::set_setting(&conn, "tag_defs_version", &version.to_string())?;
    db::set_setting(&conn, "tag_defs_sig", &defs_signature(defs))?;
    Ok(())
}

/// Return the standard dictionary of the model (an explicit action of the owner).
pub fn reset_to_defaults(db_path: &Path, e: &ModelEntry) -> Result<Vec<db::TagDef>> {
    let conn = db::open(db_path)?;
    let defs = default_defs(e.id, e.kind);
    db::replace_tag_defs(&conn, e.id, &defs)?;
    write_defs_meta(db_path, DEFAULT_TAG_VERSION, &defs)?;
    Ok(defs)
}

/// Replace the dictionary with an import: this is the owner's edit, the default update leaves it alone.
pub fn import_defs(db_path: &Path, model: &str, defs: &[db::TagDef]) -> Result<()> {
    let conn = db::open(db_path)?;
    db::replace_tag_defs(&conn, model, defs)?;
    write_defs_meta(db_path, 0, defs)
}

/// The dictionary in JSON — for moving between machines and for a backup. No archive paths.
pub fn tag_defs_to_json(defs: &[db::TagDef]) -> String {
    let arr: Vec<serde_json::Value> = defs
        .iter()
        .map(|d| {
            serde_json::json!({
                "id": d.id,
                "label": d.label,
                "prompts": d.prompts,
                "enabled": d.enabled,
                "position": d.position,
                "min_percentile": d.min_percentile,
            })
        })
        .collect();
    serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".into())
}

/// Parse a dictionary from JSON. Unknown fields are ignored, the model comes from the argument.
pub fn tag_defs_from_json(model: &str, text: &str) -> Result<Vec<db::TagDef>> {
    let value: serde_json::Value = serde_json::from_str(text).context("parsing the dictionary JSON")?;
    let arr = value
        .as_array()
        .context("expected an array of tags: [{id,label,prompts}]")?;
    let mut defs: Vec<db::TagDef> = Vec::with_capacity(arr.len());
    let mut used: HashSet<String> = HashSet::new();
    for (i, v) in arr.iter().enumerate() {
        let label = v
            .get("label")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .with_context(|| format!("tag #{i}: no label"))?;
        let base = v
            .get("id")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(str::to_string)
            .unwrap_or_else(|| slug(label));
        let mut id = base.clone();
        let mut n = 2;
        while !used.insert(id.clone()) {
            id = format!("{base}-{n}");
            n += 1;
        }
        if defs.iter().any(|d: &db::TagDef| d.label == label) {
            bail!("the label “{label}” occurs twice in the file");
        }
        let prompts: Vec<String> = v
            .get("prompts")
            .and_then(|x| x.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str())
                    .map(str::trim)
                    .filter(|p| !p.is_empty())
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        defs.push(db::TagDef {
            model: model.to_string(),
            id,
            label: label.to_string(),
            prompts,
            enabled: v.get("enabled").and_then(|x| x.as_bool()).unwrap_or(true),
            position: v
                .get("position")
                .and_then(|x| x.as_i64())
                .unwrap_or(i as i64),
            min_percentile: v
                .get("min_percentile")
                .and_then(|x| x.as_i64())
                .unwrap_or(0)
                .clamp(0, 100) as i32,
        });
    }
    Ok(defs)
}

/// A key for a tag added without an explicit id: Latin letters and digits, the rest becomes a dash.
/// A Cyrillic label gives `tag`, so `tag_defs_from_json` separates the repeats (`tag-2`),
/// and the GUI puts `custom-N` when creating its own tag.
pub fn slug(label: &str) -> String {
    let mut out = String::new();
    for c in label.chars() {
        if c.is_ascii_alphanumeric() {
            out.extend(c.to_lowercase());
        } else if !out.ends_with('-') {
            out.push('-');
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "tag".into()
    } else {
        out
    }
}

pub fn model_dir(models_dir: &Path, e: &ModelEntry) -> PathBuf {
    models_dir.join(e.dir)
}

fn file_paths(dir: &Path, e: &ModelEntry) -> Vec<PathBuf> {
    e.files.iter().map(|f| dir.join(f.name)).collect()
}

/// The model files are in place (without a sha256 check — that happens on download).
pub fn model_present_in(models_dir: &Path, e: &ModelEntry) -> bool {
    let dir = model_dir(models_dir, e);
    file_paths(&dir, e).iter().all(|p| p.is_file())
}

pub fn model_present(settings: &db::Settings) -> bool {
    model_present_in(&settings.models_dir, active_entry(settings))
}

/// Check the presence and the sha256 of every model file.
pub fn check_model_in(models_dir: &Path, e: &ModelEntry) -> Result<()> {
    let dir = model_dir(models_dir, e);
    for f in e.files {
        let path = dir.join(f.name);
        if !path.is_file() {
            bail!(
                "no files of the model “{}” in {}\ndownload it in the Settings or with `photocat models download --id {}`",
                e.title,
                dir.display(),
                e.id
            );
        }
        let got = derive::sha256_file(&path)
            .with_context(|| format!("cannot read {}", path.display()))?;
        if got != f.sha256 {
            bail!(
                "a model file is damaged ({}): {} \nsha256 {got}, expected {}\ndownload it again in the Settings",
                f.name,
                path.display(),
                f.sha256
            );
        }
    }
    Ok(())
}

/// The size of the model files in bytes (for the interface and the reports).
pub fn model_bytes_in(models_dir: &Path, e: &ModelEntry) -> i64 {
    let dir = model_dir(models_dir, e);
    file_paths(&dir, e)
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len() as i64)
        .sum()
}

pub fn check_model(settings: &db::Settings) -> Result<()> {
    check_model_in(&settings.models_dir, active_entry(settings))
}

/// Lay the token rows into one batch: pad to `max_len` on the needed side.
/// The tail of pads the tokenizer put there itself is removed: for SigLIP 2 it is on the
/// right, while the pooler reads the last token — EOS must remain the last one.
fn pad_rows(rows: &[Vec<i64>], max_len: usize, pad: i64, pad_left: bool) -> (Vec<i64>, Vec<i64>) {
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
    fn run_vision(&mut self, data: Vec<f32>, n: usize) -> Result<Vec<f32>> {
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

/// A percentile threshold over a descending list: 0% — all, 100% — only the strongest.
/// Returns the weight at which at least `count` photos pass, where `count` is the top
/// `100 − pct` percent (at 100% always at least one). Empty — `None`.
pub fn percentile_threshold(sorted_desc: &[f32], pct: i32) -> Option<f32> {
    let n = sorted_desc.len();
    if n == 0 {
        return None;
    }
    let pct = pct.clamp(0, 100);
    let count = if pct == 0 {
        n
    } else {
        ((((100 - pct) as f64 / 100.0) * n as f64).ceil() as usize).clamp(1, n)
    };
    Some(sorted_desc[count - 1])
}

/// Average the frame vectors of one photo (two per photo) and normalize.
/// Without normalization the mean of two unit vectors is shorter than one, and the cosine with prompts lies.
fn average_pairs(values: &[f32], images: usize, dim: usize) -> Vec<Vec<f32>> {
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

/// A confident tag with configurable thresholds.
pub fn confident_tag_with(
    scores: &[(String, f32)],
    show: f32,
    margin: f32,
) -> Option<(&str, f32)> {
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

/// The default thresholds.
#[allow(dead_code)]
pub fn confident_tag(scores: &[(String, f32)]) -> Option<(&str, f32)> {
    confident_tag_with(scores, TAG_SHOW, TAG_MARGIN)
}

/// A dimension check with text that shows both the obtained and the expected number.
fn ensure_dim(what: &str, got: usize, want: usize) -> Result<()> {
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

pub struct EmbedStats {
    pub model: &'static str,
    pub total: usize,
    pub computed: usize,
    pub skipped: usize,
    /// Files the pass did not take because their catalog has tags off (M26.9).
    pub pass_skipped: usize,
    pub errors: usize,
    pub seconds: f64,
    pub model_bytes: i64,
    /// The pass was stopped by the owner: the remaining files are left for next time.
    pub cancelled: bool,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for EmbedStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let prefix = i18n::t(
            self.lang,
            if self.cancelled {
                "stopped: "
            } else {
                ""
            },
        );
        let text = i18n::fill(
            self.lang,
            "CLIP ({}): {}computed {}/{}, skipped {}, errors {}, {} s, model {}",
            &[
                self.model.to_string(),
                prefix.to_string(),
                self.computed.to_string(),
                self.total.to_string(),
                self.skipped.to_string(),
                self.errors.to_string(),
                format!("{:.1}", self.seconds),
                db::human_in(self.lang, self.model_bytes),
            ],
        );
        f.write_str(&text)?;
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped — their folders have tags off)",
                &[self.pass_skipped.to_string()],
            );
            f.write_str(&note)?;
        }
        Ok(())
    }
}

/// Compute embeddings and tags for the images that have none yet (the key is sha256).
/// `refresh` drops the vectors of the active model: needed after a change of preprocessing,
/// otherwise the sha256 key honestly says "computed already".
pub fn embed_missing(
    db_path: &Path,
    threads: usize,
    batch: usize,
    progress: Option<Arc<Progress>>,
    refresh: bool,
) -> Result<EmbedStats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let entry = active_entry(&settings);
    let rows = db::load_all(&conn)?;
    // A switched-off folder takes no part in the pass (M26.1), and so does a catalog whose tags
    // are off (M26.9): the owner switches it back on to let the pass catch up. Nothing derived is
    // deleted by the switch — the existing vectors and tags stay.
    let all_rows: Vec<&FileRow> = rows
        .iter()
        .filter(|r| !settings.path_hidden(&r.path))
        .collect();
    let pass_skipped = all_rows
        .iter()
        .filter(|r| r.thumb.is_some() && !settings.pass_allows(&r.path, db::Pass::Tags))
        .count();
    let rows: Vec<&FileRow> = all_rows
        .iter()
        .copied()
        .filter(|r| settings.pass_allows(&r.path, db::Pass::Tags))
        .collect();
    if refresh {
        // Only the allowed roots lose their vectors: a pass switched off never deletes what a
        // previous pass produced (M26.9).
        let ids: Vec<i64> = rows.iter().map(|r| r.id).collect();
        let n = db::clear_embeddings_for(&conn, entry.id, &ids)?;
        if let Some(p) = &progress {
            p.set_msg(i18n::fill(
                settings.lang,
                "CLIP: dropped {} old vectors — computing them anew",
                &[n.to_string()],
            ));
        }
    }
    let done = db::embedding_shas(&conn, entry.id)?;
    let todo: Vec<&FileRow> = rows
        .iter()
        .copied()
        .filter(|r| r.thumb.is_some())
        .filter(|r| done.get(&r.id) != Some(&r.sha256))
        .collect();
    let total = rows.iter().filter(|r| r.thumb.is_some()).count();
    let skipped = total.saturating_sub(todo.len());
    if let Some(p) = &progress {
        p.set_total(todo.len());
        p.set_msg("loading the CLIP model…");
    }
    if todo.is_empty() {
        if let Some(p) = &progress {
            p.set_msg(i18n::t(settings.lang, "CLIP: everything is computed already"));
            p.finish();
        }
        return Ok(EmbedStats {
            model: entry.id,
            total,
            computed: 0,
            skipped,
            pass_skipped,
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            model_bytes: model_bytes_in(&settings.models_dir, entry),
            cancelled: false,
            lang: settings.lang,
        });
    }
    let mut model = Model::load(&settings, threads)?;
    // The tag dictionary comes from the DB: the owner's edits and the default are seeded before the pass.
    let defs = ensure_tag_defs(db_path, entry)?;
    model.set_tag_defs(&defs)?;
    if let Some(p) = &progress {
        p.set_msg(i18n::fill(
            settings.lang,
            "CLIP: embeddings, {} tags in the dictionary…",
            &[model.tag_count().to_string()],
        ));
    }
    let thumbs_dir = db::cache_dir().join("thumbs");
    let mut computed = 0usize;
    let mut errors = 0usize;
    let batch = batch.clamp(1, 64);
    for part in todo.chunks(batch) {
        // "Stop": the computed batches are in the DB already, the rest is left for the next run.
        if progress.as_ref().is_some_and(|p| p.cancelled()) {
            break;
        }
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
                // a batch may fail because of one broken preview — fall back to one by one
                if let Some(p) = &progress {
                    p.set_msg(i18n::fill(
                settings.lang,
                "CLIP: the batch failed ({}), one by one",
                &[e.to_string()],
            ));
                }
                let mut single = Vec::with_capacity(paths.len());
                for (r, path) in valid.iter().zip(&paths) {
                    match model.embed_thumbs(std::slice::from_ref(path)) {
                        Ok(v) => single.push(Some(v.into_iter().next().unwrap_or_default())),
                        Err(err) => {
                            eprintln!(
                "\n{}",
                i18n::fill(
                    settings.lang,
                    "CLIP: cannot decode {}: {}",
                    &[path.display().to_string(), err.to_string()],
                )
            );
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
            db::save_embedding(&tx, r.id, entry.id, &r.sha256, &vec)?;
            db::replace_tags(&tx, r.id, &tag_source(entry), &model.tag_scores(&vec))?;
            computed += 1;
            if let Some(p) = &progress {
                p.tick();
            }
        }
        tx.commit()?;
    }
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "CLIP: done"));
        p.finish();
    }
    Ok(EmbedStats {
        model: entry.id,
        total,
        computed,
        skipped,
        pass_skipped,
        errors,
        seconds: t0.elapsed().as_secs_f64(),
        model_bytes: model_bytes_in(&settings.models_dir, entry),
        cancelled: progress.as_ref().is_some_and(|p| p.cancelled()),
        lang: settings.lang,
    })
}

/// Recompute the tags from the stored vectors: new labels without running CLIP over the photos again.
pub struct RetagStats {
    pub model: &'static str,
    pub total: usize,
    pub tagged: usize,
    /// Vectors the pass left alone because their catalog has tags off (M26.9).
    pub pass_skipped: usize,
    pub seconds: f64,
    /// The language of the report (M21.6): the caller sets it before printing; English by default.
    pub lang: Lang,
}

impl std::fmt::Display for RetagStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let text = i18n::fill(
            self.lang,
            "tags ({}): recomputed {}/{}, {} s",
            &[
                self.model.to_string(),
                self.tagged.to_string(),
                self.total.to_string(),
                format!("{:.1}", self.seconds),
            ],
        );
        f.write_str(&text)?;
        if self.pass_skipped > 0 {
            let note = i18n::fill(
                self.lang,
                " ({} skipped — their folders have tags off)",
                &[self.pass_skipped.to_string()],
            );
            f.write_str(&note)?;
        }
        Ok(())
    }
}

/// One "Compute tags" button: embeddings for the new files first, then labels over all of them.
pub fn embed_and_retag(
    db_path: &Path,
    threads: usize,
    batch: usize,
    progress: Option<Arc<Progress>>,
) -> Result<(EmbedStats, RetagStats)> {
    let embed = embed_missing(db_path, threads, batch, progress, false)?;
    let tags = retag(db_path, threads, None)?;
    Ok((embed, tags))
}

/// Recompute tags only: reads the ready embeddings and the model prompts, without decoding photos.
pub fn retag(db_path: &Path, threads: usize, progress: Option<Arc<Progress>>) -> Result<RetagStats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let entry = active_entry(&settings);
    // A catalog with tags off keeps the tags it has; the pass simply does not touch them (M26.9).
    let paths = db::file_paths(&conn)?;
    let forbidden: HashSet<i64> = paths
        .iter()
        .filter(|(_, path)| !settings.pass_allows(path, db::Pass::Tags))
        .map(|(id, _)| *id)
        .collect();
    let all_rows = db::load_embeddings(&conn, entry.id)?;
    let pass_skipped = all_rows.iter().filter(|(id, _)| forbidden.contains(id)).count();
    let rows: Vec<(i64, Vec<f32>)> = all_rows
        .into_iter()
        .filter(|(id, _)| !forbidden.contains(id))
        .collect();
    if let Some(p) = &progress {
        p.set_total(rows.len());
        p.set_msg(i18n::t(settings.lang, "CLIP: recomputing the tags without the photos…"));
    }
    let mut model = Model::load(&settings, threads)?;
    let defs = ensure_tag_defs(db_path, entry)?;
    model.set_tag_defs(&defs)?;
    let mut tagged = 0usize;
    let tx = conn.unchecked_transaction()?;
    for (id, vec) in &rows {
        db::replace_tags(&tx, *id, &tag_source(entry), &model.tag_scores(vec))?;
        tagged += 1;
        if let Some(p) = &progress {
            p.tick();
        }
    }
    tx.commit()?;
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "CLIP: the tags are recomputed"));
        p.finish();
    }
    Ok(RetagStats {
        model: entry.id,
        total: rows.len(),
        tagged,
        pass_skipped,
        seconds: t0.elapsed().as_secs_f64(),
        lang: settings.lang,
    })
}

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

/// The result of a download: which model ended up downloaded and which options did not fit.
pub struct DownloadReport {
    pub model: &'static str,
    pub bytes: u64,
    pub files: usize,
    pub fallbacks: Vec<String>,
    /// The language of the report (M21.6): English by default.
    pub lang: Lang,
}

impl std::fmt::Display for DownloadReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut text = i18n::fill(
            self.lang,
            "model {}: {} files, {}",
            &[
                self.model.to_string(),
                self.files.to_string(),
                db::human_in(self.lang, self.bytes as i64),
            ],
        );
        if !self.fallbacks.is_empty() {
            text.push_str(&i18n::fill(
                self.lang,
                " (did not fit: {})",
                &[self.fallbacks.join("; ")],
            ));
        }
        f.write_str(&text)
    }
}

/// Download a model with the owner's consent. The chosen one first, then the compatible
/// options from the catalogue within the size limit (ADR 0015).
pub fn download_model(
    settings: &db::Settings,
    requested: &str,
    consent: bool,
    progress: Option<Arc<Progress>>,
) -> Result<DownloadReport> {
    if !consent {
        bail!(
            "{}",
            i18n::t(settings.lang, "downloading a model needs the owner's explicit consent")
        );
    }
    let max_bytes = settings.max_model_mb.saturating_mul(1024 * 1024);
    let mut candidates: Vec<&'static ModelEntry> = Vec::new();
    if let Some(e) = entry(requested) {
        candidates.push(e);
    }
    let mut rest: Vec<&'static ModelEntry> = catalog()
        .iter()
        .filter(|e| candidates.iter().all(|c| c.id != e.id))
        .collect();
    rest.sort_by_key(|e| e.bytes());
    candidates.extend(rest);

    let mut failed = Vec::new();
    for e in candidates {
        if max_bytes > 0 && e.bytes() > max_bytes {
            failed.push(format!(
                "{}: {} is bigger than the limit of {} MiB",
                e.title,
                db::human(e.bytes() as i64),
                settings.max_model_mb
            ));
            continue;
        }
        match download_entry(settings, e, progress.clone()) {
            Ok(files) => {
                return Ok(DownloadReport {
                    model: e.id,
                    bytes: e.bytes(),
                    files,
                    fallbacks: failed,
                    lang: settings.lang,
                });
            }
            Err(err) => failed.push(format!("{} — {err}", e.title)),
        }
    }
    bail!("cannot download the model: {}", failed.join("; "))
}

fn download_entry(
    settings: &db::Settings,
    e: &'static ModelEntry,
    progress: Option<Arc<Progress>>,
) -> Result<usize> {
    let dir = model_dir(&settings.models_dir, e);
    download_files(settings, &dir, e.title, e.files, progress)
}

/// Download an arbitrary set of model files (CLIP or faces) with a sha256 check.
/// A shared path so that there is one downloader (ADR 0047): the HTTP client is used only here.
pub(crate) fn download_files(
    settings: &db::Settings,
    dir: &Path,
    title: &str,
    model_files: &[ModelFile],
    progress: Option<Arc<Progress>>,
) -> Result<usize> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("cannot create {}", dir.display()))?;
    if let Some(p) = &progress {
        p.reset();
        p.set_total(model_files.len());
        p.set_msg(i18n::fill(settings.lang, "model {}: preparing", &[title.to_string()]));
    }
    let mut files = 0usize;
    for f in model_files {
        let path = dir.join(f.name);
        let ok = path.is_file()
            && derive::sha256_file(&path)
                .map(|got| got == f.sha256)
                .unwrap_or(false);
        if ok {
            files += 1;
        } else {
            if let Some(p) = &progress {
                p.set_msg(i18n::fill(
                    settings.lang,
                    "model {}: {}",
                    &[title.to_string(), f.name.to_string()],
                ));
            }
            fetch_with_fallback(f.url, &path, progress.as_deref(), title, f.name)?;
            let got = derive::sha256_file(&path)
                .with_context(|| format!("cannot read {}", path.display()))?;
            if got != f.sha256 {
                bail!(
                    "the sha256 of {} does not match: got {got}, expected {}",
                    f.name,
                    f.sha256
                );
            }
            files += 1;
        }
        if let Some(p) = &progress {
            p.tick();
        }
    }
    if let Some(p) = &progress {
        p.set_msg(i18n::t(settings.lang, "the model is downloaded"));
        p.finish();
    }
    Ok(files)
}

/// The main address and the mirror: if HuggingFace is unreachable, try hf-mirror.
fn fetch_with_fallback(
    url: &str,
    dst: &Path,
    progress: Option<&Progress>,
    title: &str,
    name: &str,
) -> Result<()> {
    let mirror = url.replace("https://huggingface.co/", "https://hf-mirror.com/");
    let mut last = String::new();
    for u in [url, mirror.as_str()] {
        if u.is_empty() {
            continue;
        }
        match http_get(u, dst, progress, title, name) {
            Ok(()) => return Ok(()),
            Err(e) => last = format!("{u}: {e}"),
        }
    }
    bail!("{last}");
}

/// The HTTP client of the weight downloader: one agent for the whole run, a connect timeout and a
/// total one, redirects followed (HuggingFace hands out CDN links). Timeouts on both sides keep a
/// stalled transfer from freezing the progress row forever.
fn http_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_connect(Some(std::time::Duration::from_secs(20)))
        .timeout_global(Some(std::time::Duration::from_secs(1800)))
        .user_agent(concat!("photocat/", env!("CARGO_PKG_VERSION")))
        .build()
        .into()
}

/// One GET into `dst`, three attempts, the body streamed straight to the file (a model is 200–400
/// MiB, it must not sit in memory). The only network call of the application (ADR 0047): a weight
/// file from the catalogue, never anything the owner has not asked for.
fn http_get(
    url: &str,
    dst: &Path,
    progress: Option<&Progress>,
    title: &str,
    name: &str,
) -> Result<()> {
    let agent = http_agent();
    let mut last = String::new();
    for attempt in 1..=3 {
        match copy_body(&agent, url, dst, progress, title, name) {
            Ok(()) => return Ok(()),
            Err(e) => {
                last = format!("attempt {attempt}: {e}");
                // A stopped transfer is not retried: "Stop" is the owner's answer, not a flaky network.
                if progress.is_some_and(|p| p.cancelled()) {
                    break;
                }
                // A short pause and a fresh attempt: the retry is for a flaky network, not for a 404.
                std::thread::sleep(std::time::Duration::from_secs(attempt));
            }
        }
    }
    bail!("{last}");
}

fn copy_body(
    agent: &ureq::Agent,
    url: &str,
    dst: &Path,
    progress: Option<&Progress>,
    title: &str,
    name: &str,
) -> Result<()> {
    let mut resp = agent
        .get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let total = resp.body().content_length();
    let mut body = resp.body_mut().as_reader();
    let mut file = std::io::BufWriter::new(
        std::fs::File::create(dst).with_context(|| format!("cannot create {}", dst.display()))?,
    );
    let mut buf = vec![0u8; 1 << 16];
    let mut done: u64 = 0;
    let mut shown: u64 = 0;
    loop {
        // "Stop" (M26.5): the model download is the one network transfer the owner can interrupt.
        // The partial file fails the sha256 check next time, so nothing half-written is trusted.
        if progress.is_some_and(|p| p.cancelled()) {
            bail!("the download was stopped");
        }
        let n = std::io::Read::read(&mut body, &mut buf)
            .with_context(|| format!("reading {url}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n])
            .with_context(|| format!("writing {}", dst.display()))?;
        done += n as u64;
        // A line per 16 MiB: the progress row says how far a 400 MiB model has got.
        if let Some(p) = progress
            && done - shown >= 16 * 1024 * 1024
        {
            shown = done;
            let msg = match total {
                Some(t) => format!(
                    "{title}: {name} — {} of {}",
                    db::human(done as i64),
                    db::human(t as i64)
                ),
                None => format!("{title}: {name} — {}", db::human(done as i64)),
            };
            p.set_msg(msg);
        }
    }
    std::io::Write::flush(&mut file).context("flushing the downloaded model")?;
    if let Some(t) = total
        && done != t
    {
        bail!("the transfer ended early: {done} of {t} bytes");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default: English labels, prompts in the language of the model; not a word from a particular archive.
    #[test]
    fn the_default_dictionary_is_shared_and_speaks_the_model_language() {
        let ru = default_defs("m", TextKind::Projected);
        let en = default_defs("m", TextKind::Direct);
        let sig = default_defs("m", TextKind::Pooler);
        assert_eq!(ru.len(), DEFAULT_TAGS.len());
        assert_eq!(ru.len(), en.len());
        assert_eq!(ru.len(), sig.len());
        assert!(sig[0].prompts[0].starts_with("a photo"));
        assert_eq!(ru[0].label, "beach");
        assert!(ru[0].prompts[0].starts_with("фотография"));
        assert!(en[0].prompts[0].starts_with("a photo"));
        for d in &ru {
            assert!(!d.prompts.is_empty(), "tag “{}” has no prompts", d.label);
            for bad in ["шкаф-купе", "гардероб", "чертёж", "комод"] {
                assert!(!d.prompts.iter().any(|p| p.contains(bad)), "the default holds “{bad}”");
            }
        }
        // The order is kept, the id is a stable key.
        assert_eq!(ru[7].id, "furniture");
        assert_eq!(ru[7].position, 7);
    }

    /// The dictionary survives export and import losslessly; tags without an id get unique keys.
    #[test]
    fn the_dictionary_survives_a_json_roundtrip() {
        let defs = default_defs("m", TextKind::Projected);
        let json = tag_defs_to_json(&defs);
        let back = tag_defs_from_json("m", &json).unwrap();
        assert_eq!(defs, back);
        let mut edited = defs.clone();
        edited[2].label = "экран".into();
        edited[2].prompts = vec!["снимок экрана".into(), "интерфейс программы".into()];
        edited[2].enabled = false;
        edited[2].min_percentile = 40;
        let back = tag_defs_from_json("m", &tag_defs_to_json(&[edited[2].clone()])).unwrap();
        assert_eq!(back[0], edited[2]);
        // Without an id: keys come from the label, colliding keys are separated, not overwritten.
        let back = tag_defs_from_json(
            "m",
            r#"[{"label":"Шкаф","prompts":["фотография шкафа"]},{"label":"Комод","prompts":[]}]"#,
        )
        .unwrap();
        assert_eq!(back.len(), 2);
        assert_ne!(back[0].id, back[1].id);
        assert!(back.iter().all(|d| d.model == "m"));
    }

    /// The default is seeded once: the owner's edits are not overwritten by a repeated run.
    #[test]
    fn the_dictionary_is_seeded_once_and_keeps_the_edits() {
        let path = std::env::temp_dir().join(format!(
            "photocat-tagdefs-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let e = &catalog()[0];
        let defs = ensure_tag_defs(&path, e).unwrap();
        assert_eq!(defs.len(), DEFAULT_TAGS.len());
        let mut d = defs[0].clone();
        d.label = "пляж и море".into();
        d.prompts = vec!["фотография пляжа".into()];
        db::save_tag_def(&db::open(&path).unwrap(), &d).unwrap();
        let again = ensure_tag_defs(&path, e).unwrap();
        assert!(again.iter().any(|x| x.label == "пляж и море"));
        assert_eq!(again.len(), DEFAULT_TAGS.len());
    }

    /// The default is updated only in an untouched dictionary: the owner's edits and an import are left alone.
    #[test]
    fn a_default_update_keeps_the_edits_and_the_import() {
        let path = std::env::temp_dir().join(format!(
            "photocat-tagup-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        let e = &catalog()[0];
        let defs = ensure_tag_defs(&path, e).unwrap();
        let conn = db::open(&path).unwrap();
        // An old seed: the fingerprint matches the content, the version is lower — the set is caught up.
        db::set_setting(&conn, "tag_defs_version", "1").unwrap();
        db::set_setting(&conn, "tag_defs_sig", &defs_signature(&defs)).unwrap();
        assert_eq!(ensure_tag_defs(&path, e).unwrap(), defs);
        // The owner's edit: the label differs, the fingerprint is left from the seed — do not touch it.
        let mut d = defs[0].clone();
        d.label = "море и берег".into();
        db::save_tag_def(&conn, &d).unwrap();
        db::set_setting(&conn, "tag_defs_version", "1").unwrap();
        let kept = ensure_tag_defs(&path, e).unwrap();
        assert!(kept.iter().any(|x| x.label == "море и берег"));
        // An import is marked as an edit (version 0) and is not overwritten by the automatic update.
        let mut custom = defs.clone();
        custom[1].label = "бумага".into();
        import_defs(&path, e.id, &custom).unwrap();
        let after = ensure_tag_defs(&path, e).unwrap();
        assert!(after.iter().any(|x| x.label == "бумага"));
        // A reset returns the standard and allows updates again.
        let reset = reset_to_defaults(&path, e).unwrap();
        assert_eq!(reset, defs);
        db::set_setting(&conn, "tag_defs_version", "1").unwrap();
        assert_eq!(ensure_tag_defs(&path, e).unwrap(), defs);
    }

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
    fn the_percentile_keeps_the_top_share_and_at_least_one() {
        let scores = [0.30, 0.20, 0.10, 0.05];
        assert_eq!(percentile_threshold(&scores, 100), Some(0.30));
        assert_eq!(percentile_threshold(&scores, 0), Some(0.05));
        // 50% — the top two entries; 70% — two as well (ceil(0.3*4) = 2).
        assert_eq!(percentile_threshold(&scores, 50), Some(0.20));
        assert_eq!(percentile_threshold(&scores, 70), Some(0.20));
        assert_eq!(percentile_threshold(&scores, 90), Some(0.30));
        assert_eq!(percentile_threshold(&[], 70), None);
    }

    #[test]
    fn the_tag_distribution_is_sorted_descending() {
        let mut tags = HashMap::new();
        tags.insert(1, vec![("мебель".to_string(), 0.1), ("дети".to_string(), 0.3)]);
        tags.insert(2, vec![("мебель".to_string(), 0.4), ("дети".to_string(), 0.2)]);
        let d = tag_distributions(&tags);
        assert_eq!(d["мебель"], vec![0.4, 0.1]);
        assert_eq!(d["дети"], vec![0.3, 0.2]);
        // At 100% every tag keeps at least one photo.
        for v in d.values() {
            assert!(percentile_threshold(v, 100).is_some());
        }
    }

    #[test]
    fn two_frames_are_averaged_and_renormalised() {
        // Two unit vectors at 90°: the mean must become 45°, length 1.
        let values = [1.0f32, 0.0, 0.0, 1.0];
        let out = average_pairs(&values, 1, 2);
        assert_eq!(out.len(), 1);
        assert!((out[0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((out[0][1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        let norm: f32 = out[0].iter().map(|x| x * x).sum();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn close_weights_read_not_sure_while_a_gap_gives_the_tag() {
        let close = vec![
            ("документ".to_string(), 0.276),
            ("интерьер".to_string(), 0.271),
        ];
        assert!(confident_tag_with(&close, 0.22, 0.01).is_none());
        let clear = vec![
            ("мебель".to_string(), 0.30),
            ("интерьер".to_string(), 0.25),
        ];
        assert_eq!(confident_tag_with(&clear, 0.22, 0.01).unwrap().0, "мебель");
        // Below the confidence threshold is "not sure" too, even with a margin.
        assert!(confident_tag_with(&clear, 0.35, 0.01).is_none());
    }

    #[test]
    fn the_tag_filter_takes_any_weight_above_the_threshold() {
        let scores = vec![
            ("одежда".to_string(), 0.24),
            ("дети".to_string(), 0.23),
            ("мебель".to_string(), 0.10),
        ];
        assert!(tag_above(&scores, "дети", 0.22));
        assert!(tag_above(&scores, "одежда", 0.22));
        assert!(!tag_above(&scores, "мебель", 0.22));
        assert!(!tag_above(&scores, "город", 0.22));
    }

    #[test]
    #[ignore = "needs PHOTOCAT_TAG_THUMB: a preview path and the model weights"]
    fn the_diagnostics_of_frames_and_the_ensemble() {
        let Some(path) = std::env::var_os("PHOTOCAT_TAG_THUMB").map(PathBuf::from) else {
            return;
        };
        let db_path = db::resolve_db(None).unwrap();
        let conn = db::open(&db_path).unwrap();
        let settings = db::load_settings(&conn);
        let mut m = Model::load(&settings, 4).unwrap();
        let dim = m.entry.dim;
        let side = m.entry.image_side;
        let norm = m.entry.norm;
        let (center, whole) = derive::clip_inputs(&path, side, &norm).unwrap();
        let mut data = center;
        data.extend_from_slice(&whole);
        let values = m.run_vision(data, 2).unwrap();
        let shots = [
            ("center crop", &values[..dim]),
            ("whole frame", &values[dim..dim * 2]),
        ];
        for (name, v) in shots {
            let tags = m.tag_scores(v);
            println!("{name}: {}", tags.iter().take(4)
                .map(|(t, s)| format!("{t} {s:.3}")).collect::<Vec<_>>().join(" / "));
        }
        let avg = average_pairs(&values, 1, dim);
        let tags = m.tag_scores(&avg[0]);
        println!("ensemble: {}", tags.iter().take(4)
            .map(|(t, s)| format!("{t} {s:.3}")).collect::<Vec<_>>().join(" / "));
    }

    #[test]
    #[ignore = "needs the model weights and an indexed corpus"]
    fn model_roundtrip_on_real_photo() {
        let db_path = db::resolve_db(None).unwrap();
        let conn = db::open(&db_path).unwrap();
        let settings = db::load_settings(&conn);
        let rows = db::load_all(&conn).unwrap();
        let r = rows.iter().find(|r| r.thumb.is_some()).expect("no photo with a preview");
        let mut m = Model::load(&settings, 4).unwrap();
        let thumb = db::cache_dir().join("thumbs").join(r.thumb.as_ref().unwrap());
        let dim = m.entry.dim;
        let v = m.embed_thumbs(&[thumb]).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), dim);
        assert!((v[0].iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-3);
        let tags = m.tag_scores(&v[0]);
        println!("{}, tags: {:?}", r.path, tags);
        let t = m.encode_texts(&["пляж", "документ"]).unwrap();
        println!(
            "cosine: beach {:.3}, document {:.3}",
            cosine(&v[0], &t[0]),
            cosine(&v[0], &t[1])
        );
    }

    /// M12: SigLIP is another architecture with 768 features. It checks the real weights
    /// and previews: `PHOTOCAT_SIGLIP_MODELS` (the models folder) and `PHOTOCAT_SIGLIP_THUMB`.
    #[test]
    #[ignore = "needs the SigLIP weights and a preview: PHOTOCAT_SIGLIP_MODELS, PHOTOCAT_SIGLIP_THUMB"]
    fn siglip_gives_768_features() {
        let (Some(dir), Some(thumb)) = (
            std::env::var_os("PHOTOCAT_SIGLIP_MODELS").map(PathBuf::from),
            std::env::var_os("PHOTOCAT_SIGLIP_THUMB").map(PathBuf::from),
        ) else {
            return;
        };
        let settings = db::Settings {
            models_dir: dir,
            model_id: "siglip-base-224-en".into(),
            ..Default::default()
        };
        let mut m = Model::load(&settings, 4).unwrap();
        assert_eq!(m.entry.dim, 768);
        let v = m.embed_thumbs(&[thumb]).unwrap();
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].len(), 768);
        assert!((v[0].iter().map(|x| x * x).sum::<f32>() - 1.0).abs() < 1e-3);
        let t = m
            .encode_texts(&["a photo of a cat", "a photo of a document"])
            .unwrap();
        assert_eq!(t[0].len(), 768);
        let defs = default_defs(m.entry.id, m.entry.kind);
        m.set_tag_defs(&defs).unwrap();
        assert_eq!(m.tag_count(), 14);
        let tags = m.tag_scores(&v[0]);
        println!(
            "siglip: a 768-dim vector, cosine cat {:.3}, document {:.3}; tags: {}",
            cosine(&v[0], &t[0]),
            cosine(&v[0], &t[1]),
            tags.iter()
                .take(4)
                .map(|(t, s)| format!("{t} {s:.3}"))
                .collect::<Vec<_>>()
                .join(" / ")
        );
    }

    /// M12: the catalogue is data. The id and the folder are unique, every field is filled,
    /// the sha256 values are real, and the dimension is not hidden in a global constant.
    #[test]
    fn the_model_catalogue_is_described_by_data() {
        let mut ids = HashSet::new();
        let mut dirs = HashSet::new();
        let mut dims = HashSet::new();
        for e in catalog() {
            assert!(ids.insert(e.id), "repeated id {}", e.id);
            assert!(dirs.insert(e.dir), "repeated folder {}", e.dir);
            dims.insert(e.dim);
            assert!(e.dim > 0);
            assert!(e.image_side >= 224);
            assert!(e.max_tokens > 0);
            assert!(!e.title.is_empty() && !e.note.is_empty());
            assert!(!e.license.is_empty(), "{} has no license", e.id);
            assert!(e.bytes() > 0 && !e.files.is_empty());
            assert!(e.files.iter().any(|f| f.name == "tokenizer.json"));
            for f in e.files {
                assert_eq!(f.sha256.len(), 64, "{}: sha256 is not 64 characters", f.name);
                assert!(f.sha256.bytes().all(|b| b.is_ascii_hexdigit()));
                assert!(f.bytes > 0);
            }
        }
        assert!(dims.len() >= 2, "the catalogue needs models of different dimensions, not one: {dims:?}");
        assert!(catalog().iter().any(|e| e.id == "siglip-base-224-en" && e.dim == 768));
        // A new model fits the default limit and does not fit the previous 160 MiB.
        let sig = entry("siglip-base-224-en").unwrap();
        assert!(sig.fits(250));
        assert!(!sig.fits(160));
        // M15.6: the multilingual SigLIP 2 does not fit into 250 MiB — it is honestly bigger than the limit,
        // and `models download` says so until the owner raises the limit to 400.
        let multi = entry("siglip2-base-224-multi").unwrap();
        assert_eq!(multi.dim, 768);
        assert!(!multi.fits(250), "393 MiB do not fit into 250");
        assert!(multi.fits(400));
        // The calibration is described for every model: only CLIP has the identity one.
        for e in catalog() {
            let (scale, shift) = e.score_calib;
            assert!(scale.is_finite() && scale > 0.0, "{}: scale {scale}", e.id);
            assert!(shift.is_finite());
            assert!(e.score(0.3).is_finite());
            assert!(e.score(0.3) > e.score(0.2), "{}: the calibration does not grow", e.id);
            if e.kind == TextKind::Pooler {
                assert_ne!(e.score_calib, (1.0, 0.0), "{}: the SigLIP scale is not calibrated", e.id);
                assert_eq!(e.score_version, 2);
            } else {
                assert_eq!(e.score_version, 1, "{}: CLIP never changed its scale", e.id);
            }
        }
        // An empty query means the first model; an unknown id — None, no panic.
        assert_eq!(entry("").unwrap().id, DEFAULT_MODEL);
        assert!(entry("no-such-model").is_none());
    }

    /// M15.6: every model in the catalogue is named and described in both languages — otherwise, in an English
    /// window the model row silently stays Russian (the tail of M21.6).
    #[test]
    fn the_model_catalogue_is_translated() {
        let ru = crate::i18n::Lang::Ru;
        for e in catalog() {
            assert_ne!(
                crate::i18n::t(ru, e.title),
                e.title,
                "model {} has no translated name",
                e.id
            );
            assert_ne!(
                crate::i18n::t(ru, e.note),
                e.note,
                "model {} has no translated description",
                e.id
            );
        }
    }

    /// M15.6: the calibration brings the SigLIP scale to the CLIP one — the weight and the margin of two
    /// models mean the same thing under the same thresholds. The numbers are medians of two sets
    /// (2340 files, `docs/ARCHITECTURE.md` § Measurements).
    #[test]
    fn the_tag_weight_calibration_equalises_the_scales() {
        let clip = entry(DEFAULT_MODEL).unwrap();
        assert_eq!(clip.score(0.086), 0.086, "the CLIP calibration is the identity");
        // Medians of top-1: CLIP 0.263, SigLIP 2 0.086, SigLIP 1 0.059.
        for (e, median, margin) in [
            (entry("siglip2-base-224-multi").unwrap(), 0.086_f32, 0.0083_f32),
            (entry("siglip-base-224-en").unwrap(), 0.059, 0.0216),
        ] {
            let s = e.score(median);
            assert!((s - 0.263).abs() < 0.006, "{}: median {s}", e.id);
            // The margin of 0.01 in the settings is the median margin of CLIP (0.0118).
            let (scale, _) = e.score_calib;
            let m = margin * scale;
            assert!((m - 0.0118).abs() < 0.002, "{}: lead {m}", e.id);
        }
        // Tag weights of different calibrations do not mix: the source is marked with the version.
        assert_eq!(tag_source(clip), "clip:clip-vit-b32-multilingual");
        assert_eq!(
            tag_source(entry("siglip2-base-224-multi").unwrap()),
            "clip:siglip2-base-224-multi@v2"
        );
    }

    /// M15.6: text padding. The SigLIP 2 pooler reads the last token, so EOS must
    /// remain the last one and the tokenizer pads must go to the left.
    #[test]
    fn text_padding_keeps_eos_last() {
        let eos = 1;
        let pad = 0;
        // The SigLIP 2 tokenizer has already padded the row to 8 on the right: [5, 6, eos, pad, pad, ...].
        let rows = vec![vec![5, 6, eos, pad, pad, pad, pad, pad], vec![7, eos, pad, pad, pad, pad, pad, pad]];
        let (ids, mask) = pad_rows(&rows, 8, pad, true);
        assert_eq!(&ids[0..8], &[pad, pad, pad, pad, pad, 5, 6, eos]);
        assert_eq!(&ids[8..16], &[pad, pad, pad, pad, pad, pad, 7, eos]);
        assert_eq!(mask[7], 1);
        assert_eq!(mask[0], 0);
        assert_eq!(ids.iter().filter(|&&v| v == eos).count(), 2, "exactly one EOS per row");
        // On the right (CLIP, SigLIP 1) — as before: the row from the start, pads in the tail.
        let (ids, mask) = pad_rows(&rows, 8, pad, false);
        assert_eq!(&ids[0..8], &[5, 6, eos, pad, pad, pad, pad, pad]);
        assert_eq!(mask[2], 1);
        // Long text does not go beyond max_len.
        let (ids, _) = pad_rows(&[vec![9, 9, 9, 9, 9]], 3, pad, false);
        assert_eq!(ids[..], [9, 9, 9]);
    }

    /// M12: the size comes from the model, and the error names both numbers and hints
    /// that vectors of different models do not mix.
    #[test]
    fn a_dimension_error_names_the_numbers() {
        assert!(ensure_dim("text", 768, 768).is_ok());
        let e = ensure_dim("text", 512, 768).unwrap_err().to_string();
        assert!(e.contains("512") && e.contains("768"), "no numbers: {e}");
        assert!(e.contains("do not mix"), "no hint: {e}");
    }

    /// M12: a download does not start without consent, even if the model is in the catalogue.
    #[test]
    fn a_download_without_consent_is_refused() {
        let settings = db::Settings::default();
        let res = download_model(&settings, "siglip-base-224-en", false, None);
        assert!(res.is_err(), "no download without consent");
        let e = res.err().unwrap().to_string();
        assert!(e.contains("consent"), "{e}");
    }

}
