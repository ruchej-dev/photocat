//! The tests of the detector, the alignment, the vectors and the matches.

use super::align::{bilinear, similarity, ARC_DST};
use super::clusters::components;
use super::detect::{nms, RawBox};
use super::embed::{normalize, FaceModel};
use super::matches::best_two_groups;
use crate::db;
use crate::db::FaceRow;
use std::collections::{HashMap, HashSet};
use image::RgbImage;

use super::*;

fn v2(a: f32, b: f32) -> Vec<f32> {
    let mut v = vec![a, b, 0.0, 0.0];
    normalize(&mut v);
    v
}

#[test]
fn components_join_only_similar_faces() {
    let a = v2(1.0, 0.0);
    let a2 = v2(0.99, 0.05);
    let b = v2(0.0, 1.0);
    let vectors: Vec<&[f32]> = vec![&a, &a2, &b];
    let comps = components(&vectors, 0.8);
    assert_eq!(comps[0], comps[1]);
    assert_ne!(comps[0], comps[2]);
    // A high threshold splits even similar ones.
    let comps = components(&vectors, 0.9999);
    assert_ne!(comps[0], comps[1]);
}

/// A unit vector at an angle in the plane: two people and their relatives can be placed by
/// their cosine without an ONNX model.
fn unit(deg: f32) -> Vec<f32> {
    let a = deg.to_radians();
    let mut v = vec![a.cos(), a.sin(), 0.0, 0.0];
    normalize(&mut v);
    v
}

fn temp_db(tag: &str) -> std::path::PathBuf {
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    std::env::temp_dir().join(format!(
        "photocat-{tag}-{}-{stamp}.db",
        std::process::id()
    ))
}

fn file_row(path: &str, sha: &str) -> db::FileRow {
    db::FileRow {
        id: 0,
        path: path.into(),
        size: 1,
        mtime: 0,
        sha256: sha.into(),
        kind: "image".into(),
        width: 320,
        height: 240,
        taken_at: None,
        taken_src: String::new(),
        lat: None,
        lon: None,
        camera: None,
        dhash: None,
        sharpness: None,
        duration_s: None,
        thumb: None,
        decision: None,
        trashed_at: None,
    }
}

