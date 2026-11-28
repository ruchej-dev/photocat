//! The tests of the whole table: every area is checked together, because `t()` sees one map.
//!
//! `use super::*` reaches the re-exports; `table::AREAS` is read directly by the duplicate test,
//! so a pair that lands in two areas is named with the area it repeats in.

use super::table::{AREAS, pairs};
use super::*;

#[test]
fn the_russian_translation_is_found_and_the_english_stays() {
    assert_eq!(t(Lang::En, "Settings"), "Settings");
    assert_eq!(t(Lang::Ru, "Settings"), "Настройки");
    assert_eq!(
        t(Lang::En, "a string without a translation"),
        "a string without a translation"
    );
    assert_eq!(
        t(Lang::Ru, "a string without a translation"),
        "a string without a translation"
    );
}

#[test]
fn the_language_code_survives_parsing_and_unknown_gives_the_main_one() {
    assert_eq!(Lang::parse("ru"), Lang::Ru);
    assert_eq!(Lang::parse(" RU "), Lang::Ru);
    assert_eq!(Lang::parse("en"), Lang::En);
    assert_eq!(Lang::parse(""), Lang::En);
    assert_eq!(Lang::parse("de"), Lang::En);
    for l in Lang::ALL {
        assert_eq!(Lang::parse(l.code()), l);
    }
}

#[test]
fn the_system_locale_gives_a_language_or_stays_silent() {
    assert_eq!(Lang::from_tag("ru_RU.UTF-8"), Some(Lang::Ru));
    assert_eq!(Lang::from_tag("ru-RU"), Some(Lang::Ru));
    assert_eq!(Lang::from_tag("en_US.utf8"), Some(Lang::En));
    assert_eq!(Lang::from_tag("de:ru"), Some(Lang::Ru));
    assert_eq!(Lang::from_tag("C"), None);
    assert_eq!(Lang::from_tag("POSIX"), None);
    assert_eq!(Lang::from_tag("de_DE"), None);
    assert_eq!(Lang::from_tag(""), None);
    // An unknown locale never panics: there is always a main language.
    assert_eq!(Lang::default(), Lang::En);
}

#[test]
fn the_position_counter_and_the_interval_in_both_languages() {
    assert_eq!(position(Lang::En, 3, 12), "3 of 12");
    assert_eq!(position(Lang::Ru, 3, 12), "3 из 12");
    assert_eq!(seconds(Lang::En, 5), "5 s");
    assert_eq!(seconds(Lang::Ru, 5), "5 с");
}

#[test]
fn the_counter_puts_the_right_number_in_both_languages() {
    assert_eq!(count(Lang::Ru, 1, "file"), "1 файл");
    assert_eq!(count(Lang::Ru, 2, "file"), "2 файла");
    assert_eq!(count(Lang::Ru, 5, "file"), "5 файлов");
    assert_eq!(count(Lang::Ru, 11, "file"), "11 файлов");
    assert_eq!(count(Lang::Ru, 21, "file"), "21 файл");
    assert_eq!(count(Lang::En, 1, "file"), "1 file");
    assert_eq!(count(Lang::En, 3, "file"), "3 files");
    // Two nouns share the English word and differ in Russian.
    assert_eq!(count(Lang::Ru, 3, "shot"), "3 снимка");
    assert_eq!(count(Lang::En, 3, "shot"), "3 photos");
    assert_eq!(count(Lang::Ru, 3, "photo"), "3 фото");
    assert_eq!(count(Lang::En, 3, "person"), "3 people");
    assert_eq!(count(Lang::Ru, 1, "person"), "1 лицо");
}

#[test]
#[should_panic(expected = "not registered")]
fn an_unknown_noun_stops_the_counter() {
    let _ = count(Lang::En, 1, "unicorn");
}

#[test]
fn months_in_both_languages_and_no_panic() {
    assert_eq!(month_name(Lang::En, 5), "May");
    assert_eq!(month_name(Lang::Ru, 5), "май");
    assert_eq!(month_name(Lang::En, 12), "December");
    assert_eq!(month_name(Lang::Ru, 0), "январь");
    assert_eq!(month_name(Lang::Ru, 99), "декабрь");
}

#[test]
fn the_size_units_follow_the_language() {
    assert_eq!(units(Lang::En), ["B", "KiB", "MiB", "GiB", "TiB"]);
    assert_eq!(units(Lang::Ru), ["Б", "КиБ", "МиБ", "ГиБ", "ТиБ"]);
}

