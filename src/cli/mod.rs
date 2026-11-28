//! The command line: `main.rs` parses and dispatches, one file per subcommand owns its work.

pub mod albums;
pub mod clusters;
pub mod doctor;
pub mod embed;
pub mod eval;
pub mod faces;
pub mod flags;
pub mod index;
pub mod models;
pub mod ocr;
pub mod report;
pub mod search;
pub mod settings;
pub mod stats;
pub mod tags;

pub use flags::{flag_num, flag_str, has_flag};

use crate::db;
use crate::i18n::{self, Lang};
use anyhow::Result;
use std::path::PathBuf;

/// The `--help` text, in the language of the settings.
pub fn usage(lang: Lang) {
    println!("{}", i18n::t(lang, i18n::USAGE.0));
}

/// The language of the report: `settings.lang` from the DB (M21.6). The first launch takes
/// the language from the locale, like the GUI does, so `photocat stats` and the window agree.
pub fn language(args: &[String]) -> Result<Lang> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    let conn = db::open(&db_path)?;
    db::ensure_lang(&conn)?;
    Ok(db::load_settings(&conn).lang)
}

/// A report piped into a reader that has already left (`photocat stats | head -3`) is not a crash:
/// the reader asked for less, and the write fails with a closed pipe. The panic `println!` raises on
/// that error becomes a quiet exit; every other panic keeps its message and its backtrace note.
pub fn quiet_broken_pipe() {
    let fallback = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if is_broken_pipe(info.payload()) {
            std::process::exit(0);
        }
        fallback(info);
    }));
}

/// The payload of the panic that a failing `print!`/`println!` raises: the standard library writes
/// “failed printing to stdout: …” and the OS names the closed pipe (“Broken pipe”, “The pipe is
/// being closed”). Anything else is not this case.
fn is_broken_pipe(payload: &(dyn std::any::Any + Send)) -> bool {
    let text = payload
        .downcast_ref::<String>()
        .map(String::as_str)
        .or_else(|| payload.downcast_ref::<&str>().copied())
        .unwrap_or("");
    text.starts_with("failed printing to") && text.to_lowercase().contains("pipe")
}

#[cfg(test)]
mod cli_i18n_tests {
    use super::is_broken_pipe;
    use crate::i18n::Lang;
    use crate::{dupes, embed, faces, i18n, index, ocr, organize, tools, verify};

    /// M24.3: a report cut short by its reader (`photocat stats | head`) exits quietly instead of a
    /// Rust backtrace; any other panic keeps its message.
    #[test]
    fn a_closed_pipe_is_not_a_crash() {
        let closed: &(dyn std::any::Any + Send) =
            &"failed printing to stdout: Broken pipe (os error 32)".to_string();
        let closed_windows: &(dyn std::any::Any + Send) =
            &"failed printing to stdout: The pipe is being closed. (os error 232)".to_string();
        let other: &(dyn std::any::Any + Send) = &"assertion failed: a pipe dream";
        assert!(is_broken_pipe(closed));
        assert!(is_broken_pipe(closed_windows));
        assert!(!is_broken_pipe(other));
    }

