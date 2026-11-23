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
