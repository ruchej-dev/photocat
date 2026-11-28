//! The language, the lookup and the number words: the mechanism, not the table.
//!
//! The source text of the interface is English; `t()` returns the Russian translation when
//! Russian is chosen. A string that has no translation yet is shown as written instead of
//! disappearing.

use std::collections::HashMap;
use std::sync::OnceLock;

/// The language of the interface. Stored in the DB as a code (`en`, `ru`), chosen in the settings.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// The main language of the project (docs/, README, commits).
    #[default]
    En,
    /// The second language: the former interface and the former documentation.
    Ru,
}

impl Lang {
    pub const ALL: [Lang; 2] = [Lang::En, Lang::Ru];

    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }

    /// An unknown or empty value means the main language, never a panic (like `GridSort::parse`).
    pub fn parse(s: &str) -> Lang {
        match s.trim().to_ascii_lowercase().as_str() {
            "ru" | "rus" | "russian" => Lang::Ru,
            _ => Lang::En,
        }
    }

    /// The label in the chooser: always in its own language, so it is recognised without a translation.
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }

    /// The language from a locale tag: `ru_RU.UTF-8`, `en_US`, `ru:en`, `ru-RU`. `None` means the tag
    /// is not about a language (`C`, `POSIX`), or the language is unknown: the caller decides then.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        // `LANGUAGE` is a preference list separated by `:`; the first known language wins.
        for part in tag.split(':') {
            let Some(head) = part.trim().split(['.', '@']).next() else {
                continue;
            };
            let Some(base) = head.split(['_', '-']).next() else {
                continue;
            };
            match base.to_ascii_lowercase().as_str() {
                "ru" => return Some(Lang::Ru),
                "en" => return Some(Lang::En),
                _ => {}
            }
        }
        None
    }

    /// The language of the system on the first launch (M21, ADR 0040). The order of the variables
    /// follows POSIX: a set `LC_ALL`/`LC_MESSAGES` decides on its own (`C` there means "no
    /// language", nothing else is looked at); otherwise `LANGUAGE` (a preference list) and `LANG`.
    /// An unknown language means English, as the owner asked.
    pub fn from_env() -> Lang {
        for key in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
            let Ok(value) = std::env::var(key) else {
                continue;
            };
            if value.trim().is_empty() {
                continue;
            }
            return Lang::from_tag(&value).unwrap_or(Lang::En);
        }
        Lang::En
    }
}

/// The translation table as a lookup by the English string.
fn map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| super::table::pairs().iter().copied().collect())
}

