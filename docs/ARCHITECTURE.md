# Архитектура photocat

## Слои

```
L3  main.rs ── app.rs            CLI и GUI
L2  index.rs   dupes.rs   organize.rs
L1  db.rs
L0  derive.rs
```

| Слой | Знает про | Не знает про |
|---|---|---|
| L0 `derive` | файлы, `image`, `image_hasher`, `kamadak-exif`, `sha2` | БД, UI, параллелизм |
| L1 `db` | `rusqlite`, `anyhow` | декодирование, egui |
| L2 `index` / `dupes` / `organize` | L0/L1 + `rayon`, `walkdir` | egui |
| L3 `app` | всё ниже + `eframe`/`egui` | прямые SQL-строки, декодирование в UI-потоке |

Граница проверяется скриптом `.pi/skills/enforce-architecture/scripts/check.sh` (grep по `use
crate::`), он же запрещает вызовы `fs::remove_*` и `fs::rename` в `src/`.

## Поток индексации

```
walkdir → Vec<PathBuf> → rayon(par_iter, jobs потоков)
   по файлу: metadata → sha256 → (image: decode → dHash + превью 320) → exif → дата
        → db::FileRow ──mpsc──► поток-писатель: одна транзакция на весь прогон
```

- Один декод на файл: размеры, dHash и превью считаются из одного `DynamicImage` (`derive::analyze_image`).
- `Progress` — атомики + `Mutex<String>`; GUI и консоль читают его, не блокируя воркеров.
- Ошибка одного файла не валит прогон: счётчик `errors`, файл пропускается.

## Схема БД

```sql
files(id, path UNIQUE, size, mtime, sha256, kind, width, height,
      taken_at, taken_src, lat, lon, camera, dhash, thumb, indexed_at)
tags(file_id, tag, score, source)      -- задел под M3
```

- `kind`: `image` | `video` | `other`.
- `taken_src`: `exif` | `filename` | `mtime` — источник даты, чтобы никогда не путать точное с
  догадкой.
- `dhash`: `u64` как `i64` (SQLite INTEGER). `NULL` у видео.
- `thumb`: путь относительно `~/.cache/photocat/thumbs/`, ключ — `sha256[..24]` (одинаковые файлы
  делят одно превью).

Изменение схемы — только через `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE` в `db::open`, данные
сохраняются.

## Дубли

1. Точные: `sha256` + размер → union-find.
2. Похожие: попарное сравнение `dhash` (`hamming ≤ max_dist`), тоже union-find.
3. Группа = компонента связности. `reason`: «точная копия», если у всех одинаковый sha256, иначе
   «похожий снимок». `waste = сумма размеров − размер самого большого`.

Сложность O(n²) по числу изображений с dHash: 1972 файла → 1.9 млн сравнений → 0.03 с. До 20 тыс.
фото это ~200 млн (≈3 с) — приемлемо; выше — BK-tree по 64-битным хешам (задача на будущее).

## Дата

Порядок: EXIF `DateTimeOriginal` → регулярка по имени (`IMG_20240517_144243`, `P_20190410_102958`,
`VID_...`, `V_...`) → `mtime`. Проверка правдоподобия: год 1990–2100, месяц 1–12, день 1–31,
часы < 24. Источник пишется в `taken_src`.

## Раскладка (`organize`)

`ГГГГ/ГГГГ-ММ-ДД/<имя>`; по умолчанию hardlink (0 байт), `--copy`, `--symlink`; коллизии имён
разрешаются суффиксом `-2`, `-3`. Без `--apply` — только план и примеры.

## Замеры

| Дата | Корпус | Операция | Результат |
|---|---|---|---|
| 2026-11-22 | 2230 файлов, 21.2 ГиБ (`Фотографии с телефона`) | `index --jobs 16`, release | 64.9 с, 0 ошибок, БД 1.2 МБ, превью 29 МБ |
| 2026-11-22 | 122 файла (жёсткие ссылки на корпус) | `index --jobs 16`, release | 1.5 с (~80 фото/с) |
| 2026-11-22 | 1972 фото | `dupes --max-dist 6` | 0.03 с, 103 группы, 238 файлов, 214.1 МиБ |
| 2026-11-22 | 122 файла | `index --jobs 8`, **debug** | 48 с — debug-сборка для картинок непригодна |

Правило: числа без корпуса и даты в таблицу не попадают.
