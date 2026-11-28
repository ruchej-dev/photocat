//! The tests of the comparison and of the manifest: temp folders only.

use super::compare::compare;
use super::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

fn temp_dir(tag: &str) -> PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let p = std::env::temp_dir().join(format!(
        "photocat-verify-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&p).unwrap();
    p
}

fn scan_of(items: &[(&str, &str, u64, &str)]) -> Scan {
    let mut s = Scan::default();
    for (rel, path, size, _) in items {
        s.files.insert(
            (*rel).to_string(),
            Entry {
                path: PathBuf::from(path),
                size: *size,
            },
        );
    }
    s
}

fn hash_map(items: &[(&str, &str)]) -> HashMap<PathBuf, String> {
    items
        .iter()
        .map(|(p, h)| (PathBuf::from(p), (*h).to_string()))
        .collect()
}

#[test]
fn different_sizes_are_not_read() {
    let a = scan_of(&[("x.jpg", "/a/x.jpg", 10, "")]);
    let b = scan_of(&[("x.jpg", "/b/x.jpg", 20, "")]);
    let calls = AtomicUsize::new(0);
    let r = compare(&a, &b, 2, Lang::default(), None, |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok("h".into())
    });
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(r.size_diff.len(), 1);
    assert_eq!(r.hashed, 0);
}

#[test]
fn an_equal_size_is_checked_by_sha256() {
    let a = scan_of(&[
        ("same.jpg", "/a/same.jpg", 10, ""),
        ("diff.jpg", "/a/diff.jpg", 20, ""),
    ]);
    let b = scan_of(&[
        ("same.jpg", "/b/same.jpg", 10, ""),
        ("diff.jpg", "/b/diff.jpg", 20, ""),
    ]);
    let hashes = hash_map(&[
        ("/a/same.jpg", "h1"),
        ("/b/same.jpg", "h1"),
        ("/a/diff.jpg", "h2"),
        ("/b/diff.jpg", "h3"),
    ]);
    let r = compare(&a, &b, 2, Lang::default(), None, |p| Ok(hashes[p].clone()));
    assert_eq!(r.same, 1);
    assert_eq!(r.content_diff, vec!["diff.jpg".to_string()]);
    assert_eq!(r.hashed, 2);
    assert_eq!(r.hashed_bytes, 30);
}

#[test]
fn files_in_only_one_copy_show_by_name() {
    let a = scan_of(&[
        ("a.jpg", "/a/a.jpg", 1, ""),
        ("both.jpg", "/a/both.jpg", 2, ""),
    ]);
    let b = scan_of(&[
        ("b.jpg", "/b/b.jpg", 1, ""),
        ("both.jpg", "/b/both.jpg", 2, ""),
    ]);
    let r = compare(&a, &b, 1, Lang::default(), None, |_| Ok("same".into()));
    assert_eq!(r.only_a, vec!["a.jpg".to_string()]);
    assert_eq!(r.only_b, vec!["b.jpg".to_string()]);
    assert_eq!(r.same, 1);
    assert!(!r.identical());
}

#[test]
fn a_read_error_goes_into_the_report_and_does_not_fail_the_check() {
    let a = scan_of(&[("x.jpg", "/a/x.jpg", 10, "")]);
    let b = scan_of(&[("x.jpg", "/b/x.jpg", 10, "")]);
    let r = compare(&a, &b, 1, Lang::default(), None, |p| {
        anyhow::bail!("no access: {}", p.display())
    });
    assert_eq!(r.read_errors.len(), 1);
    assert!(!r.identical());
}

#[test]
fn a_cancel_does_not_count_files_as_checked() {
    let a = scan_of(&[("x.jpg", "/a/x.jpg", 10, "")]);
    let b = scan_of(&[("x.jpg", "/b/x.jpg", 10, "")]);
    let p = Arc::new(Progress::default());
    p.cancel();
    let calls = AtomicUsize::new(0);
    let r = compare(&a, &b, 1, Lang::default(), Some(p), |_| {
        calls.fetch_add(1, Ordering::Relaxed);
        Ok("h".into())
    });
    assert!(r.cancelled);
    assert_eq!(r.unchecked, 1);
    assert_eq!(r.hashed, 0);
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert!(!r.identical());
}