/// M26.11: a face named once is found in every catalog. Two named people (A at 0°, B at 60°)
/// and the free faces around them: the close one joins A from either catalog; the one at 30°
/// is as close to B as to A and stays where it is; the one at 150° is far from both; a face the
/// owner placed by hand is never moved; a catalog with the faces pass off is not considered.
#[test]
fn a_named_person_is_recognized_in_every_catalog() {
    let path = temp_db("match-named");
    let mut settings = db::Settings {
        roots: vec![
            "/archive/a".into(),
            "/archive/b".into(),
            "/off/cat".into(),
        ],
        ..db::Settings::default()
    };
    settings
        .root_passes
        .insert("/off/cat".to_string(), vec![db::Pass::Faces]);
    let (ann, close_a, close_b, tie, far, by_hand, off) = {
        let conn = db::open(&path).unwrap();
        for (p, sha) in [
            ("/archive/a/one.jpg", "a1"),
            ("/archive/a/two.jpg", "a2"),
            ("/archive/b/one.jpg", "b1"),
            ("/archive/b/two.jpg", "b2"),
            ("/archive/b/three.jpg", "b3"),
            ("/archive/b/four.jpg", "b4"),
            ("/archive/b/five.jpg", "b5"),
            ("/off/cat/one.jpg", "c1"),
        ] {
            db::insert(&conn, &file_row(p, sha)).unwrap();
        }
        let id_of = |p: &str| -> i64 {
            conn.query_row("SELECT id FROM files WHERE path=?1", [p], |r| r.get(0))
                .unwrap()
        };
        // One face per photo: after the write, the last row of `faces` is the one just added.
        let add = |photo: &str, v: Vec<f32>| -> i64 {
            let file_id = id_of(photo);
            db::save_faces_for_file(
                &conn,
                file_id,
                "sha",
                &[FaceRow {
                    id: 0,
                    x: 0,
                    y: 0,
                    w: 50,
                    h: 50,
                    score: 0.9,
                    quality: 40.0,
                    thumb: None,
                    embedding: v,
                    cluster_id: None,
                    manual: false,
                }],
            )
            .unwrap();
            conn.query_row("SELECT MAX(id) FROM faces", [], |r| r.get(0))
                .unwrap()
        };
        let a_member = add("/archive/a/one.jpg", unit(2.0));
        let b_member = add("/archive/b/one.jpg", unit(60.0));
        let ann = db::create_face_cluster(&conn).unwrap();
        db::label_face_cluster(&conn, ann, "Ann").unwrap();
        let bob = db::create_face_cluster(&conn).unwrap();
        db::label_face_cluster(&conn, bob, "Bob").unwrap();
        db::save_face_assignments(
            &mut db::open(&path).unwrap(),
            &[(a_member, Some(ann)), (b_member, Some(bob))],
            true,
        )
        .unwrap();
        let close_a = add("/archive/a/two.jpg", unit(20.0));
        let close_b = add("/archive/b/two.jpg", unit(20.0));
        let tie = add("/archive/b/three.jpg", unit(30.0));
        let far = add("/archive/b/four.jpg", unit(150.0));
        let by_hand = add("/archive/b/five.jpg", unit(0.0));
        let off = add("/off/cat/one.jpg", unit(0.0));
        db::save_face_assignments(&mut db::open(&path).unwrap(), &[(by_hand, None)], true)
            .unwrap();
        (ann, close_a, close_b, tie, far, by_hand, off)
    };

    // The plan writes nothing: the close faces are still where they were.
    let plan = plan_named(&path, &settings).unwrap();
    assert!(plan.dry_run);
    assert_eq!((plan.named, plan.matched, plan.people), (2, 2, 1), "{plan}");
    assert_eq!((plan.left_small, plan.left_far), (1, 1), "{plan}");
    let before = db::load_faces(&db::open(&path).unwrap()).unwrap();
    let cluster_of = |faces: &[FaceRow], id: i64| {
        faces.iter().find(|f| f.id == id).unwrap().cluster_id
    };
    assert_eq!(cluster_of(&before, close_a), None, "the plan wrote a row");

    // The step itself (M26.17): both close faces are proposed for Ann, from two different
    // catalogs, and neither moves. They wait in the quarantine.
    let report = match_named(&path, &settings).unwrap();
    assert!(!report.dry_run);
    assert_eq!((report.named, report.matched, report.people), (2, 2, 1));
    assert_eq!((report.left_small, report.left_far), (1, 1), "{report}");
    assert!(
        report
            .to_string()
            .contains("proposed 2 faces to 1 named people — they wait in quarantine"),
        "{report}"
    );
    assert!(
        report.to_string().contains("1 left — the lead was too small"),
        "{report}"
    );
    let after = db::load_faces(&db::open(&path).unwrap()).unwrap();
    for id in [close_a, close_b] {
        assert_eq!(
            cluster_of(&after, id),
            None,
            "a proposal moved the face into Ann"
        );
    }
    let proposals = db::load_face_proposals(&db::open(&path).unwrap()).unwrap();
    assert_eq!(proposals.len(), 2, "the proposals are not stored");
    for p in &proposals {
        assert_eq!(p.target_id, ann);
        assert_eq!(p.target_label, "Ann");
        assert!(p.similarity > 0.9);
    }
    // The owner's own placement, the tie, the far face and the forbidden catalog stay.
    for id in [by_hand, far, tie, off] {
        assert_eq!(cluster_of(&after, id), None, "a face moved that should not");
    }
    assert!(
        after.iter().find(|f| f.id == by_hand).unwrap().manual,
        "the hand-placed face lost its pin"
    );
    // A confirmed proposal joins Ann and is pinned; a turned-down one stays where it is and
    // is pinned, so a second step does not offer it again.
    assert_eq!(confirm_matches(&path, &[close_a]).unwrap(), 1);
    assert_eq!(reject_matches(&path, &[close_b]).unwrap(), 1);
    let answered = db::load_faces(&db::open(&path).unwrap()).unwrap();
    assert_eq!(cluster_of(&answered, close_a), Some(ann));
    assert!(answered.iter().find(|f| f.id == close_a).unwrap().manual);
    assert_eq!(cluster_of(&answered, close_b), None);
    assert!(answered.iter().find(|f| f.id == close_b).unwrap().manual);
    // The confirmed face is a member of Ann now, so the 30° face is no longer ambiguous and
    // is offered for her; the turned-down and hand-placed faces are not offered again.
    let again = match_named(&path, &settings).unwrap();
    assert_eq!(again.matched, 1, "an answered face was offered again: {again}");
    assert_eq!((again.left_small, again.left_far), (0, 1), "{again}");
    let proposals = db::load_face_proposals(&db::open(&path).unwrap()).unwrap();
    assert_eq!(proposals.len(), 1);
    assert_eq!(proposals[0].face.id, tie);
    assert_eq!(proposals[0].target_id, ann);
    assert!(
        proposals.iter().all(|p| p.face.id != close_b),
        "a turned-down face was offered again"
    );
}

