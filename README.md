# photocat

Каталогизатор медиаархива на Rust: индексация, поиск дублей и похожих снимков, смысловые теги, OCR и лица — всё локально; раскладка по датам.

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
./target/release/photocat dupes --max-dist 6 --plan /tmp/dupes-plan.csv   # план: что оставить

# 4. Витрина (по умолчанию только план, ничего не меняет и не переименовывает)
./target/release/photocat organize --out ~/Фото-витрина
./target/release/photocat organize --out ~/Фото-витрина --by month     # ГГГГ/ГГГГ-ММ
./target/release/photocat organize --out ~/Фото-витрина --by event     # день + место по GPS
./target/release/photocat organize --out ~/Фото-витрина --apply          # hardlink
./target/release/photocat organize --out /mnt/disk --apply --copy        # копии
./target/release/photocat organize --out ~/Фото-витрина --apply --rename # имена из даты
./target/release/photocat organize --out ~/Фото-витрина --exclude-dup    # без помеченных лишними
./target/release/photocat organize --out ~/Фото-витрина --name '{date}_{camera}_{orig}'

# 5. CLIP: смысловые теги и поиск (модель скачивается один раз, дальше всё локально)
bash scripts/fetch-model.sh
./target/release/photocat embed                        # 2229 файлов ≈ 177 с на CPU (два кадра)
./target/release/photocat embed --refresh              # пересчитать векторы после смены входа
./target/release/photocat search "торт" --top 20
./target/release/photocat clusters --k 30              # → plan/M3-clusters.md

# 6. Текст на документах и скриншотах (нужен tesseract с языками rus+eng)
./target/release/photocat ocr --dry-run
./target/release/photocat ocr
./target/release/photocat search "справка" --text

# 7. Лица: детекция, группы, имена (модель 182.4 МиБ — в «Настройках» с согласием)
./target/release/photocat faces --dry-run
./target/release/photocat faces
./target/release/photocat faces --list
./target/release/photocat faces label --id 5 --name Лиза
./target/release/photocat faces split --id 123

# 8. Целостность и бэкап (только чтение, код 0 — совпало, 1 — есть расхождения)
./target/release/photocat verify ~/Фото ~/backup/Фото
./target/release/photocat manifest ~/Фото --out ~/Фото.sha256
./target/release/photocat verify --manifest ~/Фото.sha256 ~/backup/Фото

# 9. GUI
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
| Совет по группе: резкость (лапласиан), разрешение, размер — словами; точные копии — «разницы нет» | ✅ |
| Решение «оставить / лишний» в БД, сводка «помечено лишними M, освободится X», план CSV без удаления | ✅ |
| Раскладка по датам: план/dry-run, hardlink/copy/symlink, дедуп коллизий имён | ✅ |
| GUI: сетка превью с ленивой загрузкой, фильтры, календарь год/месяц/неделя, группы дублей, размер плиток | ✅ |
| Полный экран: двойной клик, мгновенное превью, слайд-шоу, масштаб, весь OCR-текст | ✅ |
| Видео: кадр-обложка, просмотр через ffplay (видео и звук), системный плеер как запасной путь | ✅ |
| Инкрементальная индексация: неизменённые файлы пропускаются (повтор 2230 файлов — 0.02 с) | ✅ |
| Несколько каталогов, встроенный выбор папки, удаление каталога только из индекса | ✅ |
| Консольная строка прогресса | ✅ |
| CLIP-эмбеддинги (ONNX, CPU), смысловой поиск «торт», «море», «документ» на русском | ✅ |
| Теги CLIP (14 подсказок: пляж, документ, еда, мебель, интерьер, город и др.) в карточке и фильтре | ✅ |
| Свои теги вручную, «Посчитать теги» по готовым векторам; лица вместе на одном снимке | ✅ |
| Кластеры CLIP: 30 групп × 12 примеров в `plan/M3-clusters.md` | ✅ |
| Лица: SCRFD + ArcFace, группы, подписи вручную, «не он» для ложных склеек | ✅ |
| OCR документов/скриншотов (tesseract), поиск по тексту с подсветкой | ✅ |
| Превью видео через ffmpeg, dHash и длительность, видео в дублях и CLIP | ✅ |
| Настройки: модель, пороги, пути к ffmpeg/ffprobe/tesseract/curl с проверкой; `doctor` | ✅ |
| «Лучший кадр из серии»: совет по резкости/разрешению/размеру | ✅ |
| Сверка двух копий архива (`verify`, размер → sha256) и слепок `sha256sum` (`manifest`) | ✅ |
| Корзина приложения и очистка в системную корзину ОС по подтверждению; занятое место и очистка кэша | ✅ |

