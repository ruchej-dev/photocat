# photocat

Каталогизатор медиаархива на Rust: индексация, поиск дублей и похожих снимков, раскладка по датам, (далее) ML-теги по содержимому.

## Сборка

```bash
cargo build --release      # первый раз ~3 минуты
# бинарник: target/release/photocat
```

## Использование

```bash
# 1. Индексация каталога (читает файлы, пишет БД и превью в ~/.cache/photocat)
./target/release/photocat index ~/Documents/nasty/mail_media/Фотографии с телефона

# 2. Статистика
./target/release/photocat stats

# 3. Дубли и серии (порог — расстояние Хэмминга по 64-битному dHash)
./target/release/photocat dupes --max-dist 6
./target/release/photocat dupes --max-dist 4 --csv /tmp/dupes.csv

# 4. План раскладки по датам (по умолчанию ничего не меняет)
./target/release/photocat organize --out ~/Фото-витрина
./target/release/photocat organize --out ~/Фото-витрина --apply          # hardlink
./target/release/photocat organize --out /mnt/disk --apply --copy        # копии
./target/release/photocat organize --out ~/Фото-витрина --apply --rename # имена из даты

# 5. GUI
./target/release/photocat gui
./target/release/photocat gui --db /путь/к/фотокат.db
```

## Что уже есть

| Возможность | Статус |
|---|---|
| Индексация: sha256, dHash, EXIF (дата, GPS, камера), размеры, превью | ✅ |
| Дата: EXIF → имя файла (`IMG_20240517_144243`) → mtime, с пометкой источника | ✅ |
| Дубли: точные (sha256) + похожие (Hamming по dHash), кластеры через union-find | ✅ |
| Отчёт dupes в консоль и CSV, объём «лишнего» | ✅ |
| Раскладка по датам: план/dry-run, hardlink/copy/symlink, дедуп коллизий имён | ✅ |
| GUI: сетка превью с ленивой загрузкой, фильтры, таймлайн по дням, группы дублей, тултип с крупным превью | ✅ |
| Консольная строка прогресса | ✅ |
| ML: CLIP-эмбеддинги, смысловые теги, поиск «фото с тортом» | ⏳ следующий этап (`ort`/`fastembed`, ONNX, CPU) |
| Лица (кластеры по людям) | ⏳ |
| OCR документов/скриншотов (tesseract) | ⏳ |
| Превью видео через ffmpeg | ⏳ |
| «Лучший кадр из серии» (резкость/экспозиция) | ⏳ |

## Хранение

* БД: `~/.cache/photocat/photocat.db` (SQLite, WAL). Пересоздаётся индексацией, оригиналы не изменяются.
* Превью: `~/.cache/photocat/thumbs/<2 симв.>/<sha256>.jpg`, 320 px по длинной стороне.
* Ничего не удаляется и не переименовывается без явных флагов.

## Замеры на реальном архиве

* 2230 файлов (21.2 ГиБ): индексация **65 с** на 16 потоках, БД 1.2 МБ, превью 29 МБ.
* 103 группы дублей/серий, 238 файлов, «лишнего» 214 МиБ.
* 2.3 млн сравнений dHash — **0.03 с** (в памяти, без БД).
