//! The language of the interface (M21, ADR 0040; ADR 0046): English is the main language,
//! Russian is the second one.
//!
//! The source text of the interface is written in English; `t()` returns the Russian translation
//! when Russian is chosen. The code therefore reads on its own, and a string that has no
//! translation yet is shown as written instead of disappearing.
//!
//! An L0 module: `std` only, no egui and no DB. The language is passed explicitly rather than kept
//! in a global variable, so a test that checks the text does not depend on its neighbours or on
//! the order they run in.

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

/// The Russian text of an English interface string; `None` — not translated yet.
pub fn russian(en: &'static str) -> Option<&'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| PAIRS.iter().copied().collect())
        .get(en)
        .copied()
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
    ("tag", ["tag", "tags"], ["тег", "тега", "тегов"]),
    ("album", ["album", "albums"], ["альбом", "альбома", "альбомов"]),
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

/// «found 4 of 10» — the same phrase in both languages, built in one place.
pub fn found(lang: Lang, shown: usize, total: usize) -> String {
    match lang {
        Lang::En => format!("found {shown} of {total}"),
        Lang::Ru => format!("найдено {shown} из {total}"),
    }
}

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

/// The `--help` text in both languages: the pair lives here so the menu cannot drift away from
/// the table and the English variant does not exist apart from the Russian one (M21.6).
/// `.0` is the English source, `.1` the Russian text.
pub const USAGE: (&str, &str) = (
    r#"photocat — media archive cataloguer

subcommands:
  index <folder> [--jobs N] [--db PATH]    index a folder
  stats [--db PATH]                        index statistics
  dupes [--max-dist N] [--csv FILE] [--plan FILE]
                                            find duplicates and similar shots, export the decisions
  organize --out DIR [--by day|month|event] [--name TEMPLATE] [--exclude-dup] [--apply]
                                            plan (and apply) the showcase: by day, month or
                                            event; names from {date} {time} {camera} {tag}
  verify <a> <b> [--jobs N] [--all]        compare two copies: size → sha256, report
  verify --manifest FILE <folder>          check a folder against a sha256 manifest
  manifest [DIR] [--out FILE] [--jobs N]  snapshot a folder in sha256sum format
  embed [--jobs N] [--batch N] [--refresh] [--retag]  CLIP embeddings and tags (local model);
                                            --refresh recomputes vectors, --retag — tags only
  tags [--list] [--add LABEL --prompt TEXT] [--enable ID] [--disable ID] [--rm ID]
       [--export FILE] [--import FILE] [--reset]
                                            the model's tag dictionary: labels, prompts, order
  faces [--jobs N] [--threshold X] [--dry-run] [--recluster] [--list]
                                            people: detection, embeddings, groups
  faces download --yes | label --id N --name NAME | split --id N
                                            face weights, name a group, split a face out
  ocr [--jobs N] [--all] [--dry-run] [--file PATH]
                                            text from documents and screenshots (tesseract)
  search <query> [--top N] [--text]        semantic search or search in the text on photos
  clusters [--k N] [--out FILE]             CLIP clusters → report with examples
  eval --folder DIR --out DIR [--sample N]  labelled check: labels.csv and contact sheets
  eval --labels FILE                        metrics of a labelled sample (top-1/top-3)
  albums list [--tree] | new --name NAME [--parent ID] [--note TEXT]
         | rename --id N --name NAME | move --id N --parent ID [--position K]
         | add|remove --id N PATH... | delete --id N [--yes] | show --id N [--tree]
                                            albums: the tree and the file order; DB rows only
  export --album N --out DIR [--name TEMPLATE] [--apply]
                                            export an album as file copies; the structure and
                                            order follow the album, {n}_{orig}
  models [list|check|download --id ID --yes]
                                            the model catalogue, checking and downloading weights
  settings [--models-dir DIR] [--model ID] [--max-model-mb N] [--ocr-lang L] [--lang en|ru]
           [--face-threshold X] [--ffmpeg PATH] [--ffprobe PATH] [--tesseract PATH] [--curl PATH]
                                            show or change the settings
  doctor                                   what the machine has: programs, OCR languages, model
  gui                                       the graphical interface (default)

default: GUI"#,
    r#"photocat — каталогизатор медиаархива

подкоманды:
  index <каталог> [--jobs N] [--db PATH]   проиндексировать каталог
  stats [--db PATH]                        статистика по индексу
  dupes [--max-dist N] [--csv FILE] [--plan FILE]
                                            найти дубли и похожие снимки, выгрузить решения
  organize --out DIR [--by day|month|event] [--name ШАБЛОН] [--exclude-dup] [--apply]
                                            план (и применение) витрины: по дням, месяцам или
                                            событиям; имена из {date} {time} {camera} {tag}
  verify <a> <b> [--jobs N] [--all]        сверка двух копий: размер → sha256, отчёт
  verify --manifest FILE <каталог>         проверить каталог по слепку sha256
  manifest [DIR] [--out FILE] [--jobs N]  слепок каталога в формате sha256sum
  embed [--jobs N] [--batch N] [--refresh] [--retag]  CLIP-эмбеддинги и теги (модель локальная);
                                            --refresh пересчитывает векторы, --retag — только теги
  tags [--list] [--add МЕТКА --prompt ТЕКСТ] [--enable ID] [--disable ID] [--rm ID]
       [--export FILE] [--import FILE] [--reset]
                                            словарь тегов модели: метки, промпты, порядок
  faces [--jobs N] [--threshold X] [--dry-run] [--recluster] [--list]
                                            лица: детекция, эмбеддинги, группы
  faces download --yes | label --id N --name ИМЯ | split --id N
                                            веса лиц, подпись группы, разъединить лицо
  ocr [--jobs N] [--all] [--dry-run] [--file PATH]
                                            текст с документов и скриншотов (tesseract)
  search <запрос> [--top N] [--text]        смысловой поиск или поиск по тексту на фото
  clusters [--k N] [--out FILE]             кластеры CLIP → отчёт с примерами
  eval --folder DIR --out DIR [--sample N]  выборка для разметки: labels.csv и листы
  eval --labels FILE                        метрики по размеченной выборке (top-1/top-3)
  albums list [--tree] | new --name ИМЯ [--parent ID] [--note ТЕКСТ]
         | rename --id N --name ИМЯ | move --id N --parent ID [--position K]
         | add|remove --id N PATH... | delete --id N [--yes] | show --id N [--tree]
                                            альбомы: дерево и порядок файлов; только строки БД
  export --album N --out DIR [--name ШАБЛОН] [--apply]
                                            выгрузить альбом копиями файлов;
                                            структура и порядок как в альбоме, {n}_{orig}
  models [list|check|download --id ID --yes]
                                            каталог моделей, проверка и загрузка весов
  settings [--models-dir DIR] [--model ID] [--max-model-mb N] [--ocr-lang L] [--lang en|ru]
           [--face-threshold X] [--ffmpeg PATH] [--ffprobe PATH] [--tesseract PATH] [--curl PATH]
                                            показать или изменить настройки
  doctor                                   что есть на машине: программы, языки OCR, модель
  gui                                       графический интерфейс (по умолчанию)

по умолчанию: GUI"#,
);

