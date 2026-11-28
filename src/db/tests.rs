//! The DB tests, kept at the module root: they use rows from every subject.
//!
//! `use super::*` reaches the re-exports; the few helpers that stay private to their subject are
//! imported from it by name.

use super::*;
use super::files::{delete_file_rows, mark_keep_conn};
use super::schema::{SCHEMA_VERSION, init};
use rusqlite::{Connection, params};
use std::path::PathBuf;

use crate::i18n::Lang;

fn row(path: &str, sha: &str, sharpness: f64) -> FileRow {
    FileRow {
        id: 0,
        path: path.into(),
        size: 1000,
        mtime: 0,
        sha256: sha.into(),
        kind: "image".into(),
        width: 100,
        height: 100,
        taken_at: None,
        taken_src: "mtime".into(),
        lat: None,
        lon: None,
        camera: None,
        dhash: Some(1),
        sharpness: Some(sharpness),
        duration_s: None,
        thumb: None,
        decision: None,
        trashed_at: None,
    }
}

fn id_of(c: &Connection, path: &str) -> i64 {
    c.query_row("SELECT id FROM files WHERE path=?1", [path], |r| r.get(0))
        .unwrap()
}

#[test]
fn faces_survive_split_and_are_reset_with_content() {
    let mut c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
    let file_id = id_of(&c, "/a.jpg");
    let face = FaceRow {
        id: 0,
        x: 10,
        y: 20,
        w: 30,
        h: 40,
        score: 0.9,
        quality: 0.8,
        thumb: Some("aa/a-0.jpg".into()),
        embedding: vec![1.0, 0.0],
        cluster_id: None,
        manual: false,
    };
    save_faces_for_file(&c, file_id, "aa", &[face]).unwrap();
    assert_eq!(load_faces(&c).unwrap().len(), 1);
    assert_eq!(
        face_scan_shas(&c).unwrap().get(&file_id),
        Some(&"aa".into())
    );
    let face_id = load_faces(&c).unwrap()[0].id;
    let clustered = create_face_cluster(&c).unwrap();
    save_face_assignments(&mut c, &[(face_id, Some(clustered))], false).unwrap();
    label_face_cluster(&c, clustered, "Лиза").unwrap();
    assert_eq!(load_face_clusters(&c).unwrap()[0].label, "Лиза");

    // The chips over the frame in the full screen: the box and the name of the group.
    let boxes = load_face_boxes(&c, file_id).unwrap();
    assert_eq!(boxes.len(), 1);
    assert_eq!((boxes[0].x, boxes[0].y, boxes[0].w, boxes[0].h), (10, 20, 30, 40));
    assert_eq!(boxes[0].label, "Лиза");
    assert_eq!(boxes[0].id, face_id);
    assert_eq!(boxes[0].cluster_id, Some(clustered));
    assert!((boxes[0].score - 0.9).abs() < 1e-6);

    let split = split_face(&mut c, face_id).unwrap();
    let moved = &load_faces(&c).unwrap()[0];
    assert_eq!(moved.cluster_id, Some(split));
    assert!(moved.manual);
    assert_ne!(split, clustered);

    // Replacing the content wipes the faces and the pass mark — they must not be shown.
    insert(&c, &row("/a.jpg", "bb", 1.0)).unwrap();
    assert!(load_faces(&c).unwrap().is_empty());
    assert!(face_scan_shas(&c).unwrap().is_empty());
}

/// M26.10: the picked faces leave the open group into their own new ones in one action; the
/// named groups that stay keep their names, and a group emptied by the split is cleared while
/// a named empty group is kept.
#[test]
fn splitting_picked_faces_moves_each_and_keeps_the_names() {
    let mut c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
    let file_id = id_of(&c, "/a.jpg");
    let face = |i: u8| FaceRow {
        id: 0,
        x: i as i32,
        y: 0,
        w: 10,
        h: 10,
        score: 0.9,
        quality: 0.8,
        thumb: None,
        embedding: vec![1.0, i as f32],
        cluster_id: None,
        manual: false,
    };
    save_faces_for_file(&c, file_id, "aa", &(0..5).map(face).collect::<Vec<_>>()).unwrap();
    let ids: Vec<i64> = load_faces(&c).unwrap().iter().map(|f| f.id).collect();
    let named = create_face_cluster(&c).unwrap();
    label_face_cluster(&c, named, "Lisa").unwrap();
    let other = create_face_cluster(&c).unwrap();
    label_face_cluster(&c, other, "Max").unwrap();
    let unnamed = create_face_cluster(&c).unwrap();
    save_face_assignments(
        &mut c,
        &[
            (ids[0], Some(named)),
            (ids[1], Some(named)),
            (ids[2], Some(named)),
            (ids[3], Some(other)),
            (ids[4], Some(unnamed)),
        ],
        false,
    )
    .unwrap();

    let out = split_faces(&mut c, &[ids[0], ids[1], 9_999]).unwrap();
    assert_eq!(out, SplitOutcome { moved: 2, missing: 1 });
    let rows = load_faces(&c).unwrap();
    let of = |id: i64| rows.iter().find(|f| f.id == id).unwrap().clone();
    let left_in_named = rows.iter().filter(|f| f.cluster_id == Some(named)).count();
    assert_eq!(left_in_named, 1, "the named group lost more than the two picked faces");
    assert_eq!(of(ids[2]).cluster_id, Some(named));
    assert_ne!(of(ids[0]).cluster_id, Some(named));
    assert_ne!(of(ids[0]).cluster_id, of(ids[1]).cluster_id);
    assert!(of(ids[0]).manual && of(ids[1]).manual);
    assert_eq!(of(ids[3]).cluster_id, Some(other));
    assert_eq!(of(ids[4]).cluster_id, Some(unnamed));
    let clusters = load_face_clusters(&c).unwrap();
    let label_of = |id: i64| {
        clusters
            .iter()
            .find(|cl| cl.id == id)
            .map(|cl| cl.label.clone())
            .unwrap_or_default()
    };
    assert_eq!(label_of(named), "Lisa");
    assert_eq!(label_of(other), "Max");

    // The last face of an unnamed group leaves: the empty group is cleared.
    split_faces(&mut c, &[ids[4]]).unwrap();
    delete_empty_face_clusters(&c).unwrap();
    let left: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM face_clusters WHERE id=?1",
            params![unnamed],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(left, 0, "the empty unnamed group was kept");
    // A named group left empty stays: the name is the owner's.
    split_faces(&mut c, &[ids[3]]).unwrap();
    delete_empty_face_clusters(&c).unwrap();
    let kept: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM face_clusters WHERE id=?1",
            params![other],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(kept, 1, "the named empty group was deleted");
}

#[test]
fn the_app_trash_hides_a_file_from_the_usual_views() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
    let id = id_of(&c, "/a.jpg");
    c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [id])
        .unwrap();
    assert!(load_all(&c).unwrap().is_empty());
    assert_eq!(load_trashed(&c).unwrap().len(), 1);
    c.execute("UPDATE files SET trashed_at=NULL WHERE id=?1", [id])
        .unwrap();
    assert_eq!(load_all(&c).unwrap().len(), 1);
    assert!(load_trashed(&c).unwrap().is_empty());
}

#[test]
fn the_batch_face_assignment_builds_a_named_group() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
    let file_id = id_of(&c, "/a.jpg");
    let mk = |x: i32| FaceRow {
        id: 0,
        x,
        y: 20,
        w: 30,
        h: 40,
        score: 0.9,
        quality: 0.8,
        thumb: None,
        embedding: vec![1.0, 0.0],
        cluster_id: None,
        manual: false,
    };
    save_faces_for_file(&c, file_id, "aa", &[mk(10), mk(60)]).unwrap();
    let ids: Vec<i64> = load_faces(&c).unwrap().iter().map(|f| f.id).collect();
    let cid = group_faces(&c, &ids, "Лиза").unwrap();
    let clusters = load_face_clusters(&c).unwrap();
    assert_eq!(clusters.len(), 1);
    assert_eq!(clusters[0].id, cid);
    assert_eq!(clusters[0].label, "Лиза");
    assert!(
        load_faces(&c)
            .unwrap()
            .iter()
            .all(|f| f.cluster_id == Some(cid) && f.manual)
    );
}

