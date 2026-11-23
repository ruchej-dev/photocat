//! Язык интерфейса (M21, ADR 0040): английский — основной, русский — второй.
//!
//! Исходный текст интерфейса пишется по-русски, а `t()` возвращает его английский
//! перевод, если выбран английский. Так перевод можно наращивать экран за экраном,
//! не теряя ни одной строки: непереведённая строка остаётся русской, а не пропадает.
//!
//! Модуль L0: только `std`, без egui и без БД. Язык передаётся явно, а не лежит
//! в глобальной переменной: тогда тест, проверяющий текст, не зависит от соседних
//! тестов и от порядка их запуска.

use std::collections::HashMap;
use std::sync::OnceLock;

/// Язык интерфейса. В БД лежит кодом (`en`, `ru`), в настройках — выбором.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Lang {
    /// Основной язык проекта (docs/, README, коммиты).
    #[default]
    En,
    /// Второй язык: прежний интерфейс и прежняя документация.
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

    /// Неизвестное или пустое значение — основной язык, без падения (как `GridSort::parse`).
    pub fn parse(s: &str) -> Lang {
        match s.trim().to_ascii_lowercase().as_str() {
            "ru" | "rus" | "russian" => Lang::Ru,
            _ => Lang::En,
        }
    }

    /// Подпись в списке выбора: всегда на своём языке, чтобы её узнали без перевода.
    pub fn label(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::Ru => "Русский",
        }
    }

    /// Язык из ярлыка локали: `ru_RU.UTF-8`, `en_US`, `ru:en`, `ru-RU`. `None` — локаль
    /// не про язык (`C`, `POSIX`) или язык нам неизвестен: тогда решает вызывающий.
    pub fn from_tag(tag: &str) -> Option<Lang> {
        // `LANGUAGE` — список предпочтений через `:`; берём первый знакомый язык.
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

    /// Язык системы на первом запуске (M21, ADR 0040). Порядок переменных — как у
    /// POSIX: заданный `LC_ALL`/`LC_MESSAGES` решает сам (`C` там — это «без языка»,
    /// дальше не смотрим); иначе `LANGUAGE` (список предпочтений) и `LANG`.
    /// Неизвестный язык — английский, как просил владелец.
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

/// Английский перевод русской строки интерфейса; `None` — строка ещё не переведена.
pub fn english(ru: &'static str) -> Option<&'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| PAIRS.iter().copied().collect())
        .get(ru)
        .copied()
}