/// Every translated string: the English source → the Russian text.
///
/// The key is exactly the string that stands in the code. The same English string is translated
/// once: `t("Settings")` in the navigation, the dialog title and a hint gives one word.
/// Filled screen by screen (see `plan/I18N.md`).
const PAIRS: &[(&str, &str)] = &[
    // --- CLI: the help text ---------------------------------------------------
    USAGE,
    // --- the window shell: navigation, the top bar, the status ----------------
    ("All files", "Все файлы"),
    ("Albums", "Альбомы"),
    ("Calendar", "Календарь"),
    ("People", "Лица"),
    ("Duplicates & series", "Дубли и серии"),
    ("Integrity", "Целостность"),
    ("Trash", "Корзина"),
    ("LIBRARY", "БИБЛИОТЕКА"),
    ("TOOLS", "ИНСТРУМЕНТЫ"),
    ("FILTER", "ФИЛЬТР"),
    ("family archive", "семейный архив"),
    ("Photos", "Фото"),
    ("Videos", "Видео"),
    ("All", "Все"),
    ("photos only", "только фото"),
    ("videos only", "только видео"),
    ("no date", "без даты"),
    ("Search by path or date", "Поиск по пути или дате"),
    ("2024-05, vacation", "2024-05, отпуск"),
    ("Folder", "Каталог"),
    (
        "path to a folder; several — with the “Folders” button",
        "путь к каталогу; несколько — кнопкой «Каталоги»",
    ),
    ("Start indexing", "Индексировать"),
    ("Indexing", "Индексация"),
    ("Refresh", "Обновить"),
    ("Settings", "Настройки"),
    ("Folders", "Каталоги"),
    (
        "add several folders, drop a folder from the index",
        "добавить несколько каталогов и убрать каталог из индекса",
    ),
    ("Hide details", "Скрыть сведения"),
    ("Details", "Сведения"),
    (
        "right panel with date, camera, tags and text",
        "правая панель с датой, камерой, тегами и текстом",
    ),
    ("Semantic", "Смысловой"),
    ("By text", "По тексту"),
    ("Search", "Найти"),
    ("Reset", "Сбросить"),
    ("loading model…", "загрузка модели…"),
    ("no recognized text yet", "нет распознанного текста"),
    ("model not downloaded", "модель не скачана"),
    ("search cleared", "поиск сброшен"),
    // --- the grid, chips, order ------------------------------------------------
    ("Tiles", "Плитки"),
    (
        "tile size (a tile: frame width, rows: row height), 100–260 px",
        "размер плитки (плитка — ширина кадра, строки — высота), 100–260 px",
    ),
    (
        "row height: the tile width follows the frame's proportion, 100–260 px",
        "высота строки: ширина плитки следует пропорции кадра, 100–260 px",
    ),
    (
        "list row size: height, thumbnail and caption size, 100–260 px",
        "размер строки списка: высота, миниатюра и кегль подписей, 100–260 px",
    ),
    ("Tile", "Плитка"),
    ("Rows", "Ряды"),
    ("List", "Список"),
    (
        "as in the mock-up: a 4:3 frame, the snapshot fitted whole",
        "как в макете: кадр 4:3, снимок вписан целиком",
    ),
    (
        "proportion rows: the tile width follows the frame",
        "строки по пропорциям: ширина плитки по кадру",
    ),
    ("a list: name, date and size in one row", "список: имя, дата и размер в строку"),
    ("Sort", "Сортировка"),
    ("Date: newest first", "Дата: новые сверху"),
    ("Date: oldest first", "Дата: старые сверху"),
    ("Name", "Имя"),
    ("Size: largest first", "Размер: крупные сверху"),
    ("Album order", "Порядок альбома"),
    ("now:", "сейчас:"),
    ("the search sets the order", "порядок задаёт поиск"),
    ("tile order: date, name or size", "порядок плиток: дата, имя или размер"),
    ("Flat", "Подряд"),
    ("By months", "По месяцам"),
    ("No date", "Без даты"),
    ("path/date:", "путь/дата:"),
    ("tag:", "тег:"),
    ("by relevance:", "по релевантности:"),
    ("no filters", "фильтров нет"),
    ("Clear selection", "Снять выделение"),
    ("Select all", "Выбрать все"),
    ("clears the selection in this grid; Esc does the same", "выделение в этой сетке снимется; Esc делает то же"),
    ("marks everything visible now; Ctrl+A does the same", "отметить всё, что видно сейчас; Ctrl+A делает то же"),
    ("picked", "выбрано"),
    (
        "Add a folder with the “Folders” button or type a path above, then press “Index”.",
        "Добавьте каталог кнопкой «Каталоги» или введите путь вверху, затем нажмите «Индексировать».",
    ),
    ("nothing found", "ничего не найдено"),
    ("comparing copies and a sha256 manifest", "сверка копий и слепок sha256"),
    ("Export", "Выгрузка"),
    (
        "search by text recognized on photos",
        "поиск по тексту, распознанному на снимках",
    ),
    (
        "semantic search over photo content",
        "смысловой поиск по содержимому снимков",
    ),
    ("receipt, invoice, 2024", "справка, квитанция, 2024"),
    (
        "cake, sea, furniture, document",
        "торт, море, мебель, документ",
    ),
    ("Time to check the archive", "Пора проверить архив"),
    ("Check now", "Проверить"),
    ("Remind me later", "Напомнить позже"),
    (
        "read the archive and compare it with the saved manifest",
        "прочитать архив и сверить с сохранённым слепком",
    ),
    (
        "hide the banner until the next launch",
        "баннер скроется до следующего запуска",
    ),
    (
        "reminder postponed until the next launch",
        "напоминание отложено до следующего запуска",
    ),
    // --- the file card and the inspector ---------------------------------------
    ("File details", "Сведения о файле"),
    ("Hide", "Скрыть"),
    ("free up room for the grid and calendar", "освободить место сетке и календарю"),
    (
        "Select a photo in the grid — its date, camera, tags and text will appear here.",
        "Выберите снимок в сетке — здесь появятся дата, камера, теги и текст.",
    ),
    ("▶ video", "▶ видео"),
    ("no preview", "нет превью"),
    ("selected", "в выделении"),
    ("Open full screen", "Открыть полный экран"),
    ("double-clicking a tile does the same", "двойной клик по плитке делает то же"),
    ("To trash", "В корзину"),
    (
        "move from the usual views to the app trash; the file on disk is unchanged",
        "убрать из обычных видов в корзину приложения; на диске файл не меняется",
    ),
    (
        "file moved to the app trash; untouched on disk",
        "файл в корзине приложения; на диске не тронут",
    ),
    ("file not moved to the trash", "файл не убран в корзину"),
    ("Date", "Дата"),
    ("Camera", "Камера"),
    ("Size", "Размер"),
    ("file name", "имя файла"),
    ("file time", "время файла"),
    ("source not set", "источник не указан"),
    (
        "remove the photo from this album; the file on disk is unchanged",
        "убрать снимок из этого альбома; файл на диске не меняется",
    ),
    ("photo removed from the album", "снимок убран из альбома"),
    ("cannot remove from the album", "из альбома не убрать"),
    ("AI tags", "Теги ИИ"),
    (
        "tag: not sure — the top weights are close",
        "тег: не уверен — первые веса близки",
    ),
    ("all tags", "все теги"),
    ("tag: not computed", "тег: не посчитан"),
    ("tag removed from this photo", "тег убран у этого фото"),
    ("tag not removed", "тег не убран"),
    ("restore hidden tags", "вернуть скрытые теги"),
    ("hidden tags restored", "скрытые теги возвращены"),
    ("tags not restored", "теги не вернуть"),
    ("My tags", "Свои теги"),
    ("none yet", "пока нет"),
    ("remove", "убрать"),
    ("own tag removed", "свой тег убран"),
    ("your own tag, e.g. furniture", "свой тег, например мебель"),
    ("Add tag", "Добавить тег"),
    ("own tag added", "свой тег добавлен"),
    ("tag not added", "тег не добавлен"),
    ("Text on the photo", "Текст на фото"),
    ("Recognize again", "Распознать заново"),
    ("Recognize text", "Распознать текст"),
    (
        "this photo only; the original is unchanged",
        "только для этого снимка; оригинал не меняется",
    ),
    (
        "button disabled: tesseract does not start",
        "кнопка погашена: tesseract не запускается",
    ),
    ("Show the full text", "Показать текст целиком"),
    (
        "full screen: the frame and all recognized text",
        "полный экран: кадр и весь распознанный текст",
    ),
    // --- duplicates and series -------------------------------------------------
    ("Find groups", "Найти группы"),
    ("Export the plan", "Экспорт плана"),
    (
        "write a CSV into the app cache: group, keep_path, drop_path, reason, \
         wasted_bytes; a list to review, not a deletion",
        "записать CSV в кэш приложения: group, keep_path, drop_path, reason, \
         wasted_bytes; это список для проверки, а не удаление",
    ),
    ("plan not written", "план не записан"),
    ("Copy the list", "Скопировать список"),
    ("the whole CSV plan to the clipboard", "весь план CSV в буфер обмена"),
    ("plan copied to the clipboard", "план скопирован в буфер обмена"),
    (
        "“keep” picks the best frame of the group: it stays, the rest are marked spare. \
         No file is deleted; the decision lives in the DB and lands in the plan.",
        "«оставить» выбирает лучший кадр группы: он остаётся, остальные помечаются лишними. \
         Файлы не удаляются; решение хранится в БД и попадает в план.",
    ),
    (
        "“Export the plan” writes a CSV into the app cache — a report to review, not a deletion.",
        "«Экспорт плана» пишет CSV в кэш приложения — это отчёт для проверки, не удаление.",
    ),
    ("Press “Find groups” on the left.", "Нажмите «Найти группы» слева."),
    ("decision", "решение"),
    ("advice", "совет"),
    ("decision not saved", "решение не сохранено"),
    ("spare", "лишний"),
    ("keep", "оставить"),
    // --- the calendar ----------------------------------------------------------
    ("Year", "Год"),
    ("Month", "Месяц"),
    ("Week", "Неделя"),
    ("Day", "День"),
    ("Today", "Сегодня"),
    ("previous year", "предыдущий год"),
    ("next year", "следующий год"),
    ("previous month with files", "предыдущий месяц с файлами"),
    ("next month with files", "следующий месяц с файлами"),
    ("previous week", "предыдущая неделя"),
    ("next week", "следующая неделя"),
    ("previous day", "предыдущий день"),
    ("next day", "следующий день"),
    ("no day selected", "день не выбран"),
    ("double-click to open the month", "двойной клик — открыть месяц"),
    ("double-click to open the week", "двойной клик — открыть неделю"),
    ("double-click to open the day", "двойной клик — открыть день"),
    ("No files in this month.", "В этом месяце файлов нет."),
    ("No files on this day.", "В этот день файлов нет."),
    ("Go to", "Перейти к"),
    (
        "Pick a day in the calendar — its photos appear below.",
        "Выберите день в календаре — ниже появятся снимки.",
    ),
    // --- people ----------------------------------------------------------------
    ("Merge threshold:", "Порог склейки:"),
    ("face model not downloaded", "модель лиц не скачана"),
    ("Face settings", "Настройки лиц"),
    ("reclustering…", "пересобираю группы…"),
    ("Stop", "Стоп"),
    ("people: stopping after the current photos…", "лица: останавливаю после текущих снимков…"),
    ("Resume", "Продолжить"),
    ("Find faces", "Найти лица"),
    (
        "a long pass: detection and embeddings run locally, no network; photos already \
         scanned are skipped by sha256",
        "долгий проход: детекция и эмбеддинги локально, без сети; уже просканированные \
         снимки пропускаются по sha256",
    ),
    ("Rebuild groups", "Пересобрать группы"),
    (
        "the threshold applies to faces; manual names and splits are kept",
        "порог применяется к лицам; ручные подписи и разъединения сохраняются",
    ),
    ("Groups", "Группы"),
    ("All people", "Все лица"),
    ("no name", "без имени"),
    ("mark the group for the “all together” search", "отметить группу для поиска «все вместе»"),
    ("Show the photos (together)", "Показать лица (вместе)"),
    ("Photos with everyone together", "Фото, где все вместе"),
    (
        "show only photos that have a face from every marked group",
        "показать только снимки, где есть лица из всех отмеченных групп",
    ),
    ("No photo has faces from all the marked groups.", "Нет снимков, где есть лица из всех отмеченных групп."),
    ("photos", "снимки"),
    ("Person's name:", "Имя человека:"),
    ("e.g. Lisa", "например, Лиза"),
    ("Save the name", "Сохранить имя"),
    (
        "A name pins the group: rebuilding by threshold will not shuffle it. A wrong merge is \
         split by the “not him” button on the tile.",
        "Подпись закрепляет группу: пересборка по порогу её не перемешает. Ложную склейку \
         разъединяет кнопка «не он» на плитке.",
    ),
    (
        "Pick a group on the left to name it, or split the tiles.",
        "Выберите группу слева, чтобы дать ей имя, или разъединяйте плитки.",
    ),
    ("People picked", "Выбрано лиц"),
    ("name for the picked people", "имя для выбранных"),
    ("Name the picked people", "Назначить имя выбранным"),
    (
        "the picked faces move into one group; an empty name creates an unnamed group",
        "отмеченные лица уедут в одну группу; пустое имя создаст группу без подписи",
    ),
    ("Unpick", "Снять выбор"),
    (
        "No faces yet: press “Find faces” on the left (the face model is required).",
        "Лиц пока нет: нажмите «Найти лица» слева (нужна модель лиц).",
    ),
    ("No faces in this group.", "В этой группе нет лиц."),
    ("no frame", "нет кадра"),
    ("not him", "не он"),
    ("split this face into a new group", "отделить это лицо в новую группу"),
    ("pick", "выбрать"),
    ("mark this face for a shared name", "отметить это лицо для общего имени"),
    ("no faces found", "лица не найдены"),
    ("groups not built", "группы не собраны"),
    ("group name removed", "подпись группы убрана"),
    ("name not saved", "подпись не сохранена"),
    ("face moved into a new group; give it a name", "лицо отделено в новую группу; дайте ей имя"),
    ("face not split off", "лицо не отделено"),
    ("faces not assigned", "лица не назначены"),
    ("people: rebuilding groups…", "лица: пересобираю группы…"),
    (
        "downloading the face model: weights only, no photos",
        "скачивание модели лиц: только веса, без фотографий",
    ),
    // --- the trash -------------------------------------------------------------
    (
        "Files in the app trash: untouched on disk. “Restore” brings them back to the archive, \
         “Empty the trash” moves them to the OS trash after a confirmation.",
        "Файлы в корзине приложения: на диске не тронуты. «Вернуть» — снова в архив, \
         «Очистить корзину» — перенос в системную корзину только после подтверждения.",
    ),
    ("The trash is empty.", "Корзина пуста."),
    ("Restore all", "Вернуть всё"),
    ("Empty the trash", "Очистить корзину"),
    (
        "moved to the OS trash; nothing is deleted for good",
        "перенос в системную корзину ОС; безвозвратного удаления нет",
    ),
    ("restored from the trash", "возвращено из корзины"),
    ("file restored from the trash", "файл возвращён из корзины"),
    ("file not restored", "файл не вернуть"),
    ("trash not emptied", "корзина не очищена"),
    ("in trash", "в корзине"),
    ("restore", "вернуть"),
    ("bring the file back into the usual views; on disk it was untouched", "вернуть файл в обычные виды; на диске он не менялся"),
    ("Empty the trash?", "Очистить корзину?"),
    (
        "Nothing is deleted for good inside the app: the files stay in the OS trash until you \
         empty it yourself. The originals will disappear from their folders in the archive — \
         the app cannot undo that.",
        "Внутри приложения безвозвратного удаления нет: файлы остаются в системной корзине, \
         пока вы сами её не очистите. Оригиналы в архиве исчезнут из своих каталогов — это \
         необратимо средствами приложения.",
    ),
    ("Move to the OS trash", "Переместить в системную корзину"),
    ("Cancel", "Отмена"),
    // --- integrity -------------------------------------------------------------
    ("comparing copies", "сверка копий"),
    ("writing the manifest", "запись слепка"),
    ("checking against the manifest", "проверка по слепку"),
    ("comparison stopped: not everything was checked", "сверка остановлена: проверено не всё"),
    ("the copies match", "копии совпадают"),
    ("manifest not written: the pass was stopped", "слепок не записан: проход остановлен"),
    ("manifest not written", "слепок не записан"),
    ("errors", "ошибок"),
    ("check stopped: not everything was checked", "проверка остановлена: проверено не всё"),
    ("the folder matches the manifest", "каталог совпадает со слепком"),
    (
        "The comparison only reads: sizes first, sha256 only for files of the same size. The \
         manifest is a sha256sum file for future checks; it is written by a button only. \
         Nothing is written into the archive folder.",
        "Сверка только читает: сначала размеры, sha256 — лишь у файлов одинакового размера. \
         Слепок — файл формата sha256sum для будущих проверок; он пишется только по кнопке. \
         В каталог архива не записывается ничего.",
    ),
    ("Folder A (archive)", "Каталог A (архив)"),
    ("Folder B (copy)", "Каталог B (копия)"),
    ("Manifest file", "Файл слепка"),
    ("folder A (archive)", "каталог A (архив)"),
    ("folder B (copy)", "каталог B (копия)"),
    ("manifest file", "файл слепка"),
    ("Show all paths", "Показать все пути"),
    ("Compare the copies", "Сверить копии"),
    ("compare folder A and folder B byte by byte", "сравнить каталог A и каталог B побайтово"),
    ("Write the manifest", "Записать слепок"),
    ("Overwrite the manifest", "Перезаписать слепок"),
    ("sha256 of every file in folder A into the manifest", "sha256 всех файлов каталога A в файл слепка"),
    ("Check against the manifest", "Проверить по слепку"),
    ("compare folder A with the saved manifest", "сравнить каталог A с сохранённым слепком"),
    (
        "the pass is running: progress and “Stop” are in the top bar",
        "проход идёт: прогресс и «Стоп» — в верхней панели",
    ),
    ("Remind me to check every", "Напоминать о проверке каждые"),
    (" days", " дн."),
    ("days", "дн."),
    ("days ago", "дн. назад"),
    ("0 — do not remind", "0 — не напоминать"),
    (
        "the term counts from the last complete pass over the archive",
        "срок считается от последнего полного прохода по архиву",
    ),
    ("reminder interval not saved", "интервал напоминания не сохранён"),
    ("no manifest set — the reminder stays quiet", "слепок не задан — напоминание молчит"),
    ("no pass has happened yet", "последнего прохода ещё не было"),
    ("matched", "совпало"),
    ("there were differences", "были расхождения"),
    (
        "the date of the last pass is unclear — we will remind on launch",
        "дата последнего прохода непонятна — напомним при запуске",
    ),
    ("reminder: no manifest set", "напоминание: слепок не задан"),
    ("reminder disabled (interval 0)", "напоминание выключено (интервал 0)"),
    ("next check in", "следующая проверка: через"),
    ("time to check: overdue", "пора проверить: срок вышел"),
    ("report of the last pass", "отчёт прошлого прохода"),
    ("Copy the last report", "Скопировать прошлый отчёт"),
    ("Copy the report", "Скопировать отчёт"),
    (
        "the same report the CLI prints; nothing was written into the archive",
        "тот же отчёт, что печатает CLI; в архив ничего не записано",
    ),
    ("manifest overwrite cancelled", "перезапись слепка отменена"),
    ("report copied to the clipboard", "отчёт скопирован в буфер обмена"),
    (
        "the last pass report was copied to the clipboard",
        "отчёт прошлого прохода скопирован в буфер обмена",
    ),
    ("please give", "укажите"),
    ("is not a folder", "не каталог"),
    (
        "folder A and folder B are the same path; nothing to compare",
        "каталог A и каталог B — один и тот же путь; сверять нечего",
    ),
    (
        "give the manifest file: the path it will be written to",
        "укажите файл слепка: это путь, по которому он будет записан",
    ),
    ("that is a folder", "это каталог"),
    ("the folder for the manifest does not exist", "каталог для слепка не существует"),
    ("give the manifest file to check against", "укажите файл слепка, по которому проверять"),
    ("manifest file not found", "файл слепка не найден"),
    (
        "a pass is already running: wait for it or press “Stop”",
        "проход уже идёт: дождитесь конца или нажмите «Стоп»",
    ),
    (
        "manifest not written: the overwrite needs a confirmation",
        "слепок не записан: нужно подтверждение перезаписи",
    ),
    ("reminder settings not saved", "настройки напоминания не сохранены"),
    ("running in the background", "идёт в фоне"),
    ("the outcome was not saved in the settings", "итог не сохранён в настройках"),
    ("report not written to", "отчёт не записан в"),
    ("error", "ошибка"),
    // --- the settings dialog ---------------------------------------------------
    ("AI model", "Модель ИИ"),
    ("Indexing & duplicates", "Индексация и дубли"),
    ("Tags", "Теги"),
    ("Storage", "Хранилище"),
    ("External tools", "Внешние программы"),
    ("Interface", "Интерфейс"),
    (
        "Changes are saved immediately; originals are never modified.",
        "Изменения сохраняются сразу; оригиналы не меняются.",
    ),
    ("Close", "Закрыть"),
    ("Interface language", "Язык интерфейса"),
    ("Language", "Язык"),
    (
        "The choice is saved immediately. English is the project's main language, Russian \
         stays second; screens not translated yet fall back to Russian.",
        "Выбор сохраняется сразу. Английский — основной язык проекта, русский остаётся \
         вторым; экраны, ещё не переведённые, показываются по-русски.",
    ),
    ("settings saved", "настройки сохранены"),
    ("Interface theme", "Тема интерфейса"),
    ("Dark", "Тёмная"),
    ("Light", "Светлая"),
    ("Follow the system", "Как в системе"),
    (
        "Dark is the main one: snapshots are looked at in the evening. Light is the same \
         grid on white, for daytime work. The theme changes neither files nor the database.",
        "Тёмная — основная: снимки смотрят вечерами. Светлая — та же сетка на белом, \
         для дневной работы. Тема не меняет ни файлов, ни базы.",
    ),
    ("settings not saved", "настройки не сохранены"),

    // --- the left panel: model, tags, text, summary (M21.4f) -------------------
    ("Compute tags", "Посчитать теги"),
    ("Model settings", "Настройки модели"),
    ("with a confident tag:", "с уверенным тегом:"),
    ("tag", "тег"),
    ("mine", "свой"),
    ("Filter threshold", "Порог фильтра"),
    (
        "no tag passes the threshold — lower the slider",
        "ни один тег не проходит порог — опустите ползунок",
    ),
    ("search", "поиск"),
    (
        "tesseract does not start — set its path in “Settings”",
        "tesseract не запускается — путь в «Настройках»",
    ),
    (
        "OCR: stopping after the current files…",
        "OCR: останавливаю после текущих файлов…",
    ),
    ("Recognize documents", "Распознать документы"),
    (
        "a long pass: documents and screenshots without a camera and with a confident tag; \
         a single photo — with the button in the file card",
        "долгий проход: документы и скриншоты без камеры и по уверенному тегу; \
         один снимок — кнопкой в карточке файла",
    ),
    ("Summary", "Сводка"),
    ("Files:", "Файлов:"),
    ("Size:", "Объём:"),
    ("No date:", "Без даты:"),
    ("No preview:", "Без превью:"),
    ("Videos without a cover:", "Видео без обложки:"),
    (
        "press “Index” — ffmpeg will create the covers",
        "нажмите «Индексировать» — обложки создаст ffmpeg",
    ),
    (
        "tags are not computed yet — press “Compute tags”",
        "теги ещё не посчитаны — нажмите «Посчитать теги»",
    ),
    (
        "100% — the strongest photo of this tag, 0% — all; 70% — the top 30%",
        "100% — самый сильный снимок этого тега, 0% — все; 70% — верхние 30%",
    ),

    // --- settings: model (M21.4f) ----------------------------------------------
    ("Model", "Модель"),
    ("dims", "призн."),
    ("Model state", "Состояние модели"),
    ("Downloaded · verified", "Скачана · проверена"),
    ("Not downloaded", "Не скачана"),
    ("Check the model", "Проверить модель"),
    ("the model is not ready", "модель не готова"),
    ("Model folder", "Папка моделей"),
    ("where to download the weights", "путь, куда скачивать веса"),
    ("Apply the path", "Применить путь"),
    ("the model path is empty", "путь к моделям пуст"),
    ("Download limit", "Лимит на скачивание"),
    (
        "A model over the limit is not downloaded. If the chosen one is unavailable, \
         the downloader looks for a compatible one within the limit.",
        "Модель сверх лимита не скачивается. Если выбранная недоступна, \
         загрузчик ищет совместимую в пределах лимита.",
    ),
    ("in place", "на месте"),
    ("not downloaded", "не скачана"),
    ("over the limit", "сверх лимита"),
    ("Network consent", "Согласие на сеть"),
    (
        "I allow the model weights to be downloaded (photos, paths and queries are not sent)",
        "разрешаю скачать веса модели (фото, пути и запросы не отправляются)",
    ),
    (
        "The network is used only when “Download the model” is pressed. \
         No user data is sent.",
        "Сеть используется только при нажатии «Скачать модель». \
         Пользовательские данные не отправляются.",
    ),
    ("Download the model", "Скачать модель"),
    (
        "the button is disabled: network consent is required",
        "кнопка погашена: нужно согласие на сеть",
    ),
    ("files:", "файлов:"),
    (
        "Tags and vectors belong to the model: after switching models they must be computed again.",
        "Теги и векторы привязаны к модели: после смены модели их нужно посчитать.",
    ),

    // --- settings: indexing and duplicates (M21.4f) ----------------------------
    ("Indexing threads", "Потоков на индексацию"),
    ("threads", "потоков"),
    ("Duplicate threshold", "Порог дублей"),
    ("bits out of 64", "бит из 64"),
    (
        "More bits — more similar photos in a group. Exact copies \
         are found at any threshold.",
        "Больше бит — больше похожих снимков в группе. Точные копии \
         находятся при любом пороге.",
    ),

    // --- settings: tags and the dictionary (M21.4f) ----------------------------
    ("Tag filter threshold", "Порог фильтра по тегу"),
    ("tags are not computed yet", "теги ещё не посчитаны"),
    ("Confident tag threshold (cosine)", "Порог уверенного тега (косинус)"),
    (
        "This is about the card, OCR and the {tag} in the showcase, not the filter: a tag is \
         highlighted only when it is above the threshold and clearly ahead of the second one.",
        "Это про карточку, OCR и {tag} в витрине, а не про фильтр: тег подсвечивается, \
         только если он выше порога и отрывается от второго.",
    ),
    ("Margin over the second tag", "Отрыв от второго тега"),
    (
        "A lower threshold — more tags and more mistakes. “Remove” on a tag in the card \
         hides it for one photo and survives a recompute.",
        "Ниже порог — больше тегов и больше ошибок. «Убрать» у тега в карточке \
         скрывает его у одного фото и переживает пересчёт.",
    ),
    ("Tag dictionary", "Словарь тегов"),
    (
        "The label and the prompts live in the DB per model. Editing prompts does not re-read \
         photos: the vectors are already stored, only the weights are recomputed.",
        "Метка и промпты живут в БД на каждую модель. Правка промптов не перечитывает \
         фото: векторы уже сохранены, пересчитываются только веса.",
    ),
    (
        "the dictionary is not seeded yet — it appears after the first “Compute tags”",
        "словарь ещё не засеян — он появится после первого «Посчитать теги»",
    ),
    (
        "a disabled tag is not computed and not shown in the filter",
        "выключенный тег не считается и не показывается в фильтре",
    ),
    (
        "the label in Russian: it is shown in the filter and in the card",
        "метка по-русски: она видна в фильтре и в карточке",
    ),
    (
        "the minimum percentile for showing this tag",
        "минимальный процентиль показа этого тега",
    ),
    ("Remove from the dictionary", "Убрать из словаря"),
    ("prompts: one per line; 2–3 general templates", "промпты: один на строку; 2–3 общих шаблона"),
    ("a new label, e.g. “Wardrobe”", "новая метка, например «Шкаф»"),
    ("prompt: “a photo of a wardrobe”", "промпт: «фотография шкафа»"),
    ("Add to the dictionary", "Добавить в словарь"),
    (
        "then “Recompute tags”: the weights are recomputed from the stored vectors",
        "затем «Пересчитать теги»: веса пересчитаются по сохранённым векторам",
    ),
    ("Recompute tags", "Пересчитать теги"),
    (
        "recompute the tag weights from the vectors already stored, without running CLIP over the photos again",
        "пересчитать веса тегов по уже сохранённым векторам, без повторного CLIP по фото",
    ),
    ("Export JSON", "Экспорт JSON"),
    (
        "the dictionary without paths or photos — it can be moved to another machine",
        "словарь без путей и фото — можно перенести на другую машину",
    ),
    ("Import JSON", "Импорт JSON"),
    (
        "replace the model dictionary with a file; the old weights disappear after a recompute",
        "заменить словарь модели файлом; старые веса исчезнут после пересчёта",
    ),
    ("Restore the defaults", "Вернуть стандартные"),
    (
        "the 14 general tags from the code; your edits will be replaced",
        "14 общих тегов из кода; ваши правки будут заменены",
    ),
    (
        "the JSON file for export/import; empty — <cache>/photocat/tags.json",
        "файл JSON для экспорта/импорта; пусто — <кэш>/photocat/tags.json",
    ),
    (
        "the dictionary has changed — press “Recompute tags”",
        "словарь изменён — нажмите «Пересчитать теги»",
    ),
    ("dictionary not saved", "словарь не сохранён"),
    (
        "the tag is removed from the dictionary — its weights disappear after “Recompute tags”",
        "тег убран из словаря — его веса исчезнут после «Пересчитать теги»",
    ),
    (
        "the dictionary is back to the default — press “Recompute tags”",
        "словарь возвращён к стандартному — нажмите «Пересчитать теги»",
    ),
    ("dictionary not reset", "словарь не сброшен"),
    ("dictionary not exported", "словарь не выгружен"),
    ("cannot read", "не прочитать"),
    ("dictionary not loaded", "словарь не загружен"),
    (
        "CLIP: recomputing tags from the stored vectors…",
        "CLIP: пересчёт тегов по сохранённым векторам…",
    ),

    // --- settings: faces (M21.4f) ----------------------------------------------
    ("Face merge threshold (cosine)", "Порог склейки лиц (косинус)"),
    (
        "A higher threshold — fewer false merges, but one person may split into groups. \
         The SCRFD and ArcFace models are ONNX, local.",
        "Выше порог — меньше ложных склеек, но один человек может разбиться \
         на группы. Модели SCRFD и ArcFace — ONNX, локально.",
    ),
    (
        "I allow the face weights to be downloaded (photos are not sent)",
        "разрешаю скачать веса лиц (фото не отправляются)",
    ),
    ("Download the face model", "Скачать модель лиц"),
    ("Check the face model", "Проверить модель лиц"),
    ("the face model is checked: sha256 matched", "модель лиц проверена: sha256 совпал"),
    ("the face model is not ready", "модель лиц не готова"),
    ("face files:", "файлов лиц:"),

    // --- settings: storage (M21.4f) --------------------------------------------
    ("Index", "Индекс"),
    ("DB", "БД"),
    ("Cache", "Кэш"),
    ("Models", "Модели"),
    ("Compute the used space", "Посчитать занятое место"),
    ("Database:", "База данных:"),
    ("Previews:", "Превью:"),
    ("Face crops:", "Кропы лиц:"),
    ("Models:", "Модели:"),
    ("Whole cache:", "Весь кэш:"),
    ("Cleaning the cache and the index", "Очистка кэша и индекса"),
    ("Clear the previews…", "Очистить превью…"),
    (
        "only <cache>/photocat/thumbs; the archive is untouched",
        "только <кэш>/photocat/thumbs; архив не трогается",
    ),
    ("Clear the face crops…", "Очистить кропы лиц…"),
    (
        "the crops and the face rows; after that “Find faces” runs again",
        "кропы и строки лиц; после этого «Найти лица» пройдёт заново",
    ),
    ("Reset the index…", "Сбросить индекс…"),
    (
        "DB rows: files, tags, OCR, vectors, faces; the originals are intact",
        "строки БД: файлы, теги, OCR, векторы, лица; оригиналы целы",
    ),
    (
        "photocat never writes into the archive folder: only DB rows and previews in the cache. \
         The date showcase is a separate command with an explicit confirmation.",
        "В каталог архива photocat не пишет: только строки в БД и превью в кэше. \
         Витрина по датам — отдельная команда с явным подтверждением.",
    ),

    // --- settings: external programs (M21.4f) ----------------------------------
    (
        "Empty — the program is looked up in PATH. A path is checked by running it when saved; \
         without the program only its part is disabled.",
        "Пусто — программа ищется в PATH. Путь проверяется запуском при сохранении; \
         без программы отключается только её работа.",
    ),
    ("empty — from PATH", "пусто — из PATH"),
    ("without it", "без неё"),
    ("Check and save the programs", "Проверить и сохранить программы"),
    ("does not work", "не работает"),
    ("programs not saved", "программы не сохранены"),
    ("programs: all taken from PATH", "программы: все берутся из PATH"),
    ("programs saved and checked", "программы сохранены и проверены"),
    ("set in the settings", "задано в настройках"),
    ("from PATH", "из PATH"),
    ("video: cover frame, dHash, previews", "видео: кадр-обложка, dHash, превью"),
    ("video: duration and resolution", "видео: длительность и разрешение"),
    ("OCR: text from documents and screenshots", "OCR: текст с документов и скриншотов"),
    ("downloading model weights on consent", "скачивание весов модели по согласию"),
    (
        "videos in the index without previews or dHash, an error in the counter; photos are unaffected",
        "видео в индексе без превью и dHash, ошибка в счётчике; фото не страдают",
    ),
    ("videos without duration or resolution", "видео без длительности и разрешения"),
    (
        "OCR is off: the button is disabled, text search stays empty",
        "OCR выключен: кнопка погашена, поиск по тексту пуст",
    ),
    (
        "the model weights will have to be placed by hand (scripts/fetch-model.sh)",
        "веса модели придётся положить вручную (scripts/fetch-model.sh)",
    ),

    // --- the full screen, the player, the folder chooser, the cache (M21.5) -----
    ("previous file — ←", "предыдущий файл — ←"),
    ("next file — →", "следующий файл — →"),
    ("ffplay in its own window", "ffplay отдельным окном"),
    ("pause and resume — space", "пауза и продолжение — пробел"),
    ("play the video — space", "смотреть видео — пробел"),
    ("slideshow — space", "слайд-шоу — пробел"),
    ("no device", "нет устройства"),
    ("Stop the video", "Стоп видео"),
    ("stop and go back to the poster frame", "остановить и вернуться к кадру"),
    ("Stop the slideshow", "Стоп слайд-шоу"),
    ("Slideshow", "Слайд-шоу"),
    (
        "space — start/stop; walks the current list",
        "пробел — пуск/стоп; листает текущий список",
    ),
    ("interval:", "интервал:"),
    ("zoom out", "уменьшить"),
    ("zoom in", "увеличить"),
    ("Fit", "Вписать"),
    ("zoom", "масштаб"),
    ("drag to move the frame", "перетаскивание — сдвиг кадра"),
    ("In the system viewer", "В системном просмотрщике"),
    ("video · press ▶ to play", "видео · нажмите ▶, чтобы смотреть"),
    ("loading the next frame…", "загрузка следующего кадра…"),
    ("the full frame is loading…", "полный кадр загружается…"),
    (
        "No text recognized. The “Recognize text” button is in the file card.",
        "Текст не распознан. Кнопка «Распознать текст» — в карточке файла.",
    ),
    ("file not found in the index — viewer closed", "файл не найден в индексе — просмотр закрыт"),
    ("opening in the system player", "открываю системным проигрывателем"),
    ("cannot open", "не открыть"),
    ("cannot open the video", "видео не открыть"),
    ("the frame was not decoded", "кадр не декодирован"),
    ("video", "видео"),
    (" (with sound)", " (со звуком)"),
    (" (without sound)", " (без звука)"),
    ("the built-in player could not open it", "встроенный плеер не взялся"),
    ("opening ffplay", "открываю ffplay"),
    ("ffplay did not start", "ffplay не запустился"),
    ("the system player", "системный проигрыватель"),
    ("ffplay: the video is closed", "ffplay: видео закрыто"),
    (
        "the decoder cannot keep up with the sound: sound is off, the video runs slower",
        "декодер не успевает за звуком: звук выключен, видео идёт медленнее",
    ),
    ("the video stopped", "видео остановилось"),
    ("the video has finished", "видео доиграло"),
    // Archive folders and the folder picker.
    ("Archive folders", "Каталоги архива"),
    (
        "You can index several folders. “Add a folder…” offers indexing straight away; \
         “Remove from the index” deletes DB rows only — the files on disk are untouched.",
        "Индексировать можно несколько каталогов. «Добавить каталог…» сразу предлагает \
         индексацию; «Убрать из индекса» удаляет только строки БД — файлы на диске целы.",
    ),
    ("No folders yet — add one with the button below.", "Каталогов пока нет — добавьте кнопкой ниже."),
    ("Remove from the index", "Убрать из индекса"),
    ("files on disk are not deleted", "файлы на диске не удаляются"),
    ("Add a folder…", "Добавить каталог…"),
    ("Add a folder", "Добавить каталог"),
    ("Index all", "Индексировать все"),
    ("Up", "Вверх"),
    ("Add and index", "Добавить и индексировать"),
    ("Add without indexing", "Добавить без индексации"),
    ("No subfolders.", "Подкаталогов нет."),
    ("folder added", "каталог добавлен"),
    ("folder removed from the index", "каталог убран из индекса"),
    ("the folder was not removed from the index", "каталог не убран из индекса"),
    ("rows deleted", "удалено строк"),
    ("the files on disk are intact", "файлы на диске целы"),
    // Cache and index cleanup: the confirmation.
    ("Clear the previews?", "Очистить превью?"),
    (
        "Only the files in <cache>/photocat/thumbs are deleted. The grid shows “no preview” \
         until the folder is indexed again. The originals are unchanged.",
        "Будут удалены только файлы <кэш>/photocat/thumbs. Сетка покажет «нет превью», \
         пока каталог не проиндексируют заново. Оригиналы не меняются.",
    ),
    ("Clear the previews", "Очистить превью"),
    ("Clear the face crops?", "Очистить кропы лиц?"),
    (
        "The crops and the face rows in the DB are deleted; the groups disappear. \
         “Find faces” creates them again. The photos and originals are unchanged.",
        "Будут удалены кропы и строки лиц в БД; группы исчезнут. «Найти лица» заново \
         создаст их. Фотографии и оригиналы не меняются.",
    ),
    ("Clear the faces", "Очистить лица"),
    ("Reset the index?", "Сбросить индекс?"),
    (
        "The DB rows are deleted: files, tags, text, vectors and faces. The settings, the models \
         and every file on disk stay. Indexing is needed again afterwards.",
        "Будут удалены строки БД: файлы, теги, текст, векторы и лица. Настройки, модели \
         и все файлы на диске остаются. После этого нужна индексация заново.",
    ),
    ("Reset the index", "Сбросить индекс"),
    ("index reset; index a folder now", "индекс сброшен; проиндексируйте каталог"),
    ("cache cleared", "кэш очищен"),
    ("cleaning failed", "очистка не удалась"),
    // --- CLI: reports and command messages (M21.6) -----------------------------
    ("stopped: ", "остановлено: "),
    ("indexed {}/{} files ({}), unchanged {}, errors {}, {} s", "проиндексировано {}/{} файлов ({}), без изменений {}, ошибок {}, за {} с"),
    ("indexing: {}/{} files, {} errors", "индексация: {}/{} файлов, ошибок {}"),
    ("a folder is needed: photocat index <folder>", "нужен каталог: photocat index <каталог>"),
    ("kind", "тип"),
    ("files", "файлов"),
    ("size", "объём"),
    ("total: {} files, {}", "всего: {} файлов, {}"),
    ("without a capture date: {}", "без даты съёмки: {}"),
    ("without a perceptual hash: {}", "без перцептивного хеша: {}"),
    ("unique sha256: {}  (exact copies: {})", "уникальных sha256: {}  (точных копий: {})"),
    ("unique dhash: {}", "уникальных dhash: {}"),
    ("videos with a duration: {}, {} in total", "видео с длительностью: {}, суммарно {}"),
    ("files with recognised text: {}, characters: {}", "файлов с распознанным текстом: {}, знаков: {}"),
    ("exact copy", "точная копия"),
    ("similar shot", "похожий снимок"),
    ("exact copies — no difference", "точные копии — разницы нет"),
    ("the differences are small — choose by eye", "отличия небольшие — выбирайте глазами"),
    ("sharper than the rest by {}%", "резче остальных на {}%"),
    ("sharper than the rest", "резче остальных"),
    ("more pixels", "больше по разрешению"),
    ("larger file", "крупнее файл"),
    ("groups: {}, files in them: {}, potentially spare: {}", "групп: {}, файлов в них: {}, потенциально лишнего: {}"),
    ("marked spare: {}, would free {}", "помечено лишними: {}, освободится {}"),
    ("#{} [{}] files: {}, duplicates worth {} · advice: {} — {}", "#{} [{}] файлов: {}, дублей на {} · совет: {} — {}"),
    ("… and {} more groups (full list: --csv)", "… и ещё {} групп (полный список: --csv)"),
    ("full list: {}", "полный список: {}"),
    ("decision plan: {}", "план решения: {}"),
    ("no text candidates: every photo has a camera and no document tag", "кандидатов на текст нет: все снимки с камерой и без тега документа"),
    ("recognised {} of {} files, {} characters, {} errors, {} s", "распознано {} из {} файлов, знаков {}, ошибок {}, за {} с"),
    (" ({} skipped, text already there)", " (пропущено {}, текст уже есть)"),
    ("candidates: {} of {} photos; tesseract ({}): {}", "кандидатов: {} из {} снимков; tesseract ({}): {}"),
    ("not found", "не найден"),
    ("OCR: {}/{} files, {} errors — {}", "OCR: {}/{} файлов, ошибок {} — {}"),
    ("scanned {} of {} photos, {} faces, {} clusters, {} errors, {} s", "просканировано {} из {} снимков, лиц {}, кластеров {}, ошибок {}, за {} с"),
    (" ({} skipped, already scanned)", " (пропущено {}, уже просканировано)"),
    ("{} faces, {} groups, {} pinned by hand, {} s", "лиц {}, групп {}, из них закреплено вручную {}, за {} с"),
    ("face model downloaded: {} files, {}", "модель лиц скачана: файлов {}, {}"),
    ("a group number and a name are needed: faces label --id N --name NAME", "нужны номер группы и имя: faces label --id N --name ИМЯ"),
    ("group {}: “{}”", "группа {}: «{}»"),
    ("a face number is needed: faces split --id N", "нужен номер лица: faces split --id N"),
    ("face {} split into group {}", "лицо {} отделено в группу {}"),
    ("not downloaded — “Settings” or faces download --yes", "не скачана — «Настройки» или faces download --yes"),
    ("photos: {}, still without faces: {}; face model: {}", "снимков: {}, ещё без лиц: {}; модель лиц: {}"),
    ("no face groups yet: run photocat faces", "групп лиц нет: запустите photocat faces"),
    ("(no name)", "(без имени)"),
    ("{} faces, {} photos", "лиц {}, фото {}"),
    ("people: {}/{} photos, {} errors — {}", "лица: {}/{} снимков, ошибок {} — {}"),
    ("CLIP ({}): {}computed {}/{}, skipped {}, errors {}, {} s, model {}", "CLIP ({}): {}посчитано {}/{}, пропущено {}, ошибок {}, за {} с, модель {}"),
    ("tags ({}): recomputed {}/{}, {} s", "теги ({}): пересчитано {}/{}, за {} с"),
    ("model {}: {} files, {}", "модель {}: файлов {}, {}"),
    (" (did not fit: {})", " (не подошли: {})"),
    ("loading the CLIP model…", "загрузка модели CLIP…"),
    ("CLIP: {}/{} files, {} errors — {}", "CLIP: {}/{} файлов, ошибок {} — {}"),
    ("clusters: {} over {} photos, {} s → {}", "кластеров: {} на {} фото, за {} с → {}"),
    // --- M15.7: the labelled tag check -----------------------------------------
    ("no labels yet: fill in {} and run again", "разметки нет: заполните {} и повторите"),
    ("rows in the labels: {} · labelled {} · “other” {} · skipped {} · not labelled {} · missing from the index {}", "строк в разметке: {} · размечено {} · «другое» {} · пропущено {} · не размечено {} · нет в индексе {}"),
    ("a dictionary label fits {} % of the shots; {} of them allow a second label", "словарь покрывает {} % снимков; у {} допустима не одна метка"),
    ("top-1: {} of {} = {} % · top-3: {} of {} = {} %", "top-1: {} из {} = {} % · top-3: {} из {} = {} %"),
    ("confident tag: {} shots, accuracy {} %; confidently wrong (“other”) {}", "уверенный тег: {} снимков, точность {} %; уверенно мимо («другое») {}"),
    ("furniture: at {} % found {} of {} (threshold {})", "мебель: при {} % найдено {} из {} (порог {})"),
    ("check {} · model {}", "проверка {} · модель {}"),
    ("a folder is needed: photocat eval --folder DIR --out DIR", "нужен каталог: photocat eval --folder DIR --out DIR"),
    ("no shots with tags in {}: index the folder and compute the tags", "в {} нет снимков с тегами: проиндексируйте каталог и посчитайте теги"),
    ("sample: {} shots of {} eligible in {} · labels: {} · guesses: {} · sheets: {}", "выборка: {} снимков из {} годных в {} · метки: {} · подсказки: {} · листов: {}"),
    ("fill the label column in {} (other / skip / labels joined by |) and run again with --labels", "заполните столбец label в {} (other / skip / метки через |) и повторите с --labels"),
    ("model {} · {}", "модель {} · {}"),
    ("# CLIP clusters (M3)\n\n", "# Кластеры CLIP (M3)\n\n"),
    ("Model `{}`, files with embeddings: {}, clusters: {}, k={}.", "Модель `{}`, файлов с эмбеддингами: {}, кластеров: {}, k={}."),
    ("A report for the owner: 12 examples per cluster.\n", "Отчёт для просмотра владельцем: 12 примеров на кластер.\n"),
    ("## Cluster {} — {} photos", "## Кластер {} — {} фото"),
    ("… and {} more", "… и ещё {}"),
    ("comparing", "сверка"),
    ("checking", "проверка"),
    ("manifest", "слепок"),
    ("{}/{} files, {} errors — {}", "{}/{} файлов, ошибок {} — {}"),
    ("comparison: {}", "сверка: {}"),
    ("     with: {}", "     с: {}"),
    ("files: A {}, B {}; identical content {}; sha256 checked {} files ({})", "файлов: A {}, B {}; совпало по содержимому {}; sha256 проверено {} файлов ({})"),
    ("size differs: {}", "размер отличается: {}"),
    ("size differs", "размер отличается"),
    ("content differs: {}", "содержимое отличается: {}"),
    ("content differs", "содержимое отличается"),
    ("only in A: {}", "только в A: {}"),
    ("only in A", "только в A"),
    ("only in B: {}", "только в B: {}"),
    ("only in B", "только в B"),
    ("scan errors: {}", "ошибок обхода: {}"),
    ("read errors: {}", "ошибок чтения: {}"),
    ("read errors", "ошибки чтения"),
    ("not checked because of the stop: {}", "не проверено из-за отмены: {}"),
    ("skipped (symlinks/special): A {}, B {}", "пропущено (симлинки/особые): A {}, B {}"),
    ("skipped (symlinks/special): {}", "пропущено (симлинки/особые): {}"),
    ("verdict: {} ({} s)", "вердикт: {} (за {} с)"),
    ("differences found", "найдены расхождения"),
    ("the folder does not match the manifest", "каталог расходится со слепком"),
    ("manifest check: {}", "проверка слепка: {}"),
    ("            folder: {}", "           каталог: {}"),
    ("matched: {}, changed: {}, missing from the folder: {}, missing from the manifest: {}", "совпало: {}, изменено: {}, нет в каталоге: {}, нет в слепке: {}"),
    ("missing from the folder", "нет в каталоге"),
    ("missing from the manifest", "нет в слепке"),
    ("changed", "изменено"),
    ("  … {} more (show all)", "  … ещё {} (показать все)"),
    ("manifest {}: {} files ({}), skipped {}, errors {}, not checked {}, {} s", "слепок {}: {} файлов ({}), пропущено {}, ошибок {}, не проверено {}, за {} с"),
    ("cancelled: ", "отменено: "),
    ("not written: ", "не записан: "),
    ("a folder is needed: photocat verify --manifest FILE DIR", "нужен каталог: photocat verify --manifest FILE DIR"),
    ("two folders are needed: photocat verify A B [--jobs N] [--all]", "нужны два каталога: photocat verify A B [--jobs N] [--all]"),
    ("by day", "по дням"),
    ("by month", "по месяцам"),
    ("by event (day + place)", "по событиям (день + место)"),
    ("showcase: {} → {}", "витрина: {} → {}"),
    ("files with a date: {}, without a date (not in the plan): {}, marked spare and left out: {}", "файлов с датой: {}, без даты (не попадут в план): {}, помечено лишними и не включено: {}"),
    ("showcase folders: {}", "каталогов витрины: {}"),
    ("examples:", "примеры:"),
    ("this is a plan (dry-run). Add --apply to create links/copies in {}", "это план (dry-run). Добавьте --apply, чтобы создать ссылки/копии в {}"),
    ("created: {}, errors: {}", "создано: {}, ошибок: {}"),
    ("a destination folder is needed: organize --out DIR [--apply] [--by day|month|event]", "нужен каталог назначения: organize --out DIR [--apply] [--by day|month|event]"),
    ("--by: expected day, month or event, got “{}”", "--by: ожидается day, month или event, получено «{}»"),
    ("album export: {} → {}", "выгрузка альбома: {} → {}"),
    ("folders {} · files {} · {}; in the trash and not exported: {}; names with a collision: {}; paths too long: {}", "каталогов {} · файлов {} · {}; в корзине и не выгружаются: {}; имён с коллизией: {}; слишком длинных путей: {}"),
    ("; empty folders: {}", "; пустых каталогов: {}"),
    ("; those files are skipped", "; такие файлы пропущены"),
    ("this is a plan (dry-run). Add --apply to copy the files (not links).", "это план (dry-run). Добавьте --apply, чтобы скопировать файлы (не ссылки)."),
    ("export: {}/{} files, {} errors", "выгрузка: {}/{} файлов, ошибок {}"),
    ("exported {} files · {}; {} already exported skipped ({} stale order in the target); {} renamed because of a collision; {} folders created; {} errors", "выгружено {} файлов · {}; пропущено уже выгруженных {} (порядок в цели устарел {}); переименовано из-за коллизии {}; каталогов создано {}; ошибок {}"),
    ("; stopped", "; остановлено"),
    ("an album number is needed: export --album N --out DIR [--name TEMPLATE] [--apply]", "нужен номер альбома: export --album N --out DIR [--name ШАБЛОН] [--apply]"),
    ("a destination folder is needed: export --album N --out DIR [--apply]", "нужен каталог назначения: export --album N --out DIR [--apply]"),
    ("model folder: {}", "папка моделей: {}"),
    ("limit: {} MiB", "лимит: {} МиБ"),
    ("the model is in order: {}", "модель в порядке: {}"),
    ("unknown models subcommand: {} (models list|check|download)", "неизвестная подкоманда models: {} (models list|check|download)"),
    ("no such model: {} (see models list)", "нет такой модели: {} (см. models list)"),
    ("no such interface language: {} (en, ru)", "нет такого языка интерфейса: {} (en, ru)"),
    ("not saved: {}", "не сохранён: {}"),
    ("model: {}", "модель: {}"),
    ("threads: {}", "потоков: {}"),
    ("duplicate threshold: {}", "порог дублей: {}"),
    ("tag: confident ≥ {}, margin {}", "тег: уверенный ≥ {}, отрыв {}"),
    ("tag filter: threshold {}% (percentile)", "фильтр по тегу: порог {}% (процентиль)"),
    ("OCR languages: {}", "языки OCR: {}"),
    ("interface language: {}", "язык интерфейса: {}"),
    ("face threshold: {}", "порог лиц: {}"),
    ("external programs (empty in the settings — looked up in PATH; without a program only its own work is off):", "внешние программы (пусто в настройках — ищутся в PATH; без программы отключается только её работа):"),
    ("does not work: {}", "не работает: {}"),
    ("not found: {} ({})", "не найдена: {} ({})"),
    ("why: {}; without it: {}", "зачем: {}; без неё: {}"),
    ("OCR languages: {} — present", "языки OCR: {} — на месте"),
    ("OCR languages: {} — missing {}; install the tesseract language packs for them", "языки OCR: {} — нет {}; установите языковые пакеты tesseract для них"),
    ("  available: {}", "  доступны: {}"),
    ("OCR languages: cannot check — tesseract not found", "языки OCR: не проверить — tesseract не найден"),
    ("CLIP model: {} · {} · {}", "модель CLIP: {} · {} · {}"),
    ("not downloaded — semantic search and tags are unavailable", "не скачана — смысловой поиск и теги недоступны"),
    ("face model: {} · {} · {}", "модель лиц: {} · {} · {}"),
    ("people: {} on {} photos, {} groups, {} of {} still without faces", "лица: {} на {} снимках, групп {}, ещё без лиц {} из {}"),
    ("DB: {} · {} · {} files, {} with text ({} characters)", "БД: {} · {} · файлов {}, с текстом {} (знаков {})"),
    ("unknown subcommand: {}", "неизвестная подкоманда: {}"),
    ("no albums yet — create one: photocat albums new --name \"Summer 2024\"", "альбомов нет — создайте: photocat albums new --name \"Дача 2024\""),
    (" · {} in the trash", " · в корзине {}"),
    ("{} files · {} subalbums · {}{}", "файлов {} · подальбомов {} · {}{}"),
    ("a name is needed: photocat albums new --name \"Summer 2024\" [--parent ID]", "нужно имя: photocat albums new --name \"Дача 2024\" [--parent ID]"),
    ("album created: {} (id {})", "альбом создан: {} (id {})"),
    ("a name is needed: photocat albums rename --id N --name NAME", "нужно имя: photocat albums rename --id N --name ИМЯ"),
    ("album renamed: {}", "альбом переименован: {}"),
    ("a parent is needed: photocat albums move --id N --parent ID [--position K]", "нужен родитель: photocat albums move --id N --parent ID [--position K]"),
    ("album moved: {}", "альбом перемещён: {}"),
    ("paths are needed: photocat albums {} --id N PATH...", "нужны пути: photocat albums {} --id N PATH..."),
    ("album “{}”: {} added, {} already there, {} files", "альбом «{}»: добавлено {}, уже было {}, файлов {}"),
    ("album “{}”: {} removed, {} left", "альбом «{}»: убрано {}, осталось {}"),
    ("will be deleted: {} — {} albums, {} file rows; the files stay on disk. Repeat with --yes.", "будет удалено: {} — альбомов {}, строк файлов {}; файлы на диске останутся. Повторите с --yes."),
    ("deleted {} albums, {} file rows; the files on disk are untouched", "удалено альбомов {}, строк файлов {}; файлы на диске не тронуты"),
    ("  subalbum {} · {} files · {}", "  подальбом {} · файлов {} · {}"),
    ("subalbum {} · {} files · {}", "подальбом {} · файлов {} · {}"),
    ("  [in the app trash]", "  [в корзине приложения]"),
    ("unknown albums command: {} (list [--tree] | new | rename | move | add | remove | delete | show)", "неизвестная команда albums: {} (list [--tree] | new | rename | move | add | remove | delete | show)"),
    ("an album number is needed: --id N", "нужен номер альбома: --id N"),
    ("model dictionary {} — {} tags", "словарь модели {} — {} тегов"),
    ("ok", "ок"),
    ("off", "выкл"),
    (", threshold {}%", ", порог {}%"),
    ("{} files · {} prompts", "файлов {} · промптов {}"),
    ("--add: an empty label is not allowed", "--add: нужна непустая метка"),
    ("the tag “{}” is already in the dictionary", "тег «{}» уже есть в словаре"),
    ("--add: at least one --prompt “a photo of …” is needed", "--add: нужен хотя бы один --prompt «фотография …»"),
    ("tag “{}” added (id {}) — recompute the weights: photocat embed --retag", "тег «{}» добавлен (id {}) — пересчитайте веса: photocat embed --retag"),
    ("tag “{}” {}", "тег «{}» {}"),
    ("enabled", "включён"),
    ("disabled", "выключен"),
    ("tag “{}” removed from the dictionary — the weights go away after photocat embed --retag", "тег «{}» убран из словаря — веса исчезнут после photocat embed --retag"),
    ("dictionary exported: {} ({} tags)", "словарь выгружен: {} ({} тегов)"),
    ("dictionary loaded: {} tags — recompute the weights: photocat embed --retag", "словарь загружен: {} тегов — пересчитайте веса: photocat embed --retag"),
    ("dictionary restored to the default ({} tags) — photocat embed --retag", "словарь возвращён к стандартному ({} тегов) — photocat embed --retag"),
    ("a query is needed: photocat search \"cake\"", "нужен запрос: photocat search \"торт\""),
    ("text on photos, query “{}”: {} files in {} ms", "текст на фото, запрос «{}»: файлов {} за {} мс"),
    ("query “{}”: {} results in {} ms", "запрос «{}»: результатов {} за {} мс"),
    ("repeat {}: {} ms", "повтор {}: {} мс"),
    ("on average: {} ms", "в среднем: {} мс"),
    // --- the titles and notes of the model catalogue (M21.6, M15.6) -----------
    ("CLIP ViT-B/32, multilingual", "CLIP ViT-B/32, мультиязычная"),
    ("CLIP ViT-B/32, English", "CLIP ViT-B/32, английская"),
    ("SigLIP base 224, English", "SigLIP base 224, английская"),
    ("SigLIP 2 base 224, multilingual", "SigLIP 2 base 224, мультиязычная"),
    ("Russian search and tags, the best quality", "русский поиск и теги, лучшее качество"),
    ("smaller, English queries only", "меньше по размеру, запросы только по-английски"),
    ("768 features, SigLIP architecture; English queries", "768 признаков, архитектура SigLIP; запросы по-английски"),
    ("768 features, 100+ languages; needs a 400 MiB limit", "768 признаков, тексты на 100+ языках; нужен лимит 400 МиБ"),
];

