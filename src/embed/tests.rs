//! The tests of the catalogue, the tags, the vectors and the download guard.

use super::model::{average_pairs, ensure_dim, normalize, pad_rows};
use super::tags::defs_signature;
use super::*;
use crate::db;
use crate::derive;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

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
