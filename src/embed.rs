//! CLIP-эмбеддинги: локальная ONNX-модель, векторы в БД, теги, косинусный поиск, k-means.
//!
//! Модели лежат файлами в папке из настроек (по умолчанию `~/.cache/photocat/models/`) и
//! проверяются по sha256. Сеть — только за весами из каталога, по согласию владельца (ADR 0015).

use crate::db::{self, FileRow, Progress};
use crate::derive;
use crate::tools::{self, Tool};
use anyhow::{Context, Result, bail};
use ort::environment::Environment;
use ort::session::Session;
use ort::value::Tensor;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};
use tokenizers::Tokenizer;

pub const DIM: usize = 512;
pub const IMAGE_SIDE: u32 = 224;
/// Максимальная длина текста (sentence_bert_config.json: 128).
pub const MAX_TOKENS: usize = 128;
/// Пороги по умолчанию (их же хранят настройки; калибровано на корпусе).
#[allow(dead_code)]
pub const TAG_SHOW: f32 = 0.22;
/// Насколько первый тег должен отрываться от второго, чтобы ему можно было верить.
#[allow(dead_code)]
pub const TAG_MARGIN: f32 = 0.01;

/// Дефолтный тег: подпись и общие шаблоны промптов на двух языках.
/// Метки и промпты живут в БД (`tag_defs`): дефолт только засевает пустой словарь.
pub struct TagDefault {
    pub id: &'static str,
    pub label: &'static str,
    pub prompts_ru: &'static [&'static str],
    pub prompts_en: &'static [&'static str],
}

/// Версия дефолтного набора: пишется в `settings.tag_defs_version`.
/// Обновление дефолта не перезатирает правки владельца — только фиксирует, что засеяно.
pub const DEFAULT_TAG_VERSION: i64 = 2;

/// 14 общих подсказок по 2–3 шаблона. Никаких слов из конкретного архива: набор обязан
/// работать на любом каталоге (M15.4/M15.5, plan/M15-tags.md §4).
pub const DEFAULT_TAGS: [TagDefault; 14] = [
    TagDefault {
        id: "beach",
        label: "пляж",
        prompts_ru: &["фотография пляжа и моря", "на снимке песчаный пляж и вода", "пляж, море и песок"],
        prompts_en: &["a photo of a beach and the sea", "a sandy beach by the water", "beach, sea and sand"],
    },
    TagDefault {
        id: "document",
        label: "документ",
        prompts_ru: &["фотография документа, лист бумаги с текстом", "на снимке лист бумаги с напечатанным текстом", "страница документа крупным планом"],
        prompts_en: &["a photo of a document, a sheet of paper with text", "a sheet of paper with printed text", "a close-up of a document page"],
    },
    TagDefault {
        id: "screenshot",
        label: "скриншот",
        prompts_ru: &["снимок экрана, интерфейс программы", "на снимке экран компьютера или телефона", "скриншот интерфейса приложения"],
        prompts_en: &["a screenshot of a computer screen", "a screen of a computer or a phone", "a screenshot of an app interface"],
    },
    TagDefault {
        id: "food",
        label: "еда",
        prompts_ru: &["фотография еды, тарелка с едой", "на снимке еда на тарелке", "блюдо на столе крупным планом"],
        prompts_en: &["a photo of food, a plate with food", "food on a plate", "a close-up of a dish on the table"],
    },
    TagDefault {
        id: "animals",
        label: "животные",
        prompts_ru: &["фотография животного, кошка или собака", "на снимке домашнее животное", "кошка или собака крупным планом"],
        prompts_en: &["a photo of an animal, a cat or a dog", "a pet at home", "a close-up of a cat or a dog"],
    },
    TagDefault {
        id: "children",
        label: "дети",
        prompts_ru: &["фотография детей, ребёнок", "на снимке ребёнок", "дети на фотографии"],
        prompts_en: &["a photo of children, a child", "a child in the picture", "children on a photo"],
    },
    TagDefault {
        id: "holiday",
        label: "праздник",
        prompts_ru: &["праздник, торт со свечами, вечеринка", "на снимке праздник и торт", "люди празднуют день рождения"],
        prompts_en: &["a party, a birthday cake with candles", "a celebration with a cake", "people celebrating a birthday"],
    },
    TagDefault {
        id: "furniture",
        label: "мебель",
        prompts_ru: &["фотография мебели, шкаф, стол, диван", "на снимке мебель в комнате", "шкаф, стол или диван крупным планом"],
        prompts_en: &["a photo of furniture, a cupboard, table or sofa", "furniture inside a room", "a close-up of a cupboard, table or sofa"],
    },
    TagDefault {
        id: "interior",
        label: "интерьер",
        prompts_ru: &["фотография комнаты, интерьер квартиры", "на снимке интерьер комнаты", "вид комнаты внутри"],
        prompts_en: &["a photo of a room interior", "the interior of a room", "a view inside a room"],
    },
    TagDefault {
        id: "clothes",
        label: "одежда",
        prompts_ru: &["фотография одежды, куртка, платье", "на снимке одежда", "куртка или платье крупным планом"],
        prompts_en: &["a photo of clothes, a jacket or a dress", "clothes in the picture", "a close-up of a jacket or a dress"],
    },
    TagDefault {
        id: "nature",
        label: "природа",
        prompts_ru: &["фотография природы, лес, поле, небо", "на снимке лес, поле или небо", "пейзаж с лесом и полем"],
        prompts_en: &["a photo of nature, forest, field or sky", "forest, field or sky", "a landscape with forest and field"],
    },
    TagDefault {
        id: "city",
        label: "город",
        prompts_ru: &["фотография улицы и домов, город", "на снимке городская улица", "дома и улица города"],
        prompts_en: &["a photo of a city street and buildings", "a city street", "buildings and a street in a city"],
    },
    TagDefault {
        id: "transport",
        label: "транспорт",
        prompts_ru: &["фотография транспорта, автомобиль, автобус", "на снимке автомобиль или автобус", "транспорт на дороге"],
        prompts_en: &["a photo of a vehicle, car or bus", "a car or a bus", "vehicles on the road"],
    },
    TagDefault {
        id: "plants",
        label: "растения",
        prompts_ru: &["фотография растений, цветы и деревья", "на снимке цветы и деревья", "растения крупным планом"],
        prompts_en: &["a photo of plants, flowers and trees", "flowers and trees", "a close-up of plants"],
    },
];

