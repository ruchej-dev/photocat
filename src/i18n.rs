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

/// The translation table as a lookup by the English string.
fn map() -> &'static HashMap<&'static str, &'static str> {
    static MAP: OnceLock<HashMap<&'static str, &'static str>> = OnceLock::new();
    MAP.get_or_init(|| PAIRS.iter().copied().collect())
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
  faces [--jobs N] [--threshold X] [--dry-run] [--recluster] [--list] [--quarantine]
                                            people: detection, embeddings, groups
  faces download --yes | label --id N --name NAME | split --id N
        | confirm --id N | reject --id N | confirm --all | reject --all
                                            face weights, name a group, split a face out,
                                            answer the quarantine
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
           [--face-threshold X] [--ffmpeg PATH] [--ffprobe PATH] [--tesseract PATH]
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
  faces [--jobs N] [--threshold X] [--dry-run] [--recluster] [--list] [--quarantine]
                                            лица: детекция, эмбеддинги, группы
  faces download --yes | label --id N --name ИМЯ | split --id N
        | confirm --id N | reject --id N | confirm --all | reject --all
                                            веса лиц, подпись группы, разъединить лицо,
                                            ответить на карантин
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
           [--face-threshold X] [--ffmpeg PATH] [--ffprobe PATH] [--tesseract PATH]
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
    ("YEARS", "ГОДЫ"),
    ("All years", "Все годы"),
    ("the years of the archive", "годы архива"),
    ("No files in this year.", "В этом году файлов нет."),
    ("No files in the archive.", "В архиве нет файлов."),
    ("double-click to open the months of the year", "двойной клик откроет месяцы года"),
    ("double-click to open the days of the week", "двойной клик откроет дни недели"),
    ("double-click to open the photos of the day", "двойной клик откроет снимки дня"),
    (
        "The field is literal: a case-insensitive part of the path or of the ISO date — “2024-05” finds May 2024, “vacation” finds a folder or a file with that word. A search by what is on the photo is in the top bar: “Semantic / By text”.",
        "Поле буквальное: часть пути или даты в ISO без учёта регистра — «2024-05» найдёт май 2024, «vacation» — папку или файл с этим словом. Поиск по тому, что на фотографии, — в верхней полосе: «Смысловой / По тексту».",
    ),
    (
        "EXIF carries the coordinates only — a place name is not in the file. A name needs a reverse geocoder: the network is forbidden by §1 of AGENTS.md, and an offline data set is a new dependency and an ADR (docs/DECISIONS.md). The button copies the coordinates.",
        "EXIF несёт только координаты — названия места в файле нет. Названию нужен обратный геокодер: сеть запрещена §1 AGENTS.md, а офлайн-набор данных — новая зависимость и ADR (`docs/DECISIONS.md`). Кнопка копирует координаты.",
    ),
    ("copy the coordinates", "копировать координаты"),
    ("coordinates copied", "координаты скопированы"),
    (
        "Walk the folders and read only the new and changed files — the path, the size and the modification time decide. The pass computes the hash, the date, the dimensions, the sharpness and a 320 px preview, and gives a video its cover through ffmpeg. It writes to the cache only — the database and the previews, never the archive — and it can be stopped and resumed.",
        "Обходит каталоги и читает только новые и изменившиеся файлы — решают путь, размер и время изменения. Проход считает хэш, дату, размеры, резкость и превью 320 px, а видео получает обложку через ffmpeg. Он пишет только в кэш — базу и превью, — никогда в архив, и его можно остановить и продолжить.",
    ),
    (
        "Re-read the database into the window: reads no file, indexes nothing and changes nothing. Use it after a change made by the CLI or in another window.",
        "Перечитывает базу в окно: не читает ни одного файла, ничего не индексирует и ничего не меняет. Нужен после изменения из CLI или из другого окна.",
    ),
    ("clear the year filter", "снять фильтр по годам"),
    ("year:", "год:"),
    ("older than {}", "раньше {}"),
    ("the year filter changed", "фильтр по годам изменился"),
    ("family archive", "семейный архив"),
    ("Photos", "Фото"),
    ("Videos", "Видео"),
    ("All", "Все"),
    ("photos only", "только фото"),
    ("videos only", "только видео"),
    ("no date", "без даты"),
    ("Search by path or date", "Поиск по пути или дате"),
    ("2024-05, vacation", "2024-05, отпуск"),
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
        "The dot and the word name a running background pass: Indexing, CLIP, OCR, People, Trash, Export, Integrity or Compact; two or more of them show “N passes”. Their progress rows and the Stop button are in the top strip, under the search field. Nothing is sent anywhere.",
        "Точка и слово называют идущий фоновый проход: индексация, CLIP, OCR, лица, корзина, экспорт, проверка целостности или сжатие; два и больше показывают «N проходов». Их строки прогресса и «Стоп» — в верхней полосе, под полем поиска. Никуда ничего не отправляется.",
    ),
    ("{} passes", "{} проходов"),
    ("Ready", "Готов к работе"),
    ("Clear the search", "Очистить поиск"),
    (
        "right panel with date, camera, tags and text",
        "правая панель с датой, камерой, тегами и текстом",
    ),
    ("Semantic", "Смысловой"),
    ("By text", "По тексту"),
    ("Search", "Найти"),
    ("Reset", "Сбросить"),
    ("loading model…", "загрузка модели…"),
    (
        "no recognized text yet — run it in Settings → Text on photos",
        "нет распознанного текста — запустите его в «Настройки → Текст на фото»",
    ),
    ("Text on photos", "Текст на фото"),
    (
        "tesseract does not start — set its path above",
        "tesseract не запускается — укажите его путь выше",
    ),
    (
        "The pass walks over the documents and screenshots without a camera and over the photos with a confident “document” or “screenshot” tag; a single photo is recognized with the button in the file card. The text goes to the database and is searched by the top bar's “By text”.",
        "Проход обходит документы и скриншоты без камеры и снимки с уверенным тегом «документ» или «скриншот»; одиночный снимок распознаётся кнопкой в карточке файла. Текст ложится в базу, и его ищет «По тексту» в верхней полосе.",
    ),
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
        "as in the mock-up: a 4:3 frame, the snapshot fills it, the sides are cut",
        "как в макете: кадр 4:3, снимок заполняет его целиком, края обрезаны",
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
    // The album cards, the album order and the way into an album (M22.20).
    ("Cards", "Плитки"),
    ("the album cards or rows, and its photos as tiles or a list", "карточки или строки альбома и его снимки плиткой или списком"),
    ("By update", "По обновлению"),
    ("By name", "По названию"),
    ("By file count", "По числу файлов"),
    ("By size", "По размеру"),
    ("Filter the albums", "Фильтр по альбомам"),
    ("a part of the album name", "часть названия альбома"),
    ("no album with the name “{}”", "нет альбома с названием «{}»"),
    ("All albums", "Все альбомы"),
    ("the cards of the top level albums", "плитки альбомов верхнего уровня"),
    ("an album is data only: the files on disk do not change", "альбом — только данные: файлы на диске не меняются"),
    ("+ Add", "+ Добавить"),
    ("put this photo into an album; the file on disk is unchanged", "положить снимок в альбом; файл на диске не меняется"),
    ("put the marked photos into an album; the files on disk stay untouched", "положить отмеченные снимки в альбом; файлы на диске не меняются"),
    ("Add to an album… ({})", "В альбом… ({})"),
    ("Add to an album", "Положить в альбом"),
    ("Add {}", "Добавить {}"),
    ("The photos go to the end of the album. The files on disk stay untouched.", "Снимки встают в конец альбома. Файлы на диске не меняются."),
    ("There are no albums yet: type a name and the album is made with these photos.", "Альбомов пока нет: введите название — альбом будет создан вместе с этими снимками."),
    ("or a new album:", "или новый альбом:"),
    ("For printing, Summer 2024", "На печать, Лето 2024"),
    ("add the photos to the chosen album or to a new one; a repeat does not duplicate rows", "положить снимки в выбранный альбом или в новый; повтор не создаёт дублей"),
    ("pick an album or type a name for a new one", "выберите альбом или введите название нового"),
    ("{} added to “{}”, {} were there already", "добавлено {} в альбом «{}», уже было {}"),
    ("now:", "сейчас:"),
    ("the search sets the order", "порядок задаёт поиск"),
    ("tile order: date, name or size", "порядок плиток: дата, имя или размер"),
    ("Flat", "Подряд"),
    ("By months", "По месяцам"),
    ("Layout", "Вид сетки"),
    ("No date", "Без даты"),
    ("No preview", "Без превью"),
    ("only the files without a date", "только файлы без даты"),
    (
        "the no-date and no-preview filters changed",
        "фильтры «без даты» и «без превью» изменились",
    ),
    (
        "the preview could not be built or is no longer in the cache: HEIC/AVIF (the decoder cannot read them), a broken file, a video without a cover (no ffmpeg or an ffmpeg error), or previews cleared in Settings → Storage. Index the folder again: the pass rebuilds the previews, and “Index” runs ffmpeg to create the covers of the videos",
        "превью не построилось или его больше нет в кэше: HEIC/AVIF (декодер их не читает), битый файл, видео без обложки (нет ffmpeg или ошибка ffmpeg) или превью, очищенные в «Настройки → Хранилище». Проиндексируйте папку снова: проход вернёт превью, а «Индексировать» запустит ffmpeg и создаст обложки видео",
    ),
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
        "Add a folder with the “+” of the catalog block, then “Re-index” in its menu.",
        "Добавьте каталог кнопкой «+» блока каталога, затем нажмите «Переиндексировать» в его меню.",
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
    ("Shooting params", "Параметры съёмки"),
    ("Date and time", "Дата и время"),
    ("Resolution", "Разрешение"),
    ("Location", "Геолокация"),
    ("file name", "имя файла"),
    ("file time", "время файла"),
    ("source not set", "источник не указан"),
    (
        "remove the photo from this album; the file on disk is unchanged",
        "убрать снимок из этого альбома; файл на диске не меняется",
    ),
    (
        "open the album in the Albums view",
        "открыть альбом в виде «Альбомы»",
    ),
    (
        "add this tag to the filter",
        "добавить этот тег в фильтр",
    ),
    (
        "remove this tag from the filter",
        "убрать этот тег из фильтра",
    ),
    ("reset the tag filter", "сбросить фильтр по тегам"),
    (
        "no photos carry all the picked tags above their thresholds — lower the threshold slider in the left panel",
        "нет снимков со всеми выбранными тегами выше их порогов — опустите ползунок порога в левой панели",
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
    (
        "No groups yet: press “Find groups” above.",
        "Групп ещё нет: нажмите «Найти группы» выше.",
    ),
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
    ("12 months of {}", "12 месяцев {}"),
    ("{} in the year · {}", "{} за год · {}"),
    (
        "Pick a day in the calendar — its photos appear below.",
        "Выберите день в календаре — ниже появятся снимки.",
    ),
    // --- people ----------------------------------------------------------------
    ("Merge threshold:", "Порог склейки:"),
    ("face model not downloaded", "модель лиц не скачана"),
    (
        "SCRFD and ArcFace (InsightFace buffalo_l): non-commercial research use",
        "SCRFD и ArcFace (InsightFace buffalo_l): только некоммерческое исследовательское \
         использование",
    ),
    (
        "the face model download: stopping…",
        "скачивание модели лиц: останавливаю…",
    ),
    (
        "the face model download is stopped: the pass can be started again",
        "скачивание модели лиц остановлено: проход можно запустить снова",
    ),
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
        "A name pins the group: rebuilding by threshold will not shuffle it, and the closeness in \
         “Tuning” adds faces only through the quarantine. A wrong merge is split by the \
         crossed-head button on the tile and the view stays where it was; tick several strangers \
         and “Split the picked” takes them all out at once.",
        "Подпись закрепляет группу: пересборка по порогу её не перемешает, а близость в \
         «Настройке» добавляет лица только через карантин. Ложную склейку разъединяет кнопка с \
         перечёркнутой головой на плитке, и вид остаётся на месте; отметьте нескольких чужих и \
         «Отделить выбранные» уберёт их всех разом.",
    ),
    (
        "Pick a group in the list to name it, or split the tiles.",
        "Выберите группу в списке, чтобы дать ей имя, или разъединяйте плитки.",
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
        "No faces yet: press “Find faces” above (the face model is required).",
        "Лиц пока нет: нажмите «Найти лица» выше (нужна модель лиц).",
    ),
    ("No faces in this group.", "В этой группе нет лиц."),
    ("no frame", "нет кадра"),
    ("split this face into a new group; this screen stays here", "отделить это лицо в новую группу; этот экран остаётся здесь"),
    ("mark this face for a shared name", "отметить это лицо для общего имени"),
    ("no faces found", "лица не найдены"),
    ("groups not built", "группы не собраны"),
    ("group name removed", "подпись группы убрана"),
    ("name not saved", "подпись не сохранена"),
    (
        "the face is in a new group; the screen stays here",
        "лицо в новой группе; экран остаётся здесь",
    ),
    (
        "the face is in a new group; the group was left empty",
        "лицо в новой группе; группа осталась пустой",
    ),
    ("Split the picked", "Отделить выбранные"),
    (
        "each picked face leaves this group into its own new one; the rest stay",
        "каждое отмеченное лицо уходит из этой группы в свою новую; остальные остаются",
    ),
    (
        "{} faces are in new groups; {} not found",
        "{} лиц в новых группах; {} не найдено",
    ),
    (
        "{} faces left the group; the screen stays here",
        "{} лиц вышло из группы; экран остаётся здесь",
    ),
    (
        "{} faces left the group and it was left empty",
        "{} лиц вышло из группы, и она осталась пустой",
    ),
    ("face not split off", "лицо не отделено"),
    ("faces not split off", "лица не отделены"),
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
    (
        "Restore: back into the usual views — the file was untouched on disk",
        "Вернуть: снова в обычные виды — файл на диске не менялся",
    ),
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
    ("Settings sections", "Разделы настроек"),
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
    (
        "tags are not computed yet — press “Compute tags”",
        "теги ещё не посчитаны — нажмите «Посчитать теги»",
    ),
    (
        "100% — the strongest photo of this tag, 0% — all; 70% — the top 30%. The cloud below counts the photos that pass this cut, tag by tag",
        "100% — самый сильный снимок этого тега, 0% — все; 70% — верхние 30%. Облако ниже считает фотографии, которые проходят этот порог, по каждому тегу",
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

    // --- settings: faces (M21.4f, moved to the People screen in M26.5) ----------
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
    // The compaction of the database file (M26.8).
    ("Database file:", "Файл базы данных:"),
    ("{}, of which {} is free", "{}, из них свободно {}"),
    (
        "the database is not created yet",
        "база данных ещё не создана",
    ),
    ("Compact the database…", "Сжать базу данных…"),
    ("Compact the database", "Сжать базу данных"),
    ("Compact the database?", "Сжать базу данных?"),
    ("Compact", "Сжатие"),
    (
        "rebuild the file in place: the free pages that deleted rows left go back to the filesystem; the archive, the previews, the faces and the models are untouched. Free pages are left by “Remove from the index”, a reset of the index, tags and faces wiped after a file changed and deleted album rows",
        "перестроить файл на месте: свободные страницы, оставшиеся от удалённых строк, вернутся файловой системе; архив, превью, лица и модели не трогаются. Свободные страницы оставляют «Убрать из индекса», сброс индекса, стёртые после изменения файла теги и лица и удалённые строки альбомов",
    ),
    ("the database is being rebuilt", "база данных перестраивается"),
    (
        "nothing to reclaim — under 1 MiB or under 5 % of the file is free. Free pages are left by “Remove from the index”, a reset of the index, tags and faces wiped after a file changed and deleted album rows",
        "возвращать нечего — свободно меньше 1 МиБ или меньше 5 % файла. Свободные страницы оставляют «Убрать из индекса», сброс индекса, стёртые после изменения файла теги и лица и удалённые строки альбомов",
    ),
    (
        "The file is rebuilt in place: the free pages that deleted rows left go back to the filesystem. The archive, the previews, the faces and the models are untouched. The app does not read the database while it runs, the pass cannot be stopped once started, and it needs free disk space of about the file's size. It takes seconds for tens of megabytes.",
        "Файл перестраивается на месте: свободные страницы, оставшиеся от удалённых строк, возвращаются файловой системе. Архив, превью, лица и модели не трогаются. Пока идёт проход, приложение не читает базу; после начала проход нельзя остановить, и ему нужно свободное место примерно с сам файл. На десятках мегабайт это секунды.",
    ),
    (
        "rebuilding the database file…",
        "перестройка файла базы данных…",
    ),
    (
        "the database is being compacted…",
        "база данных сжимается…",
    ),
    (
        "database compacted: {} → {}",
        "база данных сжата: {} → {}",
    ),
    (
        "the database is not compacted: {}",
        "база данных не сжата: {}",
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
    ("Zoom out", "Уменьшить"),
    ("Zoom in", "Увеличить"),
    ("Rotate", "Повернуть"),
    ("Inspector", "Инспектор"),
    ("Back to the grid", "К сетке"),
    ("Series", "Серия"),
    ("face", "лицо"),
    ("photo {} of the day", "снимок {} за день"),
    ("close the full screen — Esc", "закрыть полный экран — Esc"),
    ("Show faces", "Показать лица"),
    (
        "the boxes of the found faces — a click on a face lists its photos",
        "рамки найденных лиц; клик по лицу покажет его снимки",
    ),
    ("no faces found in this file", "в этом файле лица не найдены"),
    (
        "all photos with this face",
        "все снимки с этим лицом",
    ),
    ("close the face panel", "закрыть панель лиц"),
    (
        "This face is not in a group yet.",
        "Это лицо ещё не в группе.",
    ),
    (
        "100 % fits the frame into the window",
        "100 % — кадр целиком в окне",
    ),
    (
        "above 100 % the frame is dragged with the mouse",
        "выше 100 % кадр сдвигается мышью",
    ),
    ("the whole frame in the window", "кадр целиком в окне"),
    ("one image pixel per screen pixel", "пиксель снимка на пиксель экрана"),
    (
        "turn the view by 90°; the file on disk is unchanged",
        "повернуть вид на 90°; файл на диске не меняется",
    ),
    ("the file card next to the frame — I", "карточка файла рядом с кадром — I"),
    ("Fit", "Вписать"),
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
    (
        "the built-in player could not open it: {}",
        "встроенный плеер не смог открыть: {}",
    ),
    (
        "The built-in player cannot open this video",
        "Встроенный плеер не может открыть это видео",
    ),
    ("Copy the reason", "Скопировать причину"),
    ("the reason is copied", "причина скопирована"),
    (
        "Play with ffplay in a separate window",
        "Играть через ffplay в отдельном окне",
    ),
    ("Open in the system player", "Открыть в системном плеере"),
    (
        "the video leaves the app — a separate window",
        "видео уходит из приложения — отдельное окно",
    ),
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
    // The catalog block of the left panel and its menu (M26.7).
    ("Working catalog", "Рабочий каталог"),
    ("All catalogs", "Все каталоги"),
    ("Storage catalogs", "Каталоги хранилища"),
    ("connect a catalog: the folder picker", "подключить каталог: выбор папки"),
    (
        "the working catalog: {} — a click opens the menu of the storage",
        "рабочий каталог: {} — клик откроет меню хранилища",
    ),
    (
        "the whole visible index — a click opens the menu of the storage",
        "весь видимый индекс — клик откроет меню хранилища",
    ),
    (
        "the whole visible index: {} — drop the scope",
        "весь видимый индекс: {} — снять ограничение",
    ),
    (
        " · this folder is not on disk: the rows stay until the index is cleaned",
        " · этой папки нет на диске: строки остаются, пока не очищен индекс",
    ),
    (
        " · a click shows this catalog alone; the gear opens its settings",
        " · клик покажет только этот каталог; шестерёнка откроет его настройки",
    ),
    ("catalog: {}", "каталог: {}"),
    ("the catalog changed", "каталог изменился"),
    (
        "catalog “{}”: {} — the DB is untouched",
        "каталог «{}»: {} — база не тронута",
    ),
    ("all catalogs: {} — the DB is untouched", "все каталоги: {} — база не тронута"),
    (
        "the selection is cleared: the catalog changed",
        "выделение снято: каталог изменился",
    ),
    (
        "switch this catalog on again: the rows are back, nothing is read from disk",
        "снова включить каталог: строки вернутся, с диска ничего не читается",
    ),
    (
        "catalog settings: its box, “Remove from the index” and “Index this catalog”",
        "настройки каталога: его флажок, «Убрать из индекса» и «Индексировать каталог»",
    ),
    ("Connect a catalog…", "Подключить каталог…"),
    ("Re-index", "Переиндексировать"),
    (
        "index this catalog: reads only the new and changed files and writes to the cache",
        "индексировать каталог: читает только новые и изменившиеся файлы и пишет в кэш",
    ),
    (
        "this catalog was opened from the catalog menu",
        "этот каталог открыт из меню каталогов",
    ),
    (
        "connect a catalog first: the “+” above the catalog pill, or “Connect a catalog…” in its menu",
        "сначала подключите каталог: «+» над кнопкой каталога или «Подключить каталог…» в её меню",
    ),
    (
        "You can index several folders. The box switches a folder off: its files leave every view while the rows, the previews and the files on disk stay. “Add a folder…” offers indexing straight away; “Remove from the index” deletes DB rows only — the files on disk are untouched.",
        "Индексировать можно несколько каталогов. Флажок выключает каталог: его файлы уходят \
         из всех экранов, а строки, превью и файлы на диске остаются. «Добавить каталог…» \
         сразу предлагает индексацию; «Убрать из индекса» удаляет только строки БД — файлы \
         на диске целы.",
    ),
    (
        "off: its files are hidden from every view; the rows, the previews and the files on disk stay",
        "выкл.: файлы скрыты из всех экранов; строки, превью и файлы на диске остаются",
    ),
    (
        "{} is switched off: its files are hidden from every view, the rows stay",
        "{} выключен: его файлы скрыты из всех экранов, строки остаются",
    ),
    (
        "{} is switched on again: the rows are back, nothing was read from disk",
        "{} снова включён: строки вернулись, с диска ничего не читалось",
    ),
    (
        " · {} in switched-off folders",
        " · {} в выключенных каталогах",
    ),
    ("No folders yet — add one with the button below.", "Каталогов пока нет — добавьте кнопкой ниже."),
    ("Remove from the index", "Убрать из индекса"),
    ("files on disk are not deleted", "файлы на диске не удаляются"),
    ("Add a folder…", "Добавить каталог…"),
    ("Add a folder", "Добавить каталог"),
    ("Index all", "Индексировать все"),
    ("Up", "Вверх"),
    ("Go", "Перейти"),
    ("not a folder", "это не папка"),
    ("path to a folder", "путь к папке"),
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
    // --- the passes of a catalog (M26.9) -----------------------------------------
    ("Faces", "Лица"),
    (
        "run the face pass over this catalog: find people, group them and keep the face vectors. Off: no new groups and no new faces here; the faces found before stay",
        "прогонять проход по лицам на этом каталоге: находить людей, собирать их в группы и хранить векторы лиц. Выключено: новых групп и новых лиц здесь не будет, а найденные раньше лица останутся",
    ),
    (
        "compute CLIP tags over this catalog: the tag cloud and the semantic search. Off: no new tags and no search hits from this folder; the tags computed before stay",
        "считать CLIP-теги на этом каталоге: облако тегов и смысловой поиск. Выключено: новых тегов и находок поиска из этого каталога не будет, а посчитанные раньше теги останутся",
    ),
    (
        "recognize text on the photos of this catalog: documents and screenshots. Off: no new text and no “By text” hits here; the text recognized before stays",
        "распознавать текст на снимках этого каталога: документы и скриншоты. Выключено: нового текста и находок «По тексту» здесь не будет, а распознанный раньше текст останется",
    ),
    (
        "The three boxes switch the expensive analysis for a catalog: faces, CLIP tags and text. Indexing, previews, duplicates and the calendar are not part of the switch. Off is not a deletion — what was computed before stays, and the pass comes back with the box.",
        "Три флажка переключают дорогой анализ каталога: лица, CLIP-теги и текст. Индексация, превью, дубли и календарь в переключатель не входят. Выключение — не удаление: посчитанное раньше остаётся, а проход возвращается вместе с флажком.",
    ),
    (
        "forget the faces, tags and text of this catalog…",
        "забыть лица, теги и текст этого каталога…",
    ),
    (
        "Forget the faces, tags and text of this catalog?",
        "Забыть лица, теги и текст этого каталога?",
    ),
    ("Forget the faces, tags and text", "Забыть лица, теги и текст"),
    ("Forget…", "Забыть…"),
    (
        "DB rows only: faces, CLIP tags, vectors and recognized text of these files. The photos, the previews, your own tags and every file on disk stay",
        "только строки БД: лица, CLIP-теги, векторы и распознанный текст этих файлов. Снимки, превью, свои теги и все файлы на диске остаются",
    ),
    (
        "{}: the pass is on again — the next run fills what is missing",
        "{}: проход снова включён — следующий прогон дозаполнит недостающее",
    ),
    (
        "{}: the pass is off — nothing computed before is deleted",
        "{}: проход выключен — посчитанное раньше не удаляется",
    ),
    (
        "{}: nothing to forget — this catalog has no faces, tags or text",
        "{}: забывать нечего — в этом каталоге нет ни лиц, ни тегов, ни текста",
    ),
    (
        "{}: analysis forgotten — {} faces, {} tags, {} vectors, {} texts; the files and your own tags stay",
        "{}: анализ забыт — лиц {}, тегов {}, векторов {}, текстов {}; файлы и свои теги остаются",
    ),
    ("the analysis was not forgotten", "анализ не забыт"),
    (
        "this folder has the text pass off — switch the box on in the folder list",
        "у этого каталога выключен проход текста — включите флажок в списке каталогов",
    ),
    (
        "{} photos skipped — their folders have faces off",
        "{} снимков пропущено — у их каталогов выключены лица",
    ),
    (
        "{} candidates skipped — their folders have text off",
        "{} кандидатов пропущено — у их каталогов выключен текст",
    ),
    (
        " ({} skipped — their folders have tags off)",
        " ({} пропущено — у их каталогов выключены теги)",
    ),
    (
        " ({} skipped — their folders have faces off)",
        " ({} пропущено — у их каталогов выключены лица)",
    ),
    (
        " ({} skipped — their folders have text off)",
        " ({} пропущено — у их каталогов выключен текст)",
    ),
    (
        " ({} faces of catalogs with faces off kept as they are)",
        " ({} лиц каталогов с выключенными лицами оставлены как есть)",
    ),
    // --- CLI: reports and command messages (M21.6) -----------------------------
    ("stopped: ", "остановлено: "),
    ("indexed {}/{} files ({}), unchanged {}, errors {}, {} s", "проиндексировано {}/{} файлов ({}), без изменений {}, ошибок {}, за {} с"),
    ("indexing: {}/{} files, {} errors", "индексация: {}/{} файлов, ошибок {}"),
    ("a folder is needed: photocat index <folder>", "нужен каталог: photocat index <каталог>"),
    ("kind", "тип"),
    ("files", "файлов"),
    ("size", "объём"),
    ("total: {} files, {}", "всего: {} файлов, {}"),
    ("switched off: {} — {}", "выключен: {} — {}"),
    ("shown: {}", "показано: {}"),
    (
        "{} is switched off: its files are hidden and indexing skipped it — switch it on in the folder list",
        "{} выключен: его файлы скрыты, индексация его пропустила — включите его в списке каталогов",
    ),
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
    // --- M23: the modules converted to the English source -----------------------
    ("to the OS trash: {}, errors {}, {}", "в системную корзину: {}, ошибок {}, {}"),
    ("not a single file was moved: {}", "ни один файл не перенесён: {}"),
    ("trash: moving to the OS trash…", "корзина: переношу в системную корзину…"),
    ("trash: done", "корзина: готово"),
    ("the walk over the folder is done", "обход каталога завершён"),
    ("done", "готово"),
    ("error {}: {}", "ошибка {}: {}"),
    ("the writing thread died", "поток записи упал"),
    ("does not decode {}: {}", "не декодируется {}: {}"),
    ("a video without a preview {}: {}", "видео без превью {}: {}"),
    (
        "tesseract does not start ({}) — install tesseract and the language packs ({}) or give its path in the settings; the rest works without it",
        "tesseract не запускается ({}) — установите tesseract и языковые пакеты ({}) или укажите путь в настройках; остальной функционал работает без него",
    ),

    (
        "downloading the face weights needs an explicit consent (the checkbox in the settings or --yes)",
        "скачивание весов лиц требует явного согласия (галочка в «Настройках» или --yes)",
    ),
    (
        "the face model {} is bigger than the limit of {} MiB — raise the limit in the settings",
        "модель лиц {} больше лимита {} МиБ — поднимите лимит в «Настройках»",
    ),
    ("faces: model {}, threads {}", "лица: моделей {}, потоков {}"),
    ("faces: stopped", "лица: остановлено"),
    ("faces: clusters", "лица: кластеры"),
    ("the face write thread died", "поток записи лиц упал"),
    ("faces {}: {}", "лица {}: {}"),

    ("walking both folders", "обход двух каталогов"),
    ("the comparison is done", "сверка завершена"),
    ("sha256 over the matching sizes", "sha256 по совпавшим размерам"),
    ("walking the folder", "обход каталога"),
    ("sha256 of every file", "sha256 всех файлов"),
    ("the manifest is ready", "слепок готов"),
    ("sha256 against the manifest", "sha256 по слепку"),
    ("the manifest check is done", "проверка слепка завершена"),

    ("exporting the album", "выгрузка альбома"),

    ("CLIP: everything is computed already", "CLIP: всё уже посчитано"),
    ("CLIP: done", "CLIP: готово"),
    ("CLIP: recomputing the tags without the photos…", "CLIP: пересчёт тегов без фото…"),
    ("CLIP: the tags are recomputed", "CLIP: теги пересчитаны"),
    (
        "downloading a model needs the owner's explicit consent",
        "скачивание модели требует явного согласия владельца",
    ),
    ("the model is downloaded", "модель скачана"),
    ("CLIP: dropped {} old vectors — computing them anew", "CLIP: сброшено {} старых векторов — считаю заново"),
    (
        "CLIP: embeddings, {} tags in the dictionary…",
        "CLIP: эмбеддинги, тегов в словаре {}…",
    ),
    ("CLIP: the batch failed ({}), one by one", "CLIP: пакет не прошёл ({}), по одному"),
    ("CLIP: cannot decode {}: {}", "CLIP: не декодировать {}: {}"),
    ("model {}: preparing", "модель {}: подготовка"),
    ("model {}: {}", "модель {}: {}"),

    ("New album", "Новый альбом"),
    ("New sub-album", "Новый подальбом"),
    ("Rename the album", "Переименовать альбом"),
    ("Move the album", "Переместить альбом"),
    ("Delete the album?", "Удалить альбом?"),
    ("remove the filter", "снять фильтр"),
    ("Album “{name}”", "Альбом «{name}»"),
    ("picked {} · {size}", "выбрано {} · {size}"),
    ("· {files} files · {}", "· файлов {files} · {}"),
    ("collapse the sub-albums", "свернуть подальбомы"),
    ("expand the sub-albums", "раскрыть подальбомы"),
    ("open the album; the cover shows its first photos in order", "открыть альбом; обложка показывает его первые снимки по порядку"),
    ("open the album", "открыть альбом"),
    ("{} · {} · {} — open the album", "{} · {} · {} — открыть альбом"),
    ("{} files", "файлов {}"),
    ("photocat — a media archive cataloguer", "photocat — каталог медиаархива"),
    ("{} files in the index, {}", "в индексе {} файлов, {}"),
    ("index read error: {e}", "ошибка чтения индекса: {e}"),
    ("albums not read: {e}", "альбомы не прочитаны: {e}"),
    ("album “{path}”: {files} files{} — album order", "альбом «{path}»: файлов {files}{} — порядок альбома"),
    (", {} in the trash", ", в корзине {}"),
    ("the album is not open: {e}", "альбом не открыт: {e}"),
    ("pick an album in the tree first", "сначала выберите альбом в дереве"),
    ("Will be deleted: {}", "Будет удалено: {}"),
    (
        "The album rows are removed; the files stay on disk.",
        "Строки альбома удаляются; файлы остаются на диске.",
    ),
    ("album created: {}", "альбом создан: {}"),
    ("no album picked", "альбом не выбран"),
    ("{} albums deleted; the files on disk are untouched", "удалено альбомов {}; файлы на диске не тронуты"),
    ("the album is not changed: {e}", "альбом не изменён: {e}"),
    ("give the target folder — it must exist already", "укажите каталог назначения — он должен уже существовать"),
    ("build the plan first", "сначала постройте план"),
    ("the album export runs in the background", "выгрузка альбома идёт в фоне"),
    ("the manifest file exists already: {}. Press “Overwrite the manifest” to replace it whole, or “Cancel”.", "файл слепка уже есть: {}. Нажмите «Перезаписать слепок», чтобы заменить его целиком, или «Отмена»."),
    ("/home/…/Photos from the phone", "/home/…/Фотографии с телефона"),
    ("/mnt/backup/Photos from the phone", "/mnt/backup/Фотографии с телефона"),
    ("<cache>/photocat/manifest.sha256", "<кэш>/photocat/manifest.sha256"),
    ("the last pass: {when} · {outcome}", "последний проход: {when} · {outcome}"),
    ("there is no album", "альбома нет"),
    ("nothing is picked", "ничего не выбрано"),
    ("{} added to the album, {} were there already; {} files now", "в альбом добавлено {}, уже было {}; теперь файлов {}"),
    ("there is nothing to remove from the album", "в альбоме нечего убирать"),
    ("{} removed from the album; {} left", "из альбома убрано {}; осталось {}"),
    ("pick a photo in the grid first", "сначала выберите снимок в сетке"),
    ("the album order is changed", "порядок в альбоме изменён"),
    ("the file is at the edge already", "файл уже на краю"),
    ("Root", "Корень"),
    ("A new parent. Own sub-albums are unavailable in the list — otherwise there would be a cycle.", "Новый родитель. Свои подальбомы в списке недоступны — иначе получился бы цикл."),
    ("The album name: up to 80 characters, unique among siblings regardless of case.", "Имя альбома: до 80 символов, уникально среди соседей без учёта регистра."),
    ("Dacha 2024, May, For printing", "Дача 2024, Май, На печать"),
    ("Create", "Создать"),
    ("Save", "Сохранить"),
    ("Move", "Переместить"),
    ("Delete the album", "Удалить альбом"),
    ("export: {d}/{t}", "выгрузка: {d}/{t}"),
    ("exporting…", "выгрузка…"),
    ("Export the album to disk", "Выгрузить альбом на диск"),
    ("Album “{title}”", "Альбом «{title}»"),
    ("File copies: the structure and the order repeat the album. The originals and the archive folder do not change; nothing in the target is overwritten or deleted.", "Копии файлов: структура и порядок повторяют альбом. Оригиналы и каталог архива не меняются; в цели ничего не перезаписывается и не удаляется."),
    ("Folder:", "Каталог:"),
    ("/mnt/disk/export — the folder exists already", "/mnt/disk/выгрузка — каталог уже существует"),
    ("File name:", "Имя файла:"),
    ("Placeholders: {n} {date} {time} {camera} {tag} {orig} {ext} {hash8}. {n} is the position in the album; sub-album folders carry no numbers.", "Подстановки: {n} {date} {time} {camera} {tag} {orig} {ext} {hash8}. {n} — позиция в альбоме; каталоги подальбомов без номеров."),
    ("Show the plan", "Показать план"),
    ("Export {} files", "Выгрузить {} файлов"),
    ("copies, not links; a repeated pass duplicates nothing", "копии, не ссылки; повторный прогон ничего не дублирует"),
    ("the export report is copied to the clipboard", "отчёт выгрузки скопирован в буфер обмена"),
    ("Will be moved to the OS trash.", "Будет перенесено в системную корзину ОС."),
    ("trash: moving {} files to the OS trash…", "корзина: переношу {} файлов в системную корзину…"),
    ("the model {} is verified: the sha256 matches", "модель {} проверена: sha256 совпал"),
    (" MiB", " МиБ"),
    ("above the threshold: {} of {} files with tags; the percentile is counted per tag", "выше порога: {} из {} файлов с тегами; процентиль считается отдельно по каждому тегу"),
    ("dictionary: “{}” saved", "словарь: «{}» сохранён"),
    ("the dictionary is not saved: {e}", "словарь не сохранён: {e}"),
    ("the tag “{label}” is in the dictionary already", "тег «{label}» уже есть в словаре"),
    ("the tag “{}” is added — press “Recompute the tags”", "тег «{}» добавлен — нажмите «Пересчитать теги»"),
    ("the dictionary is loaded: {} tags — press “Recompute the tags”", "словарь загружен: {} тегов — нажмите «Пересчитать теги»"),
    ("the filters are reset", "фильтры сброшены"),
    ("search by the content of the photos", "поиск по содержимому снимков"),
    ("search the recognized text", "поиск по распознанному тексту"),
    ("{} files · {} with EXIF · synced", "{} файлов · {} с EXIF · синхронизировано"),
    ("manifest {manifest} · the last pass {date} · the term is over", "слепок {manifest} · прошлый проход {date} · срок вышел"),
    ("manifest {manifest} · there has been no pass yet", "слепок {manifest} · прохода ещё не было"),
    ("Picked", "Выбрано"),
    ("Similarity threshold: {} bits out of 64", "Порог похожести: {} бит из 64"),
    ("{} groups, {} duplicates, in {:.1} s", "групп: {}, дублей на {}, за {:.1} с"),
    ("{} in the groups, {} marked spare, {} will be freed", "в группах {}, помечено лишними {}, освободится {}"),
    ("the plan is written: {} ({} rows)", "план записан: {} (строк {})"),
    ("the sharpness is not computed for {} photos — index the folder", "резкость не посчитана у {} снимков — проиндексируйте каталог"),
    ("{} faces in {} photos, {} groups", "лиц: {} на {} снимках, групп: {}"),
    ("{} of {} photos have no faces yet", "ещё без лиц: {} из {} снимков"),
    ("run CLIP locally over the previews: {} approximate categories; look for the thing you need with a word search; nothing is sent anywhere", "локально прогнать CLIP по превью: {} приблизительных категорий; нужный предмет ищите смысловым поиском словами; ничего не отправляется"),
    ("a free word search understands any words, not only the {} tags", "свободный смысловой поиск понимает любые слова, не только {} тегов"),
    ("with text: {} of {} photos", "с текстом: {} из {} снимков"),
    ("{} candidates, {} left", "кандидатов: {}, осталось {}"),
    ("the photo is open in the calendar: {}", "снимок открыт в календаре: {}"),
    ("the photo has no date — it is open in all files, selected", "у снимка нет даты — открыт в общих файлах с выделением"),
    ("downloading a model: weights only, no user data", "скачивание модели: только веса, без пользовательских данных"),
    ("the view changed", "изменился вид"),
    ("the folder and date filter changed", "изменился фильтр по пути/дате"),
    ("the photo and video filter changed", "изменился фильтр фото/видео"),
    ("the tag filter changed", "изменился фильтр по тегу"),
    ("the search changed", "изменился поиск"),
    ("the album was closed", "закрыт альбом"),
    ("another album was opened", "открыт другой альбом"),
    ("the grid of the album changed", "изменился состав сетки альбома"),
    ("the order changed", "изменился порядок"),
    ("the selection is cleared: {}", "выделение снято: {}"),
    ("the selection is cleared", "выделение снято"),
    ("picked {} · {}", "выбрано {} · {}"),
    ("few tags ({} files) — the percentile is noisy, the raw threshold {:.2} is used", "мало тегов ({} файлов) — процентиль шумный, берётся сырой порог {:.2}"),
    ("CLIP: computing the embeddings and the tags…", "CLIP: считаю эмбеддинги и теги…"),
    ("the selected photo", "выбранного снимка"),
    ("OCR: recognizing the text of {}…", "OCR: распознаю текст с {}…"),
    ("OCR: a batch pass over {} candidates…", "OCR: пакетный проход по {} кандидатам…"),
    ("text search “{q}”: {} files in {} ms", "поиск по тексту «{q}»: файлов {} за {} мс"),
    ("the text search failed: {e}", "поиск по тексту не удался: {e}"),
    ("the word search is unavailable: the model is not downloaded (see the Settings)", "смысловой поиск недоступен: модель не скачана (см. «Настройки»)"),
    ("word search “{q}”: {} files in {} ms", "смысловой поиск «{q}»: {} файлов за {} мс"),
    ("the search failed: {e}", "поиск не удался: {e}"),
    ("CLIP: loading the model for the query “{q}”…", "CLIP: загружаю модель для запроса «{q}»…"),
    ("faces: {} models, the threshold {:.2}…", "лица: моделей {}, порог {threshold:.2}…"),
    ("give a folder in the top bar or add one with the “Folders” button", "укажите каталог в верхней панели или добавьте его кнопкой «Каталоги»"),
    ("not a folder: {}", "не каталог: {}"),
    ("indexing {}…", "индексация {}…"),
    ("indexing {} folders…", "индексация {} каталогов…"),
    (" (skipped: {})", " (пропущено: {})"),
    (" (switched off: {})", " (выключено: {})"),
    (
        "every folder is switched off: {} — switch one on in the folder list",
        "все каталоги выключены: {} — включите один в списке каталогов",
    ),
    ("indexing {}: {e}", "индексация {}: {e}"),
    ("remove the tag from this photo; the original does not change", "убрать тег у этого фото; оригинал не меняется"),
    ("the indexing failed: {e}", "индексация не удалась: {e}"),
    ("CLIP is not computed: {e}", "CLIP не посчитан: {e}"),
    ("the tags are not recomputed: {e}", "теги не пересчитаны: {e}"),
    ("word search: {} files in {} ms", "смысловой поиск: {} файлов за {} мс"),
    ("the model is not downloaded: {e}", "модель не скачана: {e}"),
    ("the text is recognized: {} characters", "текст распознан: знаков {}"),
    ("no text is found on this photo", "на этом снимке текста не найдено"),
    ("the text is not recognized: {e:#}", "текст не распознан: {e:#}"),
    ("the export was interrupted without a report", "выгрузка прервана без отчёта"),
    ("the check was interrupted without a report", "проверка прервана без отчёта"),
    ("{} of {} files", "файлов {} из {}"),
    ("Album “{}”", "Альбом «{}»"),
    ("{}, {} spare", "{}, зря {}"),
    ("{} faces in {} photos", "лиц {} на {} снимках"),
    ("{} files, {}", "файлов {}, {}"),
    ("no photos with the tag “{tag}” above the threshold {}% ({:.2}) — lower the threshold slider in the left panel", "нет снимков с тегом «{tag}» выше порога {}% ({tag_filter_raw:.2}) — опустите ползунок порога в левой панели"),
    ("Reset the filter", "Сбросить фильтр"),
    ("+ New album", "+ Новый альбом"),
    ("An album is a set of photos with an order and sub-albums. Data only: the files on disk do not change.", "Альбом — это набор снимков с порядком и подальбомами. Только данные: файлы на диске не меняются."),
    ("Pick an album in the tree or create a new one. Inside are the sub-albums and the photos in the order they will lie on disk.", "Выберите альбом в дереве или создайте новый. Внутри — подальбомы и снимки в том порядке, в котором они лягут на диск."),
    ("Create the first album with “+ New album”: inside are the sub-albums and the photos in the order they will lie on disk.", "Создайте первый альбом кнопкой «+ Новый альбом»: внутри — подальбомы и снимки в том порядке, в котором они лягут на диск."),
    ("+ Sub-album", "+ Подальбом"),
    ("Rename", "Переименовать"),
    ("Move to…", "Переместить в…"),
    ("move the album up among its siblings", "поднять альбом среди соседей"),
    ("Down", "Ниже"),
    ("move the album down among its siblings", "опустить альбом среди соседей"),
    ("delete the rows of the album and its sub-albums; the files on disk stay", "удалить строки альбома и подальбомов; файлы на диске останутся"),
    ("Sub-albums", "Подальбомы"),
    ("In the album", "В альбоме"),
    ("the photos of this album in its order", "снимки этого альбома в его порядке"),
    ("a temporary look at the whole archive: pick photos and press “Add the picked ones”", "временный взгляд на весь архив: выберите снимки и нажмите «Добавить выбранные»"),
    ("Add the picked ones ({picked_now})", "Добавить выбранные ({picked_now})"),
    ("add the marked photos to the end of the album; a repeat does not duplicate rows", "добавить отмеченные снимки в конец альбома; повтор не плодит строк"),
    ("Remove from the album ({in_album})", "Убрать из альбома ({in_album})"),
    ("the album rows go away; the files on disk do not change", "строки альбома уйдут; файлы на диске не меняются"),
    ("File up", "Файл выше"),
    ("move the selected photo up in the album order", "сдвинуть выбранный снимок вверх в порядке альбома"),
    ("File down", "Файл ниже"),
    ("move the selected photo down in the album order", "сдвинуть выбранный снимок вниз в порядке альбома"),
    ("Export to disk…", "Выгрузить на диск…"),
    ("file copies into a folder: the structure and order as in the album; nothing in the target is overwritten", "копии файлов в каталог: структура и порядок как в альбоме; в цели ничего не перезаписывается"),
    ("A temporary look: the grid shows the whole archive. Pick photos and press “Add the picked ones”.", "Временный взгляд: сетка показывает весь архив. Выделите снимки и нажмите «Добавить выбранные»."),
    ("The archive has no photos under the current filter.", "В архиве нет снимков под текущим фильтром."),
    ("The album has no files yet — switch to “All files”, pick photos and press “Add the picked ones”.", "В альбоме пока нет файлов — переключитесь на «Все файлы», выберите снимки и нажмите «Добавить выбранные»."),
    ("Every file of the album is in the app trash — restore them or remove the rows.", "Все файлы альбома в корзине приложения — верните их или уберите строки."),
    ("#{} — {} · {} files · {} spare", "#{} — {} · файлов {} · зря {}"),
    ("{}: keep {name} — {}", "{}: оставить {name} — {}"),
    ("{} — {} files, {}", "{} — {} файлов, {}"),
    ("No date — {} files", "Без даты — {} файлов"),
    ("photos: {}", "снимков: {}"),
    ("file", "файл"),
    ("kept {name}, {} marked spare", "оставлен {name}, помечено лишними {}"),
    ("the group is named: “{label}”", "группа подписана: «{label}»"),
    ("{} faces are gathered into a new group; give it a name", "{} лиц собраны в новую группу; дайте ей имя"),
    ("{} faces are assigned to the group “{}”", "{} лиц назначены группе «{}»"),
    ("{} selected · {}", "выбрано {} · {}"),
    ("{} · {}{} · {} sub-albums · {}", "{} · {}{} · {} подальбомов · {}"),
    ("{} in the index, {}", "в индексе {}, {}"),
    ("{}, {} · {} in the trash", "{}, {} · в корзине {}"),
    ("{} this month, {}", "{}, за месяц {}"),
    ("album “{}”: {}{} — album order", "альбом «{}»: {}{} — порядок альбома"),
    ("faces: {} models, the threshold {}…", "лица: моделей {}, порог {}…"),
    ("indexing {}: {}", "индексация {}: {}"),
    ("export: {}/{}", "выгрузка: {}/{}"),
    ("manifest {} · the last pass {} · due {} days ago", "слепок {} · прошлый проход {} · срок вышел {} дн. назад"),
    ("{} · {} files{} · {} sub-albums · {}", "{} · {} файлов{} · {} подальбомов · {}"),

    ("Add the picked ones ({})", "Добавить выбранные ({})"),
    ("CLIP is not computed: {}", "CLIP не посчитан: {}"),
    ("CLIP: loading the model for the query “{}”…", "CLIP: загружаю модель для запроса «{}»…"),
    ("Remove from the album ({})", "Убрать из альбома ({})"),
    ("albums not read: {}", "альбомы не прочитаны: {}"),
    ("few tags ({} files) — the percentile is noisy, the raw threshold {} is used", "мало тегов ({} файлов) — процентиль шумный, берётся сырой порог {}"),
    ("index read error: {}", "ошибка чтения индекса: {}"),
    ("kept {}, {} marked spare", "оставлен {}, помечено лишними {}"),
    ("manifest {} · the last pass {} · the term is over", "слепок {} · прошлый проход {} · срок вышел"),
    ("manifest {} · there has been no pass yet", "слепок {} · прохода ещё не было"),
    ("text search “{}”: {} files in {} ms", "поиск по тексту «{}»: файлов {} за {} мс"),
    ("the album is not changed: {}", "альбом не изменён: {}"),
    ("the album is not open: {}", "альбом не открыт: {}"),
    ("the dictionary is not saved: {}", "словарь не сохранён: {}"),
    ("the group is named: “{}”", "группа подписана: «{}»"),
    ("the indexing failed: {}", "индексация не удалась: {}"),
    ("the last pass: {} · {}", "последний проход: {} · {}"),
    ("the model is not downloaded: {}", "модель не скачана: {}"),
    ("the search failed: {}", "поиск не удался: {}"),
    ("the tag “{}” is in the dictionary already", "тег «{}» уже есть в словаре"),
    ("the tags are not recomputed: {}", "теги не пересчитаны: {}"),
    ("the text is not recognized: {}", "текст не распознан: {}"),
    ("the text search failed: {}", "поиск по тексту не удался: {}"),
    ("word search “{}”: {} files in {} ms", "смысловой поиск «{}»: {} файлов за {} мс"),
    ("{}, {} duplicates, in {} s", "{}, дублей на {}, за {} с"),
    ("{}, {} s", "{}, {} с"),
    ("{}: keep {} — {}", "{}: оставить {} — {}"),

    // The default tag labels: English in the source, translated for the Russian window.
    ("beach", "пляж"),
    ("document", "документ"),
    ("screenshot", "скриншот"),
    ("food", "еда"),
    ("animals", "животные"),
    ("children", "дети"),
    ("celebration", "праздник"),
    ("furniture", "мебель"),
    ("interior", "интерьер"),
    ("clothes", "одежда"),
    ("nature", "природа"),
    ("city", "город"),
    ("transport", "транспорт"),
    ("plants", "растения"),

    // --- M26.2: the warning mark of a limited feature and its info dialog -------
    (
        "some features are limited — click for the details",
        "часть возможностей ограничена — нажмите, чтобы узнать подробности",
    ),
    ("Some features are limited", "Часть возможностей ограничена"),
    (
        "photocat works without them: only their own work is off, everything else works as before.",
        "photocat работает и без них: отключается только их работа, всё остальное работает как прежде.",
    ),
    ("How to install on {}:", "Как установить в системе {}:"),
    (
        "the tesseract installer offers the Russian and English language packs",
        "установщик tesseract предлагает русский и английский языковые пакеты",
    ),
    (
        "the command brings the Russian and English language packs of tesseract",
        "команда ставит и русский с английским языковые пакеты tesseract",
    ),
    (
        "The pages the programs come from — named here only, nothing is opened:",
        "Страницы, откуда берутся программы, — только названы, ничего не открывается:",
    ),
    (
        "photocat installs and downloads nothing itself and opens no browser: copy the command and run it in a terminal.",
        "photocat сам ничего не устанавливает и не скачивает и не открывает браузер: скопируйте команду и выполните её в терминале.",
    ),
    (
        "After installing, press “Find the programs” in “Settings → External programs” (“Text on photos” for tesseract) — or leave the field empty, and photocat finds the program in PATH.",
        "После установки нажмите «Найти программы» в «Настройках → Внешние программы» (для tesseract — в «Текст на фото») — или оставьте поле пустым, и photocat найдёт программу в PATH.",
    ),
    ("Copy the commands", "Скопировать команды"),
    ("the commands are copied to the clipboard", "команды скопированы в буфер обмена"),

    // --- M26.3: "Find the programs" -------------------------------------------
    ("Find the programs", "Найти программы"),
    (
        "looks in PATH and in the usual places of the system; an empty field is filled, a path you set is checked and never overwritten",
        "ищет в PATH и в обычных местах системы; пустое поле заполняется, заданный путь проверяется и не перезаписывается",
    ),
    ("looking for the programs…", "ищу программы…"),
    ("the programs are found: {} of {}", "программы найдены: {} из {}"),
    ("found {} {} at {}", "найдено: {} {} — {}"),
    (
        "found {} at {}, but it does not start: {}",
        "найдено: {} — {}, но не запускается: {}",
    ),
    ("not found: {}", "не найдено: {}"),
    (
        "press “Find the programs” again after installing",
        "после установки нажмите «Найти программы» ещё раз",
    ),

    // --- M26.11, the quarantine since M26.17: a proposal waits for a confirmation ------
    (
        "no named people yet: there is nothing to match to",
        "названных людей пока нет: сопоставлять некому",
    ),
    (
        "would propose {} faces to {} named people",
        "будет предложено {} лиц {} названным людям",
    ),
    (
        "proposed {} faces to {} named people — they wait in quarantine",
        "предложено {} лиц {} названным людям — они ждут в карантине",
    ),
    (
        "no new proposal yet: {} named",
        "новых предложений пока нет: названных — {}",
    ),
    (
        "{} left — the lead was too small",
        "{} осталось — перевес слишком мал",
    ),
    (
        "{} left — no named person is close enough",
        "{} осталось — ни один названный человек не достаточно близок",
    ),
    // --- M26.16: the thresholds explain themselves and count their effect --------------
    ("Tuning", "Настройка"),
    (
        "the merge threshold and the two sliders of the matching step; they decide the next pass, not the group on screen",
        "порог склейки и два ползунка сопоставления; они решают следующий проход, а не группу на экране",
    ),
    ("Merge threshold", "Порог склейки"),
    ("Closeness to a named person", "Близость к названному"),
    ("Lead over the next person", "Перевес над следующим"),
    (
        "how close a free face must be to a named person to be offered to you",
        "насколько близко неназванное лицо должно быть к названному человеку, чтобы его вам предложили",
    ),
    (
        "a face is offered only when it is at least that close and leads the next person by this much; a wrong match is turned down on its tile",
        "лицо предлагается, только когда оно настолько близко и опережает следующего человека на эту величину; ошибочное предложение отклоняется на плитке",
    ),
    (
        "These values decide the next pass — not the group on screen. “Rebuild groups” applies the merge threshold and looks for the named people; a name is checked as soon as it is saved.",
        "Эти значения решают следующий проход, а не группу на экране. «Пересобрать группы» применяет порог склейки и ищет названных людей; имя проверяется сразу после сохранения.",
    ),
    (
        "counting the free faces at {} / {}…",
        "считаю неназванные лица при {} / {}…",
    ),
    (
        "No named people yet: name a group and the free faces that resemble it will be offered here.",
        "Названных людей пока нет: назовите группу, и похожие на неё неназванные лица появятся здесь.",
    ),
    (
        "At {} / {}: 0 of {} free faces would join a named person ({} close but ambiguous, {} farther)",
        "При {} / {}: из {} неназванных лиц к названному человеку не подойдёт никто ({} близко, но неоднозначно, {} дальше)",
    ),
    (
        "At {} / {}: {} of {} free faces would join a named person ({} close but ambiguous, {} farther)",
        "При {} / {}: {} из {} неназванных лиц подойдут к названному человеку ({} близко, но неоднозначно, {} дальше)",
    ),
    ("Look for the named people", "Искать названных людей"),
    (
        "compares the free faces with the named people and puts the close ones into the quarantine; no group changes until you confirm a face",
        "сравнивает неназванные лица с названными людьми и помещает похожие в карантин; ни одна группа не меняется, пока вы не подтвердите лицо",
    ),
    // --- M26.17: the quarantine --------------------------------------------------------
    ("Quarantine", "Карантин"),
    (
        "faces that resemble a person you named; nothing has joined a group until you confirm",
        "лица, похожие на названного человека; ни одно не входит в группу, пока вы не подтвердите",
    ),
    (
        "in quarantine: {} — open the group list to confirm them",
        "в карантине: {} — откройте список групп, чтобы подтвердить",
    ),
    (
        "Each face resembles a person you named. Nothing has joined a group: the tick confirms it, the crossed head turns it down and the face stays where it is.",
        "Каждое лицо похоже на названного человека. В группу пока никто не вошёл: галочка подтверждает, перечёркнутая голова отклоняет, и лицо остаётся на месте.",
    ),
    (
        "Nothing waits for confirmation. Name a group, then open “Tuning” and press “Look for the named people”: the close faces will be offered here.",
        "Подтверждать нечего. Назовите группу, откройте «Настройку» и нажмите «Искать названных людей»: похожие лица появятся здесь.",
    ),
    ("Confirm all", "Подтвердить все"),
    (
        "every face below joins the person it resembles; a wrong one is split off on its tile afterwards",
        "каждое лицо ниже входит к человеку, на которого похоже; ошибочное потом отделяется кнопкой на плитке",
    ),
    (
        "confirm: this face joins the person it resembles",
        "подтвердить: лицо входит к человеку, на которого похоже",
    ),
    (
        "turn down: not this person; the face stays where it is",
        "отклонить: это не тот человек; лицо остаётся на месте",
    ),
    (
        "looks like {} at {} — confirm it or turn it down; nothing has changed yet",
        "похоже на {} с близостью {} — подтвердите или отклоните; пока ничего не изменилось",
    ),
    ("looking for the named people…", "ищу названных людей…"),
    ("nothing waits in quarantine", "в карантине ничего не ждёт"),
    (
        "the named people are not looked for",
        "названные люди не найдены",
    ),
    (
        "a face number is needed: faces confirm --id N",
        "нужен номер лица: faces confirm --id N",
    ),
    (
        "the free faces are not counted",
        "неназванные лица не посчитаны",
    ),
    (
        "1 face joined the person it resembles",
        "1 лицо вошло к человеку, на которого похоже",
    ),
    (
        "{} faces joined the person they resemble",
        "{} лиц вошли к человеку, на которого похожи",
    ),
    (
        "1 face stays where it is — the person is not them",
        "1 лицо остаётся на месте — это не тот человек",
    ),
    (
        "{} faces stay where they are — the person is not them",
        "{} лиц остаются на месте — это не тот человек",
    ),
    ("the faces are not confirmed", "лица не подтверждены"),
    ("the faces are not turned down", "лица не отклонены"),
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
            ("main.rs", include_str!("main.rs")),
            ("cli/mod.rs", include_str!("cli/mod.rs")),
            ("cli/report.rs", include_str!("cli/report.rs")),
            ("cli/flags.rs", include_str!("cli/flags.rs")),
            ("cli/index.rs", include_str!("cli/index.rs")),
            ("cli/stats.rs", include_str!("cli/stats.rs")),
            ("cli/embed.rs", include_str!("cli/embed.rs")),
            ("cli/faces.rs", include_str!("cli/faces.rs")),
            ("cli/ocr.rs", include_str!("cli/ocr.rs")),
            ("cli/search.rs", include_str!("cli/search.rs")),
            ("cli/models.rs", include_str!("cli/models.rs")),
            ("cli/settings.rs", include_str!("cli/settings.rs")),
            ("cli/eval.rs", include_str!("cli/eval.rs")),
            ("cli/clusters.rs", include_str!("cli/clusters.rs")),
            ("cli/doctor.rs", include_str!("cli/doctor.rs")),
            ("cli/albums.rs", include_str!("cli/albums.rs")),
            ("cli/tags.rs", include_str!("cli/tags.rs")),
            ("db/mod.rs", include_str!("db/mod.rs")),
            ("db/schema.rs", include_str!("db/schema.rs")),
            ("db/settings.rs", include_str!("db/settings.rs")),
            ("db/files.rs", include_str!("db/files.rs")),
            ("db/tags.rs", include_str!("db/tags.rs")),
            ("db/faces.rs", include_str!("db/faces.rs")),
            ("db/albums.rs", include_str!("db/albums.rs")),
            ("db/progress.rs", include_str!("db/progress.rs")),
            ("db/human.rs", include_str!("db/human.rs")),
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
