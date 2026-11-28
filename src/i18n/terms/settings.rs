//! The settings dialog, its panes and the model catalogue.

/// The words of this area, English source → Russian text.
pub const TERMS: &[(&str, &str)] = &[
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

    // --- the titles and notes of the model catalogue (M21.6, M15.6) -----------
    ("CLIP ViT-B/32, multilingual", "CLIP ViT-B/32, мультиязычная"),
    ("CLIP ViT-B/32, English", "CLIP ViT-B/32, английская"),
    ("SigLIP base 224, English", "SigLIP base 224, английская"),
    ("SigLIP 2 base 224, multilingual", "SigLIP 2 base 224, мультиязычная"),
    ("Russian search and tags, the best quality", "русский поиск и теги, лучшее качество"),
    ("smaller, English queries only", "меньше по размеру, запросы только по-английски"),
    ("768 features, SigLIP architecture; English queries", "768 признаков, архитектура SigLIP; запросы по-английски"),
    ("768 features, 100+ languages; needs a 400 MiB limit", "768 признаков, тексты на 100+ языках; нужен лимит 400 МиБ"),
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

];