/// Как текстовая башня отдаёт вектор: через проекцию 768→512 или сразу `text_embeds`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TextKind {
    /// Multilingual DistilBERT + Dense 768→512 (русский поиск).
    Projected,
    /// CLIP text tower, выход `text_embeds` 512, без проекции (английский).
    Direct,
}

pub struct ModelFile {
    pub name: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub bytes: u64,
}

pub struct ModelEntry {
    /// Ключ эмбеддингов в БД.
    pub id: &'static str,
    /// Имя подкаталога в папке моделей (исторически `clip-vit-b32`, не меняем).
    pub dir: &'static str,
    pub title: &'static str,
    pub note: &'static str,
    pub kind: TextKind,
    pub files: &'static [ModelFile],
}

impl ModelEntry {
    pub fn bytes(&self) -> u64 {
        self.files.iter().map(|f| f.bytes).sum()
    }
    /// Влезает ли модель в лимит настроек (МиБ).
    pub fn fits(&self, max_mb: u64) -> bool {
        self.bytes() <= max_mb.saturating_mul(1024 * 1024)
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

static CATALOG: [ModelEntry; 2] = [
    ModelEntry {
        id: DEFAULT_MODEL,
        dir: "clip-vit-b32",
        title: "CLIP ViT-B/32, мультиязычная",
        note: "русский поиск и теги, лучшее качество",
        kind: TextKind::Projected,
        files: &MULTILINGUAL_FILES,
    },
    ModelEntry {
        id: "clip-vit-b32-en",
        dir: "clip-vit-b32-en",
        title: "CLIP ViT-B/32, английская",
        note: "меньше по размеру, запросы только по-английски",
        kind: TextKind::Direct,
        files: &ENGLISH_FILES,
    },
];

pub fn catalog() -> &'static [ModelEntry] {
    &CATALOG
}

/// Модель по id; пустой id — первая из каталога.
pub fn entry(id: &str) -> Option<&'static ModelEntry> {
    if id.is_empty() {
        CATALOG.first()
    } else {
        CATALOG.iter().find(|e| e.id == id)
    }
}

/// Активная модель из настроек; если id устарел — первая совместимая.
pub fn active_entry(settings: &db::Settings) -> &'static ModelEntry {
    entry(&settings.model_id).unwrap_or(&CATALOG[0])
}

pub fn tag_source(e: &ModelEntry) -> String {
    format!("clip:{}", e.id)
}