/// M26.11 on the real archive: how close the free faces come to the people the owner has
/// named, and what one pass costs. A copy of the DB, never the owner's own.
/// `PHOTOCAT_BENCH_DB=<copy> cargo test --release -- --ignored \
///  a_real_archive_matches_a_named_person_quickly --nocapture`
#[test]
#[ignore = "needs the real archive DB and moves faces in it"]
fn a_real_archive_matches_a_named_person_quickly() {
    let Ok(db_path) = std::env::var("PHOTOCAT_BENCH_DB") else {
        return;
    };
    let path = std::path::PathBuf::from(db_path);
    let settings = db::load_settings(&db::open(&path).unwrap());
    let faces = db::load_faces(&db::open(&path).unwrap()).unwrap();
    let clusters = db::load_face_clusters(&db::open(&path).unwrap()).unwrap();
    let named: Vec<(i64, String)> = clusters
        .iter()
        .filter(|c| !c.label.trim().is_empty())
        .map(|c| (c.id, format!("{} ({})", c.label, c.faces)))
        .collect();
    println!(
        "{} faces, {} named people: {}",
        faces.len(),
        named.len(),
        named
            .iter()
            .map(|(_, l)| l.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!(
        "face_match {:.2}, margin {:.2}",
        settings.face_match, settings.face_match_margin
    );
    // The distance of every free face to the best named person: the tail decides the default.
    let named_ids: HashSet<i64> = named.iter().map(|(id, _)| *id).collect();
    let mut targets: HashMap<i64, Vec<&[f32]>> = HashMap::new();
    for f in &faces {
        if let Some(cid) = f.cluster_id
            && named_ids.contains(&cid)
        {
            targets.entry(cid).or_default().push(f.embedding.as_slice());
        }
    }
    let paths = db::face_paths(&db::open(&path).unwrap()).unwrap();
    let mut best: Vec<f32> = faces
        .iter()
        .filter(|f| !f.manual && f.cluster_id.is_none_or(|c| !named_ids.contains(&c)))
        .filter(|f| {
            paths
                .get(&f.id)
                .is_some_and(|p| settings.pass_allows(p, db::Pass::Faces))
        })
        .map(|f| best_two_groups(&f.embedding, &targets).0)
        .collect();
    best.sort_by(|a, b| b.total_cmp(a));
    let above = |x: f32| best.iter().filter(|s| **s >= x).count();
    println!(
        "{} free faces; best similarity: max {:.3}, p99 {:.3}, p95 {:.3}; \
         above 0.50 {} / 0.45 {} / 0.40 {} / 0.35 {} / 0.30 {}",
        best.len(),
        best.first().copied().unwrap_or(0.0),
        best.get(best.len() / 100).copied().unwrap_or(0.0),
        best.get(best.len() / 20).copied().unwrap_or(0.0),
        above(0.50),
        above(0.45),
        above(0.40),
        above(0.35),
        above(0.30),
    );
    for run in 1..=2 {
        let t = std::time::Instant::now();
        let r = match_named(&path, &settings).unwrap();
        println!(
            "run {run}: {r} — {:.1} ms of wall clock, {:.1} ms in the step",
            t.elapsed().as_secs_f64() * 1000.0,
            r.seconds * 1000.0
        );
    }
    let proposals = db::load_face_proposals(&db::open(&path).unwrap()).unwrap();
    println!("proposals waiting in quarantine: {}", proposals.len());
}

#[test]
fn nms_keeps_best_of_overlapping_boxes() {
    let mk = |r: [f32; 4], score: f32| RawBox {
        rect: r,
        score,
        kps: [[0.0; 2]; 5],
    };
    let boxes = vec![
        mk([0.0, 0.0, 100.0, 100.0], 0.9),
        mk([5.0, 5.0, 100.0, 100.0], 0.8),
        mk([200.0, 200.0, 300.0, 300.0], 0.7),
    ];
    let kept = nms(boxes, 0.4);
    assert_eq!(kept.len(), 2);
    assert!((kept[0].score - 0.9).abs() < 1e-6);
}

#[test]
fn alignment_is_identity_when_points_match() {
    let (a, b, tx, ty) = similarity(&ARC_DST, &ARC_DST);
    assert!((a - 1.0).abs() < 1e-5, "{a}");
    assert!(b.abs() < 1e-5, "{b}");
    assert!(tx.abs() < 1e-4, "{tx}");
    assert!(ty.abs() < 1e-4, "{ty}");
}

#[test]
fn alignment_moves_points_to_arc_dst() {
    // The points are rotated and shifted: the matrix must bring them back to the canonical ones.
    let (a, b, tx, ty) = similarity(&ARC_DST, &ARC_DST);
    for p in ARC_DST {
        let x = a * p[0] - b * p[1] + tx;
        let y = b * p[0] + a * p[1] + ty;
        assert!((x - p[0]).abs() < 1e-3);
        assert!((y - p[1]).abs() < 1e-3);
    }
}

#[test]
#[ignore = "needs the downloaded CLIP and face weights"]
fn clip_and_faces_share_one_onnx_environment() {
    let settings = db::Settings::default();
    // ORT allows one environment per process: CLIP must not block the faces.
    let _clip = crate::embed::Model::load(&settings, 1).unwrap();
    let _faces = FaceModel::load(&settings, 1).unwrap();
}

#[test]
fn bilinear_returns_corner_pixel() {
    let mut img = RgbImage::new(2, 2);
    img.put_pixel(0, 0, image::Rgb([10, 20, 30]));
    img.put_pixel(1, 0, image::Rgb([40, 50, 60]));
    assert_eq!(bilinear(&img, 0.0, 0.0, 2, 2).0, [10, 20, 30]);
    assert_eq!(bilinear(&img, -5.0, -5.0, 2, 2).0, [10, 20, 30]);
}
