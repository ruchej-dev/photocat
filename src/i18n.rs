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

/// Короткое название дня недели (0 — понедельник): календарь берёт его здесь.
/// Неделя начинается с понедельника и на обоих языках.
pub fn weekday(lang: Lang, day: usize) -> &'static str {
    let i = day.min(6);
    match lang {
        Lang::En => EN_WEEKDAYS[i],
        Lang::Ru => RU_WEEKDAYS[i],
    }
}

const RU_WEEKDAYS: [&str; 7] = ["Пн", "Вт", "Ср", "Чт", "Пт", "Сб", "Вс"];
const EN_WEEKDAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

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
    // --- карточка файла и инспектор --------------------------------------------
    ("Сведения о файле", "File details"),
    ("Скрыть", "Hide"),
    ("освободить место сетке и календарю", "free up room for the grid and calendar"),
    (
        "Выберите снимок в сетке — здесь появятся дата, камера, теги и текст.",
        "Select a photo in the grid — its date, camera, tags and text will appear here.",
    ),
    ("▶ видео", "▶ video"),
    ("нет превью", "no preview"),
    ("в выделении", "selected"),
    ("Открыть полный экран", "Open full screen"),
    ("двойной клик по плитке делает то же", "double-clicking a tile does the same"),
    ("В корзину", "To trash"),
    (
        "убрать из обычных видов в корзину приложения; на диске файл не меняется",
        "move from the usual views to the app trash; the file on disk is unchanged",
    ),
    (
        "файл в корзине приложения; на диске не тронут",
        "file moved to the app trash; untouched on disk",
    ),
    ("файл не убран в корзину", "file not moved to the trash"),
    ("Дата", "Date"),
    ("Камера", "Camera"),
    ("Размер", "Size"),
    ("имя файла", "file name"),
    ("время файла", "file time"),
    ("источник не указан", "source not set"),
    (
        "убрать снимок из этого альбома; файл на диске не меняется",
        "remove the photo from this album; the file on disk is unchanged",
    ),
    ("снимок убран из альбома", "photo removed from the album"),
    ("из альбома не убрать", "cannot remove from the album"),
    ("Теги ИИ", "AI tags"),
    (
        "тег: не уверен — первые веса близки",
        "tag: not sure — the top weights are close",
    ),
    ("все теги", "all tags"),
    ("тег: не посчитан", "tag: not computed"),
    ("тег убран у этого фото", "tag removed from this photo"),
    ("тег не убран", "tag not removed"),
    ("вернуть скрытые теги", "restore hidden tags"),
    ("скрытые теги возвращены", "hidden tags restored"),
    ("теги не вернуть", "tags not restored"),
    ("Свои теги", "My tags"),
    ("пока нет", "none yet"),
    ("убрать", "remove"),
    ("свой тег убран", "own tag removed"),
    ("свой тег, например мебель", "your own tag, e.g. furniture"),
    ("Добавить тег", "Add tag"),
    ("свой тег добавлен", "own tag added"),
    ("тег не добавлен", "tag not added"),
    ("Текст на фото", "Text on the photo"),
    ("Распознать заново", "Recognize again"),
    ("Распознать текст", "Recognize text"),
    (
        "только для этого снимка; оригинал не меняется",
        "this photo only; the original is unchanged",
    ),
    (
        "кнопка погашена: tesseract не запускается",
        "button disabled: tesseract does not start",
    ),
    ("Показать текст целиком", "Show the full text"),
    (
        "полный экран: кадр и весь распознанный текст",
        "full screen: the frame and all recognized text",
    ),
    // --- дубли и серии ----------------------------------------------------------
    ("Найти группы", "Find groups"),
    ("Экспорт плана", "Export the plan"),
    (
        "записать CSV в кэш приложения: group, keep_path, drop_path, reason, \
         wasted_bytes; это список для проверки, а не удаление",
        "write a CSV into the app cache: group, keep_path, drop_path, reason, \
         wasted_bytes; a list to review, not a deletion",
    ),
    ("план не записан", "plan not written"),
    ("Скопировать список", "Copy the list"),
    ("весь план CSV в буфер обмена", "the whole CSV plan to the clipboard"),
    ("план скопирован в буфер обмена", "plan copied to the clipboard"),
    (
        "«оставить» выбирает лучший кадр группы: он остаётся, остальные помечаются лишними. \
         Файлы не удаляются; решение хранится в БД и попадает в план.",
        "“keep” picks the best frame of the group: it stays, the rest are marked spare. \
         No file is deleted; the decision lives in the DB and lands in the plan.",
    ),
    (
        "«Экспорт плана» пишет CSV в кэш приложения — это отчёт для проверки, не удаление.",
        "“Export the plan” writes a CSV into the app cache — a report to review, not a deletion.",
    ),
    ("Нажмите «Найти группы» слева.", "Press “Find groups” on the left."),
    ("решение", "decision"),
    ("совет", "advice"),
    ("решение не сохранено", "decision not saved"),
    ("лишний", "spare"),
    ("оставить", "keep"),
    // --- календарь --------------------------------------------------------------
    ("Год", "Year"),
    ("Месяц", "Month"),
    ("Неделя", "Week"),
    ("День", "Day"),
    ("Сегодня", "Today"),
    ("предыдущий год", "previous year"),
    ("следующий год", "next year"),
    ("предыдущий месяц с файлами", "previous month with files"),
    ("следующий месяц с файлами", "next month with files"),
    ("предыдущая неделя", "previous week"),
    ("следующая неделя", "next week"),
    ("предыдущий день", "previous day"),
    ("следующий день", "next day"),
    ("день не выбран", "no day selected"),
    ("двойной клик — открыть месяц", "double-click to open the month"),
    ("двойной клик — открыть неделю", "double-click to open the week"),
    ("двойной клик — открыть день", "double-click to open the day"),
    ("В этом месяце файлов нет.", "No files in this month."),
    ("В этот день файлов нет.", "No files on this day."),
    ("Перейти к", "Go to"),
    (
        "Выберите день в календаре — ниже появятся снимки.",
        "Pick a day in the calendar — its photos appear below.",
    ),
    // --- лица --------------------------------------------------------------------
    ("Порог склейки:", "Merge threshold:"),
    ("модель лиц не скачана", "face model not downloaded"),
    ("Настройки лиц", "Face settings"),
    ("пересобираю группы…", "reclustering…"),
    ("Стоп", "Stop"),
    ("лица: останавливаю после текущих снимков…", "people: stopping after the current photos…"),
    ("Продолжить", "Resume"),
    ("Найти лица", "Find faces"),
    (
        "долгий проход: детекция и эмбеддинги локально, без сети; уже просканированные \
         снимки пропускаются по sha256",
        "a long pass: detection and embeddings run locally, no network; photos already \
         scanned are skipped by sha256",
    ),
    ("Пересобрать группы", "Rebuild groups"),
    (
        "порог применяется к лицам; ручные подписи и разъединения сохраняются",
        "the threshold applies to faces; manual names and splits are kept",
    ),
    ("Группы", "Groups"),
    ("Все лица", "All people"),
    ("без имени", "no name"),
    ("отметить группу для поиска «все вместе»", "mark the group for the “all together” search"),
    ("Показать лица (вместе)", "Show the photos (together)"),
    ("Фото, где все вместе", "Photos with everyone together"),
    (
        "показать только снимки, где есть лица из всех отмеченных групп",
        "show only photos that have a face from every marked group",
    ),
    ("Нет снимков, где есть лица из всех отмеченных групп.", "No photo has faces from all the marked groups."),
    ("снимки", "photos"),
    ("Имя человека:", "Person's name:"),
    ("например, Лиза", "e.g. Lisa"),
    ("Сохранить имя", "Save the name"),
    (
        "Подпись закрепляет группу: пересборка по порогу её не перемешает. Ложную склейку \
         разъединяет кнопка «не он» на плитке.",
        "A name pins the group: rebuilding by threshold will not shuffle it. A wrong merge is \
         split by the “not him” button on the tile.",
    ),
    (
        "Выберите группу слева, чтобы дать ей имя, или разъединяйте плитки.",
        "Pick a group on the left to name it, or split the tiles.",
    ),
    ("Выбрано лиц", "People picked"),
    ("имя для выбранных", "name for the picked people"),
    ("Назначить имя выбранным", "Name the picked people"),
    (
        "отмеченные лица уедут в одну группу; пустое имя создаст группу без подписи",
        "the picked faces move into one group; an empty name creates an unnamed group",
    ),
    ("Снять выбор", "Unpick"),
    (
        "Лиц пока нет: нажмите «Найти лица» слева (нужна модель лиц).",
        "No faces yet: press “Find faces” on the left (the face model is required).",
    ),
    ("В этой группе нет лиц.", "No faces in this group."),
    ("нет кадра", "no frame"),
    ("не он", "not him"),
    ("отделить это лицо в новую группу", "split this face into a new group"),
    ("выбрать", "pick"),
    ("отметить это лицо для общего имени", "mark this face for a shared name"),
    ("лица не найдены", "no faces found"),
    ("группы не собраны", "groups not built"),
    ("подпись группы убрана", "group name removed"),
    ("подпись не сохранена", "name not saved"),
    ("лицо отделено в новую группу; дайте ей имя", "face moved into a new group; give it a name"),
    ("лицо не отделено", "face not split off"),
    ("лица не назначены", "faces not assigned"),
    ("лица: пересобираю группы…", "people: rebuilding groups…"),
    (
        "скачивание модели лиц: только веса, без фотографий",
        "downloading the face model: weights only, no photos",
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
    fn дни_недели_на_двух_языках() {
        assert_eq!(weekday(Lang::En, 0), "Mon");
        assert_eq!(weekday(Lang::En, 6), "Sun");
        assert_eq!(weekday(Lang::Ru, 0), "Пн");
        assert_eq!(weekday(Lang::Ru, 6), "Вс");
        // Вне диапазона — воскресенье, а не паника.
        assert_eq!(weekday(Lang::Ru, 9), "Вс");
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