#[test]
fn schema_upgrade_keeps_old_db() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE files(
            id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
            mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
            taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
            thumb TEXT, indexed_at INTEGER NOT NULL);",
    )
    .unwrap();
    init(&c).unwrap();
    init(&c).unwrap(); // a repeated call does not fail
    let r = row("/old.jpg", "aa", 7.5);
    insert(&c, &r).unwrap();
    let loaded = load_all(&c).unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].sharpness, Some(7.5));
    assert_eq!(loaded[0].decision, None);
    assert_eq!(loaded[0].duration_s, None);
}

/// Schema 7: tags of different models live side by side, and a sha256 change drops every CLIP tag.
#[test]
fn migration_7_does_not_mix_the_tags_of_two_models() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE files(
            id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
            mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
            taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
            thumb TEXT, indexed_at INTEGER NOT NULL);
         CREATE TABLE tags(
            file_id INTEGER NOT NULL, tag TEXT NOT NULL, score REAL, source TEXT,
            PRIMARY KEY(file_id, tag));
         CREATE TRIGGER invalidate_clip_after_sha_change
         AFTER UPDATE OF sha256 ON files WHEN old.sha256 IS NOT new.sha256
         BEGIN
            DELETE FROM embeddings WHERE file_id = new.id;
            DELETE FROM tags WHERE file_id = new.id AND source = 'clip';
         END;
         INSERT INTO files(id,path,size,sha256,kind,indexed_at)
            VALUES(1,'/a.jpg',10,'aa','image',0);
         INSERT INTO tags(file_id,tag,score,source) VALUES
            (1,'пляж',0.30,'clip:m1');
         PRAGMA user_version=6;",
    )
    .unwrap();
    init(&c).unwrap();
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(load_tags(&c, "clip:m1").unwrap()[&1][0].0, "пляж");
    assert!(load_tags(&c, "clip:m2").unwrap().is_empty());
    // The key now allows both models on one file — it used to be a conflict.
    replace_tags(&c, 1, "clip:m1", &[("пляж".into(), 0.35)]).unwrap();
    replace_tags(&c, 1, "clip:m2", &[("пляж".into(), 0.45)]).unwrap();
    assert_eq!(load_tags(&c, "clip:m1").unwrap()[&1].len(), 1);
    assert_eq!(load_tags(&c, "clip:m2").unwrap()[&1].len(), 1);
    // A content change drops the tags of every model, not only the legacy `clip`.
    c.execute("UPDATE files SET sha256='bb' WHERE id=1", []).unwrap();
    assert!(load_tags(&c, "clip:m1").unwrap().is_empty());
    assert!(load_tags(&c, "clip:m2").unwrap().is_empty());
}

#[test]
fn hidden_tag_disappears_from_filter() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let r = row("/a.jpg", "aa", 10.0);
    insert(&c, &r).unwrap();
    let id = id_of(&c, "/a.jpg");
    replace_tags(&c, id, "clip", &[("пляж".into(), 0.3), ("еда".into(), 0.1)]).unwrap();
    assert_eq!(load_tags(&c, "clip").unwrap()[&id].len(), 2);
    c.execute(
        "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,'пляж',0)",
        params![id],
    )
    .unwrap();
    let left = load_tags(&c, "clip").unwrap();
    assert_eq!(left[&id].len(), 1);
    assert_eq!(left[&id][0].0, "еда");
}

#[test]
fn hidden_tag_cleared_when_file_content_changes() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let r = row("/a.jpg", "aa", 10.0);
    insert(&c, &r).unwrap();
    let id = id_of(&c, "/a.jpg");
    c.execute(
        "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,'пляж',0)",
        params![id],
    )
    .unwrap();
    let mut changed = r.clone();
    changed.sha256 = "bb".into();
    insert(&c, &changed).unwrap();
    assert_eq!(load_hidden_counts(&c).unwrap().len(), 0);
}

#[test]
fn fts_query_is_prefix_only_and_safe() {
    assert_eq!(
        fts_query("  справка 2024 ").as_deref(),
        Some("\"справка\"* AND \"2024\"*")
    );
    assert_eq!(fts_query(""), None);
    assert_eq!(fts_query("  \"*  "), None); // only special characters — not an FTS error, an empty query
    assert_eq!(fts_query("a\"b*").as_deref(), Some("\"ab\"*"));
}

#[test]
fn ocr_text_is_searchable_and_highlighted() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/doc.jpg", "aa", 1.0)).unwrap();
    let id = id_of(&c, "/doc.jpg");
    save_ocr(&c, id, "aa", "rus+eng", "Справка о доходах\nза 2024 год").unwrap();
    assert_eq!(ocr_summary(&c).unwrap(), (1, 29));
    assert_eq!(
        load_ocr(&c, id).unwrap().as_deref(),
        Some("Справка о доходах\nза 2024 год")
    );
    let hits = search_ocr(&c, "справ", 10).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].0, id);
    assert!(
        hits[0].1.contains('«') && hits[0].1.contains('»'),
        "{}",
        hits[0].1
    );
    assert!(search_ocr(&c, "договор", 10).unwrap().is_empty());
    // the FTS syntax must not turn user input into an error
    assert!(search_ocr(&c, "\"),", 10).is_ok());
}

#[test]
fn ocr_text_is_invalidated_when_content_changes() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let r = row("/doc.jpg", "aa", 1.0);
    insert(&c, &r).unwrap();
    let id = id_of(&c, "/doc.jpg");
    save_ocr(&c, id, "aa", "rus+eng", "квитанция").unwrap();
    let mut changed = r.clone();
    changed.sha256 = "bb".into();
    insert(&c, &changed).unwrap();
    assert_eq!(load_ocr(&c, id).unwrap(), None);
    assert!(search_ocr(&c, "квитанция", 10).unwrap().is_empty());
}

#[test]
fn top_tag_ignores_hidden_and_low_scores() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
    let id = id_of(&c, "/a.jpg");
    replace_tags(
        &c,
        id,
        "clip:m",
        &[("документ".into(), 0.30), ("дети".into(), 0.10)],
    )
    .unwrap();
    assert_eq!(load_visible_tags(&c).unwrap()[&id][0].0, "документ");
    c.execute(
        "INSERT INTO tag_hidden(file_id,tag,hidden_at) VALUES(?1,'документ',0)",
        params![id],
    )
    .unwrap();
    assert_eq!(load_visible_tags(&c).unwrap()[&id][0].0, "дети");
}

#[test]
fn the_old_tag_threshold_migrates_to_a_percentile() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    // An old DB: the filter slider drove tag_show down to 0.50, there is no percentile key.
    c.execute(
        "INSERT INTO settings(key,value) VALUES('tag_show','0.50')",
        [],
    )
    .unwrap();
    let s = load_settings(&c);
    assert_eq!(s.tag_show_pct, 70);
    assert!((s.tag_show - 0.22).abs() < 1e-6);
    // Once saved, the migration is fixed in the DB and does not run again.
    c.execute("UPDATE settings SET value='0.31' WHERE key='tag_show'", []).unwrap();
    save_settings(&c, &s).unwrap();
    c.execute("UPDATE settings SET value='0.31' WHERE key='tag_show'", []).unwrap();
    let back = load_settings(&c);
    assert_eq!(back.tag_show_pct, 70);
    assert!((back.tag_show - 0.31).abs() < 1e-6);
}