## Продолжить в новой сессии

Проект самодостаточен для передачи другому агенту (или себе через месяц):

| Файл | Зачем |
|---|---|
| `AGENTS.md` | конституция: ограничения, слои, команды, правила изменений |
| `plan/STATUS.md` | `STATE:` — где мы сейчас, что проверено, что дальше, открытые вопросы |
| `plan/BACKLOG.md` | задачи M2–M13 с критериями приёмки |
| `docs/ARCHITECTURE.md` | слои, поток индексации, схема БД, замеры |
| `docs/UI.md` | экраны, метрики, состояния, тексты, клавиатура |
| `docs/DECISIONS.md` | 23 коротких ADR: egui, SQLite, dHash, решения в БД, CLIP/ONNX, лица, внешние программы, неприкосновенность файлов, сверка и слепок |
| `.pi/skills/ui-design` | процедура и рубрика для любого экрана |
| `.pi/skills/add-view` | добавить вид/вкладку/фильтр целиком, с ловушками borrow checker |
| `.pi/skills/enforce-architecture` | машинная проверка слоёв и запретов (`scripts/check.sh`) |
| `.pi/skills/corpus-bench` | замер на реальном архиве + базовая линия (`scripts/bench.sh`) |
| `.pi/prompts/next-task.md` | `/next-task` — взять задачу из плана и довести до цифр |

Порядок входа: `plan/STATUS.md` → `plan/BACKLOG.md` → `docs/ARCHITECTURE.md` → файлы задачи.
Проверки перед коммитом:

```bash
cargo clippy --all-targets
bash .pi/skills/enforce-architecture/scripts/check.sh
bash .pi/skills/corpus-bench/scripts/bench.sh --quick
./target/release/photocat embed --db /tmp/bench.db   # если трогали CLIP
```

## Хранение

* БД: `~/.cache/photocat/photocat.db` (SQLite, WAL). Индекс пересоздаётся индексацией, оригиналы не изменяются; решения по дублям (`decision`) сохраняются, пока у файла тот же `sha256`.
* Превью: `~/.cache/photocat/thumbs/<2 симв.>/<sha256>.jpg`, 320 px по длинной стороне.
* Модель CLIP: `~/.cache/photocat/models/clip-vit-b32/`, 218 МиБ, sha256 проверяется при загрузке.
  В сеть ходит только `scripts/fetch-model.sh`, запущенный вручную; GUI и CLI — никогда.
* Ничего не удаляется и не переименовывается без явных флагов.

## Замеры на реальном архиве

* 2230 файлов (21.2 ГиБ): индексация **60.2 с** на 16 потоках, БД 1.3 МБ, превью 29 МБ.
* 103 группы дублей/серий, 238 файлов, «лишнего» 214 МиБ; резкость посчитана у всех файлов групп.
* 2.3 млн сравнений dHash — **0.03 с** (в памяти, без БД).
* CLIP-эмбеддинги 1971 фото: **66.9 с** на CPU (34 мс/фото), векторы 3.85 МиБ, БД 11 МБ.
  Смысловой поиск по 20 тыс. векторов — **48–51 мс**; повторный `embed` — 0.025 с (пропуск по sha256).