#[test]
fn the_manifest_matches_sha256sum_and_escapes_names() {
    let hash = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    assert_eq!(
        manifest_line(hash, "2024/2024-05-17/a.jpg"),
        format!("{hash}  2024/2024-05-17/a.jpg\n")
    );
    let line = manifest_line(hash, "a\\b\nc\rd");
    assert_eq!(line, format!("\\{hash}  a\\\\b\\nc\\rd\n"));
    let parsed = parse_manifest(&line).unwrap();
    assert_eq!(parsed.len(), 1);
    assert_eq!(parsed[0].rel, "a\\b\nc\rd");
    assert_eq!(parsed[0].hash, hash);
}

#[test]
fn manifest_parsing_supports_binary_mode_and_refuses_garbage() {
    let hash = "a".repeat(64);
    let ok = parse_manifest(&format!("{hash} *file.jpg\n")).unwrap();
    assert_eq!(ok[0].rel, "file.jpg");
    assert!(parse_manifest("not a manifest\n").is_err());
    assert!(parse_manifest(&format!("{hash}  one\n{hash}  one\n")).is_err());
    assert!(parse_manifest(&format!("\\{hash}  bad\\q\n")).is_err());
    assert!(parse_manifest(&format!("{hash}  \n")).is_err());
    assert!(parse_manifest(&format!("{hash}  ../secret\n")).is_err());
    assert!(parse_manifest(&format!("{hash}  /etc/passwd\n")).is_err());
    assert!(parse_manifest(&format!("{hash}  ./x\n")).is_err());
}

#[test]
fn the_manifest_and_the_folder_check_work_on_disk() {
    let root = temp_dir("roundtrip");
    std::fs::create_dir_all(root.join("2024")).unwrap();
    std::fs::write(root.join("a.txt"), b"hello").unwrap();
    std::fs::write(root.join("2024/b.txt"), b"world").unwrap();
    let out = root.join("archive.sha256");
    let stats = manifest(&root, Some(&out), 2, Lang::default(), None).unwrap();
    assert!(stats.written);
    assert_eq!(stats.files, 2);
    assert_eq!(stats.bytes, 10);
    assert_eq!(stats.errors, 0);

    let text = std::fs::read_to_string(&out).unwrap();
    let parsed = parse_manifest(&text).unwrap();
    let names: Vec<&str> = parsed.iter().map(|e| e.rel.as_str()).collect();
    assert_eq!(names, vec!["2024/b.txt", "a.txt"]);

    let check = check_manifest(&root, &out, 2, Lang::default(), None).unwrap();
    assert!(check.identical(), "{:?}", check.changed);
    assert_eq!(check.same, 2);

    std::fs::write(root.join("a.txt"), b"changed").unwrap();
    let check = check_manifest(&root, &out, 2, Lang::default(), None).unwrap();
    assert_eq!(check.changed, vec!["a.txt".to_string()]);
    assert!(!check.identical());

    std::fs::write(root.join("extra.txt"), b"x").unwrap();
    let check = check_manifest(&root, &out, 2, Lang::default(), None).unwrap();
    assert_eq!(check.extra, vec!["extra.txt".to_string()]);
    assert!(!check.identical());
}

#[test]
fn verifying_two_folders_finds_the_size_and_the_content() {
    let a = temp_dir("verify-a");
    let b = temp_dir("verify-b");
    for dir in [&a, &b] {
        std::fs::create_dir_all(dir.join("sub")).unwrap();
    }
    std::fs::write(a.join("same.jpg"), b"hello").unwrap();
    std::fs::write(b.join("same.jpg"), b"hello").unwrap();
    std::fs::write(a.join("size.jpg"), b"aa").unwrap();
    std::fs::write(b.join("size.jpg"), b"bbb").unwrap();
    std::fs::write(a.join("sub/content.jpg"), b"hello").unwrap();
    std::fs::write(b.join("sub/content.jpg"), b"world").unwrap();
    std::fs::write(a.join("only-a.jpg"), b"a").unwrap();
    std::fs::write(b.join("only-b.jpg"), b"b").unwrap();

    let r = verify(&a, &b, 2, Lang::default(), None).unwrap();
    assert_eq!(r.same, 1);
    assert_eq!(r.size_diff.len(), 1);
    assert_eq!(r.content_diff, vec!["sub/content.jpg".to_string()]);
    assert_eq!(r.only_a, vec!["only-a.jpg".to_string()]);
    assert_eq!(r.only_b, vec!["only-b.jpg".to_string()]);
    assert!(!r.identical());
}

