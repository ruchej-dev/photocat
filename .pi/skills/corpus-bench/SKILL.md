---
name: corpus-bench
description: "Measure photocat on the owner's real archive and record the numbers: indexing time, duplicate group count, cache size, clustering speed. Use before closing a task that touches the index, duplicates, layout or previews, and when numbers are needed for `docs/ARCHITECTURE.md`."
license: MIT
compatibility: pi
metadata:
  audience: agents
  workflow: perf
---

*Русская версия: [SKILL.ru.md](SKILL.ru.md).*

# Measuring on the real corpus

The numbers in `docs/ARCHITECTURE.md` are not decoration: they are what the next session looks at to
tell whether it faces a regression. A measurement is always taken in a release build and always on
the same corpus.

## Corpus

The main one: `~/Documents/nasty/mail_media/Фотографии с телефона` — 2230 files, 21.2 GiB
(1972 images, 258 videos). It is **read only**: not a single measurement command writes into it.

The quick one: 120 images hard-linked into `~/.cache/pctest/imgs` (hard links take no space; they do
not work between `/tmp` (tmpfs) and the home directory — ADR/AGENTS §8).

## Commands

```bash
bash .pi/skills/corpus-bench/scripts/bench.sh            # a full run over the main corpus
bash .pi/skills/corpus-bench/scripts/bench.sh --quick    # 120 files
```

The script builds the release build if needed, indexes into a **temporary** DB (it does not touch
the owner's cache), prints a table and returns: 0 — all measurements were taken.

## A manual run (when one measurement is needed, not a table)

```bash
cargo build --release
D=/tmp/pc-bench.db
/usr/bin/time -f 'indexing: %e s, %M KB RSS' \
  ./target/release/photocat index "$HOME/Documents/nasty/mail_media/Фотографии с телефона" \
    --jobs 16 --db "$D"
./target/release/photocat stats --db "$D"
./target/release/photocat dupes --db "$D" --max-dist 6 --quiet
./target/release/photocat organize --db "$D" --out /tmp/pc-plan | head -12
./target/release/photocat embed --db "$D" --jobs 8 --batch 8
./target/release/photocat search "cake" --db "$D" --top 5
./target/release/photocat clusters --db "$D" --k 30 --out /tmp/M3-clusters.md
./target/release/photocat eval --db "$D" --folder <dir> --out /tmp/tag-eval --sample 120
./target/release/photocat eval --db "$D" --labels /tmp/tag-eval/labels.csv
du -sh "$D" ~/.cache/photocat/thumbs ~/.cache/photocat/models/clip-vit-b32
```

## Expected values (a baseline, not a dogma)

| Metric | Baseline (2026-11-22) | Alarm if |
|---|---|---|
| Indexing 2230 files, `--jobs 16` (incl. video via ffmpeg) | 84.7 s | > 130 s |
| Throughput | ~34 files/s, ~80 images/s | < 15 files/s |
| Indexing errors | 1 (the known broken JPEG `P_20200724_154351.jpg`) | > 1 |
| DB | 1.3 MB | > 12 MB |
| Previews | 33 MB (29 MB photos + 4 MB video covers) | > 100 MB |
| `dupes --max-dist 6` | 103 groups / 238 files / 214.1 MiB | groups < 80 or > 150 |
| Clustering 2229 files, k=30 | 1.0 s | > 5 s |
| Files without a dHash | 1 (the broken JPEG; videos now have a dHash) | > 1 (means video or decoding broke) |
| Videos with a cover, a dHash and a duration | 258/258, 3:19:46 in total | < 250 |
| Files without a date | 0 | > 5% |
| `embed` 2229 files (photos + videos), `--jobs 8 --batch 8` | 77.3 s (34.7 ms/file) | > 200 s |
| `embed` 2340 files, SigLIP 2 base 224 multilingual, `--jobs 8` | 398.8 s (0.170 s/file), one frame; CLIP on the same run 147 s — the machine, not the model, sets the absolute numbers, the ratio is 2.7× | > 600 s |
| `embed` 2340 files, SigLIP base 224 English | 404.5 s (0.173 s/file), one frame | > 600 s |
| `embed --retag`, 2340 files, the vectors already in the DB | 2–4 s (SigLIP 1 2.1 s, SigLIP 2 3.5 s) — no photo is read | > 15 s |
| `eval --labels`, 120 shots of `Фотографии с телефона`, CLIP multilingual (2026-11-24) | top-1 85.4 %, top-3 97.6 %, confident tag 79.7 % | top-1 < 70 % or top-3 < 90 % means the tag logic regressed |
| The CLIP multilingual model (4 files) | 217.5 MiB | the files do not match by sha256 |
| The CLIP en model (3 files) | 148.6 MiB | > 5 s for a check/network |
| The SigLIP base 224 English model (3 files) | 203.5 MiB | the files do not match by sha256 |
| The SigLIP 2 base 224 multilingual model (3 files) | 393.3 MiB, download 83 s (vision 90.2 MiB, text 270.3 MiB — a 256k-token vocabulary, tokenizer 32.8 MiB) | the files do not match by sha256; at the default 250 MiB limit the download is refused on purpose |
| DB with CLIP (2229 files) | 12 MB, vectors 4.35 MiB, 15 603 tags | > 40 MB per corpus |
| A repeated `embed` | 0.03 s, everything skipped by sha256+model | > 5 s |
| Search over 2229 vectors (the model already loaded) | 11 ms | > 200 ms |
| OCR candidates (`ocr --dry-run`), 1972 shots | 435 | > 700 or < 150 |
| `ocr --jobs 8` over 435 candidates (tesseract 5.5.4) | 106.2 s, 434/435, 66 444 characters | > 200 s |
| A repeated `ocr` | 0.0 s, 434 skipped by sha256 | > 5 s |
| Text search (`search --text`), 434 texts | 2–3 ms | > 50 ms |
| DB with CLIP and OCR | 12.7 MB (+276 KiB for text) | > 40 MB |
| `verify` of two corpus copies, `--jobs 8` | 19.9 s, 2230/2230 matched | > 120 s |
| `manifest` of the corpus, `--jobs 8` | 10.1 s, 2230 rows, 193 KiB | > 60 s |
| `verify --manifest` over the corpus | 11.4 s, 2230/2230 | > 60 s |
| `export` 500 files / 2.4 GiB (a 3-level tree), `--apply` | 2.0 s, repeat 20 ms, 0 new | > 30 s or repeat > 5 s |
| `export` 2230 files / 21.2 GiB, `--apply` | 111 ms (the FS gave `copy_file_range`), repeat 45 ms | > 120 s or repeat > 5 s |

## Rules

1. **Release only.** A debug build is 30 times slower on images and spoils the number (48 s against
   1.5 s on 122 files).
2. **One corpus — one row in the table.** "Roughly two thousand" is not allowed.
3. **Record the date and what changed.** A row in `docs/ARCHITECTURE.md` §Measurements:
   `| date | corpus | operation | result |`.
4. **Do not measure on the owner's cache.** A temporary DB in `/tmp`, so his index is not spoiled.
5. If a measurement is worse than the baseline — figure it out first, then write to the plan. An
   unexplained regression is a defect.

## After the measurement

- A row in `docs/ARCHITECTURE.md` §Measurements.
- One sentence in `plan/STATUS.md`: what got better/worse.
- If the meaning of a number changed (for example, videos now have a preview and a dHash) — update
  the baseline in this skill in the same commit, otherwise the skill starts lying.
