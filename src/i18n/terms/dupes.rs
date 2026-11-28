//! Duplicates and series.

/// The words of this area, English source → Russian text.
pub const TERMS: &[(&str, &str)] = &[
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
];