/// Дефолтный словарь модели: подписи русские, промпты — на языке её текстовой башни.
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
                TextKind::Direct => d.prompts_en,
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

/// Словарь модели из БД; пустой засевается дефолтом один раз.
/// Правки владельца не перезатираются: дефолт добавляется в пустую таблицу и
/// обновляется с новой версией только если словарь с момента засева не правили
/// (отпечаток `tag_defs_sig` совпадает).
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

/// Отпечаток словаря: им отличаем «стандартный набор, никто не правил» от правок.
/// Не криптография, а детектор изменений: FNV-1a по отсортированным строкам.
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

/// Вернуть стандартный словарь модели (явное действие владельца).
pub fn reset_to_defaults(db_path: &Path, e: &ModelEntry) -> Result<Vec<db::TagDef>> {
    let conn = db::open(db_path)?;
    let defs = default_defs(e.id, e.kind);
    db::replace_tag_defs(&conn, e.id, &defs)?;
    write_defs_meta(db_path, DEFAULT_TAG_VERSION, &defs)?;
    Ok(defs)
}

/// Заменить словарь импортом: это правка владельца, автообновление дефолта её не трогает.
pub fn import_defs(db_path: &Path, model: &str, defs: &[db::TagDef]) -> Result<()> {
    let conn = db::open(db_path)?;
    db::replace_tag_defs(&conn, model, defs)?;
    write_defs_meta(db_path, 0, defs)
}

/// Словарь в JSON — для обмена между машинами и резервной копии. Без путей архива.
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