    /// M21.6: the CLI reports speak the language of `settings.lang`. Every report is a pair
    /// of lines, English and Russian, from the same `lang` field, so it is visible that the
    /// language is not global and both variants are alive.
    #[test]
    fn english_translates_the_command_reports() {
        // index
        let mut index = index::Stats {
            total: 3,
            indexed: 2,
            skipped: 1,
            errors: 0,
            bytes: 2048,
            seconds: 1.5,
            cancelled: false,
            lang: Lang::En,
        };
        let text = index.to_string();
        assert!(text.contains("indexed 2/3 files"), "{text}");
        assert!(text.contains("2.0 KiB"), "{text}");
        index.lang = Lang::Ru;
        assert!(index.to_string().contains("проиндексировано 2/3 файлов"));
        index.cancelled = true;
        assert!(index.to_string().starts_with("остановлено: "));
        index.lang = Lang::En;
        assert!(index.to_string().starts_with("stopped: "));

        // ocr
        let mut ocr = ocr::Stats {
            candidates: 4,
            done: 3,
            skipped: 1,
            pass_skipped: 0,
            errors: 0,
            chars: 120,
            seconds: 2.0,
            cancelled: false,
            lang: Lang::En,
        };
        let text = ocr.to_string();
        assert!(text.contains("recognised 3 of 4 files"), "{text}");
        assert!(text.contains("(1 skipped, text already there)"), "{text}");
        ocr.lang = Lang::Ru;
        assert!(ocr.to_string().contains("распознано 3 из 4 файлов"));

        // faces
        let mut faces = faces::Stats {
            images: 10,
            done: 8,
            skipped: 2,
            pass_skipped: 0,
            faces: 20,
            clusters: 5,
            matched: Default::default(),
            errors: 0,
            seconds: 3.0,
            cancelled: false,
            lang: Lang::En,
        };
        assert!(faces.to_string().contains("scanned 8 of 10 photos"));
        faces.lang = Lang::Ru;
        assert!(faces.to_string().contains("просканировано 8 из 10 снимков"));

        // The matching step of M26.11: one report, the same numbers in both languages, and its
        // reasons named — a dry run says "would match".
        let mut matched = faces::MatchReport {
            matched: 12,
            people: 3,
            named: 5,
            left_small: 4,
            left_far: 2,
            seconds: 0.07,
            dry_run: false,
            lang: Lang::En,
        };
        assert_eq!(
            matched.to_string(),
            "proposed 12 faces to 3 named people — they wait in quarantine; 4 left — the lead was too small; \
             2 left — no named person is close enough"
        );
        matched.lang = Lang::Ru;
        let ru = matched.to_string();
        assert!(
            ru.contains("предложено 12 лиц 3 названным людям")
                && ru.contains("4 осталось — перевес слишком мал"),
            "{ru}"
        );
        matched.dry_run = true;
        matched.lang = Lang::En;
        assert!(matched.to_string().starts_with("would propose 12 faces"));
        matched.matched = 0;
        matched.people = 0;
        assert!(matched.to_string().starts_with("no new proposal yet: 5 named"));
        matched.named = 0;
        assert!(matched
            .to_string()
            .contains("no named people yet: there is nothing to match to"));

        let mut clusters = faces::ClusterStats {
            faces: 20,
            clusters: 5,
            fixed: 2,
            pass_skipped: 0,
            matched: faces::MatchReport {
                matched: 12,
                people: 3,
                named: 5,
                lang: Lang::En,
                ..Default::default()
            },
            seconds: 1.0,
            lang: Lang::En,
        };
        assert!(clusters.to_string().contains("20 faces, 5 groups"));
        assert!(
            clusters
                .to_string()
                .ends_with("; proposed 12 faces to 3 named people — they wait in quarantine"),
            "{clusters}"
        );
        clusters.lang = Lang::Ru;
        assert!(clusters.to_string().contains("лиц 20, групп 5"));
        clusters.matched.lang = Lang::Ru;
        assert!(clusters.to_string().contains("предложено 12 лиц 3 названным людям"));

        // embed
        let mut embed = embed::EmbedStats {
            model: "clip-vit-b32",
            total: 10,
            computed: 9,
            skipped: 1,
            pass_skipped: 0,
            errors: 0,
            seconds: 4.0,
            model_bytes: 2048,
            cancelled: false,
            lang: Lang::En,
        };
        let text = embed.to_string();
        assert!(text.contains("computed 9/10"), "{text}");
        assert!(text.contains("2.0 KiB"), "{text}");
        embed.lang = Lang::Ru;
        assert!(embed.to_string().contains("посчитано 9/10"));

        let mut tags = embed::RetagStats {
            model: "clip-vit-b32",
            total: 10,
            tagged: 10,
            pass_skipped: 0,
            seconds: 1.0,
            lang: Lang::En,
        };
        assert!(tags.to_string().contains("tags (clip-vit-b32): recomputed 10/10"));
        tags.lang = Lang::Ru;
        assert!(tags.to_string().contains("теги (clip-vit-b32): пересчитано 10/10"));

        // organize: both the plan summary and the export report
        let mut export = organize::ExportReport {
            copied: 2,
            bytes: 2048,
            lang: Lang::En,
            ..Default::default()
        };
        let text = export.to_string();
        assert!(text.contains("exported 2 files"), "{text}");
        assert!(text.contains("2.0 KiB"), "{text}");
        export.lang = Lang::Ru;
        assert!(export.to_string().contains("выгружено 2 файлов"));
        assert_eq!(organize::Layout::Day.title(Lang::En), "by day");
        assert_eq!(organize::Layout::Day.title(Lang::Ru), "по дням");

        // tools: the same words `doctor` prints and Settings shows
        assert_eq!(
            tools::Tool::Tesseract.purpose(Lang::En),
            "OCR: text from documents and screenshots"
        );
        assert_eq!(
            tools::Tool::Tesseract.purpose(Lang::Ru),
            "OCR: текст с документов и скриншотов"
        );
        let t = tools::Tools::default();
        assert_eq!(t.source(tools::Tool::Tesseract, Lang::En), "from PATH");
        assert_eq!(t.source(tools::Tool::Tesseract, Lang::Ru), "из PATH");
    }

