# AGENTS.md — the photocat project constitution

*Русский вариант: [AGENTS.ru.md](AGENTS.ru.md).*

`photocat` is a native media-archive cataloguer written in Rust: folder indexing, duplicate and
series detection, date-based arrangement, and later — ML tags by content. The user is the owner of a
family archive (65 GB of photos and videos), not a photographer and not a server administrator.

This file outranks habits and outranks the task. If a task requires breaking a rule — ask the
owner, do not "slightly" break it.

## 1. Hard constraints

- **Originals are untouchable.** No code deletes, renames or moves archive files. Only creating new
  things is allowed: DB rows, previews in the cache, links in the showcase. The only places that
  create files in the user's folders are `organize` and `export`, and only with an explicit
  `--apply` (a date showcase and album copies). One exception, explicitly requested by the owner:
  the app "Trash" marks a file in the DB, and "Empty trash" moves it to the **OS** trash after
  confirmation (`src/recycle.rs`). There is no permanent deletion in the app. The cache
  (`<cache>/photocat/`) is cleaned only by `src/storage.rs`, and only after confirmation; that code
  never touches the archive.
- **Rust + egui/eframe only.** No Electron, Tauri, webview, JS/TS — neither as a dependency nor as
  a build step.
- **Network only for external assets and only with consent.** The app may download models and
  weights (HTTPS) after the owner's explicit consent: a button in the settings plus a size limit.
  Sending any user data anywhere is forbidden: photos, videos, paths, names, dates, tags, queries,
  statistics, telemetry. No automatic download without a click. Everything downloaded is verified by
  sha256 and stored on disk; the offline mode keeps working.
- **Cross-platform.** The target environment is Linux (Arch + Wayland), but the code does not lean
  on the OS: the cache (`db::cache_dir`), symlinks, external processes (`ffmpeg`, `ffprobe`,
  `curl`, `tesseract`) and paths have fallbacks; home paths are never hardcoded in code or UI.
  Platform branches come only in pairs `#[cfg(unix)]` / `#[cfg(windows)]`, never "good enough for
  now".
- **Expensive work goes to the background.** Decoding, hashing, inference, clustering: never on the
  UI thread; progress is visible, the window never freezes, a long pass can be stopped with
  "Stop" and resumed.
- **Heavy operations show a plan first.** `organize` without `--apply` is a dry run; indexing writes
  to the cache only; ML tags go to a report first.
- **License.** photocat is distributed under the Apache License 2.0 (`LICENSE`, `NOTICE`). A new
  dependency must be license-compatible with Apache-2.0 and recorded in `docs/DECISIONS.md`
  together with what we would do without it.

## 2. How to start a session (reading order)

Do not read the code "from scratch" and do not scan the repository. A cheap entry:

1. `plan/STATUS.md` — the `STATE:` line and the "Next" section. The single source of truth about
   where we are.
2. `plan/BACKLOG.md` — tasks in order, each with an acceptance criterion.
3. `docs/ARCHITECTURE.md` — the module map, DB schema, invariants (once per session).
4. Only then — the files the task names, and their direct callers.

If a hint in `plan/STATUS.md` or `plan/BACKLOG.md` is wrong — fix it in the same change. A document
that lies is a defect.

A "continue the project" task has a template: `/next-task` (`.pi/prompts/next-task.md`).

These files are being translated to English; until a translation exists, the Russian original is the
source of truth (see `plan/I18N.md` for the order).

## 3. Code map