#[test]
fn settings_survive_roundtrip() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let s = Settings {
        model_id: "clip-vit-b32-en".into(),
        models_dir: PathBuf::from("/tmp/models"),
        max_model_mb: 160,
        tag_show: 0.27,
        tag_margin: 0.02,
        tag_show_pct: 42,
        tile_size: 210.0,
        grid_layout: GridLayout::Flat,
        grid_sort: GridSort::Name,
        lang: crate::i18n::Lang::Ru,
        ocr_lang: "rus".into(),
        tools: crate::tools::Tools {
            ffmpeg: "/opt/ffmpeg".into(),
            tesseract: "/opt/tesseract".into(),
            ..Default::default()
        },
        ..Default::default()
    };
    save_settings(&c, &s).unwrap();
    let back = load_settings(&c);
    assert_eq!(back.model_id, "clip-vit-b32-en");
    assert_eq!(back.models_dir, PathBuf::from("/tmp/models"));
    assert_eq!(back.max_model_mb, 160);
    assert!((back.tag_show - 0.27).abs() < 1e-6);
    assert_eq!(back.tag_show_pct, 42);
    assert!((back.tile_size - 210.0).abs() < 1e-6);
    assert_eq!(back.grid_layout, GridLayout::Flat);
    assert_eq!(back.grid_sort, GridSort::Name);
    assert_eq!(back.lang, crate::i18n::Lang::Ru);
    assert!((back.tag_margin - 0.02).abs() < 1e-6);
    assert_eq!(back.ocr_lang, "rus");
    assert_eq!(back.tools.ffmpeg, "/opt/ffmpeg");
    assert_eq!(back.tools.tesseract, "/opt/tesseract");
}

#[test]
fn an_unknown_grid_layout_takes_the_default() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    save_settings(&c, &Settings::default()).unwrap();
    c.execute(
        "UPDATE settings SET value='wide' WHERE key='grid_layout'",
        [],
    )
    .unwrap();
    c.execute("UPDATE settings SET value='?' WHERE key='grid_sort'", [])
        .unwrap();
    let back = load_settings(&c);
    assert_eq!(back.grid_layout, GridLayout::Months);
    assert_eq!(back.grid_sort, GridSort::DateDesc);
    assert_eq!(GridSort::parse("album"), GridSort::Album);
    assert_eq!(GridSort::Album.as_str(), "album");
}

/// "Due / not due" — every case from M10: no manifest, switched off, exactly due,
/// overdue, too early, and the clock turned back.
#[test]
fn the_reminder_counts_the_due_date_from_the_manifest() {
    let now = chrono::DateTime::parse_from_rfc3339("2026-11-24T12:00:00Z")
        .unwrap()
        .with_timezone(&chrono::Utc);
    let base = Settings {
        verify_root: "/tmp/архив".into(),
        verify_manifest: "/tmp/архив.sha256".into(),
        verify_interval_days: 30,
        ..Default::default()
    };
    // No manifest — nothing to remind about, even with a term set.
    let no_manifest = Settings {
        verify_manifest: String::new(),
        ..base.clone()
    };
    assert_eq!(verify_due(&no_manifest, now), VerifyDue::NoManifest);
    // Interval 0 — switched off by the owner.
    let off = Settings {
        verify_interval_days: 0,
        ..base.clone()
    };
    assert_eq!(verify_due(&off, now), VerifyDue::Disabled);
    // No last pass — due straight away.
    assert_eq!(verify_due(&base, now), VerifyDue::Due { days_over: 0 });
    // A broken date gives the same answer: better to remind than to stay silent.
    let broken = Settings {
        verify_last_at: "вчера".into(),
        ..base.clone()
    };
    assert_eq!(verify_due(&broken, now), VerifyDue::Due { days_over: 0 });
    // Exactly 30 days — due.
    let exactly = Settings {
        verify_last_at: (now - chrono::Duration::days(30)).to_rfc3339(),
        ..base.clone()
    };
    assert_eq!(verify_due(&exactly, now), VerifyDue::Due { days_over: 0 });
    // Overdue by 5 days.
    let overdue = Settings {
        verify_last_at: (now - chrono::Duration::days(35)).to_rfc3339(),
        ..base.clone()
    };
    assert_eq!(verify_due(&overdue, now), VerifyDue::Due { days_over: 5 });
    // Too early: 29 days and 12 hours out of 30 — one day is left.
    let early = Settings {
        verify_last_at: (now - chrono::Duration::days(29) - chrono::Duration::hours(12))
            .to_rfc3339(),
        ..base.clone()
    };
    assert_eq!(verify_due(&early, now), VerifyDue::NotYet { days_left: 1 });
    // The clock was turned back: a date in the future — not due, and no panic.
    let future = Settings {
        verify_last_at: (now + chrono::Duration::days(5)).to_rfc3339(),
        ..base.clone()
    };
    assert_eq!(verify_due(&future, now), VerifyDue::NotYet { days_left: 35 });
}

#[test]
fn the_reminder_survives_a_restart() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let s = Settings {
        verify_root: "/tmp/архив".into(),
        verify_manifest: "/tmp/архив.sha256".into(),
        verify_interval_days: 14,
        verify_last_at: "2026-11-24T12:00:00+00:00".into(),
        verify_last_ok: true,
        verify_last_report: "сверка: всё совпало\n".into(),
        ..Default::default()
    };
    save_settings(&c, &s).unwrap();
    let back = load_settings(&c);
    assert_eq!(back.verify_root, "/tmp/архив");
    assert_eq!(back.verify_manifest, "/tmp/архив.sha256");
    assert_eq!(back.verify_interval_days, 14);
    assert_eq!(back.verify_last_at, "2026-11-24T12:00:00+00:00");
    assert!(back.verify_last_ok);
    assert_eq!(back.verify_last_report, "сверка: всё совпало\n");
}

#[test]
fn face_threshold_survives_roundtrip() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let s = Settings {
        face_threshold: 0.51,
        ..Default::default()
    };
    save_settings(&c, &s).unwrap();
    let back = load_settings(&c);
    assert!((back.face_threshold - 0.51).abs() < 1e-6);
}

/// M26.11: the two numbers of the matching step are settings, not a schema change: an absent or
/// broken value reads as the default, and a saved one comes back.
#[test]
fn the_matching_thresholds_survive_a_roundtrip() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    assert!((Settings::default().face_match - 0.50).abs() < 1e-6);
    assert!((Settings::default().face_match_margin - 0.03).abs() < 1e-6);
    let s = Settings {
        face_match: 0.62,
        face_match_margin: 0.05,
        ..Default::default()
    };
    save_settings(&c, &s).unwrap();
    let back = load_settings(&c);
    assert!((back.face_match - 0.62).abs() < 1e-6);
    assert!((back.face_match_margin - 0.05).abs() < 1e-6);
    // A broken value does not lose the setting: the default stands.
    c.execute("UPDATE settings SET value='not a number' WHERE key='face_match'", [])
        .unwrap();
    assert!((load_settings(&c).face_match - 0.50).abs() < 1e-6);
}