/// The Russian text of an English interface string; `None` — not translated yet.
pub fn russian(en: &'static str) -> Option<&'static str> {
    map().get(en).copied()
}

/// A word that comes from data — a tag label, a model title, a duplicate reason: the table may know
/// it (the default set does), while the owner's own wording has no translation and stays as it is.
pub fn word(lang: Lang, text: &str) -> &str {
    match lang {
        Lang::En => text,
        Lang::Ru => map().get(text).copied().unwrap_or(text),
    }
}

/// Interface text: the English source as written, or its Russian translation.
/// A string missing from the table is shown in English: the source is always readable, and the gap
/// is a visible state rather than a key leaking into the window.
pub fn t(lang: Lang, en: &'static str) -> &'static str {
    match lang {
        Lang::En => en,
        Lang::Ru => russian(en).unwrap_or(en),
    }
}

/// Substitute values into a template: `{}` are replaced in order, the rest of the text stays as it
/// is. CLI reports (M21.6) keep the whole phrase in the table and pass the numbers here, so the
/// word order is set by the translation and not by `format!` in the code. Extra values are ignored,
/// missing ones leave `{}` in place: a report never disappears because of a typo in a translation.
pub fn fill(lang: Lang, en: &'static str, values: &[String]) -> String {
    let tpl = t(lang, en);
    let mut out = String::with_capacity(tpl.len() + 32);
    let mut rest = tpl;
    let mut used = 0usize;
    while let Some(i) = rest.find("{}") {
        out.push_str(&rest[..i]);
        match values.get(used) {
            Some(v) => out.push_str(v),
            None => out.push_str("{}"),
        }
        used += 1;
        rest = &rest[i + 2..];
    }
    out.push_str(rest);
    out
}

/// The nouns the interface counts, with their plural forms: English `[one, many]`, Russian
/// `[one, 2–4, 5+]`. The forms live here so no Russian word appears on a screen.
const NOUNS: &[(&str, [&str; 2], [&str; 3])] = &[
    ("file", ["file", "files"], ["файл", "файла", "файлов"]),
    ("photo", ["photo", "photos"], ["фото", "фото", "фото"]),
    ("shot", ["photo", "photos"], ["снимок", "снимка", "снимков"]),
    ("person", ["person", "people"], ["лицо", "лица", "лиц"]),
    ("group", ["group", "groups"], ["группа", "группы", "групп"]),
    (
        "proposal",
        ["proposal", "proposals"],
        ["предложение", "предложения", "предложений"],
    ),
    ("tag", ["tag", "tags"], ["тег", "тега", "тегов"]),
    ("album", ["album", "albums"], ["альбом", "альбома", "альбомов"]),
    ("year", ["year", "years"], ["год", "года", "лет"]),
    ("week", ["week", "weeks"], ["неделя", "недели", "недель"]),
    (
        "sub_album",
        ["sub-album", "sub-albums"],
        ["подальбом", "подальбома", "подальбомов"],
    ),
];

/// A counter with the right number: English has two plural forms, Russian three.
/// English needs two forms, Russian three: [one, 2–4, 5+]. The noun is a key of `NOUNS`.
pub fn count(lang: Lang, n: usize, noun: &'static str) -> String {
    let entry = NOUNS
        .iter()
        .find(|(id, _, _)| *id == noun)
        .unwrap_or_else(|| panic!("the counted noun `{noun}` is not registered in NOUNS"));
    match lang {
        Lang::En => {
            let word = if n == 1 { entry.1[0] } else { entry.1[1] };
            format!("{n} {word}")
        }
        Lang::Ru => {
            let last2 = n % 100;
            let word = if (11..=14).contains(&last2) {
                entry.2[2]
            } else {
                match n % 10 {
                    1 => entry.2[0],
                    2..=4 => entry.2[1],
                    _ => entry.2[2],
                }
            };
            format!("{n} {word}")
        }
    }
}

/// The size units of the interface: bytes and their binary multiples.
/// The number format is one; only these five labels change with the language.
pub fn units(lang: Lang) -> [&'static str; 5] {
    match lang {
        Lang::En => ["B", "KiB", "MiB", "GiB", "TiB"],
        Lang::Ru => ["Б", "КиБ", "МиБ", "ГиБ", "ТиБ"],
    }
}

/// The name of a month by its number 1–12 in the language of the interface: the grid and the
/// calendar take it from here.
pub fn month_name(lang: Lang, month: usize) -> &'static str {
    let i = month.clamp(1, 12) - 1;
    match lang {
        Lang::En => EN_MONTHS[i],
        Lang::Ru => RU_MONTHS[i],
    }
}

const RU_MONTHS: [&str; 12] = [
    "январь",
    "февраль",
    "март",
    "апрель",
    "май",
    "июнь",
    "июль",
    "август",
    "сентябрь",
    "октябрь",
    "ноябрь",
    "декабрь",
];

const EN_MONTHS: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

/// The short name of a weekday (0 — Monday): the calendar takes it from here.
/// The week starts on Monday in both languages.
pub fn weekday(lang: Lang, day: usize) -> &'static str {
    let i = day.min(6);
    match lang {
        Lang::En => EN_WEEKDAYS[i],
        Lang::Ru => RU_WEEKDAYS[i],
    }
}

const RU_WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
const EN_WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// «by relevance: 4 files» — the whole phrase is translated, the number is substituted.
pub fn relevance(lang: Lang, n: usize) -> String {
    format!("{} {}", t(lang, "by relevance:"), count(lang, n, "file"))
}

/// «Select all (4)».
pub fn select_all(lang: Lang, n: usize) -> String {
    format!("{} ({n})", t(lang, "Select all"))
}

/// «3 of 12» — the counter of the full screen.
pub fn position(lang: Lang, index: usize, total: usize) -> String {
    match lang {
        Lang::En => format!("{index} of {total}"),
        Lang::Ru => format!("{index} из {total}"),
    }
}

/// «3 s» — the slideshow interval.
pub fn seconds(lang: Lang, n: u64) -> String {
    match lang {
        Lang::En => format!("{n} s"),
        Lang::Ru => format!("{n} с"),
    }
}
