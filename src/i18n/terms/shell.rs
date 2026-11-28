//! The window shell: navigation, the top bar, the status and the left panel.

/// The words of this area, English source → Russian text.
pub const TERMS: &[(&str, &str)] = &[
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
];
