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

/// Подставить значения в переведённый шаблон: `{}` заменяются по порядку, остальной
/// текст остаётся как есть. Отчёты CLI (M21.6) держат всю фразу в таблице, а числа
/// подставляют сюда — тогда порядок слов задаёт перевод, а не `format!` в коде.
/// Лишние значения игнорируются, недостающие оставляют `{}` на месте: отчёт не
/// исчезает из-за опечатки в переводе.
pub fn fill(lang: Lang, ru: &'static str, values: &[String]) -> String {
    let tpl = t(lang, ru);
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

/// «3 из 12» / «3 of 12» — счётчик полного экрана.
pub fn position(lang: Lang, index: usize, total: usize) -> String {
    match lang {
        Lang::En => format!("{index} of {total}"),
        Lang::Ru => format!("{index} из {total}"),
    }
}

/// «3 с» / «3 s» — интервал слайд-шоу.
pub fn seconds(lang: Lang, n: u64) -> String {
    match lang {
        Lang::En => format!("{n} s"),
        Lang::Ru => format!("{n} с"),
    }
}

/// Текст `--help` на двух языках: пара живёт здесь, чтобы меню не разъехалось
/// с таблицей, а английский вариант не существовал отдельно от русского (M21.6).
pub const USAGE: (&str, &str) = (
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
);

/// Все переведённые строки: русский оригинал → английский текст.
///
/// Ключ — ровно та строка, что стоит в коде. Одна и та же русская строка переводится
/// один раз: `t("Настройки")` в навигации, заголовке диалога и подсказке даёт одно слово.
/// Пополняется экран за экраном (см. `plan/I18N.md`).
const PAIRS: &[(&str, &str)] = &[
    // --- CLI: справка ---------------------------------------------------------
    USAGE,
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
    (
        "размер плитки (плитка — ширина кадра, строки — высота), 100–260 px",
        "tile size (a tile: frame width, rows: row height), 100–260 px",
    ),
    ("Плитка", "Tile"),
    ("Ряды", "Rows"),
    ("Список", "List"),
    (
        "как в макете: кадр 4:3, снимок вписан целиком",
        "as in the mock-up: a 4:3 frame, the snapshot fitted whole",
    ),
    (
        "строки по пропорциям: ширина плитки по кадру",
        "proportion rows: the tile width follows the frame",
    ),
    ("список: имя, дата и размер в строку", "a list: name, date and size in one row"),
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
    // --- корзина ---------------------------------------------------------------
    (
        "Файлы в корзине приложения: на диске не тронуты. «Вернуть» — снова в архив, \
         «Очистить корзину» — перенос в системную корзину только после подтверждения.",
        "Files in the app trash: untouched on disk. “Restore” brings them back to the archive, \
         “Empty the trash” moves them to the OS trash after a confirmation.",
    ),
    ("Корзина пуста.", "The trash is empty."),
    ("Вернуть всё", "Restore all"),
    ("Очистить корзину", "Empty the trash"),
    (
        "перенос в системную корзину ОС; безвозвратного удаления нет",
        "moved to the OS trash; nothing is deleted for good",
    ),
    ("возвращено из корзины", "restored from the trash"),
    ("файл возвращён из корзины", "file restored from the trash"),
    ("файл не вернуть", "file not restored"),
    ("корзина не очищена", "trash not emptied"),
    ("в корзине", "in trash"),
    ("вернуть", "restore"),
    ("вернуть файл в обычные виды; на диске он не менялся", "bring the file back into the usual views; on disk it was untouched"),
    ("Очистить корзину?", "Empty the trash?"),
    (
        "Внутри приложения безвозвратного удаления нет: файлы остаются в системной корзине, \
         пока вы сами её не очистите. Оригиналы в архиве исчезнут из своих каталогов — это \
         необратимо средствами приложения.",
        "Nothing is deleted for good inside the app: the files stay in the OS trash until you \
         empty it yourself. The originals will disappear from their folders in the archive — \
         the app cannot undo that.",
    ),
    ("Переместить в системную корзину", "Move to the OS trash"),
    ("Отмена", "Cancel"),
    // --- целостность ------------------------------------------------------------
    ("сверка копий", "comparing copies"),
    ("запись слепка", "writing the manifest"),
    ("проверка по слепку", "checking against the manifest"),
    ("сверка остановлена: проверено не всё", "comparison stopped: not everything was checked"),
    ("копии совпадают", "the copies match"),
    ("слепок не записан: проход остановлен", "manifest not written: the pass was stopped"),
    ("слепок не записан", "manifest not written"),
    ("ошибок", "errors"),
    ("проверка остановлена: проверено не всё", "check stopped: not everything was checked"),
    ("каталог совпадает со слепком", "the folder matches the manifest"),
    (
        "Сверка только читает: сначала размеры, sha256 — лишь у файлов одинакового размера. \
         Слепок — файл формата sha256sum для будущих проверок; он пишется только по кнопке. \
         В каталог архива не записывается ничего.",
        "The comparison only reads: sizes first, sha256 only for files of the same size. The \
         manifest is a sha256sum file for future checks; it is written by a button only. \
         Nothing is written into the archive folder.",
    ),
    ("Каталог A (архив)", "Folder A (archive)"),
    ("Каталог B (копия)", "Folder B (copy)"),
    ("Файл слепка", "Manifest file"),
    ("каталог A (архив)", "folder A (archive)"),
    ("каталог B (копия)", "folder B (copy)"),
    ("файл слепка", "manifest file"),
    ("Показать все пути", "Show all paths"),
    ("Сверить копии", "Compare the copies"),
    ("сравнить каталог A и каталог B побайтово", "compare folder A and folder B byte by byte"),
    ("Записать слепок", "Write the manifest"),
    ("Перезаписать слепок", "Overwrite the manifest"),
    ("sha256 всех файлов каталога A в файл слепка", "sha256 of every file in folder A into the manifest"),
    ("Проверить по слепку", "Check against the manifest"),
    ("сравнить каталог A с сохранённым слепком", "compare folder A with the saved manifest"),
    (
        "проход идёт: прогресс и «Стоп» — в верхней панели",
        "the pass is running: progress and “Stop” are in the top bar",
    ),
    ("Напоминать о проверке каждые", "Remind me to check every"),
    (" дн.", " days"),
    ("дн.", "days"),
    ("дн. назад", "days ago"),
    ("0 — не напоминать", "0 — do not remind"),
    (
        "срок считается от последнего полного прохода по архиву",
        "the term counts from the last complete pass over the archive",
    ),
    ("интервал напоминания не сохранён", "reminder interval not saved"),
    ("слепок не задан — напоминание молчит", "no manifest set — the reminder stays quiet"),
    ("последнего прохода ещё не было", "no pass has happened yet"),
    ("совпало", "matched"),
    ("были расхождения", "there were differences"),
    (
        "дата последнего прохода непонятна — напомним при запуске",
        "the date of the last pass is unclear — we will remind on launch",
    ),
    ("напоминание: слепок не задан", "reminder: no manifest set"),
    ("напоминание выключено (интервал 0)", "reminder disabled (interval 0)"),
    ("следующая проверка: через", "next check in"),
    ("пора проверить: срок вышел", "time to check: overdue"),
    ("отчёт прошлого прохода", "report of the last pass"),
    ("Скопировать прошлый отчёт", "Copy the last report"),
    ("Скопировать отчёт", "Copy the report"),
    (
        "тот же отчёт, что печатает CLI; в архив ничего не записано",
        "the same report the CLI prints; nothing was written into the archive",
    ),
    ("перезапись слепка отменена", "manifest overwrite cancelled"),
    ("отчёт скопирован в буфер обмена", "report copied to the clipboard"),
    (
        "отчёт прошлого прохода скопирован в буфер обмена",
        "the last pass report was copied to the clipboard",
    ),
    ("укажите", "please give"),
    ("не каталог", "is not a folder"),
    (
        "каталог A и каталог B — один и тот же путь; сверять нечего",
        "folder A and folder B are the same path; nothing to compare",
    ),
    (
        "укажите файл слепка: это путь, по которому он будет записан",
        "give the manifest file: the path it will be written to",
    ),
    ("это каталог", "that is a folder"),
    ("каталог для слепка не существует", "the folder for the manifest does not exist"),
    ("укажите файл слепка, по которому проверять", "give the manifest file to check against"),
    ("файл слепка не найден", "manifest file not found"),
    (
        "проход уже идёт: дождитесь конца или нажмите «Стоп»",
        "a pass is already running: wait for it or press “Stop”",
    ),
    (
        "слепок не записан: нужно подтверждение перезаписи",
        "manifest not written: the overwrite needs a confirmation",
    ),
    ("настройки напоминания не сохранены", "reminder settings not saved"),
    ("идёт в фоне", "running in the background"),
    ("итог не сохранён в настройках", "the outcome was not saved in the settings"),
    ("отчёт не записан в", "report not written to"),
    ("ошибка", "error"),
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
    ("Тема интерфейса", "Interface theme"),
    ("Тёмная", "Dark"),
    ("Светлая", "Light"),
    ("Как в системе", "Follow the system"),
    (
        "Тёмная — основная: снимки смотрят вечерами. Светлая — та же сетка на белом, \
         для дневной работы. Тема не меняет ни файлов, ни базы.",
        "Dark is the main one: snapshots are looked at in the evening. Light is the same \
         grid on white, for daytime work. The theme changes neither files nor the database.",
    ),
    ("настройки не сохранены", "settings not saved"),

    // --- левая панель: модель, теги, текст, сводка (M21.4f) --------------------
    ("Посчитать теги", "Compute tags"),
    ("Настройки модели", "Model settings"),
    ("с уверенным тегом:", "with a confident tag:"),
    ("тег", "tag"),
    ("свой", "mine"),
    ("Порог фильтра", "Filter threshold"),
    (
        "ни один тег не проходит порог — опустите ползунок",
        "no tag passes the threshold — lower the slider",
    ),
    ("поиск", "search"),
    (
        "tesseract не запускается — путь в «Настройках»",
        "tesseract does not start — set its path in “Settings”",
    ),
    (
        "OCR: останавливаю после текущих файлов…",
        "OCR: stopping after the current files…",
    ),
    ("Распознать документы", "Recognize documents"),
    (
        "долгий проход: документы и скриншоты без камеры и по уверенному тегу; \
         один снимок — кнопкой в карточке файла",
        "a long pass: documents and screenshots without a camera and with a confident tag; \
         a single photo — with the button in the file card",
    ),
    ("Сводка", "Summary"),
    ("Файлов:", "Files:"),
    ("Объём:", "Size:"),
    ("Без даты:", "No date:"),
    ("Без превью:", "No preview:"),
    ("Видео без обложки:", "Videos without a cover:"),
    (
        "нажмите «Индексировать» — обложки создаст ffmpeg",
        "press “Index” — ffmpeg will create the covers",
    ),
    (
        "теги ещё не посчитаны — нажмите «Посчитать теги»",
        "tags are not computed yet — press “Compute tags”",
    ),
    (
        "100% — самый сильный снимок этого тега, 0% — все; 70% — верхние 30%",
        "100% — the strongest photo of this tag, 0% — all; 70% — the top 30%",
    ),

    // --- настройки: модель (M21.4f) ---------------------------------------------
    ("Модель", "Model"),
    ("призн.", "dims"),
    ("Состояние модели", "Model state"),
    ("Скачана · проверена", "Downloaded · verified"),
    ("Не скачана", "Not downloaded"),
    ("Проверить модель", "Check the model"),
    ("модель не готова", "the model is not ready"),
    ("Папка моделей", "Model folder"),
    ("путь, куда скачивать веса", "where to download the weights"),
    ("Применить путь", "Apply the path"),
    ("путь к моделям пуст", "the model path is empty"),
    ("Лимит на скачивание", "Download limit"),
    (
        "Модель сверх лимита не скачивается. Если выбранная недоступна, \
         загрузчик ищет совместимую в пределах лимита.",
        "A model over the limit is not downloaded. If the chosen one is unavailable, \
         the downloader looks for a compatible one within the limit.",
    ),
    ("на месте", "in place"),
    ("не скачана", "not downloaded"),
    ("сверх лимита", "over the limit"),
    ("Согласие на сеть", "Network consent"),
    (
        "разрешаю скачать веса модели (фото, пути и запросы не отправляются)",
        "I allow the model weights to be downloaded (photos, paths and queries are not sent)",
    ),
    (
        "Сеть используется только при нажатии «Скачать модель». \
         Пользовательские данные не отправляются.",
        "The network is used only when “Download the model” is pressed. \
         No user data is sent.",
    ),
    ("Скачать модель", "Download the model"),
    (
        "кнопка погашена: нужно согласие на сеть",
        "the button is disabled: network consent is required",
    ),
    ("файлов:", "files:"),
    (
        "Теги и векторы привязаны к модели: после смены модели их нужно посчитать.",
        "Tags and vectors belong to the model: after switching models they must be computed again.",
    ),

    // --- настройки: индексация и дубли (M21.4f) ---------------------------------
    ("Потоков на индексацию", "Indexing threads"),
    ("потоков", "threads"),
    ("Порог дублей", "Duplicate threshold"),
    ("бит из 64", "bits out of 64"),
    (
        "Больше бит — больше похожих снимков в группе. Точные копии \
         находятся при любом пороге.",
        "More bits — more similar photos in a group. Exact copies \
         are found at any threshold.",
    ),

    // --- настройки: теги и словарь (M21.4f) -------------------------------------
    ("Порог фильтра по тегу", "Tag filter threshold"),
    ("теги ещё не посчитаны", "tags are not computed yet"),
    ("Порог уверенного тега (косинус)", "Confident tag threshold (cosine)"),
    (
        "Это про карточку, OCR и {tag} в витрине, а не про фильтр: тег подсвечивается, \
         только если он выше порога и отрывается от второго.",
        "This is about the card, OCR and the {tag} in the showcase, not the filter: a tag is \
         highlighted only when it is above the threshold and clearly ahead of the second one.",
    ),
    ("Отрыв от второго тега", "Margin over the second tag"),
    (
        "Ниже порог — больше тегов и больше ошибок. «Убрать» у тега в карточке \
         скрывает его у одного фото и переживает пересчёт.",
        "A lower threshold — more tags and more mistakes. “Remove” on a tag in the card \
         hides it for one photo and survives a recompute.",
    ),
    ("Словарь тегов", "Tag dictionary"),
    (
        "Метка и промпты живут в БД на каждую модель. Правка промптов не перечитывает \
         фото: векторы уже сохранены, пересчитываются только веса.",
        "The label and the prompts live in the DB per model. Editing prompts does not re-read \
         photos: the vectors are already stored, only the weights are recomputed.",
    ),
    (
        "словарь ещё не засеян — он появится после первого «Посчитать теги»",
        "the dictionary is not seeded yet — it appears after the first “Compute tags”",
    ),
    (
        "выключенный тег не считается и не показывается в фильтре",
        "a disabled tag is not computed and not shown in the filter",
    ),
    (
        "метка по-русски: она видна в фильтре и в карточке",
        "the label in Russian: it is shown in the filter and in the card",
    ),
    (
        "минимальный процентиль показа этого тега",
        "the minimum percentile for showing this tag",
    ),
    ("Убрать из словаря", "Remove from the dictionary"),
    ("промпты: один на строку; 2–3 общих шаблона", "prompts: one per line; 2–3 general templates"),
    ("новая метка, например «Шкаф»", "a new label, e.g. “Wardrobe”"),
    ("промпт: «фотография шкафа»", "prompt: “a photo of a wardrobe”"),
    ("Добавить в словарь", "Add to the dictionary"),
    (
        "затем «Пересчитать теги»: веса пересчитаются по сохранённым векторам",
        "then “Recompute tags”: the weights are recomputed from the stored vectors",
    ),
    ("Пересчитать теги", "Recompute tags"),
    (
        "пересчитать веса тегов по уже сохранённым векторам, без повторного CLIP по фото",
        "recompute the tag weights from the vectors already stored, without running CLIP over the photos again",
    ),
    ("Экспорт JSON", "Export JSON"),
    (
        "словарь без путей и фото — можно перенести на другую машину",
        "the dictionary without paths or photos — it can be moved to another machine",
    ),
    ("Импорт JSON", "Import JSON"),
    (
        "заменить словарь модели файлом; старые веса исчезнут после пересчёта",
        "replace the model dictionary with a file; the old weights disappear after a recompute",
    ),
    ("Вернуть стандартные", "Restore the defaults"),
    (
        "14 общих тегов из кода; ваши правки будут заменены",
        "the 14 general tags from the code; your edits will be replaced",
    ),
    (
        "файл JSON для экспорта/импорта; пусто — <кэш>/photocat/tags.json",
        "the JSON file for export/import; empty — <cache>/photocat/tags.json",
    ),
    (
        "словарь изменён — нажмите «Пересчитать теги»",
        "the dictionary has changed — press “Recompute tags”",
    ),
    ("словарь не сохранён", "dictionary not saved"),
    (
        "тег убран из словаря — его веса исчезнут после «Пересчитать теги»",
        "the tag is removed from the dictionary — its weights disappear after “Recompute tags”",
    ),
    (
        "словарь возвращён к стандартному — нажмите «Пересчитать теги»",
        "the dictionary is back to the default — press “Recompute tags”",
    ),
    ("словарь не сброшен", "dictionary not reset"),
    ("словарь не выгружен", "dictionary not exported"),
    ("не прочитать", "cannot read"),
    ("словарь не загружен", "dictionary not loaded"),
    (
        "CLIP: пересчёт тегов по сохранённым векторам…",
        "CLIP: recomputing tags from the stored vectors…",
    ),

    // --- настройки: лица (M21.4f) -----------------------------------------------
    ("Порог склейки лиц (косинус)", "Face merge threshold (cosine)"),
    (
        "Выше порог — меньше ложных склеек, но один человек может разбиться \
         на группы. Модели SCRFD и ArcFace — ONNX, локально.",
        "A higher threshold — fewer false merges, but one person may split into groups. \
         The SCRFD and ArcFace models are ONNX, local.",
    ),
    (
        "разрешаю скачать веса лиц (фото не отправляются)",
        "I allow the face weights to be downloaded (photos are not sent)",
    ),
    ("Скачать модель лиц", "Download the face model"),
    ("Проверить модель лиц", "Check the face model"),
    ("модель лиц проверена: sha256 совпал", "the face model is checked: sha256 matched"),
    ("модель лиц не готова", "the face model is not ready"),
    ("файлов лиц:", "face files:"),

    // --- настройки: хранилище (M21.4f) ------------------------------------------
    ("Индекс", "Index"),
    ("БД", "DB"),
    ("Кэш", "Cache"),
    ("Модели", "Models"),
    ("Посчитать занятое место", "Compute the used space"),
    ("База данных:", "Database:"),
    ("Превью:", "Previews:"),
    ("Кропы лиц:", "Face crops:"),
    ("Модели:", "Models:"),
    ("Весь кэш:", "Whole cache:"),
    ("Очистка кэша и индекса", "Cleaning the cache and the index"),
    ("Очистить превью…", "Clear the previews…"),
    (
        "только <кэш>/photocat/thumbs; архив не трогается",
        "only <cache>/photocat/thumbs; the archive is untouched",
    ),
    ("Очистить кропы лиц…", "Clear the face crops…"),
    (
        "кропы и строки лиц; после этого «Найти лица» пройдёт заново",
        "the crops and the face rows; after that “Find faces” runs again",
    ),
    ("Сбросить индекс…", "Reset the index…"),
    (
        "строки БД: файлы, теги, OCR, векторы, лица; оригиналы целы",
        "DB rows: files, tags, OCR, vectors, faces; the originals are intact",
    ),
    (
        "В каталог архива photocat не пишет: только строки в БД и превью в кэше. \
         Витрина по датам — отдельная команда с явным подтверждением.",
        "photocat never writes into the archive folder: only DB rows and previews in the cache. \
         The date showcase is a separate command with an explicit confirmation.",
    ),

    // --- настройки: внешние программы (M21.4f) ----------------------------------
    (
        "Пусто — программа ищется в PATH. Путь проверяется запуском при сохранении; \
         без программы отключается только её работа.",
        "Empty — the program is looked up in PATH. A path is checked by running it when saved; \
         without the program only its part is disabled.",
    ),
    ("пусто — из PATH", "empty — from PATH"),
    ("без неё", "without it"),
    ("Проверить и сохранить программы", "Check and save the programs"),
    ("не работает", "does not work"),
    ("программы не сохранены", "programs not saved"),
    ("программы: все берутся из PATH", "programs: all taken from PATH"),
    ("программы сохранены и проверены", "programs saved and checked"),
    ("задано в настройках", "set in the settings"),
    ("из PATH", "from PATH"),
    ("видео: кадр-обложка, dHash, превью", "video: cover frame, dHash, previews"),
    ("видео: длительность и разрешение", "video: duration and resolution"),
    ("OCR: текст с документов и скриншотов", "OCR: text from documents and screenshots"),
    ("скачивание весов модели по согласию", "downloading model weights on consent"),
    (
        "видео в индексе без превью и dHash, ошибка в счётчике; фото не страдают",
        "videos in the index without previews or dHash, an error in the counter; photos are unaffected",
    ),
    ("видео без длительности и разрешения", "videos without duration or resolution"),
    (
        "OCR выключен: кнопка погашена, поиск по тексту пуст",
        "OCR is off: the button is disabled, text search stays empty",
    ),
    (
        "веса модели придётся положить вручную (scripts/fetch-model.sh)",
        "the model weights will have to be placed by hand (scripts/fetch-model.sh)",
    ),

    // --- полный экран, плеер, выбор каталога, очистка кэша (M21.5) ----------------
    ("предыдущий файл — ←", "previous file — ←"),
    ("следующий файл — →", "next file — →"),
    ("ffplay отдельным окном", "ffplay in its own window"),
    ("пауза и продолжение — пробел", "pause and resume — space"),
    ("смотреть видео — пробел", "play the video — space"),
    ("слайд-шоу — пробел", "slideshow — space"),
    ("нет устройства", "no device"),
    ("Стоп видео", "Stop the video"),
    ("остановить и вернуться к кадру", "stop and go back to the poster frame"),
    ("Стоп слайд-шоу", "Stop the slideshow"),
    ("Слайд-шоу", "Slideshow"),
    (
        "пробел — пуск/стоп; листает текущий список",
        "space — start/stop; walks the current list",
    ),
    ("интервал:", "interval:"),
    ("уменьшить", "zoom out"),
    ("увеличить", "zoom in"),
    ("Вписать", "Fit"),
    ("масштаб", "zoom"),
    ("перетаскивание — сдвиг кадра", "drag to move the frame"),
    ("В системном просмотрщике", "In the system viewer"),
    ("видео · нажмите ▶, чтобы смотреть", "video · press ▶ to play"),
    ("загрузка следующего кадра…", "loading the next frame…"),
    ("полный кадр загружается…", "the full frame is loading…"),
    (
        "Текст не распознан. Кнопка «Распознать текст» — в карточке файла.",
        "No text recognized. The “Recognize text” button is in the file card.",
    ),
    ("файл не найден в индексе — просмотр закрыт", "file not found in the index — viewer closed"),
    ("открываю системным проигрывателем", "opening in the system player"),
    ("открываю системным плеером", "opening in the system player"),
    ("не открыть", "cannot open"),
    ("видео не открыть", "cannot open the video"),
    ("кадр не декодирован", "the frame was not decoded"),
    ("видео", "video"),
    (" (со звуком)", " (with sound)"),
    (" (без звука)", " (without sound)"),
    ("встроенный плеер не взялся", "the built-in player could not open it"),
    ("открываю ffplay", "opening ffplay"),
    ("ffplay не запустился", "ffplay did not start"),
    ("системный проигрыватель", "the system player"),
    ("ffplay: видео закрыто", "ffplay: the video is closed"),
    (
        "декодер не успевает за звуком: звук выключен, видео идёт медленнее",
        "the decoder cannot keep up with the sound: sound is off, the video runs slower",
    ),
    ("видео остановилось", "the video stopped"),
    ("видео доиграло", "the video has finished"),
    // Каталоги архива и выбор папки.
    ("Каталоги архива", "Archive folders"),
    (
        "Индексировать можно несколько каталогов. «Добавить каталог…» сразу предлагает \
         индексацию; «Убрать из индекса» удаляет только строки БД — файлы на диске целы.",
        "You can index several folders. “Add a folder…” offers indexing straight away; \
         “Remove from the index” deletes DB rows only — the files on disk are untouched.",
    ),
    ("Каталогов пока нет — добавьте кнопкой ниже.", "No folders yet — add one with the button below."),
    ("Убрать из индекса", "Remove from the index"),
    ("файлы на диске не удаляются", "files on disk are not deleted"),
    ("Добавить каталог…", "Add a folder…"),
    ("Добавить каталог", "Add a folder"),
    ("Индексировать все", "Index all"),
    ("Вверх", "Up"),
    ("Добавить и индексировать", "Add and index"),
    ("Добавить без индексации", "Add without indexing"),
    ("Подкаталогов нет.", "No subfolders."),
    ("каталог добавлен", "folder added"),
    ("каталог убран из индекса", "folder removed from the index"),
    ("каталог не убран из индекса", "the folder was not removed from the index"),
    ("удалено строк", "rows deleted"),
    ("файлы на диске целы", "the files on disk are intact"),
    // Очистка кэша и индекса: подтверждение.
    ("Очистить превью?", "Clear the previews?"),
    (
        "Будут удалены только файлы <кэш>/photocat/thumbs. Сетка покажет «нет превью», \
         пока каталог не проиндексируют заново. Оригиналы не меняются.",
        "Only the files in <cache>/photocat/thumbs are deleted. The grid shows “no preview” \
         until the folder is indexed again. The originals are unchanged.",
    ),
    ("Очистить превью", "Clear the previews"),
    ("Очистить кропы лиц?", "Clear the face crops?"),
    (
        "Будут удалены кропы и строки лиц в БД; группы исчезнут. «Найти лица» заново \
         создаст их. Фотографии и оригиналы не меняются.",
        "The crops and the face rows in the DB are deleted; the groups disappear. \
         “Find faces” creates them again. The photos and originals are unchanged.",
    ),
    ("Очистить лица", "Clear the faces"),
    ("Сбросить индекс?", "Reset the index?"),
    (
        "Будут удалены строки БД: файлы, теги, текст, векторы и лица. Настройки, модели \
         и все файлы на диске остаются. После этого нужна индексация заново.",
        "The DB rows are deleted: files, tags, text, vectors and faces. The settings, the models \
         and every file on disk stay. Indexing is needed again afterwards.",
    ),
    ("Сбросить индекс", "Reset the index"),
    ("индекс сброшен; проиндексируйте каталог", "index reset; index a folder now"),
    ("кэш очищен", "cache cleared"),
    ("очистка не удалась", "cleaning failed"),
    // --- CLI: отчёты и сообщения команд (M21.6) -------------------------------
    ("остановлено: ", "stopped: "),
    ("проиндексировано {}/{} файлов ({}), без изменений {}, ошибок {}, за {} с", "indexed {}/{} files ({}), unchanged {}, errors {}, {} s"),
    ("индексация: {}/{} файлов, ошибок {}", "indexing: {}/{} files, {} errors"),
    ("нужен каталог: photocat index <каталог>", "a folder is needed: photocat index <folder>"),
    ("тип", "kind"),
    ("файлов", "files"),
    ("объём", "size"),
    ("всего: {} файлов, {}", "total: {} files, {}"),
    ("без даты съёмки: {}", "without a capture date: {}"),
    ("без перцептивного хеша: {}", "without a perceptual hash: {}"),
    ("уникальных sha256: {}  (точных копий: {})", "unique sha256: {}  (exact copies: {})"),
    ("уникальных dhash: {}", "unique dhash: {}"),
    ("видео с длительностью: {}, суммарно {}", "videos with a duration: {}, {} in total"),
    ("файлов с распознанным текстом: {}, знаков: {}", "files with recognised text: {}, characters: {}"),
    ("точная копия", "exact copy"),
    ("похожий снимок", "similar shot"),
    ("точные копии — разницы нет", "exact copies — no difference"),
    ("отличия небольшие — выбирайте глазами", "the differences are small — choose by eye"),
    ("резче остальных на {}%", "sharper than the rest by {}%"),
    ("резче остальных", "sharper than the rest"),
    ("больше по разрешению", "more pixels"),
    ("крупнее файл", "larger file"),
    ("групп: {}, файлов в них: {}, потенциально лишнего: {}", "groups: {}, files in them: {}, potentially spare: {}"),
    ("помечено лишними: {}, освободится {}", "marked spare: {}, would free {}"),
    ("#{} [{}] файлов: {}, дублей на {} · совет: {} — {}", "#{} [{}] files: {}, duplicates worth {} · advice: {} — {}"),
    ("… и ещё {} групп (полный список: --csv)", "… and {} more groups (full list: --csv)"),
    ("полный список: {}", "full list: {}"),
    ("план решения: {}", "decision plan: {}"),
    ("кандидатов на текст нет: все снимки с камерой и без тега документа", "no text candidates: every photo has a camera and no document tag"),
    ("распознано {} из {} файлов, знаков {}, ошибок {}, за {} с", "recognised {} of {} files, {} characters, {} errors, {} s"),
    (" (пропущено {}, текст уже есть)", " ({} skipped, text already there)"),
    ("кандидатов: {} из {} снимков; tesseract ({}): {}", "candidates: {} of {} photos; tesseract ({}): {}"),
    ("не найден", "not found"),
    ("OCR: {}/{} файлов, ошибок {} — {}", "OCR: {}/{} files, {} errors — {}"),
    ("просканировано {} из {} снимков, лиц {}, кластеров {}, ошибок {}, за {} с", "scanned {} of {} photos, {} faces, {} clusters, {} errors, {} s"),
    (" (пропущено {}, уже просканировано)", " ({} skipped, already scanned)"),
    ("лиц {}, групп {}, из них закреплено вручную {}, за {} с", "{} faces, {} groups, {} pinned by hand, {} s"),
    ("модель лиц скачана: файлов {}, {}", "face model downloaded: {} files, {}"),
    ("нужны номер группы и имя: faces label --id N --name ИМЯ", "a group number and a name are needed: faces label --id N --name NAME"),
    ("группа {}: «{}»", "group {}: “{}”"),
    ("нужен номер лица: faces split --id N", "a face number is needed: faces split --id N"),
    ("лицо {} отделено в группу {}", "face {} split into group {}"),
    ("не скачана — «Настройки» или faces download --yes", "not downloaded — “Settings” or faces download --yes"),
    ("снимков: {}, ещё без лиц: {}; модель лиц: {}", "photos: {}, still without faces: {}; face model: {}"),
    ("групп лиц нет: запустите photocat faces", "no face groups yet: run photocat faces"),
    ("(без имени)", "(no name)"),
    ("лиц {}, фото {}", "{} faces, {} photos"),
    ("лица: {}/{} снимков, ошибок {} — {}", "people: {}/{} photos, {} errors — {}"),
    ("CLIP ({}): {}посчитано {}/{}, пропущено {}, ошибок {}, за {} с, модель {}", "CLIP ({}): {}computed {}/{}, skipped {}, errors {}, {} s, model {}"),
    ("теги ({}): пересчитано {}/{}, за {} с", "tags ({}): recomputed {}/{}, {} s"),
    ("модель {}: файлов {}, {}", "model {}: {} files, {}"),
    (" (не подошли: {})", " (did not fit: {})"),
    ("загрузка модели CLIP…", "loading the CLIP model…"),
    ("CLIP: {}/{} файлов, ошибок {} — {}", "CLIP: {}/{} files, {} errors — {}"),
    ("кластеров: {} на {} фото, за {} с → {}", "clusters: {} over {} photos, {} s → {}"),
    // --- M15.7: размеченная проверка тегов ---------------------------------------
    ("разметки нет: заполните {} и повторите", "no labels yet: fill in {} and run again"),
    ("строк в разметке: {} · размечено {} · «другое» {} · пропущено {} · не размечено {} · нет в индексе {}", "rows in the labels: {} · labelled {} · “other” {} · skipped {} · not labelled {} · missing from the index {}"),
    ("словарь покрывает {} % снимков; у {} допустима не одна метка", "a dictionary label fits {} % of the shots; {} of them allow a second label"),
    ("top-1: {} из {} = {} % · top-3: {} из {} = {} %", "top-1: {} of {} = {} % · top-3: {} of {} = {} %"),
    ("уверенный тег: {} снимков, точность {} %; уверенно мимо («другое») {}", "confident tag: {} shots, accuracy {} %; confidently wrong (“other”) {}"),
    ("мебель: при {} % найдено {} из {} (порог {})", "furniture: at {} % found {} of {} (threshold {})"),
    ("проверка {} · модель {}", "check {} · model {}"),
    ("нужен каталог: photocat eval --folder DIR --out DIR", "a folder is needed: photocat eval --folder DIR --out DIR"),
    ("в {} нет снимков с тегами: проиндексируйте каталог и посчитайте теги", "no shots with tags in {}: index the folder and compute the tags"),
    ("выборка: {} снимков из {} годных в {} · метки: {} · подсказки: {} · листов: {}", "sample: {} shots of {} eligible in {} · labels: {} · guesses: {} · sheets: {}"),
    ("заполните столбец label в {} (other / skip / метки через |) и повторите с --labels", "fill the label column in {} (other / skip / labels joined by |) and run again with --labels"),
    ("модель {} · {}", "model {} · {}"),
    ("# Кластеры CLIP (M3)\n\n", "# CLIP clusters (M3)\n\n"),
    ("Модель `{}`, файлов с эмбеддингами: {}, кластеров: {}, k={}.", "Model `{}`, files with embeddings: {}, clusters: {}, k={}."),
    ("Отчёт для просмотра владельцем: 12 примеров на кластер.\n", "A report for the owner: 12 examples per cluster.\n"),
    ("## Кластер {} — {} фото", "## Cluster {} — {} photos"),
    ("… и ещё {}", "… and {} more"),
    ("сверка", "comparing"),
    ("проверка", "checking"),
    ("слепок", "manifest"),
    ("{}/{} файлов, ошибок {} — {}", "{}/{} files, {} errors — {}"),
    ("сверка: {}", "comparison: {}"),
    ("     с: {}", "     with: {}"),
    ("файлов: A {}, B {}; совпало по содержимому {}; sha256 проверено {} файлов ({})", "files: A {}, B {}; identical content {}; sha256 checked {} files ({})"),
    ("размер отличается: {}", "size differs: {}"),
    ("размер отличается", "size differs"),
    ("содержимое отличается: {}", "content differs: {}"),
    ("содержимое отличается", "content differs"),
    ("только в A: {}", "only in A: {}"),
    ("только в A", "only in A"),
    ("только в B: {}", "only in B: {}"),
    ("только в B", "only in B"),
    ("ошибок обхода: {}", "scan errors: {}"),
    ("ошибок чтения: {}", "read errors: {}"),
    ("ошибки чтения", "read errors"),
    ("не проверено из-за отмены: {}", "not checked because of the stop: {}"),
    ("пропущено (симлинки/особые): A {}, B {}", "skipped (symlinks/special): A {}, B {}"),
    ("пропущено (симлинки/особые): {}", "skipped (symlinks/special): {}"),
    ("вердикт: {} (за {} с)", "verdict: {} ({} s)"),
    ("найдены расхождения", "differences found"),
    ("каталог расходится со слепком", "the folder does not match the manifest"),
    ("проверка слепка: {}", "manifest check: {}"),
    ("           каталог: {}", "            folder: {}"),
    ("совпало: {}, изменено: {}, нет в каталоге: {}, нет в слепке: {}", "matched: {}, changed: {}, missing from the folder: {}, missing from the manifest: {}"),
    ("нет в каталоге", "missing from the folder"),
    ("нет в слепке", "missing from the manifest"),
    ("изменено", "changed"),
    ("  … ещё {} (показать все)", "  … {} more (show all)"),
    ("слепок {}: {} файлов ({}), пропущено {}, ошибок {}, не проверено {}, за {} с", "manifest {}: {} files ({}), skipped {}, errors {}, not checked {}, {} s"),
    ("отменено: ", "cancelled: "),
    ("не записан: ", "not written: "),
    ("нужен каталог: photocat verify --manifest FILE DIR", "a folder is needed: photocat verify --manifest FILE DIR"),
    ("нужны два каталога: photocat verify A B [--jobs N] [--all]", "two folders are needed: photocat verify A B [--jobs N] [--all]"),
    ("по дням", "by day"),
    ("по месяцам", "by month"),
    ("по событиям (день + место)", "by event (day + place)"),
    ("витрина: {} → {}", "showcase: {} → {}"),
    ("файлов с датой: {}, без даты (не попадут в план): {}, помечено лишними и не включено: {}", "files with a date: {}, without a date (not in the plan): {}, marked spare and left out: {}"),
    ("каталогов витрины: {}", "showcase folders: {}"),
    ("примеры:", "examples:"),
    ("это план (dry-run). Добавьте --apply, чтобы создать ссылки/копии в {}", "this is a plan (dry-run). Add --apply to create links/copies in {}"),
    ("создано: {}, ошибок: {}", "created: {}, errors: {}"),
    ("нужен каталог назначения: organize --out DIR [--apply] [--by day|month|event]", "a destination folder is needed: organize --out DIR [--apply] [--by day|month|event]"),
    ("--by: ожидается day, month или event, получено «{}»", "--by: expected day, month or event, got “{}”"),
    ("выгрузка альбома: {} → {}", "album export: {} → {}"),
    ("каталогов {} · файлов {} · {}; в корзине и не выгружаются: {}; имён с коллизией: {}; слишком длинных путей: {}", "folders {} · files {} · {}; in the trash and not exported: {}; names with a collision: {}; paths too long: {}"),
    ("; пустых каталогов: {}", "; empty folders: {}"),
    ("; такие файлы пропущены", "; those files are skipped"),
    ("это план (dry-run). Добавьте --apply, чтобы скопировать файлы (не ссылки).", "this is a plan (dry-run). Add --apply to copy the files (not links)."),
    ("выгрузка: {}/{} файлов, ошибок {}", "export: {}/{} files, {} errors"),
    ("выгружено {} файлов · {}; пропущено уже выгруженных {} (порядок в цели устарел {}); переименовано из-за коллизии {}; каталогов создано {}; ошибок {}", "exported {} files · {}; {} already exported skipped ({} stale order in the target); {} renamed because of a collision; {} folders created; {} errors"),
    ("; остановлено", "; stopped"),
    ("нужен номер альбома: export --album N --out DIR [--name ШАБЛОН] [--apply]", "an album number is needed: export --album N --out DIR [--name TEMPLATE] [--apply]"),
    ("нужен каталог назначения: export --album N --out DIR [--apply]", "a destination folder is needed: export --album N --out DIR [--apply]"),
    ("папка моделей: {}", "model folder: {}"),
    ("лимит: {} МиБ", "limit: {} MiB"),
    ("модель в порядке: {}", "the model is in order: {}"),
    ("неизвестная подкоманда models: {} (models list|check|download)", "unknown models subcommand: {} (models list|check|download)"),
    ("нет такой модели: {} (см. models list)", "no such model: {} (see models list)"),
    ("нет такого языка интерфейса: {} (en, ru)", "no such interface language: {} (en, ru)"),
    ("не сохранён: {}", "not saved: {}"),
    ("модель: {}", "model: {}"),
    ("потоков: {}", "threads: {}"),
    ("порог дублей: {}", "duplicate threshold: {}"),
    ("тег: уверенный ≥ {}, отрыв {}", "tag: confident ≥ {}, margin {}"),
    ("фильтр по тегу: порог {}% (процентиль)", "tag filter: threshold {}% (percentile)"),
    ("языки OCR: {}", "OCR languages: {}"),
    ("язык интерфейса: {}", "interface language: {}"),
    ("порог лиц: {}", "face threshold: {}"),
    ("внешние программы (пусто в настройках — ищутся в PATH; без программы отключается только её работа):", "external programs (empty in the settings — looked up in PATH; without a program only its own work is off):"),
    ("не работает: {}", "does not work: {}"),
    ("не найдена: {} ({})", "not found: {} ({})"),
    ("зачем: {}; без неё: {}", "why: {}; without it: {}"),
    ("языки OCR: {} — на месте", "OCR languages: {} — present"),
    ("языки OCR: {} — нет {}; установите языковые пакеты tesseract для них", "OCR languages: {} — missing {}; install the tesseract language packs for them"),
    ("  доступны: {}", "  available: {}"),
    ("языки OCR: не проверить — tesseract не найден", "OCR languages: cannot check — tesseract not found"),
    ("модель CLIP: {} · {} · {}", "CLIP model: {} · {} · {}"),
    ("не скачана — смысловой поиск и теги недоступны", "not downloaded — semantic search and tags are unavailable"),
    ("модель лиц: {} · {} · {}", "face model: {} · {} · {}"),
    ("лица: {} на {} снимках, групп {}, ещё без лиц {} из {}", "people: {} on {} photos, {} groups, {} of {} still without faces"),
    ("БД: {} · {} · файлов {}, с текстом {} (знаков {})", "DB: {} · {} · {} files, {} with text ({} characters)"),
    ("неизвестная подкоманда: {}", "unknown subcommand: {}"),
    ("альбомов нет — создайте: photocat albums new --name \"Дача 2024\"", "no albums yet — create one: photocat albums new --name \"Summer 2024\""),
    (" · в корзине {}", " · {} in the trash"),
    ("файлов {} · подальбомов {} · {}{}", "{} files · {} subalbums · {}{}"),
    ("нужно имя: photocat albums new --name \"Дача 2024\" [--parent ID]", "a name is needed: photocat albums new --name \"Summer 2024\" [--parent ID]"),
    ("альбом создан: {} (id {})", "album created: {} (id {})"),
    ("нужно имя: photocat albums rename --id N --name ИМЯ", "a name is needed: photocat albums rename --id N --name NAME"),
    ("альбом переименован: {}", "album renamed: {}"),
    ("нужен родитель: photocat albums move --id N --parent ID [--position K]", "a parent is needed: photocat albums move --id N --parent ID [--position K]"),
    ("альбом перемещён: {}", "album moved: {}"),
    ("нужны пути: photocat albums {} --id N PATH...", "paths are needed: photocat albums {} --id N PATH..."),
    ("альбом «{}»: добавлено {}, уже было {}, файлов {}", "album “{}”: {} added, {} already there, {} files"),
    ("альбом «{}»: убрано {}, осталось {}", "album “{}”: {} removed, {} left"),
    ("будет удалено: {} — альбомов {}, строк файлов {}; файлы на диске останутся. Повторите с --yes.", "will be deleted: {} — {} albums, {} file rows; the files stay on disk. Repeat with --yes."),
    ("удалено альбомов {}, строк файлов {}; файлы на диске не тронуты", "deleted {} albums, {} file rows; the files on disk are untouched"),
    ("  подальбом {} · файлов {} · {}", "  subalbum {} · {} files · {}"),
    ("подальбом {} · файлов {} · {}", "subalbum {} · {} files · {}"),
    ("  [в корзине приложения]", "  [in the app trash]"),
    ("неизвестная команда albums: {} (list [--tree] | new | rename | move | add | remove | delete | show)", "unknown albums command: {} (list [--tree] | new | rename | move | add | remove | delete | show)"),
    ("нужен номер альбома: --id N", "an album number is needed: --id N"),
    ("словарь модели {} — {} тегов", "model dictionary {} — {} tags"),
    ("ок", "on"),
    ("выкл", "off"),
    (", порог {}%", ", threshold {}%"),
    ("файлов {} · промптов {}", "{} files · {} prompts"),
    ("--add: нужна непустая метка", "--add: an empty label is not allowed"),
    ("тег «{}» уже есть в словаре", "the tag “{}” is already in the dictionary"),
    ("--add: нужен хотя бы один --prompt «фотография …»", "--add: at least one --prompt “a photo of …” is needed"),
    ("тег «{}» добавлен (id {}) — пересчитайте веса: photocat embed --retag", "tag “{}” added (id {}) — recompute the weights: photocat embed --retag"),
    ("тег «{}» {}", "tag “{}” {}"),
    ("включён", "on"),
    ("выключен", "off"),
    ("тег «{}» убран из словаря — веса исчезнут после photocat embed --retag", "tag “{}” removed from the dictionary — the weights go away after photocat embed --retag"),
    ("словарь выгружен: {} ({} тегов)", "dictionary exported: {} ({} tags)"),
    ("словарь загружен: {} тегов — пересчитайте веса: photocat embed --retag", "dictionary loaded: {} tags — recompute the weights: photocat embed --retag"),
    ("словарь возвращён к стандартному ({} тегов) — photocat embed --retag", "dictionary restored to the default ({} tags) — photocat embed --retag"),
    ("нужен запрос: photocat search \"торт\"", "a query is needed: photocat search \"cake\""),
    ("текст на фото, запрос «{}»: файлов {} за {} мс", "text on photos, query “{}”: {} files in {} ms"),
    ("запрос «{}»: результатов {} за {} мс", "query “{}”: {} results in {} ms"),
    ("повтор {}: {} мс", "repeat {}: {} ms"),
    ("в среднем: {} мс", "on average: {} ms"),
    // --- названия и описания моделей в каталоге (M21.6, M15.6) ------------------
    ("CLIP ViT-B/32, мультиязычная", "CLIP ViT-B/32, multilingual"),
    ("CLIP ViT-B/32, английская", "CLIP ViT-B/32, English"),
    ("SigLIP base 224, английская", "SigLIP base 224, English"),
    ("SigLIP 2 base 224, мультиязычная", "SigLIP 2 base 224, multilingual"),
    ("русский поиск и теги, лучшее качество", "Russian search and tags, the best quality"),
    ("меньше по размеру, запросы только по-английски", "smaller, English queries only"),
    ("768 признаков, архитектура SigLIP; запросы по-английски", "768 features, SigLIP architecture; English queries"),
    ("768 признаков, тексты на 100+ языках; нужен лимит 400 МиБ", "768 features, 100+ languages; needs a 400 MiB limit"),
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
    fn счётчик_позиции_и_интервал_на_двух_языках() {
        assert_eq!(position(Lang::En, 3, 12), "3 of 12");
        assert_eq!(position(Lang::Ru, 3, 12), "3 из 12");
        assert_eq!(seconds(Lang::En, 5), "5 s");
        assert_eq!(seconds(Lang::Ru, 5), "5 с");
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

    /// `fill` подставляет значения по порядку и не теряет текст, если значений
    /// меньше, чем заполнителей: отчёт остаётся читаемым, а не пустым.
    #[test]
    fn fill_подставляет_значения_по_порядку() {
        let values = ["2".to_string(), "3".to_string()];
        assert_eq!(
            fill(Lang::Ru, "проиндексировано {}/{} файлов", &values),
            "проиндексировано 2/3 файлов"
        );
        // Перевод задаёт порядок слов, а не код.
        assert_eq!(
            fill(Lang::En, "всего: {} файлов, {}", &values),
            "total: 2 files, 3"
        );
        assert_eq!(fill(Lang::Ru, "без значений {}", &[]), "без значений {}");
        assert_eq!(
            fill(Lang::Ru, "лишние значения {}", &["1".to_string(), "2".to_string()]),
            "лишние значения 1"
        );
    }

    /// Заполнители в переводе должны совпадать с оригиналом по числу: иначе
    /// английский отчёт потеряет число или оставит `{}` на месте.
    #[test]
    fn в_переводах_столько_же_заполнителей() {
        for (ru, en) in PAIRS {
            let ru_n = ru.matches("{}").count();
            let en_n = en.matches("{}").count();
            assert_eq!(ru_n, en_n, "разное число `{{}}` в переводе: «{ru}» → «{en}»");
        }
    }

    /// Каждая строка, которую код передаёт в `t()` или `fill()`, есть в таблице:
    /// забытая пара иначе молча показала бы русский текст в английском отчёте.
    /// Проверяются модули, которые печатают отчёты CLI (M21.6).
    #[test]
    fn каждая_строка_отчётов_есть_в_таблице() {
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
                // Строки без кириллицы — не текст, а шаблон из одних чисел.
                if !key.chars().any(|c| c.is_alphabetic() && !c.is_ascii()) {
                    continue;
                }
                if !PAIRS.iter().any(|(ru, _)| *ru == key) {
                    missing.push(format!("{name}: «{key}»"));
                }
            }
        }
        assert!(missing.is_empty(), "нет перевода: {}", missing.join(", "));
    }

    /// Строковые литералы, переданные в `i18n::t(` / `i18n::fill(` вторым аргументом.
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