#[test]
fn weekdays_in_both_languages() {
    assert_eq!(weekday(Lang::En, 0), "Mon");
    assert_eq!(weekday(Lang::En, 6), "Sun");
    assert_eq!(weekday(Lang::Ru, 0), "Пн");
    assert_eq!(weekday(Lang::Ru, 6), "Вс");
    // Out of range means Sunday, not a panic.
    assert_eq!(weekday(Lang::Ru, 9), "Вс");
}

/// The area tables are one table: every area is named once, none is empty, and no English string
/// lands in two of them — a duplicate would silently pick one translation.
#[test]
fn the_area_tables_are_named_once_and_hold_no_duplicate_key() {
    let mut names = std::collections::HashSet::new();
    let mut seen = std::collections::HashSet::new();
    let mut total = 0;
    for (name, terms) in AREAS {
        assert!(!terms.is_empty(), "the area `{name}` has no terms");
        assert!(names.insert(*name), "the area `{name}` is listed twice");
        for (en, ru) in *terms {
            assert!(!en.trim().is_empty(), "an empty key in `{name}`");
            assert!(
                !ru.trim().is_empty(),
                "an empty translation for {en} in `{name}`"
            );
            assert!(
                seen.insert(*en),
                "the key repeats across areas: {en} in `{name}`"
            );
        }
        total += terms.len();
    }
    assert_eq!(total, pairs().len(), "the ordered table lost or doubled a pair");
}

#[test]
fn the_table_has_no_empty_or_duplicate_keys() {
    let mut seen = std::collections::HashSet::new();
    for (en, ru) in pairs() {
        assert!(!en.trim().is_empty(), "an empty key");
        assert!(!ru.trim().is_empty(), "an empty translation for {en}");
        assert_ne!(en, ru, "the translation repeats the source: {en}");
        assert!(seen.insert(*en), "the key repeats: {en}");
    }
}

#[test]
fn every_translated_string_is_really_russian() {
    for (en, ru) in pairs() {
        assert!(
            ru.chars().any(|c| c.is_alphabetic() && !c.is_ascii()),
            "a translation without Cyrillic: {en} → {ru}"
        );
        assert!(
            !en.chars().any(|c| c.is_alphabetic() && !c.is_ascii()),
            "a key without English letters: {en}"
        );
    }
}

/// `fill` substitutes values in order and keeps the text readable when there are fewer values
/// than placeholders: a report stays a report instead of turning empty.
#[test]
fn fill_substitutes_values_in_order() {
    let values = ["2".to_string(), "3".to_string()];
    assert_eq!(
        fill(Lang::En, "total: {} files, {}", &values),
        "total: 2 files, 3"
    );
    // The translation sets the word order, not the code.
    assert_eq!(
        fill(Lang::Ru, "total: {} files, {}", &values),
        "всего: 2 файлов, 3"
    );
    assert_eq!(
        fill(Lang::En, "without values {}", &[]),
        "without values {}"
    );
    assert_eq!(
        fill(
            Lang::En,
            "extra values {}",
            &["1".to_string(), "2".to_string()]
        ),
        "extra values 1"
    );
}

/// The placeholders of a translation must match the source in number: otherwise the Russian
/// report loses a number or leaves `{}` in place.
#[test]
fn the_translations_have_the_same_number_of_placeholders() {
    for (en, ru) in pairs() {
        let en_n = en.matches("{}").count();
        let ru_n = ru.matches("{}").count();
        assert_eq!(en_n, ru_n, "a different number of `{{}}`: «{en}» → «{ru}»");
    }
}

/// Every English string the code passes to `t()` or `fill()` is in the table: a forgotten pair
/// would otherwise leave the Russian interface with an English label. Modules that still pass
/// Russian source strings are pending the M23 refactor and are skipped here.
/// A word from data: the default tag labels are English in the source, the Russian window
/// reads them from the table, and the owner's own label stays exactly as typed.
#[test]
fn a_data_word_is_translated_but_an_own_label_is_not() {
    assert_eq!(word(Lang::En, "beach"), "beach");
    assert_eq!(word(Lang::Ru, "beach"), "пляж");
    assert_eq!(word(Lang::Ru, "document"), "документ");
    assert_eq!(word(Lang::Ru, "Шкаф"), "Шкаф");
}

