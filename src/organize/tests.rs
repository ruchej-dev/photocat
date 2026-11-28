//! The tests of the plan, the export and the apply: rows from the DB, nothing but temp on disk.

use super::export::{apply_export_in, export_name, number_width};
use super::plan::numbered;
use super::*;
use crate::db::{FileRow, Progress};
use crate::i18n::Lang;
use crate::derive;
use std::path::PathBuf;


fn row(id: i64, path: &str, taken: Option<&str>) -> FileRow {
    FileRow {
        id,
        path: path.to_string(),
        size: 1000,
        mtime: 0,
        sha256: format!("{id:064}"),
        kind: "image".into(),
        width: 100,
        height: 100,
        taken_at: taken.map(|s| s.to_string()),
        taken_src: "exif".into(),
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

fn plan_of(rows: &[FileRow], opts: &Options) -> Plan {
    build_plan(rows, opts)
}

fn dst(plan: &Plan, i: usize) -> String {
    plan.items[i].dst.to_string_lossy().to_string()
}

#[test]
fn days_and_months_give_a_different_depth() {
    let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
    let mut opts = Options::new("/out");
    let day = plan_of(&rows, &opts);
    assert_eq!(dst(&day, 0), "/out/2024/2024-05-17/IMG_1.jpg");
    opts.layout = Layout::Month;
    let month = plan_of(&rows, &opts);
    assert_eq!(dst(&month, 0), "/out/2024/2024-05/IMG_1.jpg");
}

#[test]
fn an_event_splits_a_day_by_place_and_no_gps_keeps_it_in_the_day() {
    let mut a = row(1, "/a/a.jpg", Some("2024-05-17T10:00:00"));
    a.lat = Some(55.751_244);
    a.lon = Some(37.617_494);
    let mut b = row(2, "/a/b.jpg", Some("2024-05-17T11:00:00"));
    b.lat = Some(55.751_9);
    b.lon = Some(37.617_1);
    let c = row(3, "/a/c.jpg", Some("2024-05-17T12:00:00"));
    let mut d = row(4, "/a/d.jpg", Some("2024-05-17T09:00:00"));
    d.lat = Some(59.93);
    d.lon = Some(30.31);
    let mut opts = Options::new("/out");
    opts.layout = Layout::Event;
    let plan = plan_of(&[a, b, c, d], &opts);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/55.75N_37.62E/a.jpg");
    assert_eq!(dst(&plan, 1), "/out/2024/2024-05-17/55.75N_37.62E/b.jpg");
    assert_eq!(dst(&plan, 2), "/out/2024/2024-05-17/59.93N_30.31E/d.jpg");
    assert_eq!(dst(&plan, 3), "/out/2024/2024-05-17/c.jpg");
    assert_eq!(plan.dirs, 3);
}

#[test]
fn the_gps_cell_knows_south_and_west() {
    assert_eq!(gps_cell(-33.868_8, -151.209_3), "33.87S_151.21W");
    assert_eq!(gps_cell(-0.001, 0.000_1), "0.00N_0.00E");
}

#[test]
fn the_name_template_fills_date_time_camera_and_extension() {
    let mut r = row(1, "/a/DSC_0001.JPG", Some("2024-05-17T14:42:43"));
    r.camera = Some("Xiaomi Redmi Note 8 Pro".into());
    let mut opts = Options::new("/out");
    opts.name = Some("{date}_{time}_{camera}.{ext}".into());
    let plan = plan_of(&[r], &opts);
    assert_eq!(
        dst(&plan, 0),
        "/out/2024/2024-05-17/20240517_144243_Xiaomi_Redmi_Note_8_Pro.JPG"
    );
}

#[test]
fn a_tag_goes_into_the_name_only_when_confident() {
    let r = row(1, "/a/x.jpg", Some("2024-05-17T14:42:43"));
    let mut opts = Options::new("/out");
    opts.name = Some("{date}_{tag}_{orig}".into());
    opts.tags
        .insert(1, vec![("дети".into(), 0.24), ("документ".into(), 0.05)]);
    let plan = plan_of(std::slice::from_ref(&r), &opts);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_дети_x.jpg");

    opts.tags.insert(1, vec![("дети".into(), 0.24), ("еда".into(), 0.235)]);
    let plan = plan_of(&[r], &opts);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_x.jpg");
}

#[test]
fn empty_substitutions_leave_no_separators() {
    let r = row(1, "/a/x.jpg", Some("2024-05-17T14:42:43"));
    let mut opts = Options::new("/out");
    opts.name = Some("{tag}_{date}_{camera}_{orig}".into());
    let plan = plan_of(&[r], &opts);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_x.jpg");
}

#[test]
fn forbidden_characters_are_replaced_and_collisions_numbered() {
    let a = row(1, "/a/photo 1.jpg", Some("2024-05-17T14:42:43"));
    let b = row(2, "/b/photo 1.jpg", Some("2024-05-17T15:00:00"));
    let mut opts = Options::new("/out");
    opts.name = Some("{date}_{orig}".into());
    let plan = plan_of(&[a, b], &opts);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_photo_1.jpg");
    assert_eq!(dst(&plan, 1), "/out/2024/2024-05-17/20240517_photo_1-2.jpg");
}

#[test]
fn without_a_template_the_name_stays_original() {
    let rows = [row(1, "/a/IMG_20240517_144243.jpg", Some("2024-05-17T14:42:43"))];
    let plan = plan_of(&rows, &Options::new("/out"));
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/IMG_20240517_144243.jpg");
}

#[test]
fn the_renaming_stays_compatible_with_the_old_format() {
    let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
    let mut opts = Options::new("/out");
    opts.name = Some(RENAME_TEMPLATE.into());
    let plan = plan_of(&rows, &opts);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/20240517_144243_IMG_1.jpg");
}

#[test]
fn the_spare_are_excluded_while_undated_are_counted() {
    let mut a = row(1, "/a/a.jpg", Some("2024-05-17T14:42:43"));
    a.decision = Some("drop".into());
    let mut b = row(2, "/a/b.jpg", Some("2024-05-17T14:42:44"));
    b.decision = Some("keep".into());
    let c = row(3, "/a/c.jpg", None);
    let mut opts = Options::new("/out");
    opts.exclude_dup = true;
    let plan = plan_of(&[a, b, c], &opts);
    assert_eq!(plan.items.len(), 1);
    assert_eq!(plan.excluded, 1);
    assert_eq!(plan.no_date, 1);
    assert_eq!(dst(&plan, 0), "/out/2024/2024-05-17/b.jpg");

    opts.exclude_dup = false;
    let plan = plan_of(&[row(4, "/a/d.jpg", Some("2024-05-17T14:42:45"))], &opts);
    assert_eq!(plan.excluded, 0);
    assert_eq!(plan.items.len(), 1);
}

#[test]
fn a_template_with_an_unknown_placeholder_is_refused() {
    assert!(validate_template("{date}_{foo}").is_err());
    assert!(validate_template("{date}_{").is_err());
    assert!(validate_template(RENAME_TEMPLATE).is_ok());
    assert!(validate_template(&PLACEHOLDERS.join("")).is_ok());
}

#[test]
fn the_plan_creates_nothing_without_apply() {
    let dir = std::env::temp_dir().join(format!("photocat-organize-{}", std::process::id()));
    let rows = [row(1, "/a/IMG_1.jpg", Some("2024-05-17T14:42:43"))];
    let _ = build_plan(&rows, &Options::new(&dir));
    assert!(!dir.exists());
}

#[test]
fn a_collision_number_does_not_break_a_file_without_an_extension() {
    let dst = PathBuf::from("/out/2024/2024-05-17/file");
    assert_eq!(numbered(&dst, 2), PathBuf::from("/out/2024/2024-05-17/file-2"));
    let dst = PathBuf::from("/out/file.jpg");
    assert_eq!(numbered(&dst, 3), PathBuf::from("/out/file-3.jpg"));
}

// ------------------------------------------------------------ M20: the export

fn tmp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!(
        "photocat-export-{tag}-{}-{n}",
        std::process::id()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn export_row(id: i64, path: &str, taken: Option<&str>) -> FileRow {
    let mut r = row(id, path, taken);
    r.size = 100 + id;
    r
}

fn album(id: i64, parent: i64, name: &str, files: Vec<FileRow>) -> ExportAlbum {
    ExportAlbum {
        id,
        parent_id: parent,
        name: name.into(),
        files,
    }
}

#[test]
fn the_export_repeats_the_tree_and_numbers_the_files() {
    let albums = [
        album(
            1,
            0,
            "Дача 2024",
            vec![export_row(1, "/a/IMG_1.jpg", Some("2024-05-17T10:00:00"))],
        ),
        album(
            2,
            1,
            "Май",
            vec![
                export_row(2, "/a/IMG_2.jpg", Some("2024-05-01T10:00:00")),
                export_row(3, "/a/IMG_3.jpg", Some("2024-05-02T10:00:00")),
            ],
        ),
        album(3, 1, "Июнь", Vec::new()),
    ];
    let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
    assert_eq!(plan.root, PathBuf::from("Дача_2024"));
    let rels: Vec<String> = plan
        .dirs
        .iter()
        .map(|d| d.rel.to_string_lossy().to_string())
        .collect();
    assert_eq!(
        rels,
        ["Дача_2024", "Дача_2024/Май", "Дача_2024/Июнь"]
    );
    assert_eq!(plan.dirs[1].depth, 1);
    assert_eq!(plan.dirs[2].files, 0, "an empty album stays in the plan");
    assert_eq!(plan.files(), 3);
    assert!(plan.items.iter().any(|i| i.rel.ends_with("01_IMG_2.jpg")));
    assert!(plan.items.iter().any(|i| i.rel.ends_with("02_IMG_3.jpg")));
    assert!(plan.items.iter().any(|i| i.rel.ends_with("Дача_2024/01_IMG_1.jpg")));
    assert!(plan.summary(Lang::Ru).contains("файлов 3"));
}

#[test]
fn the_number_width_grows_with_the_file_count() {
    assert_eq!(number_width(1), 2);
    assert_eq!(number_width(99), 2);
    assert_eq!(number_width(100), 3);
    assert_eq!(number_width(1000), 4);
}

#[test]
fn folder_names_are_cleaned_and_stay_unique_among_siblings() {
    let albums = [
        album(1, 0, "Дача/2024", Vec::new()),
        album(2, 1, "Дача:2024", Vec::new()),
        album(3, 1, "CON", Vec::new()),
        album(4, 1, "", Vec::new()),
    ];
    let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
    let names: Vec<String> = plan
        .dirs
        .iter()
        .map(|d| d.rel.file_name().unwrap().to_string_lossy().to_string())
        .collect();
    assert_eq!(names, ["Дача_2024", "Дача_2024-2", "_CON", "album-4"]);
    assert_eq!(plan.renamed, 1);
}

#[test]
fn a_file_name_collision_is_split_by_a_number() {
    let albums = [album(
        1,
        0,
        "На печать",
        vec![
            export_row(1, "/a/x.jpg", None),
            export_row(2, "/b/x.jpg", None),
        ],
    )];
    let mut opts = ExportOptions::new("/out");
    opts.name = "{orig}".into();
    let plan = plan_album(&albums, &opts).unwrap();
    assert_eq!(plan.items[0].rel, PathBuf::from("На_печать/x.jpg"));
    assert_eq!(plan.items[1].rel, PathBuf::from("На_печать/x-2.jpg"));
    assert_eq!(plan.renamed, 1);
}

#[test]
fn the_export_template_knows_n_and_hash8() {
    assert!(validate_export_template(EXPORT_TEMPLATE).is_ok());
    assert!(validate_export_template("{n}_{date}_{time}_{camera}_{tag}_{orig}_{ext}").is_ok());
    assert!(validate_export_template("{hash8}").is_ok());
    assert!(validate_export_template("{foo}").is_err());
    // The showcase template does not know `{n}`, and the other way round.
    assert!(validate_template("{n}_{orig}").is_err());
    assert!(validate_export_template("{date}_{orig}").is_ok());
}

#[test]
fn a_name_from_the_template_carries_the_position_date_and_hash() {
    let r = export_row(1, "/a/IMG 1.jpg", Some("2024-05-17T14:42:43"));
    let opts = ExportOptions::new("/out");
    let name = export_name("{n}_{date}_{time}_{orig}_{hash8}", &r, &opts, 7, 2);
    assert_eq!(name, "07_20240517_144243_IMG_1_00000000.jpg");
}

#[test]
fn the_trash_and_a_long_path_are_counted_and_not_exported() {
    let mut trashed = export_row(2, "/a/b.jpg", Some("2024-05-17T10:00:00"));
    trashed.trashed_at = Some(1_716_000_000);
    let albums = [album(
        1,
        0,
        "Архив",
        vec![export_row(1, "/a/a.jpg", Some("2024-05-17T09:00:00")), trashed],
    )];
    let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
    assert_eq!(plan.files(), 1);
    assert_eq!(plan.trashed, 1);

    // 240 characters of path: 120 for the root and 120 for the file — beyond the limit.
    let long_out = PathBuf::from(format!("/{}", "o".repeat(150)));
    let albums = [album(
        1,
        0,
        &"к".repeat(80),
        vec![export_row(1, &format!("/a/{}.jpg", "x".repeat(80)), None)],
    )];
    let plan = plan_album(&albums, &ExportOptions::new(&long_out)).unwrap();
    assert_eq!(plan.too_long, 1);
    assert_eq!(plan.files(), 0);
}

#[test]
fn the_export_copies_rather_than_links_and_a_repeat_is_idempotent() {
    let dir = tmp_dir("copy");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let src = dir.join("src.jpg");
    std::fs::write(&src, b"photocat export").unwrap();
    let sha = derive::sha256_file(&src).unwrap();
    let mut r = export_row(1, src.to_string_lossy().as_ref(), None);
    r.size = 15;
    r.sha256 = sha;
    let albums = [album(1, 0, "На печать", vec![r])];
    let plan = plan_album(&albums, &ExportOptions::new(&out)).unwrap();
    let first = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
    assert_eq!(first.copied, 1);
    assert_eq!(first.failed, 0, "{:?}", first.errors);
    let dst = out.join("На_печать/01_src.jpg");
    assert!(dst.is_file());
    assert_eq!(std::fs::read(&dst).unwrap(), b"photocat export");
    assert!(!std::fs::symlink_metadata(&dst).unwrap().file_type().is_symlink());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_ne!(
            std::fs::metadata(&src).unwrap().ino(),
            std::fs::metadata(&dst).unwrap().ino(),
            "the export is a copy, not a hardlink"
        );
    }
    let second = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
    assert_eq!(second.copied, 0);
    assert_eq!(second.skipped, 1);
    assert_eq!(second.failed, 0);
}

#[test]
fn a_name_taken_by_other_content_gets_two_and_is_not_overwritten() {
    let dir = tmp_dir("collision");
    let out = dir.join("out");
    std::fs::create_dir_all(out.join("На_печать")).unwrap();
    let taken = out.join("На_печать/01_src.jpg");
    std::fs::write(&taken, b"someone else").unwrap();
    let src = dir.join("src.jpg");
    std::fs::write(&src, b"photocat export").unwrap();
    let mut r = export_row(1, src.to_string_lossy().as_ref(), None);
    r.size = 15;
    r.sha256 = derive::sha256_file(&src).unwrap();
    let albums = [album(1, 0, "На печать", vec![r])];
    let plan = plan_album(&albums, &ExportOptions::new(&out)).unwrap();
    let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
    assert_eq!(report.copied, 1);
    assert_eq!(report.renamed, 1);
    assert_eq!(std::fs::read(&taken).unwrap(), b"someone else");
    assert_eq!(
        std::fs::read(out.join("На_печать/01_src-2.jpg")).unwrap(),
        b"photocat export"
    );
}

#[test]
fn reordering_gives_a_stale_order_and_no_duplicates() {
    let dir = tmp_dir("stale");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let a = dir.join("a.jpg");
    let b = dir.join("b.jpg");
    std::fs::write(&a, b"aaaa").unwrap();
    std::fs::write(&b, b"bbbb").unwrap();
    let mut ra = export_row(1, a.to_string_lossy().as_ref(), None);
    ra.size = 4;
    ra.sha256 = derive::sha256_file(&a).unwrap();
    let mut rb = export_row(2, b.to_string_lossy().as_ref(), None);
    rb.size = 4;
    rb.sha256 = derive::sha256_file(&b).unwrap();
    let first = plan_album(
        &[album(1, 0, "Порядок", vec![ra.clone(), rb.clone()])],
        &ExportOptions::new(&out),
    )
    .unwrap();
    assert_eq!(apply_export_in(&first, Lang::default(), None, &dir.join("cache")).copied, 2);
    // The order changes: the numbered names differ, but the content is in the target already.
    let second = plan_album(
        &[album(1, 0, "Порядок", vec![rb, ra])],
        &ExportOptions::new(&out),
    )
    .unwrap();
    let report = apply_export_in(&second, Lang::default(), None, &dir.join("cache"));
    assert_eq!(report.copied, 0);
    assert_eq!(report.stale, 2);
    assert_eq!(report.renamed, 0);
    let files = std::fs::read_dir(out.join("Порядок")).unwrap().count();
    assert_eq!(files, 2, "no duplicates appeared at the destination");
}

#[test]
fn an_empty_album_creates_an_empty_folder() {
    let dir = tmp_dir("empty");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let planets = [album(1, 0, "Пустой", Vec::new())];
    let plan = plan_album(&planets, &ExportOptions::new(&out)).unwrap();
    let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
    assert_eq!(report.copied, 0);
    assert!(out.join("Пустой").is_dir());
    assert_eq!(std::fs::read_dir(out.join("Пустой")).unwrap().count(), 0);
}

#[test]
fn a_missing_file_does_not_fail_the_pass() {
    let dir = tmp_dir("missing");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let src = dir.join("ok.jpg");
    std::fs::write(&src, b"ok").unwrap();
    let mut good = export_row(1, src.to_string_lossy().as_ref(), None);
    good.size = 2;
    good.sha256 = derive::sha256_file(&src).unwrap();
    let gone = export_row(2, dir.join("gone.jpg").to_string_lossy().as_ref(), None);
    let albums = [album(1, 0, "Смесь", vec![good, gone])];
    let plan = plan_album(&albums, &ExportOptions::new(&out)).unwrap();
    let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
    assert_eq!(report.copied, 1);
    assert_eq!(report.failed, 1);
    assert!(report.errors.iter().any(|e| e.contains("no such file")));
}

#[test]
fn a_cancel_before_the_start_creates_nothing_and_remembers_what_was_created() {
    let dir = tmp_dir("cancel");
    let out = dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    let src = dir.join("a.jpg");
    std::fs::write(&src, b"a").unwrap();
    let mut r = export_row(1, src.to_string_lossy().as_ref(), None);
    r.size = 1;
    r.sha256 = derive::sha256_file(&src).unwrap();
    let plan = plan_album(&[album(1, 0, "Стоп", vec![r])], &ExportOptions::new(&out)).unwrap();
    let progress = Progress::default();
    progress.cancel();
    let report = apply_export_in(&plan, Lang::default(), Some(&progress), &dir.join("cache"));
    assert!(report.cancelled);
    assert_eq!(report.created.len(), 0);
    assert!(!out.join("Стоп/01_a.jpg").exists());

    // A normal pass lists the created files (for the report and the test).
    let report = apply_export_in(&plan, Lang::default(), None, &dir.join("cache"));
    assert_eq!(report.created, vec![out.join("Стоп/01_a.jpg")]);
}

#[test]
fn an_out_inside_the_archive_is_refused() {
    let root = tmp_dir("roots");
    let inside = root.join("photos");
    std::fs::create_dir_all(&inside).unwrap();
    let roots = vec![root.to_string_lossy().to_string()];
    assert!(check_out_dir(&inside, &roots).is_err());
    assert!(check_out_dir(&root, &roots).is_err());
    let outside = tmp_dir("outside");
    assert!(check_out_dir(&outside, &roots).is_ok());
    assert!(check_out_dir(&root.join("nope"), &roots).is_err());
}

#[test]
fn depth_three_and_the_sibling_order_are_kept() {
    let albums = [
        album(1, 0, "Дача 2024", Vec::new()),
        album(2, 1, "Май", Vec::new()),
        album(3, 2, "Неделя 1", Vec::new()),
        album(4, 1, "Июнь", Vec::new()),
    ];
    let plan = plan_album(&albums, &ExportOptions::new("/out")).unwrap();
    assert_eq!(plan.dirs[2].rel, PathBuf::from("Дача_2024/Май/Неделя_1"));
    assert_eq!(plan.dirs[2].depth, 2);
    assert_eq!(plan.dirs[3].rel, PathBuf::from("Дача_2024/Июнь"));
    assert_eq!(plan.dirs.len(), 4);
}