/// M26.17: the matching step writes proposals and moves nothing; a confirmed face joins the
/// person it resembles and is pinned, a turned-down one stays where it is and is pinned too.
/// A proposal to a person who loses the name (or a group that goes away) does not survive.
#[test]
fn the_quarantine_confirms_and_turns_down_without_moving_anything_else() {
    let mut c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/a.jpg", "aa", 1.0)).unwrap();
    let file_id = id_of(&c, "/a.jpg");
    let face = |i: u8| FaceRow {
        id: 0,
        x: i as i32,
        y: 0,
        w: 10,
        h: 10,
        score: 0.9,
        quality: 0.8,
        thumb: None,
        embedding: vec![1.0, i as f32],
        cluster_id: None,
        manual: false,
    };
    save_faces_for_file(&c, file_id, "aa", &[face(0), face(1), face(2)])
        .unwrap();
    let ids: Vec<i64> = load_faces(&c).unwrap().iter().map(|f| f.id).collect();
    let named = create_face_cluster(&c).unwrap();
    label_face_cluster(&c, named, "Lisa").unwrap();
    // The owner placed the third face in the group by hand.
    save_face_assignments(&mut c, &[(ids[2], Some(named))], true).unwrap();
    // The step proposes two faces for Lisa and moves neither.
    assert_eq!(
        replace_face_matches(&mut c, &[(ids[0], named, 0.77), (ids[1], named, 0.61)])
            .unwrap(),
        2
    );
    let proposals = load_face_proposals(&c).unwrap();
    assert_eq!(proposals.len(), 2);
    assert_eq!(proposals[0].face.id, ids[0], "the closest proposal is first");
    assert_eq!(proposals[0].target_label, "Lisa");
    assert!((proposals[0].similarity - 0.77).abs() < 1e-6);
    let faces = load_faces(&c).unwrap();
    for id in [ids[0], ids[1]] {
        assert_eq!(
            faces.iter().find(|f| f.id == id).unwrap().cluster_id,
            None,
            "a proposal moved a face"
        );
    }
    // A second step replaces the proposals and leaves the hand-placed face alone.
    assert_eq!(
        replace_face_matches(&mut c, &[(ids[1], named, 0.61)]).unwrap(),
        1
    );
    assert_eq!(load_face_proposals(&c).unwrap().len(), 1);
    // One confirmed: it joins Lisa and is pinned; the other stays proposed.
    assert_eq!(confirm_face_matches(&mut c, &[ids[1]]).unwrap(), 1);
    let faces = load_faces(&c).unwrap();
    let joined = faces.iter().find(|f| f.id == ids[1]).unwrap();
    assert_eq!(joined.cluster_id, Some(named));
    assert!(joined.manual, "a confirmed face must survive a rebuild");
    assert_eq!(load_face_proposals(&c).unwrap().len(), 0);
    // The turned-down face keeps its place and is pinned, so the next step leaves it.
    assert_eq!(
        replace_face_matches(&mut c, &[(ids[0], named, 0.77)]).unwrap(),
        1
    );
    assert_eq!(reject_face_matches(&mut c, &[ids[0]]).unwrap(), 1);
    let faces = load_faces(&c).unwrap();
    let turned_down = faces.iter().find(|f| f.id == ids[0]).unwrap();
    assert_eq!(turned_down.cluster_id, None);
    assert!(turned_down.manual);
    assert!(load_face_proposals(&c).unwrap().is_empty());
    // Nothing on disk is involved: every face row is still there.
    assert_eq!(load_faces(&c).unwrap().len(), 3);
    // A group that lost its name is not a person: the proposal is dropped, not confirmed.
    replace_face_matches(&mut c, &[(ids[0], named, 0.77)]).unwrap();
    c.execute("UPDATE face_clusters SET label='' WHERE id=?1", params![named])
        .unwrap();
    assert_eq!(confirm_face_matches(&mut c, &[ids[0]]).unwrap(), 0);
    assert!(load_face_proposals(&c).unwrap().is_empty());
    c.execute("UPDATE face_clusters SET label='Lisa' WHERE id=?1", params![named])
        .unwrap();
    // A proposal to a group that is gone goes with it.
    replace_face_matches(&mut c, &[(ids[0], named, 0.77)]).unwrap();
    c.execute("DELETE FROM face_clusters WHERE id=?1", params![named])
        .unwrap();
    delete_empty_face_clusters(&c).unwrap();
    assert!(load_face_proposals(&c).unwrap().is_empty());
}

/// M26.17: the proposal table of an older DB is created on the upgrade; a face an older
/// version matched by itself stays in its group and is pinned, so nothing jumps.
#[test]
fn the_upgrade_pins_the_old_automatic_matches() {
    let mut c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    insert(&c, &row("/old.jpg", "aa", 1.0)).unwrap();
    let file_id = id_of(&c, "/old.jpg");
    save_faces_for_file(
        &c,
        file_id,
        "aa",
        &[FaceRow {
            id: 0,
            x: 0,
            y: 0,
            w: 10,
            h: 10,
            score: 0.9,
            quality: 0.8,
            thumb: None,
            embedding: vec![1.0, 0.0],
            cluster_id: None,
            manual: false,
        }],
    )
    .unwrap();
    let id = load_faces(&c).unwrap()[0].id;
    let named = create_face_cluster(&c).unwrap();
    label_face_cluster(&c, named, "Lisa").unwrap();
    save_face_assignments(&mut c, &[(id, Some(named))], false).unwrap();
    // The state of a schema 9 row: in a named group, not pinned, with the cosine.
    c.execute(
        "UPDATE faces SET manual=0, match_score=0.61 WHERE id=?1",
        params![id],
    )
    .unwrap();
    c.pragma_update(None, "user_version", 9).unwrap();
    init(&c).unwrap();
    let face = load_faces(&c).unwrap().remove(0);
    assert_eq!(face.cluster_id, Some(named), "the face left its group on the upgrade");
    assert!(face.manual, "the old automatic match is not pinned");
    let score: Option<f32> = c
        .query_row("SELECT match_score FROM faces WHERE id=?1", [id], |r| r.get(0))
        .unwrap();
    assert_eq!(score, None, "the old “auto” mark survived the upgrade");
    assert!(load_face_proposals(&c).unwrap().is_empty());
}

#[test]
fn video_duration_survives_roundtrip() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let mut r = row("/v.mp4", "bb", 0.0);
    r.kind = "video".into();
    r.duration_s = Some(42.4);
    insert(&c, &r).unwrap();
    let loaded = load_all(&c).unwrap();
    assert_eq!(loaded[0].duration_s, Some(42.4));
}

#[test]
fn duration_is_readable_in_minutes_and_hours() {
    assert_eq!(duration(0.0), "0:00");
    assert_eq!(duration(12.4), "0:12");
    assert_eq!(duration(65.0), "1:05");
    assert_eq!(duration(3725.0), "1:02:05");
    assert_eq!(duration(-5.0), "0:00");
    assert_eq!(duration(f64::NAN), "0:00");
}

#[test]
fn clip_data_is_invalidated_when_content_changes() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let r = row("/a.jpg", "aa", 10.0);
    insert(&c, &r).unwrap();
    let id = id_of(&c, "/a.jpg");
    save_embedding(&c, id, "m", "aa", &[1.0, 0.0]).unwrap();
    replace_tags(&c, id, "clip", &[("пляж".into(), 0.3)]).unwrap();
    assert_eq!(embedding_shas(&c, "m").unwrap().len(), 1);
    assert_eq!(load_tags(&c, "clip").unwrap().len(), 1);

    let mut changed = r.clone();
    changed.sha256 = "bb".into();
    insert(&c, &changed).unwrap();
    assert!(embedding_shas(&c, "m").unwrap().is_empty());
    assert!(load_tags(&c, "clip").unwrap().is_empty());
}