/// Разобрать словарь из JSON. Неизвестные поля игнорируются, модель берётся из аргумента.
pub fn tag_defs_from_json(model: &str, text: &str) -> Result<Vec<db::TagDef>> {
    let value: serde_json::Value = serde_json::from_str(text).context("разбор JSON словаря")?;
    let arr = value
        .as_array()
        .context("ожидался массив тегов: [{id,label,prompts}]" )?;
    let mut defs: Vec<db::TagDef> = Vec::with_capacity(arr.len());
    let mut used: HashSet<String> = HashSet::new();
    for (i, v) in arr.iter().enumerate() {
        let label = v
            .get("label")
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .with_context(|| format!("тег #{i}: нет label"))?;
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
            bail!("метка «{label}» встречается в файле дважды");
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

/// Ключ для тега, добавленного без явного id: латиница/цифры, остальное — дефис.
/// Кириллическая метка даёт `tag`, поэтому `tag_defs_from_json` разводит повторы
/// (`tag-2`), а GUI при создании своего тега ставит `custom-N`.
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

/// Файлы модели на месте (без проверки sha256 — она при загрузке).
pub fn model_present_in(models_dir: &Path, e: &ModelEntry) -> bool {
    let dir = model_dir(models_dir, e);
    file_paths(&dir, e).iter().all(|p| p.is_file())
}

pub fn model_present(settings: &db::Settings) -> bool {
    model_present_in(&settings.models_dir, active_entry(settings))
}

/// Проверить наличие и sha256 всех файлов модели.
pub fn check_model_in(models_dir: &Path, e: &ModelEntry) -> Result<()> {
    let dir = model_dir(models_dir, e);
    for f in e.files {
        let path = dir.join(f.name);
        if !path.is_file() {
            bail!(
                "нет файлов модели «{}»: {}\nскачайте в «Настройках» или: bash scripts/fetch-model.sh",
                e.title,
                path.display()
            );
        }
        let got = derive::sha256_file(&path)
            .with_context(|| format!("не прочитать {}", path.display()))?;
        if got != f.sha256 {
            bail!(
                "файл модели повреждён ({}): {} \nsha256 {got}, ожидался {}\nскачайте заново в «Настройках»",
                f.name,
                path.display(),
                f.sha256
            );
        }
    }
    Ok(())
}

/// Размер файлов модели в байтах (для интерфейса и отчётов).
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

pub struct Model {
    _env: Environment,
    vision: Session,
    text: Session,
    tokenizer: Tokenizer,
    dense: Vec<f32>,
    /// Метка → векторы её промптов (по одному на шаблон); только включённые теги.
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
            .map_err(|e| anyhow::anyhow!("не загрузить токенизатор: {e}"))?;
        let dense = if entry.kind == TextKind::Projected {
            read_dense(&f("text_dense.safetensors"))?
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

    /// Подготовить векторы промптов из словаря: 2–3 шаблона на метку.
    /// Вес тега — лучший из его шаблонов, а не среднее: описание предмета может
    /// совпасть только с одной формулировкой, и среднее такую догадку размывает.
    /// Векторы фото не пересчитываются — правка словаря стоит один прогон текстовой башни.
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
        Ok((ids, mask, batch, max_len))
    }

    /// Текстовые эмбеддинги (для запроса и промптов). Векторы нормализованы.
    pub fn encode_texts(&mut self, texts: &[&str]) -> Result<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }
        let (ids, mask, batch, max_len) = self.tokenize(texts)?;
        let mut out = Vec::with_capacity(batch);
        if self.entry.kind == TextKind::Direct {
            let outputs = self.text.run(ort::inputs![
                "input_ids" => Tensor::from_array((vec![batch as i64, max_len as i64], ids)).map_err(oe)?,
            ]).map_err(oe)?;
            let (shape, data) = outputs["text_embeds"]
                .try_extract_tensor::<f32>()
                .map_err(oe)?;
            let dim = *shape.last().unwrap_or(&0) as usize;
            if dim != DIM {
                bail!("текстовая модель вернула {dim} признаков, ожидалось {DIM}");
            }
            for row in data.chunks(DIM) {
                let mut v = row.to_vec();
                normalize(&mut v);
                out.push(v);
            }
            if out.len() != batch {
                bail!("текстовая модель вернула {} векторов на {batch} запросов", out.len());
            }
            return Ok(out);
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
    ///
    /// На каждый снимок модель видит два кадра — центр-кроп и весь кадр с полями;
    /// векторы усредняются и снова нормализуются. Один вектор на снимок в БД.
    pub fn embed_thumbs(&mut self, thumbs: &[PathBuf]) -> Result<Vec<Vec<f32>>> {
        if thumbs.is_empty() {
            return Ok(Vec::new());
        }
        let plane = (IMAGE_SIDE * IMAGE_SIDE) as usize;
        let mut data = Vec::with_capacity(thumbs.len() * 2 * plane * 3);
        for p in thumbs {
            let (center, whole) = derive::clip_inputs(p, IMAGE_SIDE)?;
            data.extend_from_slice(&center);
            data.extend_from_slice(&whole);
        }
        let values = self.run_vision(data, thumbs.len() * 2)?;
        let out = average_pairs(&values, thumbs.len(), DIM);
        if out.len() != thumbs.len() {
            bail!("зрительная модель вернула {} векторов на {} картинок", out.len(), thumbs.len());
        }
        Ok(out)
    }

    /// Прогнать зрительную модель по готовым CHW-входам; векторы нормализованы.
    /// `n` — число кадров, для которых подготовлены данные (не снимков).
    fn run_vision(&mut self, data: Vec<f32>, n: usize) -> Result<Vec<f32>> {
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
        let count = values.len() / DIM;
        if count != n {
            bail!("зрительная модель вернула {count} векторов на {n} кадров");
        }
        let mut out = values.to_vec();
        for chunk in out.chunks_mut(DIM) {
            normalize(chunk);
        }
        Ok(out)
    }

    /// Теги для одного вектора: все включённые теги словаря с весом по убыванию.
    /// У метки несколько шаблонов — берётся её лучший вес.
    pub fn tag_scores(&self, image: &[f32]) -> Vec<(String, f32)> {
        let mut best: Vec<(String, f32)> = Vec::new();
        for (label, v) in &self.tag_vectors {
            let c = cosine(image, v);
            match best.iter_mut().find(|(l, _)| l == label) {
                Some((_, s)) => *s = s.max(c),
                None => best.push((label.clone(), c)),
            }
        }
        best.sort_by(|a, b| b.1.total_cmp(&a.1));
        best
    }

    /// Сколько включённых тегов сейчас в словаре.
    pub fn tag_count(&self) -> usize {
        let mut labels: Vec<&str> = self.tag_vectors.iter().map(|(l, _)| l.as_str()).collect();
        labels.sort_unstable();
        labels.dedup();
        labels.len()
    }
}

/// Фильтр по тегу: подходит любой вес выше порога, а не только самый верхний.
/// Так 14 подсказок не «съедают» друг друга: фото с детьми видно и в «дети», и в «одежда».
pub fn tag_above(scores: &[(String, f32)], want: &str, show: f32) -> bool {
    scores
        .iter()
        .any(|(tag, score)| tag == want && *score >= show)
}

/// Самый сильный вес среди тегов одного файла; `None`, если тегов нет.
pub fn max_score(scores: &[(String, f32)]) -> Option<f32> {
    scores.iter().map(|(_, s)| *s).reduce(f32::max)
}

/// Распределение весов каждого тега по каталогу, по убыванию — для процентильного порога.
/// Считается один раз при загрузке тегов, а не на каждый кадр интерфейса.
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

/// Распределение лучшего веса каждого файла, по убыванию — процентиль «все теги».
pub fn top_score_distribution(tags: &HashMap<i64, Vec<(String, f32)>>) -> Vec<f32> {
    let mut v: Vec<f32> = tags.values().filter_map(|t| max_score(t)).collect();
    v.sort_by(|a, b| b.total_cmp(a));
    v
}

/// Порог-процентиль по убывающему списку: 0% — все, 100% — только самый сильный.
/// Возвращает вес, при котором проходит не меньше `count` снимков, где `count` —
/// верхние `100 − pct` процентов (на 100% всегда хотя бы один). Пусто — `None`.
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

/// Усреднить векторы кадров одного снимка (по два на снимок) и нормализовать.
/// Без нормализации среднее двух единичных векторов короче единицы, и косинус с промптами врёт.
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

/// Уверенный тег с настраиваемыми порогами.
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

/// Пороги по умолчанию.
#[allow(dead_code)]
pub fn confident_tag(scores: &[(String, f32)]) -> Option<(&str, f32)> {
    confident_tag_with(scores, TAG_SHOW, TAG_MARGIN)
}

fn oe<R>(e: ort::Error<R>) -> anyhow::Error {
    anyhow::anyhow!("ONNX Runtime: {e}")
}

/// Одно ONNX-окружение на процесс: ORT не разрешает создать второе, а CLIP и лица живут рядом.
static ORT_ENV: OnceLock<Environment> = OnceLock::new();

pub(crate) fn shared_environment() -> Result<Environment> {
    if let Some(env) = ORT_ENV.get() {
        return Ok(env.clone());
    }
    let env = ort::init().with_name("photocat").build().map_err(oe)?;
    let _ = ORT_ENV.set(env.clone());
    Ok(env)
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
    pub errors: usize,
    pub seconds: f64,
    pub model_bytes: i64,
    /// Прогон прерван владельцем: остальные файлы остались на следующий раз.
    pub cancelled: bool,
}

impl std::fmt::Display for EmbedStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CLIP ({}): {}посчитано {}/{}, пропущено {}, ошибок {}, за {:.1} с, модель {}",
            self.model,
            if self.cancelled { "остановлено: " } else { "" },
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
/// `refresh` сбрасывает векторы активной модели: нужен после смены препроцессинга,
/// иначе ключ sha256 честно скажет «уже посчитано».
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
    if refresh {
        let n = db::clear_embeddings(&conn, entry.id)?;
        if let Some(p) = &progress {
            p.set_msg(format!("CLIP: сброшено {n} старых векторов — считаю заново"));
        }
    }
    let rows = db::load_all(&conn)?;
    let done = db::embedding_shas(&conn, entry.id)?;
    let todo: Vec<&FileRow> = rows
        .iter()
        .filter(|r| r.thumb.is_some())
        .filter(|r| done.get(&r.id) != Some(&r.sha256))
        .collect();
    let total = rows.iter().filter(|r| r.thumb.is_some()).count();
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
            model: entry.id,
            total,
            computed: 0,
            skipped,
            errors: 0,
            seconds: t0.elapsed().as_secs_f64(),
            model_bytes: model_bytes_in(&settings.models_dir, entry),
            cancelled: false,
        });
    }
    let mut model = Model::load(&settings, threads)?;
    // Словарь тегов — из БД: правки владельца и дефолт засеяны до прогона.
    let defs = ensure_tag_defs(db_path, entry)?;
    model.set_tag_defs(&defs)?;
    if let Some(p) = &progress {
        p.set_msg(format!("CLIP: эмбеддинги, тегов в словаре {}…", model.tag_count()));
    }
    let thumbs_dir = db::cache_dir().join("thumbs");
    let mut computed = 0usize;
    let mut errors = 0usize;
    let batch = batch.clamp(1, 64);
    for part in todo.chunks(batch) {
        // «Стоп»: посчитанные пакеты уже в БД, остальные остаются на следующий запуск.
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
        p.set_msg("CLIP: готово");
        p.finish();
    }
    Ok(EmbedStats {
        model: entry.id,
        total,
        computed,
        skipped,
        errors,
        seconds: t0.elapsed().as_secs_f64(),
        model_bytes: model_bytes_in(&settings.models_dir, entry),
        cancelled: progress.as_ref().is_some_and(|p| p.cancelled()),
    })
}