| File | Layer | Responsible for | Must not know about |
|---|---|---|---|
| `src/i18n.rs` | L0 | UI language: `Lang`, `t(lang, text)`, the ru→en table | DB, UI, decoding |
| `src/derive.rs` | L0 | sha256, dHash, EXIF (date/GPS/camera), previews, date from the name | DB, UI, indexer |
| `src/tools.rs` | L0 | paths to `ffmpeg`/`ffprobe`/`tesseract`/`curl`, `PATH` lookup, version probing | DB, UI, decoding |
| `src/db.rs` | L1 | SQLite schema, inserts/queries, albums (tree and order), `Progress`, `human()` | egui, image decoding |
| `src/storage.rs` | L1 | used space, cache and index cleanup | archive, egui |
| `src/index.rs` | L2 | `walkdir` traversal, rayon parallelism, writer thread | egui |
| `src/dupes.rs` | L2 | union-find, exact and similar duplicates, CSV | egui |
| `src/embed.rs` | L2 | CLIP embeddings, tags, cosine search, k-means | egui, DB rows |
| `src/ocr.rs` | L2 | text candidates, the `tesseract` pass, writing text to the DB | egui, image decoding |
| `src/faces.rs` | L2 | SCRFD detection, alignment, ArcFace embeddings, face clusters | egui, raw SQL strings |
| `src/organize.rs` | L2 | date plan, hardlink/copy/symlink, album export | egui |
| `src/verify.rs` | L2 | comparing two copies, `sha256sum` manifest | DB schema, egui |
| `src/recycle.rs` | L2 | moving to the OS trash after confirmation, clearing rows | previews, egui |
| `src/player.rs` | L2 | built-in player: `rff` demux, H.264/HEVC/AAC decode, `cpal` sound, frame as RGBA | DB, egui |
| `src/app.rs` | L3 | the whole egui GUI | direct SQL calls |
| `src/main.rs` | L3 | CLI parsing, subcommands | — |

The one-dependency rule: **a module depends only on lower layers**. Checked by
`bash .pi/skills/enforce-architecture/scripts/check.sh`.

## 4. Data

- `tags`: rows `(file_id, tag, score, source)` where `source='clip:<model_id>'`; labels and prompts
  live in `tag_defs` per model (the owner edits them). A changed `sha256` wipes the CLIP tags and the
  file vector.
- DB: `~/.cache/photocat/photocat.db` (SQLite, WAL). Rebuilt by indexing; we never write into the
  archive folder. `files.trashed_at` is the app trash (the file on disk is intact), `user_tags` are
  the owner's own tags. The folder list lives in `settings.roots` as one string; a folder is removed
  from the index by deleting rows, not files. Albums (`albums`, `album_files`) are a tree and a file
  order, also rows only: the name is unique among siblings, `position` is dense, one snapshot is one
  row in an album. Deleting an album clears rows only.
- UI language: `settings.lang` = `en` (main) or `ru` (second). The choice is made in
  "Settings → Interface" and applied to the whole window.
- Previews: `~/.cache/photocat/thumbs/<2 chars>/<sha256[..24]>.jpg`, 320 px on the long side.
- Faces: crops `~/.cache/photocat/faces/<2 chars>/<sha256[..24]>-<N>.jpg`, 112×112; vectors and
  groups are in the DB.
- Models: by default `<cache>/photocat/models/<id>/`, faces — `<models_dir>/faces/` (182.4 MiB);
  path, size limit and model choice are in the app settings. Downloading is consent-only and
  sha256-checked; `scripts/fetch-model.sh` stays the manual fallback for CLIP.
- Cache for 2230 files: DB 1.3 MB, previews 29 MB; after CLIP + OCR + faces the DB is ~22 MB (CLIP
  vectors 4.4 MiB, faces 2498 × 2 KiB), face crops ~11 MB. An order-of-magnitude jump is a
  regression.
- The schema changes only through `CREATE TABLE IF NOT EXISTS` + `ALTER TABLE` in `db::init` with
  data preserved, and `db::SCHEMA_VERSION` is bumped along with it: while the version matches, `open`
  writes nothing, so reads are not blocked by a long run. Indexing 2230 files takes 65 s, but it must
  not be lost because of a schema edit.

## 5. Commands