#[test]
fn every_report_string_is_in_the_table() {
    let sources = [
        ("main.rs", include_str!("../main.rs")),
        ("cli/mod.rs", include_str!("../cli/mod.rs")),
        ("cli/report.rs", include_str!("../cli/report.rs")),
        ("cli/flags.rs", include_str!("../cli/flags.rs")),
        ("cli/index.rs", include_str!("../cli/index.rs")),
        ("cli/stats.rs", include_str!("../cli/stats.rs")),
        ("cli/embed.rs", include_str!("../cli/embed.rs")),
        ("cli/faces.rs", include_str!("../cli/faces.rs")),
        ("cli/ocr.rs", include_str!("../cli/ocr.rs")),
        ("cli/search.rs", include_str!("../cli/search.rs")),
        ("cli/models.rs", include_str!("../cli/models.rs")),
        ("cli/settings.rs", include_str!("../cli/settings.rs")),
        ("cli/eval.rs", include_str!("../cli/eval.rs")),
        ("cli/clusters.rs", include_str!("../cli/clusters.rs")),
        ("cli/doctor.rs", include_str!("../cli/doctor.rs")),
        ("cli/albums.rs", include_str!("../cli/albums.rs")),
        ("cli/tags.rs", include_str!("../cli/tags.rs")),
        ("db/mod.rs", include_str!("../db/mod.rs")),
        ("db/schema.rs", include_str!("../db/schema.rs")),
        ("db/settings.rs", include_str!("../db/settings.rs")),
        ("db/files.rs", include_str!("../db/files.rs")),
        ("db/tags.rs", include_str!("../db/tags.rs")),
        ("db/faces.rs", include_str!("../db/faces.rs")),
        ("db/albums.rs", include_str!("../db/albums.rs")),
        ("db/progress.rs", include_str!("../db/progress.rs")),
        ("db/human.rs", include_str!("../db/human.rs")),
        ("index.rs", include_str!("../index.rs")),
        ("dupes.rs", include_str!("../dupes.rs")),
        ("organize/mod.rs", include_str!("../organize/mod.rs")),
        ("organize/plan.rs", include_str!("../organize/plan.rs")),
        ("organize/apply.rs", include_str!("../organize/apply.rs")),
        ("organize/export.rs", include_str!("../organize/export.rs")),
        ("verify/mod.rs", include_str!("../verify/mod.rs")),
        ("verify/compare.rs", include_str!("../verify/compare.rs")),
        ("verify/manifest.rs", include_str!("../verify/manifest.rs")),
        ("embed/mod.rs", include_str!("../embed/mod.rs")),
        ("embed/model.rs", include_str!("../embed/model.rs")),
        ("embed/tags.rs", include_str!("../embed/tags.rs")),
        ("embed/search.rs", include_str!("../embed/search.rs")),
        ("embed/kmeans.rs", include_str!("../embed/kmeans.rs")),
        ("embed/download.rs", include_str!("../embed/download.rs")),
        ("ocr.rs", include_str!("../ocr.rs")),
        ("faces.rs", include_str!("../faces.rs")),
        ("tools.rs", include_str!("../tools.rs")),
    ];
    let mut missing = Vec::new();
    for (name, src) in sources {
        for key in literal_keys(src) {
            // An English key must be in the table; a Russian one waits for its conversion.
            // A template of placeholders and single letters (A/B columns) is not a phrase.
            if !key.chars().any(|c| c.is_ascii_lowercase()) {
                continue;
            }
            if !pairs().iter().any(|(en, _)| *en == key) {
                missing.push(format!("{name}: «{key}»"));
            }
        }
    }
    assert!(missing.is_empty(), "no translation: {}", missing.join(", "));
}

/// The string literals passed to `i18n::t(` / `i18n::fill(` as the second argument.
fn literal_keys(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for marker in ["i18n::t(", "i18n::fill("] {
        let mut rest = src;
        while let Some(i) = rest.find(marker) {
            rest = &rest[i + marker.len()..];
            let Some(comma) = rest.find(',') else { continue };
            let tail = &rest[comma + 1..];
            let Some(q) = tail.find('"') else { continue };
            if tail.find(')').is_some_and(|p| p < q) {
                continue;
            }
            let mut value = String::new();
            let mut chars = tail[q + 1..].chars();
            while let Some(c) = chars.next() {
                match c {
                    '\\' => match chars.next() {
                        Some('n') => value.push('\n'),
                        Some('t') => value.push('\t'),
                        Some('"') => value.push('"'),
                        Some('\\') => value.push('\\'),
                        Some(other) => value.push(other),
                        None => break,
                    },
                    '"' => break,
                    other => value.push(other),
                }
            }
            out.push(value);
        }
    }
    out
}