/// Пересчитать теги по уже сохранённым векторам: новые подписи без повторного CLIP по фото.
pub struct RetagStats {
    pub model: &'static str,
    pub total: usize,
    pub tagged: usize,
    pub seconds: f64,
}

impl std::fmt::Display for RetagStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "теги ({}): пересчитано {}/{}, за {:.1} с",
            self.model, self.tagged, self.total, self.seconds
        )
    }
}

/// Одна кнопка «Посчитать теги»: сначала эмбеддинги для новых файлов, затем подписи по всем.
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

/// Пересчитать только теги: читает готовые эмбеддинги и промпты модели, без декодирования фото.
pub fn retag(db_path: &Path, threads: usize, progress: Option<Arc<Progress>>) -> Result<RetagStats> {
    let t0 = std::time::Instant::now();
    let conn = db::open(db_path)?;
    let settings = db::load_settings(&conn);
    let entry = active_entry(&settings);
    let rows = db::load_embeddings(&conn, entry.id)?;
    if let Some(p) = &progress {
        p.set_total(rows.len());
        p.set_msg("CLIP: пересчёт тегов без фото…");
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
        p.set_msg("CLIP: теги пересчитаны");
        p.finish();
    }
    Ok(RetagStats {
        model: entry.id,
        total: rows.len(),
        tagged,
        seconds: t0.elapsed().as_secs_f64(),
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
    let settings = db::load_settings(&conn);
    let entry = active_entry(&settings);
    let rows = db::load_all(&conn)?;
    let by_id: HashMap<i64, &FileRow> = rows.iter().map(|r| (r.id, r)).collect();
    let vectors = db::load_embeddings(&conn, entry.id)?;
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
        "Модель `{}`, файлов с эмбеддингами: {}, кластеров: {}, k={k}.\n\n",
        entry.id,
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

/// Итог загрузки: какая модель в итоге скачана и какие варианты не подошли.
pub struct DownloadReport {
    pub model: &'static str,
    pub bytes: u64,
    pub files: usize,
    pub fallbacks: Vec<String>,
}

impl std::fmt::Display for DownloadReport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "модель {}: файлов {}, {}",
            self.model,
            self.files,
            db::human(self.bytes as i64)
        )?;
        if !self.fallbacks.is_empty() {
            write!(f, " (не подошли: {})", self.fallbacks.join("; "))?;
        }
        Ok(())
    }
}