```bash
cargo build --release           # ~3 min from scratch, seconds afterwards
cargo run                       # dev profile: dependencies at opt-level 3, our code at 1 —
                                # video plays in `cargo run` too (plain debug was 2.2 fps)
cargo clippy --all-targets      # before every commit
bash .pi/skills/enforce-architecture/scripts/check.sh   # layer bounds; the archive stays untouched
./target/release/photocat index <folder>     # indexing (writes to the cache)
./target/release/photocat stats
./target/release/photocat dupes --max-dist 6
./target/release/photocat albums list --tree             # album tree (data only)
./target/release/photocat albums new --name "Summer 2024" # album; add/remove by indexed paths
./target/release/photocat export --album 1 --out DIR     # export plan; --apply — file copies
./target/release/photocat organize --out DIR            # dry run
./target/release/photocat verify A B                    # two copies: size → sha256, exit 0/1
./target/release/photocat manifest DIR --out file.sha256 # snapshot for future checks
./target/release/photocat verify --manifest file.sha256 DIR  # check against the snapshot
./target/release/photocat embed                          # CLIP embeddings and tags (local)
./target/release/photocat embed --refresh                # recompute vectors (input changed)
./target/release/photocat embed --retag                  # tags only, from stored vectors
./target/release/photocat tags [--list|--add TAG --prompt TEXT|--enable ID|--disable ID|--rm ID]
                               [--export FILE|--import FILE|--reset]  # model tag dictionary
./target/release/photocat search "cake" --top 20         # semantic search
./target/release/photocat faces                          # faces: detection, embeddings, groups
./target/release/photocat faces --dry-run                # how many photos have no faces yet
./target/release/photocat faces --list                   # face groups with names and counts
./target/release/photocat faces --recluster --threshold 0.45  # rebuild groups
./target/release/photocat play video.mp4                 # built-in player, headless (benchmark)
./target/release/photocat play video.mp4 --decode-only   # decode only: fps and frame cost
./target/release/photocat faces download --yes           # face weights (network, consent only)
./target/release/photocat faces label --id 5 --name Lisa # name a group
./target/release/photocat faces split --id 123           # split a face out of a group
./target/release/photocat ocr                            # text from documents and screenshots
./target/release/photocat ocr --dry-run                  # how many files OCR would take
./target/release/photocat ocr --file photo.jpg           # text from one photo
./target/release/photocat search "receipt" --text        # search recognized text
./target/release/photocat doctor                         # what is present: ffmpeg, tesseract, langs, model
./target/release/photocat clusters --k 30                # clusters → plan/M3-clusters.md
./target/release/photocat models list                    # model catalogue and what is downloaded
./target/release/photocat models download --id ID --yes  # weights (network, consent only)
./target/release/photocat settings [--lang en|ru]        # model, path, limit, thresholds, language
./target/release/photocat gui                            # GUI (language comes from settings)
# weights: the "Download model" button in Settings (with consent) or by hand:
bash scripts/fetch-model.sh
bash .pi/skills/corpus-bench/scripts/bench.sh            # measure on the real corpus
```

## 6. Change rules

- One commit — one topic; the message is in English, imperative, about the essence, not "fix".
- Documents that describe behaviour (`docs/`, `plan/`) are updated first, then the code — in the same
  commit.
- **Documentation and commit messages are English.** The Russian documentation is kept as the second
  variant (`*.ru.md`) and is linked from the English main file. A document translated to English is
  moved to `NAME.ru.md`, not deleted.
- **Communication with the owner is Russian.** The owner reads Russian; the repository speaks
  English.
- **UI strings are translated, not duplicated.** A Russian source string goes through
  `i18n::t(lang, "…")`; the English text is added to the `PAIRS` table in `src/i18n.rs`. A screen is
  considered translated only when every label on it goes through `t()`. Untranslated screens keep
  showing Russian — that is a visible state, not an error, and `plan/I18N.md` tracks the coverage.
- UI identifiers are English, UI labels are translated; numbers with units always go through
  `db::human()`.
- A new view or panel follows the `add-view` skill. A new screen follows the `ui-design` skill.
- Do not add a dependency until `docs/DECISIONS.md` records why it is there and what we would do
  without it, and that its license fits Apache-2.0.
- New measurements (time, size, group counts) go to `docs/ARCHITECTURE.md` § "Measurements", with a
  date and the corpus. Numbers without a corpus mean nothing.

## 7. How to verify

1. **Headless always**: `index` on a test corpus, `stats`, `dupes`, `organize` (dry run) — they check
   the logic without a single pixel.
