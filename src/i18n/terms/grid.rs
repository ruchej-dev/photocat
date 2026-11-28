//! The grid, its chips and the order controls.

/// The words of this area, English source → Russian text.
pub const TERMS: &[(&str, &str)] = &[
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

];