#[test]
fn decision_survives_same_content_and_resets_on_change() {
    let mut c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let r = row("/a.jpg", "aa", 10.0);
    insert(&c, &r).unwrap();
    let id = id_of(&c, "/a.jpg");
    let other = row("/b.jpg", "bb", 20.0);
    insert(&c, &other).unwrap();
    let other_id = id_of(&c, "/b.jpg");
    mark_keep_conn(&mut c, id, &[other_id]).unwrap();

    let loaded = load_all(&c).unwrap();
    let a = loaded.iter().find(|r| r.path == "/a.jpg").unwrap();
    let b = loaded.iter().find(|r| r.path == "/b.jpg").unwrap();
    assert_eq!(a.decision.as_deref(), Some("keep"));
    assert_eq!(b.decision.as_deref(), Some("drop"));

    insert(&c, &r).unwrap(); // re-indexing the same file
    assert_eq!(load_all(&c).unwrap()[0].decision.as_deref(), Some("keep"));

    let mut changed = r.clone();
    changed.sha256 = "cc".into();
    insert(&c, &changed).unwrap(); // the file was replaced — the decision went stale
    assert_eq!(load_all(&c).unwrap()[0].decision, None);
}
fn three_files(c: &Connection) -> Vec<i64> {
    for (path, sha) in [("/a.jpg", "aa"), ("/b.jpg", "bb"), ("/c.jpg", "cc")] {
        insert(c, &row(path, sha, 1.0)).unwrap();
    }
    vec![id_of(c, "/a.jpg"), id_of(c, "/b.jpg"), id_of(c, "/c.jpg")]
}

#[test]
fn the_album_tree_follows_the_order_and_counts_files() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let dacha = create_album(&c, 0, "Дача 2024", "").unwrap();
    let may = create_album(&c, dacha, "Май", "").unwrap();
    let june = create_album(&c, dacha, "Июнь", "").unwrap();
    let year = create_album(&c, may, "1 год", "").unwrap();
    let to_print = create_album(&c, 0, "На печать", "").unwrap();
    add_to_album(&c, dacha, &[ids[0], ids[1]]).unwrap();
    add_to_album(&c, june, &[ids[2]]).unwrap();

    let tree = albums_tree(&c).unwrap();
    let names: Vec<&str> = tree.iter().map(|n| n.name.as_str()).collect();
    assert_eq!(names, ["Дача 2024", "Май", "1 год", "Июнь", "На печать"]);
    let depths: Vec<usize> = tree.iter().map(|n| n.depth).collect();
    assert_eq!(depths, [0, 1, 2, 1, 0]);
    let root = tree.iter().find(|n| n.id == dacha).unwrap();
    assert_eq!((root.files, root.subalbums), (2, 2));
    assert!(tree.iter().any(|n| n.id == year && n.files == 0));
    assert_eq!(album_path(&c, year).unwrap(), "Дача 2024 / Май / 1 год");
    assert_eq!(
        albums_of_file(&c, ids[0]).unwrap(),
        vec!["Дача 2024".to_string()]
    );

    // The order of subalbums is data: "May" can go below "June".
    move_album(&c, may, dacha, Some(1)).unwrap();
    let tree = albums_tree(&c).unwrap();
    let siblings: Vec<&str> = tree
        .iter()
        .filter(|n| n.parent_id == dacha)
        .map(|n| n.name.as_str())
        .collect();
    assert_eq!(siblings, ["Июнь", "Май"]);
    // The "To print" branch stayed in place.
    assert_eq!(
        tree.iter().find(|n| n.id == to_print).unwrap().position,
        1
    );
}

#[test]
fn an_album_carries_up_to_four_previews_in_its_order() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let mut ids = Vec::new();
    for i in 0..6 {
        let mut r = row(&format!("/{i}.jpg"), &format!("sha{i}"), 1.0);
        r.thumb = Some(format!("thumbs/{i}.jpg"));
        insert(&c, &r).unwrap();
        ids.push(id_of(&c, &format!("/{i}.jpg")));
    }
    let album = create_album(&c, 0, "Day", "").unwrap();
    // The album order decides which four previews the card gets, not the id order.
    add_to_album(&c, album, &[ids[3], ids[5], ids[1], ids[4], ids[0], ids[2]]).unwrap();
    let node = |c: &Connection| {
        albums_tree(c)
            .unwrap()
            .into_iter()
            .find(|n| n.id == album)
            .unwrap()
    };
    assert_eq!(
        node(&c).covers,
        ["thumbs/3.jpg", "thumbs/5.jpg", "thumbs/1.jpg", "thumbs/4.jpg"]
    );
    // A trashed file and a file without a preview drop out; the next ones fill the mosaic.
    c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [ids[3]])
        .unwrap();
    c.execute("UPDATE files SET thumb=NULL WHERE id=?1", [ids[5]])
        .unwrap();
    assert_eq!(
        node(&c).covers,
        ["thumbs/1.jpg", "thumbs/4.jpg", "thumbs/0.jpg", "thumbs/2.jpg"]
    );
}

#[test]
fn the_name_is_unique_among_siblings_and_survives_an_empty_one() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let dacha_2024 = create_album(&c, 0, "Дача 2024", "").unwrap();
    let dacha_2025 = create_album(&c, 0, "Дача 2025", "").unwrap();
    assert!(create_album(&c, 0, "дача 2024", "").is_err());
    // Equal names are allowed in different branches.
    create_album(&c, dacha_2024, "Май", "").unwrap();
    create_album(&c, dacha_2025, "Май", "").unwrap();
    assert!(create_album(&c, dacha_2024, "  ", "").is_err());
    assert!(create_album(&c, 0, &"я".repeat(81), "").is_err());
    // 80 characters are allowed already; trailing spaces are trimmed.
    let id = create_album(&c, 0, &format!("  {}  ", "я".repeat(80)), "").unwrap();
    assert_eq!(album_path(&c, id).unwrap().chars().count(), 80);
    assert!(rename_album(&c, dacha_2024, "Дача 2025").is_err());
    rename_album(&c, dacha_2024, "Дача на даче").unwrap();
    assert_eq!(album_path(&c, dacha_2024).unwrap(), "Дача на даче");
}

#[test]
fn moving_refuses_a_cycle_and_renumbers_the_siblings() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let a = create_album(&c, 0, "A", "").unwrap();
    let b = create_album(&c, a, "B", "").unwrap();
    let d = create_album(&c, b, "D", "").unwrap();
    let e = create_album(&c, 0, "E", "").unwrap();
    assert!(move_album(&c, a, b, None).is_err());
    assert!(move_album(&c, a, a, None).is_err());
    // Moving to the end and to the start: the positions stay dense 0..n−1.
    move_album(&c, d, 0, None).unwrap();
    let roots = |c: &Connection| -> Vec<String> {
        albums_tree(c)
            .unwrap()
            .into_iter()
            .filter(|n| n.parent_id == 0)
            .map(|n| n.name)
            .collect()
    };
    assert_eq!(roots(&c), ["A", "E", "D"]);
    assert_eq!(
        albums_tree(&c)
            .unwrap()
            .iter()
            .filter(|n| n.parent_id == 0)
            .map(|n| n.position)
            .collect::<Vec<_>>(),
        [0, 1, 2]
    );
    move_album(&c, d, 0, Some(0)).unwrap();
    assert_eq!(roots(&c), ["D", "A", "E"]);
    // The names stay unique among the new siblings.
    assert!(move_album(&c, e, a, None).is_ok());
    assert!(create_album(&c, a, "E", "").is_err());
}

#[test]
fn deleting_a_subtree_touches_rows_only() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let dacha = create_album(&c, 0, "Дача", "").unwrap();
    let may = create_album(&c, dacha, "Май", "").unwrap();
    let june = create_album(&c, dacha, "Июнь", "").unwrap();
    add_to_album(&c, may, &ids[..2]).unwrap();
    add_to_album(&c, june, &ids[2..]).unwrap();
    let to_print = create_album(&c, 0, "На печать", "").unwrap();
    add_to_album(&c, to_print, &ids).unwrap();

    assert_eq!(delete_album(&c, dacha).unwrap(), 3);
    assert!(albums_tree(&c).unwrap().iter().all(|n| n.name != "Май"));
    assert_eq!(album_items(&c, may).unwrap().len(), 0);
    assert_eq!(album_items(&c, to_print).unwrap().len(), 3);
    // The files on disk are not ours: the `files` rows are intact.
    assert_eq!(load_all(&c).unwrap().len(), 3);
    // "To print" stayed the only root with position 0.
    let root = &albums_tree(&c).unwrap()[0];
    assert_eq!((root.name.as_str(), root.position), ("На печать", 0));
}

