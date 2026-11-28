//! The trash and integrity screens with their confirmations.

/// The words of this area, English source → Russian text.
pub const TERMS: &[(&str, &str)] = &[
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
];