#[test]
fn a_cancel_writes_no_incomplete_manifest() {
    let root = temp_dir("cancelled");
    std::fs::write(root.join("a.txt"), b"hello").unwrap();
    let out = root.join("list.sha256");
    // A partial manifest must not appear on disk: otherwise the check would call
    // the unread files "extra".
    let p = Arc::new(Progress::default());
    p.cancel();
    let stats = manifest(&root, Some(&out), 1, Lang::default(), Some(p)).unwrap();
    assert!(!stats.written);
    assert_eq!(stats.unchecked, 1);
    assert_eq!(stats.errors, 0);
    assert!(!out.exists());
}

/// The CLI and the "Integrity" screen see the same text: counters, paths and the verdict.
#[test]
fn the_check_report_writes_the_counters_the_paths_and_the_verdict() {
    let r = Report {
        files_a: 2,
        files_b: 2,
        only_a: vec!["only.jpg".into()],
        only_b: Vec::new(),
        size_diff: vec![("size.jpg".into(), 10, 20)],
        content_diff: Vec::new(),
        read_errors: Vec::new(),
        same: 1,
        hashed: 1,
        hashed_bytes: 10,
        unchecked: 0,
        scan_errors: 0,
        skipped_a: 0,
        skipped_b: 0,
        cancelled: false,
        seconds: 1.5,
        lang: Lang::Ru,
    };
    let text = verify_report(Path::new("/a"), Path::new("/b"), &r, true);
    assert!(text.contains("только в A: 1"), "{text}");
    assert!(text.contains("только в A:\n  only.jpg"), "{text}");
    assert!(text.contains("размер отличается:\n  size.jpg"), "{text}");
    assert!(text.contains("вердикт: найдены расхождения"), "{text}");

    // Without "show all" the list is cut and says so.
    let mut many = r;
    many.only_a = (0..25).map(|n| format!("p{n}.jpg")).collect();
    let text = verify_report(Path::new("/a"), Path::new("/b"), &many, false);
    assert!(text.contains("… ещё 5 (показать все)"), "{text}");
    assert!(!text.contains("p24.jpg"), "{text}");
}

/// The manifest path appears in the report only when the file was really written:
/// a stopped or failed pass must not look like a success.
#[test]
fn the_manifest_report_does_not_pass_a_cancel_for_a_write() {
    let stats = ManifestStats {
        files: 3,
        bytes: 30,
        skipped: 0,
        errors: 0,
        unchecked: 0,
        cancelled: false,
        written: true,
        seconds: 0.5,
        lang: Lang::Ru,
    };
    let text = manifest_report(
        Path::new("/archive"),
        Some(Path::new("/tmp/list.sha256")),
        &stats,
    );
    assert!(text.contains("слепок /archive: 3 файлов"), "{text}");
    assert!(text.contains("→ /tmp/list.sha256"), "{text}");

    let cancelled = ManifestStats {
        cancelled: true,
        written: false,
        ..stats
    };
    let text = manifest_report(
        Path::new("/archive"),
        Some(Path::new("/tmp/list.sha256")),
        &cancelled,
    );
    assert!(text.starts_with("отменено: "), "{text}");
    assert!(!text.contains("→"), "{text}");
}

#[test]
fn the_manifest_check_report_names_the_mismatches() {
    let r = CheckReport {
        missing: vec!["gone.jpg".into()],
        extra: Vec::new(),
        changed: vec!["edit.jpg".into()],
        read_errors: Vec::new(),
        same: 4,
        unchecked: 0,
        scan_errors: 0,
        skipped: 1,
        cancelled: false,
        seconds: 0.2,
        lang: Lang::Ru,
    };
    let text = check_report(
        Path::new("/tmp/list.sha256"),
        Path::new("/archive"),
        &r,
        true,
    );
    assert!(text.contains("совпало: 4, изменено: 1"), "{text}");
    assert!(text.contains("нет в каталоге:\n  gone.jpg"), "{text}");
    assert!(text.contains("каталог расходится со слепком"), "{text}");
}