/// Скачать модель с согласия владельца. Сначала выбранная, затем — совместимые варианты
/// из каталога в пределах лимита размера (ADR 0015).
pub fn download_model(
    settings: &db::Settings,
    requested: &str,
    consent: bool,
    progress: Option<Arc<Progress>>,
) -> Result<DownloadReport> {
    if !consent {
        bail!("скачивание модели требует явного согласия владельца");
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
                "{} — {} больше лимита {} МиБ",
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
                });
            }
            Err(err) => failed.push(format!("{} — {err}", e.title)),
        }
    }
    bail!("не удалось скачать модель: {}", failed.join("; "))
}

fn download_entry(
    settings: &db::Settings,
    e: &'static ModelEntry,
    progress: Option<Arc<Progress>>,
) -> Result<usize> {
    let dir = model_dir(&settings.models_dir, e);
    download_files(settings, &dir, e.title, e.files, progress)
}

/// Скачать произвольный набор файлов модели (CLIP или лица) с проверкой sha256.
/// Общий путь нужен, чтобы загрузчик был один и `curl` запускался только здесь (ADR 0016).
pub(crate) fn download_files(
    settings: &db::Settings,
    dir: &Path,
    title: &str,
    model_files: &[ModelFile],
    progress: Option<Arc<Progress>>,
) -> Result<usize> {
    std::fs::create_dir_all(dir)
        .with_context(|| format!("не создать {}", dir.display()))?;
    if let Some(p) = &progress {
        p.reset();
        p.set_total(model_files.len());
        p.set_msg(format!("модель {title}: подготовка"));
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
                p.set_msg(format!("модель {title}: {}", f.name));
            }
            fetch_with_fallback(&settings.tools, f.url, &path)?;
            let got = derive::sha256_file(&path)
                .with_context(|| format!("не прочитать {}", path.display()))?;
            if got != f.sha256 {
                bail!(
                    "sha256 {} не совпал: {got}, ожидался {}",
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
        p.set_msg("модель скачана");
        p.finish();
    }
    Ok(files)
}

/// Основной адрес и зеркало: если HuggingFace недоступен, пробуем hf-mirror.
fn fetch_with_fallback(tools: &tools::Tools, url: &str, dst: &Path) -> Result<()> {
    let mirror = url.replace("https://huggingface.co/", "https://hf-mirror.com/");
    let mut last = String::new();
    for u in [url, mirror.as_str()] {
        if u.is_empty() {
            continue;
        }
        match curl_get(tools, u, dst) {
            Ok(()) => return Ok(()),
            Err(e) => last = format!("{u}: {e}"),
        }
    }
    bail!("{last}");
}

fn curl_get(tools: &tools::Tools, url: &str, dst: &Path) -> Result<()> {
    let mut cmd = tools.command(Tool::Curl);
    cmd.args([
        "-fL",
        "--retry",
        "3",
        "--connect-timeout",
        "20",
        "--max-time",
        "1800",
        "--silent",
        "--show-error",
        "-o",
    ])
    .arg(dst)
    .arg(url);
    let out = cmd.output().with_context(|| {
        format!(
            "не запустить curl ({}: {}): скачивание моделей требует curl",
            tools.source(Tool::Curl),
            tools.program(Tool::Curl)
        )
    })?;
    if !out.status.success() {
        bail!(
            "curl: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Дефолт: русские подписи, промпты на языке модели; ни одного слова из конкретного архива.
    #[test]
    fn дефолтный_словарь_общий_и_на_языке_модели() {
        let ru = default_defs("m", TextKind::Projected);
        let en = default_defs("m", TextKind::Direct);
        assert_eq!(ru.len(), DEFAULT_TAGS.len());
        assert_eq!(ru.len(), en.len());
        assert_eq!(ru[0].label, "пляж");
        assert!(ru[0].prompts[0].starts_with("фотография"));
        assert!(en[0].prompts[0].starts_with("a photo"));
        for d in &ru {
            assert!(!d.prompts.is_empty(), "у тега «{}» нет промптов", d.label);
            for bad in ["шкаф-купе", "гардероб", "чертёж", "комод"] {
                assert!(!d.prompts.iter().any(|p| p.contains(bad)), "в дефолте «{bad}»");
            }
        }
        // Порядок сохраняется, id — стабильные ключи.
        assert_eq!(ru[7].id, "furniture");
        assert_eq!(ru[7].position, 7);
    }

    /// Словарь переживает экспорт/импорт без потерь; теги без id получают уникальные ключи.
    #[test]
    fn словарь_переживает_json_roundtrip() {
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
        // Без id: ключи из метки, совпадающие ключи разводятся, а не затирают друг друга.
        let back = tag_defs_from_json(
            "m",
            r#"[{"label":"Шкаф","prompts":["фотография шкафа"]},{"label":"Комод","prompts":[]}]"#,
        )
        .unwrap();
        assert_eq!(back.len(), 2);
        assert_ne!(back[0].id, back[1].id);
        assert!(back.iter().all(|d| d.model == "m"));
    }

    /// Дефолт засевается один раз: правка владельца не перезатирается повторным запуском.
    #[test]
    fn словарь_засевается_один_раз_и_не_перезаписывает_правки() {
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

    /// Дефолт обновляется только в нетронутом словаре: правки владельца и импорт не трогаются.
    #[test]
    fn обновление_дефолта_не_затирает_правки_и_импорт() {
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
        // Старый засев: отпечаток совпадает с содержимым, версия ниже — набор догоняется.
        db::set_setting(&conn, "tag_defs_version", "1").unwrap();
        db::set_setting(&conn, "tag_defs_sig", &defs_signature(&defs)).unwrap();
        assert_eq!(ensure_tag_defs(&path, e).unwrap(), defs);
        // Правка владельца: метка другая, отпечаток остался от засева — не трогаем.
        let mut d = defs[0].clone();
        d.label = "море и берег".into();
        db::save_tag_def(&conn, &d).unwrap();
        db::set_setting(&conn, "tag_defs_version", "1").unwrap();
        let kept = ensure_tag_defs(&path, e).unwrap();
        assert!(kept.iter().any(|x| x.label == "море и берег"));
        // Импорт помечается как правка (версия 0) и автообновлением не затирается.
        let mut custom = defs.clone();
        custom[1].label = "бумага".into();
        import_defs(&path, e.id, &custom).unwrap();
        let after = ensure_tag_defs(&path, e).unwrap();
        assert!(after.iter().any(|x| x.label == "бумага"));
        // Сброс возвращает стандарт и снова разрешает обновления.
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
    fn процентиль_оставляет_верхнюю_долю_и_всегда_хотя_бы_один() {
        let scores = [0.30, 0.20, 0.10, 0.05];
        assert_eq!(percentile_threshold(&scores, 100), Some(0.30));
        assert_eq!(percentile_threshold(&scores, 0), Some(0.05));
        // 50% — верхние две записи; 70% — тоже две (ceil(0.3*4) = 2).
        assert_eq!(percentile_threshold(&scores, 50), Some(0.20));
        assert_eq!(percentile_threshold(&scores, 70), Some(0.20));
        assert_eq!(percentile_threshold(&scores, 90), Some(0.30));
        assert_eq!(percentile_threshold(&[], 70), None);
    }

    #[test]
    fn распределение_тегов_отсортировано_по_убыванию() {
        let mut tags = HashMap::new();
        tags.insert(1, vec![("мебель".to_string(), 0.1), ("дети".to_string(), 0.3)]);
        tags.insert(2, vec![("мебель".to_string(), 0.4), ("дети".to_string(), 0.2)]);
        let d = tag_distributions(&tags);
        assert_eq!(d["мебель"], vec![0.4, 0.1]);
        assert_eq!(d["дети"], vec![0.3, 0.2]);
        assert_eq!(top_score_distribution(&tags), vec![0.4, 0.3]);
        // На 100% у каждого тега остаётся хотя бы один снимок.
        for v in d.values() {
            assert!(percentile_threshold(v, 100).is_some());
        }
    }

    #[test]
    fn два_кадра_усредняются_и_снова_нормируются() {
        // Два единичных вектора под 90°: среднее должно стать 45°, длина 1.
        let values = [1.0f32, 0.0, 0.0, 1.0];
        let out = average_pairs(&values, 1, 2);
        assert_eq!(out.len(), 1);
        assert!((out[0][0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        assert!((out[0][1] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
        let norm: f32 = out[0].iter().map(|x| x * x).sum();
        assert!((norm - 1.0).abs() < 1e-6);
    }

    #[test]
    fn близкие_веса_дают_не_уверен_а_отрыв_даёт_тег() {
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
        // Ниже порога уверенности — тоже «не уверен», даже с отрывом.
        assert!(confident_tag_with(&clear, 0.35, 0.01).is_none());
    }

    #[test]
    fn фильтр_по_тегу_берёт_любой_вес_выше_порога() {
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
    #[ignore = "нужен PHOTOCAT_TAG_THUMB: путь к превью и веса модели"]
    fn диагностика_кадров_и_ансамбля() {
        let Some(path) = std::env::var_os("PHOTOCAT_TAG_THUMB").map(PathBuf::from) else {
            return;
        };
        let db_path = db::resolve_db(None).unwrap();
        let conn = db::open(&db_path).unwrap();
        let settings = db::load_settings(&conn);
        let mut m = Model::load(&settings, 4).unwrap();
        let (center, whole) = derive::clip_inputs(&path, IMAGE_SIDE).unwrap();
        let mut data = center;
        data.extend_from_slice(&whole);
        let values = m.run_vision(data, 2).unwrap();
        let shots = [
            ("центр-кроп", &values[..DIM]),
            ("весь кадр", &values[DIM..DIM * 2]),
        ];
        for (name, v) in shots {
            let tags = m.tag_scores(v);
            println!("{name}: {}", tags.iter().take(4)
                .map(|(t, s)| format!("{t} {s:.3}")).collect::<Vec<_>>().join(" / "));
        }
        let avg = average_pairs(&values, 1, DIM);
        let tags = m.tag_scores(&avg[0]);
        println!("ансамбль: {}", tags.iter().take(4)
            .map(|(t, s)| format!("{t} {s:.3}")).collect::<Vec<_>>().join(" / "));
    }

    #[test]
    #[ignore = "нужны веса модели и проиндексированный корпус"]
    fn model_roundtrip_on_real_photo() {
        let db_path = db::resolve_db(None).unwrap();
        let conn = db::open(&db_path).unwrap();
        let settings = db::load_settings(&conn);
        let rows = db::load_all(&conn).unwrap();
        let r = rows.iter().find(|r| r.thumb.is_some()).expect("нет фото с превью");
        let mut m = Model::load(&settings, 4).unwrap();
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