/// Текст интерфейса: на русском языке — как написан, на английском — перевод.
/// Непереведённая строка показывается по-русски: это видимое состояние, а не ошибка.
pub fn t(lang: Lang, ru: &'static str) -> &'static str {
    match lang {
        Lang::Ru => ru,
        Lang::En => english(ru).unwrap_or(ru),
    }
}

/// Счётчик с правильным числом: «3 файла» / «3 files», «1 файл» / «1 file».
/// Русские формы — [единственное, 2–4, 5 и больше]; в английском хватает двух.
pub fn count(
    lang: Lang,
    n: usize,
    ru: [&'static str; 3],
    en_one: &'static str,
    en_many: &'static str,
) -> String {
    match lang {
        Lang::En => {
            let word = if n == 1 { en_one } else { en_many };
            format!("{n} {word}")
        }
        Lang::Ru => {
            let last2 = n % 100;
            let word = if (11..=14).contains(&last2) {
                ru[2]
            } else {
                match n % 10 {
                    1 => ru[0],
                    2..=4 => ru[1],
                    _ => ru[2],
                }
            };
            format!("{n} {word}")
        }
    }
}

/// Название месяца числом 1–12 на языке интерфейса: сетка и календарь берут его здесь.
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

/// «найдено 4 из 10» / «found 4 of 10».
pub fn found(lang: Lang, shown: usize, total: usize) -> String {
    match lang {
        Lang::En => format!("found {shown} of {total}"),
        Lang::Ru => format!("найдено {shown} из {total}"),
    }
}

/// «по релевантности: 4 файла» / «by relevance: 4 files».
pub fn relevance(lang: Lang, n: usize) -> String {
    format!(
        "{} {}",
        t(lang, "по релевантности:"),
        count(lang, n, ["файл", "файла", "файлов"], "file", "files")
    )
}

/// «Выбрать все (4)» / «Select all (4)».
pub fn select_all(lang: Lang, n: usize) -> String {
    format!("{} ({n})", t(lang, "Выбрать все"))
}

/// Все переведённые строки: русский оригинал → английский текст.
///
/// Ключ — ровно та строка, что стоит в коде. Одна и та же русская строка переводится
/// один раз: `t("Настройки")` в навигации, заголовке диалога и подсказке даёт одно слово.
/// Пополняется экран за экраном (см. `plan/I18N.md`).
const PAIRS: &[(&str, &str)] = &[
    // --- остов приложения: навигация, верхняя панель, статус -------------------
    ("Все файлы", "All files"),
    ("Альбомы", "Albums"),
    ("Календарь", "Calendar"),
    ("Лица", "People"),
    ("Дубли и серии", "Duplicates & series"),
    ("Целостность", "Integrity"),
    ("Корзина", "Trash"),
    ("БИБЛИОТЕКА", "LIBRARY"),
    ("ИНСТРУМЕНТЫ", "TOOLS"),
    ("ФИЛЬТР", "FILTER"),
    ("семейный архив", "family archive"),
    ("Фото", "Photos"),
    ("Видео", "Videos"),
    ("Все", "All"),
    ("только фото", "photos only"),
    ("только видео", "videos only"),
    ("без даты", "no date"),
    ("Поиск по пути или дате", "Search by path or date"),
    ("2024-05, отпуск", "2024-05, vacation"),
    ("Каталог", "Folder"),
    (
        "путь к каталогу; несколько — кнопкой «Каталоги»",
        "path to a folder; several — with the “Folders” button",
    ),
    ("Индексировать", "Index"),
    ("Индексация", "Indexing"),
    ("Обновить", "Refresh"),
    ("Настройки", "Settings"),
    ("Каталоги", "Folders"),
    (
        "добавить несколько каталогов и убрать каталог из индекса",
        "add several folders, drop a folder from the index",
    ),
    ("Скрыть сведения", "Hide details"),
    ("Сведения", "Details"),
    (
        "правая панель с датой, камерой, тегами и текстом",
        "right panel with date, camera, tags and text",
    ),
    ("Смысловой", "Semantic"),
    ("По тексту", "By text"),
    ("Найти", "Search"),
    ("Сбросить", "Reset"),
    ("загрузка модели…", "loading model…"),
    ("нет распознанного текста", "no recognized text yet"),
    ("модель не скачана", "model not downloaded"),
    ("поиск сброшен", "search cleared"),
    // --- сетка, чипы, порядок --------------------------------------------------
    ("Плитки", "Tiles"),
    ("размер плиток, 100–260 px", "tile size, 100–260 px"),
    ("Сортировка", "Sort"),
    ("Дата: новые сверху", "Date: newest first"),
    ("Дата: старые сверху", "Date: oldest first"),
    ("Имя", "Name"),
    ("Размер: крупные сверху", "Size: largest first"),
    ("Порядок альбома", "Album order"),
    ("сейчас:", "now:"),
    ("порядок задаёт поиск", "the search sets the order"),
    ("порядок плиток: дата, имя или размер", "tile order: date, name or size"),
    ("Подряд", "Flat"),
    ("По месяцам", "By months"),
    ("Без даты", "No date"),
    ("путь/дата:", "path/date:"),
    ("тег:", "tag:"),
    ("по релевантности:", "by relevance:"),
    ("фильтров нет", "no filters"),
    ("Снять выделение", "Clear selection"),
    ("Выбрать все", "Select all"),
    ("выделение в этой сетке снимется; Esc делает то же", "clears the selection in this grid; Esc does the same"),
    ("отметить всё, что видно сейчас; Ctrl+A делает то же", "marks everything visible now; Ctrl+A does the same"),
    ("выбрано", "selected"),
    (
        "Добавьте каталог кнопкой «Каталоги» или введите путь вверху, затем нажмите «Индексировать».",
        "Add a folder with the “Folders” button or type a path above, then press “Index”.",
    ),
    ("ничего не найдено", "nothing found"),
    ("сверка копий и слепок sha256", "comparing copies and a sha256 manifest"),
    ("Выгрузка", "Export"),
    (
        "поиск по тексту, распознанному на снимках",
        "search by text recognized on photos",
    ),
    (
        "смысловой поиск по содержимому снимков",
        "semantic search over photo content",
    ),
    ("справка, квитанция, 2024", "receipt, invoice, 2024"),
    (
        "торт, море, мебель, документ",
        "cake, sea, furniture, document",
    ),
    ("Пора проверить архив", "Time to check the archive"),
    ("Проверить", "Check now"),
    ("Напомнить позже", "Remind me later"),
    (
        "прочитать архив и сверить с сохранённым слепком",
        "read the archive and compare it with the saved manifest",
    ),
    (
        "баннер скроется до следующего запуска",
        "hide the banner until the next launch",
    ),
    (
        "напоминание отложено до следующего запуска",
        "reminder postponed until the next launch",
    ),
    // --- диалог «Настройки» ----------------------------------------------------
    ("Модель ИИ", "AI model"),
    ("Индексация и дубли", "Indexing & duplicates"),
    ("Теги", "Tags"),
    ("Хранилище", "Storage"),
    ("Внешние программы", "External tools"),
    ("Интерфейс", "Interface"),
    (
        "Изменения сохраняются сразу; оригиналы не меняются.",
        "Changes are saved immediately; originals are never modified.",
    ),
    ("Закрыть", "Close"),
    ("Язык интерфейса", "Interface language"),
    ("Язык", "Language"),
    (
        "Выбор сохраняется сразу. Английский — основной язык проекта, русский остаётся \
         вторым; экраны, ещё не переведённые, показываются по-русски.",
        "The choice is saved immediately. English is the project's main language, Russian \
         stays second; screens not translated yet fall back to Russian.",
    ),
    ("настройки сохранены", "settings saved"),
    ("настройки не сохранены", "settings not saved"),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn английский_перевод_находится_а_русский_остаётся() {
        assert_eq!(t(Lang::En, "Настройки"), "Settings");
        assert_eq!(t(Lang::Ru, "Настройки"), "Настройки");
        assert_eq!(t(Lang::Ru, "строка без перевода"), "строка без перевода");
        assert_eq!(t(Lang::En, "строка без перевода"), "строка без перевода");
    }

    #[test]
    fn код_языка_переживает_разбор_и_неизвестное_даёт_основной() {
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
    fn локаль_системы_даёт_язык_или_молчит() {
        assert_eq!(Lang::from_tag("ru_RU.UTF-8"), Some(Lang::Ru));
        assert_eq!(Lang::from_tag("ru-RU"), Some(Lang::Ru));
        assert_eq!(Lang::from_tag("en_US.utf8"), Some(Lang::En));
        assert_eq!(Lang::from_tag("de:ru"), Some(Lang::Ru));
        assert_eq!(Lang::from_tag("C"), None);
        assert_eq!(Lang::from_tag("POSIX"), None);
        assert_eq!(Lang::from_tag("de_DE"), None);
        assert_eq!(Lang::from_tag(""), None);
        // Неизвестная локаль не превращается в панику: всегда есть основной язык.
        assert_eq!(Lang::default(), Lang::En);
    }

    #[test]
    fn счётчик_ставит_правильное_число_на_обоих_языках() {
        let ru = ["файл", "файла", "файлов"];
        assert_eq!(count(Lang::Ru, 1, ru, "file", "files"), "1 файл");
        assert_eq!(count(Lang::Ru, 2, ru, "file", "files"), "2 файла");
        assert_eq!(count(Lang::Ru, 5, ru, "file", "files"), "5 файлов");
        assert_eq!(count(Lang::Ru, 11, ru, "file", "files"), "11 файлов");
        assert_eq!(count(Lang::Ru, 21, ru, "file", "files"), "21 файл");
        assert_eq!(count(Lang::En, 1, ru, "file", "files"), "1 file");
        assert_eq!(count(Lang::En, 3, ru, "file", "files"), "3 files");
    }

    #[test]
    fn месяцы_на_двух_языках_и_без_паники() {
        assert_eq!(month_name(Lang::En, 5), "May");
        assert_eq!(month_name(Lang::Ru, 5), "май");
        assert_eq!(month_name(Lang::En, 12), "December");
        assert_eq!(month_name(Lang::Ru, 0), "январь");
        assert_eq!(month_name(Lang::Ru, 99), "декабрь");
    }

    #[test]
    fn таблица_без_пустых_и_повторных_ключей() {
        let mut seen = std::collections::HashSet::new();
        for (ru, en) in PAIRS {
            assert!(!ru.trim().is_empty(), "пустой ключ");
            assert!(!en.trim().is_empty(), "пустой перевод для {ru}");
            assert_ne!(ru, en, "перевод совпадает с оригиналом: {ru}");
            assert!(seen.insert(*ru), "ключ повторяется: {ru}");
        }
    }

    #[test]
    fn каждая_переведённая_строка_действительно_русская() {
        for (ru, _) in PAIRS {
            assert!(
                ru.chars().any(|c| c.is_alphabetic() && !c.is_ascii()),
                "ключ без кириллицы: {ru}"
            );
        }
    }
}