2. **Corpus**: the real 2230 files from `~/Documents/nasty/mail_media/Фотографии с телефона` (see
   `corpus-bench`). Expected: indexing ~85 s (photos + video covers via ffmpeg), 103 duplicate
   groups, 238 files, 214 MiB; `embed` over photos and videos ~77 s, 2229 files.
3. **GUI**: compilation + a short run (`timeout 6 ./target/release/photocat gui`). Opening a window
   on the owner's screen without a warning — only for a smoke test and no longer than a few seconds;
   the owner does the real review.
4. Acceptance rule: a change is not ready until the result is measured on real data and written to
   `plan/STATUS.md`.

## 8. Known limitations (state of things, not bugs)

- Videos are indexed with a cover frame, dHash, duration and resolution via `ffmpeg`/`ffprobe`
  (M4). Without a dHash one broken JPEG remains; videos do not take part in CLIP search yet.
- The similarity threshold of 6/64 produces false positives (different photos in one group) — dHash
  alone must not drive automatic deletions.
- HEIC/AVIF: `image` does not read HEIC.
- `hardlink` does not work across filesystems (`/tmp` is tmpfs): build the showcase on the same disk
  or use `--copy`/`--symlink`.
- CLIP tags are approximate: a tag is confident only when the first one leads the second by 0.01;
  "not sure" is a normal state, not a bug. The model needs AVX2 (int8 quantization), 218 MiB. The
  input is two frames per photo (center crop + the whole frame with margins), vectors averaged; this
  removes "document" from the leaders on vertical renders, but "furniture" on its own is not leading
  yet.
- People (M6): SCRFD + ArcFace, threshold 0.45 by default; 2498 faces on 856 corpus photos in 630 s,
  617 groups. The detector occasionally finds a "face" on a screen or a statue and mixes up relatives
  — a false merge is split with the "not him" button, a name pins a group. The face model is
  182.4 MiB. OCR needs `tesseract` in `PATH` and the `rus`/`eng` language packs; without it
  everything else works and the "Recognize text" button is disabled. Out of 435 corpus candidates,
  text was found on 146 — photos without text keep an empty string and are not recomputed.
- `plan/M3-clusters.md` is a local report with archive paths, it is not committed.
- The app trash is not physical: "Empty trash" moves files to the OS trash via `trash`; cache cleanup
  removes only `<cache>/photocat/*`. Both actions require confirmation.
- Video plays in the built-in player (M14, ADR 0030): H.264 and HEVC Main, AAC, sound through
  `cpal`; no external programs needed. Three threads: video, audio, presentation. Audio is the
  master, only while it actually plays; otherwise wall-clock time drives the frames, and video never
  stalls. 1080p holds real time (29.8 fps for a 30 fps source), 2520×1080 — 22.9 fps with up to
  0.5 s of lag; 4K does not play. The frame is downscaled to 1280 px before presentation. Codecs out
  of scope (VP9, Opus, 4:2:2) and broken files go to external `ffplay` if present, otherwise to the
  system player.
- Re-indexing skips files whose path, size and mtime match. If the content changed while the size and
  time were preserved, sha256 is not recomputed — the price of speed.
- Album export does not rename or delete what is already exported: after reordering, a file stays
  under its old number, and a repeated export says "target order is stale: N"; the remedy is a manual
  export to a new folder.
- CLIP tags are hints from a dictionary: 14 general labels with 2–3 templates each by default; the tag
  weight is the best template. Labels and prompts are edited in "Settings → Tags" (`tag_defs` per
  model), recomputed with `embed --retag` without re-running CLIP on the photos. Free-word search
  stays the main way to find a thing.
- Interface language coverage: the shell (navigation, top bar, banner, settings dialog) is
  translated; content panes are translated screen by screen — `plan/I18N.md`.

## 9. Repository skills

| Skill | When |
|---|---|
| `.pi/skills/ui-design` | any screen, panel, grid cell, word in the interface; it also holds the normative layout, interaction and "findable in a second" rules |
| `.pi/skills/add-view` | a new view/tab/filter in the GUI: from the `View` enum to the docs entry |
| `.pi/skills/enforce-architecture` | adding/moving code, a new dependency, before a commit |
| `.pi/skills/corpus-bench` | measuring on the real archive and recording the numbers |