#[test]
fn adding_is_idempotent_and_the_order_is_dense() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let album = create_album(&c, 0, "Май", "").unwrap();
    assert_eq!(add_to_album(&c, album, &ids).unwrap(), (3, 0));
    assert_eq!(add_to_album(&c, album, &ids).unwrap(), (0, 3));
    assert_eq!(add_to_album(&c, album, &[ids[0], ids[1]]).unwrap(), (0, 2));
    assert_eq!(add_to_album(&c, album, &[9999]).unwrap(), (0, 0));
    let order: Vec<i64> = album_items(&c, album)
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(order, ids);
    // "Up/Down" changes the order only, not the content.
    assert!(move_in_album(&c, album, ids[2], -1).unwrap());
    let order: Vec<i64> = album_items(&c, album)
        .unwrap()
        .iter()
        .map(|r| r.id)
        .collect();
    assert_eq!(order, [ids[0], ids[2], ids[1]]);
    assert!(!move_in_album(&c, album, ids[0], -1).unwrap());
    // The middle one was removed — the positions are 0,1,2 again without a hole.
    assert_eq!(remove_from_album(&c, album, &[ids[2]]).unwrap(), 1);
    let positions: Vec<i64> = {
        let mut st = c
            .prepare("SELECT position FROM album_files WHERE album_id=?1 ORDER BY position")
            .unwrap();
        let rows = st.query_map([album], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    assert_eq!(positions, [0, 1]);
}

#[test]
fn the_album_cards_follow_the_chosen_order_not_the_data() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let small = create_album(&c, 0, "Малый", "").unwrap();
    let big = create_album(&c, 0, "Большой", "").unwrap();
    let middle = create_album(&c, 0, "Средний", "").unwrap();
    add_to_album(&c, big, &ids).unwrap();
    add_to_album(&c, middle, &ids[..1]).unwrap();
    let order = |sort: AlbumSort| {
        let tree = albums_tree(&c).unwrap();
        let node = |id: i64| tree.iter().find(|n| n.id == id).unwrap().clone();
        let mut cards = vec![node(small), node(big), node(middle)];
        cards.sort_by(|a, b| album_order(a, b, sort));
        cards.into_iter().map(|n| n.name).collect::<Vec<_>>()
    };
    assert_eq!(order(AlbumSort::Position), ["Малый", "Большой", "Средний"]);
    assert_eq!(order(AlbumSort::Name), ["Большой", "Малый", "Средний"]);
    assert_eq!(order(AlbumSort::Count), ["Большой", "Средний", "Малый"]);
    assert_eq!(order(AlbumSort::Size), ["Большой", "Средний", "Малый"]);
    // "By update": the album that changed last goes first. The order on disk is untouched.
    c.execute(
        "UPDATE albums SET updated_at=?1 WHERE id=?2",
        params!["1714557600", small],
    )
    .unwrap();
    c.execute(
        "UPDATE albums SET updated_at=?1 WHERE id=?2",
        params!["1717236000", middle],
    )
    .unwrap();
    c.execute(
        "UPDATE albums SET updated_at=?1 WHERE id=?2",
        params!["1711965600", big],
    )
    .unwrap();
    assert_eq!(order(AlbumSort::Updated), ["Средний", "Малый", "Большой"]);
    let positions: Vec<i64> = albums_tree(&c).unwrap().iter().map(|n| n.position).collect();
    assert_eq!(positions, [0, 1, 2], "the album order is data, the cards are a look");
    // A parse of an unknown value means the default.
    assert_eq!(AlbumSort::parse("galleries"), AlbumSort::Position);
    assert_eq!(AlbumSort::parse("count"), AlbumSort::Count);
}

#[test]
fn a_change_of_the_contents_moves_the_album_card_up() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let album = create_album(&c, 0, "Май", "").unwrap();
    let stamp = |c: &Connection| {
        albums_tree(c)
            .unwrap()
            .iter()
            .find(|n| n.id == album)
            .unwrap()
            .updated_at
            .clone()
    };
    // A known old stamp: the date moves only when the contents really change.
    c.execute(
        "UPDATE albums SET updated_at='1' WHERE id=?1",
        [album],
    )
    .unwrap();
    assert_eq!(stamp(&c), "1");
    // An add of nothing changes nothing.
    add_to_album(&c, album, &[9999]).unwrap();
    assert_eq!(stamp(&c), "1", "nothing was added, nothing changed");
    add_to_album(&c, album, &ids[..1]).unwrap();
    assert_ne!(stamp(&c), "1", "the content changed");
    c.execute(
        "UPDATE albums SET updated_at='1' WHERE id=?1",
        [album],
    )
    .unwrap();
    remove_from_album(&c, album, &ids[..1]).unwrap();
    assert_ne!(stamp(&c), "1", "a removal is a change too");
}

#[test]
fn a_file_in_the_trash_stays_an_album_row() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let album = create_album(&c, 0, "Май", "").unwrap();
    add_to_album(&c, album, &ids).unwrap();
    c.execute("UPDATE files SET trashed_at=1 WHERE id=?1", [ids[1]])
        .unwrap();
    let node = &albums_tree(&c).unwrap()[0];
    assert_eq!((node.files, node.trashed), (3, 1));
    // The grid takes the undeleted ones only, the album data is not lost.
    let visible = album_items(&c, album)
        .unwrap()
        .into_iter()
        .filter(|r| r.trashed_at.is_none())
        .count();
    assert_eq!(visible, 2);
}

#[test]
fn deleting_the_rows_of_a_file_clears_the_album_and_compacts_the_order() {
    let mut c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let ids = three_files(&c);
    let album = create_album(&c, 0, "Май", "").unwrap();
    add_to_album(&c, album, &ids).unwrap();
    // The same as "drop a folder" and the index reset: `files` rows → `album_files`.
    delete_file_rows(&c, &[ids[1]]).unwrap();
    let left = album_items(&c, album).unwrap();
    assert_eq!(left.len(), 2);
    assert!(left.iter().all(|r| r.id != ids[1]));
    let positions: Vec<i64> = {
        let mut st = c
            .prepare("SELECT position FROM album_files WHERE album_id=?1 ORDER BY position")
            .unwrap();
        let rows = st.query_map([album], |r| r.get(0)).unwrap();
        rows.collect::<rusqlite::Result<Vec<_>>>().unwrap()
    };
    assert_eq!(positions, [0, 1]);
    let _ = &mut c;
}

/// The labels that make OCR look for text: every dictionary counts, a disabled tag does not.
#[test]
fn text_tag_labels_come_from_every_dictionary() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let mk = |model: &str, id: &str, label: &str, pos: i64| TagDef {
        model: model.into(),
        id: id.into(),
        label: label.into(),
        prompts: vec!["a photo".into()],
        enabled: true,
        position: pos,
        min_percentile: 0,
    };
    save_tag_def(&c, &mk("m1", "document", "document", 0)).unwrap();
    save_tag_def(&c, &mk("m1", "screenshot", "screenshot", 1)).unwrap();
    save_tag_def(&c, &mk("m1", "beach", "beach", 2)).unwrap();
    // The owner renamed the tag in a second model and switched one off in the first.
    save_tag_def(&c, &mk("m2", "document", "papers", 0)).unwrap();
    let mut off = mk("m1", "screenshot", "screenshot", 1);
    off.enabled = false;
    save_tag_def(&c, &off).unwrap();
    assert_eq!(text_tag_labels(&c), ["document", "papers"]);
}

