//! The tag dictionary, the scores, the embedding pass and the retag pass.

use super::download::model_bytes_in;
use super::model::{active_entry, tag_source, Model, ModelEntry, TextKind};
use crate::db::{self, FileRow, Progress};
use crate::i18n::{self, Lang};
use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

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
pub(super) fn defs_signature(defs: &[db::TagDef]) -> String {
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