    /// M21.6: the duplicates give a feature and advice parts instead of a ready Russian phrase —
    /// the same set is printed in both languages.
    #[test]
    fn english_translates_the_duplicate_advice() {
        assert_eq!(dupes::Reason::Exact.label(Lang::En), "exact copy");
        assert_eq!(dupes::Reason::Similar.label(Lang::Ru), "похожий снимок");

        let advice = dupes::Advice {
            same_content: false,
            parts: vec![
                dupes::AdvicePart::SharperBy(150.0),
                dupes::AdvicePart::MorePixels,
            ],
        };
        assert_eq!(
            advice.label(Lang::En),
            "sharper than the rest by 150% · more pixels"
        );
        assert_eq!(
            advice.label(Lang::Ru),
            "резче остальных на 150% · больше по разрешению"
        );
        let same = dupes::Advice {
            same_content: true,
            parts: Vec::new(),
        };
        assert_eq!(same.label(Lang::En), "exact copies — no difference");
        assert_eq!(same.label(Lang::Ru), "точные копии — разницы нет");
    }

    /// M21.6: the comparison text, the manifest check and the manifest result are the same
    /// source of words as in the CLI, and they follow the `lang` field.
    #[test]
    fn english_translates_the_verification_reports() {
        let mut report = verify::Report {
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
            lang: Lang::En,
        };
        let text = verify::verify_report(std::path::Path::new("/a"), std::path::Path::new("/b"), &report, true);
        assert!(text.contains("comparison: /a"), "{text}");
        assert!(text.contains("only in A: 1"), "{text}");
        assert!(text.contains("size differs:\n  size.jpg"), "{text}");
        assert!(text.contains("verdict: differences found"), "{text}");
        report.lang = Lang::Ru;
        let text = verify::verify_report(std::path::Path::new("/a"), std::path::Path::new("/b"), &report, true);
        assert!(text.contains("только в A: 1"), "{text}");
        assert!(text.contains("вердикт: найдены расхождения"), "{text}");

        let mut check = verify::CheckReport {
            missing: vec!["gone.jpg".into()],
            extra: Vec::new(),
            changed: Vec::new(),
            read_errors: Vec::new(),
            same: 4,
            unchecked: 0,
            scan_errors: 0,
            skipped: 1,
            cancelled: false,
            seconds: 0.2,
            lang: Lang::En,
        };
        let text = verify::check_report(
            std::path::Path::new("/tmp/list.sha256"),
            std::path::Path::new("/archive"),
            &check,
            true,
        );
        assert!(text.contains("manifest check: /tmp/list.sha256"), "{text}");
        assert!(text.contains("missing from the folder:\n  gone.jpg"), "{text}");
        assert!(text.contains("the folder does not match the manifest"), "{text}");
        check.lang = Lang::Ru;
        let text = verify::check_report(
            std::path::Path::new("/tmp/list.sha256"),
            std::path::Path::new("/archive"),
            &check,
            true,
        );
        assert!(text.contains("нет в каталоге:\n  gone.jpg"), "{text}");

        let mut stats = verify::ManifestStats {
            files: 3,
            bytes: 30,
            skipped: 0,
            errors: 0,
            unchecked: 0,
            cancelled: false,
            written: true,
            seconds: 0.5,
            lang: Lang::En,
        };
        let text = verify::manifest_report(std::path::Path::new("/archive"), None, &stats);
        assert!(text.contains("manifest /archive: 3 files"), "{text}");
        stats.lang = Lang::Ru;
        assert!(
            verify::manifest_report(std::path::Path::new("/archive"), None, &stats)
                .contains("слепок /archive: 3 файлов")
        );
    }

    /// M21.6: `--help` is part of the CLI and is translated with it; the English text holds no
    /// Cyrillic, and the Russian one stays Russian.
    #[test]
    fn the_cli_help_is_translated() {
        let en = i18n::t(Lang::En, i18n::USAGE.0);
        assert!(en.starts_with("photocat — media archive cataloguer"), "{en}");
        assert!(en.contains("subcommands:"));
        assert!(en.contains("default: GUI"));
        assert!(
            !en.chars().any(|c| c.is_alphabetic() && ('\u{0400}'..='\u{04ff}').contains(&c)),
            "the English help still holds Cyrillic: {en}"
        );
        let ru = i18n::t(Lang::Ru, i18n::USAGE.0);
        assert!(ru.starts_with("photocat — каталогизатор медиаархива"), "{ru}");
    }
}