/// The tag dictionary: an edit is saved in place, the label is unique, a removal clears one row.
#[test]
fn the_tag_dictionary_is_stored_in_the_db_per_model() {
    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    let mk = |id: &str, label: &str, pos: i64| TagDef {
        model: "m1".into(),
        id: id.into(),
        label: label.into(),
        prompts: vec!["фотография пляжа".into(), "море".into()],
        enabled: true,
        position: pos,
        min_percentile: 0,
    };
    save_tag_def(&c, &mk("beach", "пляж", 0)).unwrap();
    save_tag_def(&c, &mk("sea", "море", 1)).unwrap();
    let defs = load_tag_defs(&c, "m1").unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(defs[0].label, "пляж");
    assert_eq!(defs[0].prompts.len(), 2);
    // Editing by id: the label and the prompts change, the row is not duplicated.
    let mut d = defs[0].clone();
    d.label = "пляж и берег".into();
    d.prompts = vec!["фотография берега".into()];
    d.enabled = false;
    d.min_percentile = 30;
    save_tag_def(&c, &d).unwrap();
    let defs = load_tag_defs(&c, "m1").unwrap();
    assert_eq!(defs.len(), 2);
    assert_eq!(defs[0].label, "пляж и берег");
    assert!(!defs[0].enabled);
    assert_eq!(defs[0].min_percentile, 30);
    // The second model has its own dictionary, foreign labels do not interfere.
    let mut other = mk("beach", "пляж", 0);
    other.model = "m2".into();
    save_tag_def(&c, &other).unwrap();
    assert_eq!(load_tag_defs(&c, "m2").unwrap().len(), 1);
    // The label is unique inside a model: otherwise two `tags` rows with one key.
    let mut clash = mk("clash", "море", 2);
    clash.model = "m1".into();
    assert!(save_tag_def(&c, &clash).is_err());
    assert_eq!(delete_tag_def(&c, "m1", "sea").unwrap(), 1);
    assert_eq!(load_tag_defs(&c, "m1").unwrap().len(), 1);
}

#[test]
fn an_old_db_gains_albums_and_keeps_its_files() {
    let c = Connection::open_in_memory().unwrap();
    c.execute_batch(
        "CREATE TABLE files(
            id INTEGER PRIMARY KEY, path TEXT NOT NULL UNIQUE, size INTEGER NOT NULL,
            mtime INTEGER, sha256 TEXT, kind TEXT NOT NULL, width INTEGER, height INTEGER,
            taken_at TEXT, taken_src TEXT, lat REAL, lon REAL, camera TEXT, dhash INTEGER,
            thumb TEXT, indexed_at INTEGER NOT NULL);
         INSERT INTO files(path,size,kind,thumb,indexed_at)
            VALUES('/old.jpg',10,'image','aa/old.jpg',0);
         INSERT INTO files(path,size,kind,thumb,indexed_at)
            VALUES('/old.mp4',20,'video','bb/old.jpg',0);
         PRAGMA user_version=4;",
    )
    .unwrap();
    init(&c).unwrap();
    let version: i64 = c.query_row("PRAGMA user_version", [], |r| r.get(0)).unwrap();
    assert_eq!(version, SCHEMA_VERSION);
    assert_eq!(load_all(&c).unwrap().len(), 2);
    // Schema 8 (M22): image previews are recomputed with the EXIF rotation, so the path
    // is cleared; a video cover stays, no rows are lost.
    let rows = load_all(&c).unwrap();
    let image = rows.iter().find(|r| r.kind == "image").unwrap();
    assert!(image.thumb.is_none(), "the image preview must be recomputed");
    let video = rows.iter().find(|r| r.kind == "video").unwrap();
    assert_eq!(video.thumb.as_deref(), Some("bb/old.jpg"));
    let album = create_album(&c, 0, "Старое", "").unwrap();
    add_to_album(&c, album, &[id_of(&c, "/old.jpg")]).unwrap();
    assert_eq!(album_items(&c, album).unwrap().len(), 1);
    init(&c).unwrap(); // a repeated init does not fail and does not duplicate tables or rows
    assert_eq!(albums_tree(&c).unwrap().len(), 1);
    assert_eq!(album_items(&c, album).unwrap().len(), 1);
}

/// M26.1: a root is matched by path components, never by a string prefix.
#[test]
fn a_root_is_matched_by_path_components() {
    assert!(under_root("/archive/2024/a.jpg", "/archive/2024"));
    assert!(under_root("/archive/2024/", "/archive/2024"));
    assert!(!under_root("/archive/20242/a.jpg", "/archive/2024"));
    assert!(!under_root("/archive/2024/a.jpg", ""));
    assert!(under_root("/archive/2024/a.jpg", "/"));
    let mut s = Settings::default();
    s.set_root_off("/archive/2024/", true);
    assert!(s.root_off("/archive/2024"));
    assert!(s.path_hidden("/archive/2024/a.jpg"));
    assert!(!s.path_hidden("/archive/2025/a.jpg"));
    // A second switch adds no duplicate, a switch-on removes it.
    s.set_root_off("/archive/2024", true);
    assert_eq!(s.roots_off.len(), 1);
    s.set_root_off("/archive/2024", false);
    assert!(s.roots_off.is_empty());
}