#[cfg(test)]
mod tests {
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

    #[test]
    fn the_table_has_no_empty_or_duplicate_keys() {
        let mut seen = std::collections::HashSet::new();
        for (en, ru) in PAIRS {
            assert!(!en.trim().is_empty(), "an empty key");
            assert!(!ru.trim().is_empty(), "an empty translation for {en}");
            assert_ne!(en, ru, "the translation repeats the source: {en}");
            assert!(seen.insert(*en), "the key repeats: {en}");
        }
    }

    #[test]
    fn every_translated_string_is_really_russian() {
        for (en, ru) in PAIRS {
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
        for (en, ru) in PAIRS {
            let en_n = en.matches("{}").count();
            let ru_n = ru.matches("{}").count();
            assert_eq!(en_n, ru_n, "a different number of `{{}}`: «{en}» → «{ru}»");
        }
    }

    /// Every English string the code passes to `t()` or `fill()` is in the table: a forgotten pair
    /// would otherwise leave the Russian interface with an English label. Modules that still pass
    /// Russian source strings are pending the M23 refactor and are skipped here.
    #[test]
    fn every_report_string_is_in_the_table() {
        let sources = [
            ("main.rs", include_str!("main.rs")),
            ("db.rs", include_str!("db.rs")),
            ("index.rs", include_str!("index.rs")),
            ("dupes.rs", include_str!("dupes.rs")),
            ("organize.rs", include_str!("organize.rs")),
            ("verify.rs", include_str!("verify.rs")),
            ("embed.rs", include_str!("embed.rs")),
            ("ocr.rs", include_str!("ocr.rs")),
            ("faces.rs", include_str!("faces.rs")),
            ("tools.rs", include_str!("tools.rs")),
        ];
        let mut missing = Vec::new();
        for (name, src) in sources {
            for key in literal_keys(src) {
                // An English key must be in the table; a Russian one waits for its conversion.
                // A template of placeholders and single letters (A/B columns) is not a phrase.
                if !key.chars().any(|c| c.is_ascii_lowercase()) {
                    continue;
                }
                if !PAIRS.iter().any(|(en, _)| *en == key) {
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
}