/// M26.1: `stats` names the switched-off folders with their own counts, separately from the
/// shown files, and says nothing when every folder is on.
#[test]
fn the_statistics_name_the_switched_off_folders() {
    let path = std::env::temp_dir().join(format!(
        "photocat-stats-off-{}-{:?}-{:?}.db",
        std::process::id(),
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    {
        let c = open(&path).unwrap();
        insert(&c, &row("/archive/on/a.jpg", "aa", 1.0)).unwrap();
        insert(&c, &row("/archive/off/b.jpg", "bb", 1.0)).unwrap();
        let mut s = load_settings(&c);
        s.roots_off = vec!["/archive/off".into()];
        save_settings(&c, &s).unwrap();
    }
    let text = stats_text(Lang::En, &path).unwrap();
    assert!(
        text.contains("switched off: /archive/off — 1 file ·"),
        "the switched-off folder is not named: {text}"
    );
    assert!(
        text.contains("shown: 1 file ·"),
        "the shown files are not counted apart: {text}"
    );
    let ru = stats_text(Lang::Ru, &path).unwrap();
    assert!(
        ru.contains("выключен: /archive/off"),
        "the Russian report is missing the switched-off folder: {ru}"
    );
    {
        let c = open(&path).unwrap();
        let mut s = load_settings(&c);
        s.roots_off.clear();
        save_settings(&c, &s).unwrap();
    }
    let text = stats_text(Lang::En, &path).unwrap();
    assert!(
        !text.contains("switched off") && !text.contains("shown:"),
        "a report with nothing switched off names switches: {text}"
    );
}

/// M26.8: rows that left the database leave free pages, and `vacuum` returns them to the
/// filesystem. The rows that stay are intact, and a second call has nothing to do.
#[test]
fn a_vacuum_gives_the_free_pages_back_and_keeps_every_row() {
    let path = std::env::temp_dir().join(format!(
        "photocat-vacuum-{}-{:?}-{:?}.db",
        std::process::id(),
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    {
        let c = open(&path).unwrap();
        c.execute_batch("CREATE TABLE junk(id INTEGER PRIMARY KEY, data BLOB)")
            .unwrap();
        for i in 0..64u8 {
            c.execute(
                "INSERT INTO junk(id, data) VALUES (?1, ?2)",
                params![i as i64, vec![i; 64 * 1024]],
            )
            .unwrap();
        }
        c.execute("DELETE FROM junk WHERE id % 2 = 0", []).unwrap();
        // The free pages are in the main file before the test looks at them.
        c.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)").unwrap();
    }
    let before = db_space(&path).unwrap();
    assert!(
        before.free_bytes >= 1024 * 1024,
        "the deleted rows left no free pages: {before:?}"
    );
    assert!(before.worth_compacting(), "nothing to reclaim: {before:?}");

    let compaction = vacuum(&path).unwrap();
    assert_eq!(compaction.before, before);
    assert_eq!(
        compaction.after.free_bytes, 0,
        "free pages are left after the compaction: {:?}",
        compaction.after
    );
    assert!(
        compaction.after.file_bytes < before.file_bytes,
        "the file did not shrink: {} → {}",
        before.file_bytes,
        compaction.after.file_bytes
    );
    assert!(!compaction.after.worth_compacting());

    let c = open(&path).unwrap();
    let kept: i64 = c
        .query_row("SELECT COUNT(*) FROM junk", [], |r| r.get(0))
        .unwrap();
    assert_eq!(kept, 32, "the compaction lost rows");
    let odd: i64 = c
        .query_row(
            "SELECT COUNT(*) FROM junk WHERE id % 2 = 1 AND length(data) = ?1",
            params![64 * 1024],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(odd, 32, "the rows that stayed lost their data");
    drop(c);

    // A second press finds nothing to reclaim and touches nothing.
    let again = vacuum(&path).unwrap();
    assert_eq!(again.before, again.after, "a second vacuum is not a no-op");
}

/// The M26.8 measurement on a copy of the real database:
/// `PHOTOCAT_BENCH_DB=<copy> cargo test --release -- --ignored a_real_database_gives_its_free_pages_back --nocapture`.
#[test]
#[ignore]
fn a_real_database_gives_its_free_pages_back() {
    let Ok(path) = std::env::var("PHOTOCAT_BENCH_DB") else {
        eprintln!("PHOTOCAT_BENCH_DB is not set — the measurement is skipped");
        return;
    };
    let path = std::path::PathBuf::from(path);
    let started = std::time::Instant::now();
    let compaction = vacuum(&path).unwrap();
    let elapsed = started.elapsed().as_secs_f32();
    let c = open(&path).unwrap();
    let files: i64 = c
        .query_row("SELECT COUNT(*) FROM files", [], |r| r.get(0))
        .unwrap();
    println!(
        "vacuum: {} → {} (free {} → {}), {files} files, {elapsed:.3} s",
        human(compaction.before.file_bytes as i64),
        human(compaction.after.file_bytes as i64),
        human(compaction.before.free_bytes as i64),
        human(compaction.after.free_bytes as i64),
    );
    assert_eq!(compaction.after.free_bytes, 0);
    assert!(files > 100, "{files} files — that is not the corpus");
}

/// M26.9: a catalog switches a pass off for itself only; the choice is data in the settings,
/// survives a restart and leaves no key for a root that is gone.
#[test]
fn a_catalog_switches_the_passes_off_for_itself_only() {
    let mut s = Settings {
        roots: vec!["/archive/a".into(), "/archive/b".into()],
        ..Settings::default()
    };
    for pass in Pass::ALL {
        assert!(
            s.pass_allows("/archive/b/x.jpg", pass),
            "a default catalog must allow every pass"
        );
    }
    s.set_root_pass("/archive/b/", Pass::Tags, false);
    assert!(!s.pass_allows("/archive/b/x.jpg", Pass::Tags));
    assert!(s.pass_allows("/archive/b/x.jpg", Pass::Faces));
    assert!(s.pass_allows("/archive/a/x.jpg", Pass::Tags));
    assert!(!s.root_pass("/archive/b", Pass::Tags));
    assert!(s.root_pass("/archive/b", Pass::Faces));
    // A catalog switched off for display allows no pass at all (M26.1).
    s.set_root_off("/archive/b", true);
    for pass in Pass::ALL {
        assert!(!s.pass_allows("/archive/b/x.jpg", pass));
    }
    s.set_root_off("/archive/b", false);

    let c = Connection::open_in_memory().unwrap();
    init(&c).unwrap();
    save_settings(&c, &s).unwrap();
    let back = load_settings(&c);
    assert_eq!(back.root_passes, s.root_passes);
    assert!(!back.pass_allows("/archive/b/x.jpg", Pass::Tags));
    let mut back = back;
    back.set_root_pass("/archive/b", Pass::Tags, true);
    assert!(back.root_passes.is_empty(), "switching on leaves an empty key");

    // The root is gone: the next read drops its exception.
    s.roots = vec!["/archive/a".into()];
    save_settings(&c, &s).unwrap();
    assert!(load_settings(&c).root_passes.is_empty());
}

/// M26.9: "forget this catalog's analysis" removes the derived rows of that root only — the
/// file rows, the owner's own tags and the other catalog's analysis stay.
#[test]
fn forgetting_a_catalog_removes_only_its_analysis() {
    let path = std::env::temp_dir().join(format!(
        "photocat-forget-{}-{:?}-{:?}.db",
        std::process::id(),
        std::thread::current().id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    let (a, b) = {
        let c = open(&path).unwrap();
        insert(&c, &row("/archive/a/one.jpg", "aa", 1.0)).unwrap();
        insert(&c, &row("/archive/b/two.jpg", "bb", 1.0)).unwrap();
        let ids = (id_of(&c, "/archive/a/one.jpg"), id_of(&c, "/archive/b/two.jpg"));
        for (id, sha) in [(ids.0, "aa"), (ids.1, "bb")] {
            replace_tags(&c, id, "clip:m", &[("beach".into(), 0.3)]).unwrap();
            save_embedding(&c, id, "m", sha, &[1.0, 0.0]).unwrap();
            save_ocr(&c, id, sha, "eng", "text").unwrap();
            c.execute(
                "INSERT INTO user_tags(file_id,tag,created_at) VALUES(?1,'mine',0)",
                params![id],
            )
            .unwrap();
            c.execute(
                "INSERT INTO faces(file_id,sha256,x,y,w,h,score,quality,thumb,embedding,created_at)
                 VALUES(?1,?2,0,0,0,0,0.9,0.5,NULL,x'00000000',0)",
                params![id, sha],
            )
            .unwrap();
        }
        ids
    };
    let c = open(&path).unwrap();
    let forgotten = forget_catalog_derived(&c, "/archive/b").unwrap();
    assert_eq!(forgotten.files, 1);
    assert_eq!(forgotten.tags, 1);
    assert_eq!(forgotten.vectors, 1);
    assert_eq!(forgotten.ocr, 1);
    assert_eq!(forgotten.faces, 1);
    let count = |sql: &str, id: i64| -> i64 {
        c.query_row(sql, params![id], |r| r.get(0)).unwrap()
    };
    for table in ["tags", "embeddings", "ocr", "faces"] {
        assert_eq!(
            count(&format!("SELECT COUNT(*) FROM {table} WHERE file_id=?1"), b),
            0,
            "{table} rows of the forgotten catalog are still there"
        );
        assert_eq!(
            count(&format!("SELECT COUNT(*) FROM {table} WHERE file_id=?1"), a),
            1,
            "{table} rows of the other catalog were removed"
        );
    }
    // The owner's own tags and the file rows are not derived data.
    assert_eq!(count("SELECT COUNT(*) FROM user_tags WHERE file_id=?1", b), 1);
    assert_eq!(count("SELECT COUNT(*) FROM files WHERE id=?1", b), 1);
    assert_eq!(forget_catalog_derived(&c, "/archive/b").unwrap(), Forgotten { files: 1, ..Forgotten::default() });
}
