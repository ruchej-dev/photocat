# STATUS

*Русская версия: [STATUS.ru.md](STATUS.ru.md).*

> From M21 on this file is written in English; the older entries were translated from the Russian
> original in M21.8 and the numbers were kept. See `plan/I18N.md`.

STATE: done:M21.1 (mechanism, shell), M21.2 (grid), M21.3 (file card and inspector), M21.4a (duplicates screen), M21.4b (calendar), M21.4c (faces), M21.4d (trash), M21.4e (integrity), M21.4f (left panel blocks, settings panes), M21.5 (viewer, player controls, folder dialogs, storage confirmation), M21.6 (CLI reports), M21.7a (the architecture document), M21.7b (the interface document), M21.7c (the decisions log), M21.8a (`plan/STATUS.md`), M21.8b (`plan/BACKLOG.md`), M21.8c (the remaining `plan/*.md`), M21.9 (the repository skills), M15.6 (the multilingual SigLIP 2 model and the calibration of the tag weight), M15.7 (a blind labelled check on two folders; ADR 0042), M15.6 tail (the fp16 towers against int8 — int8 costs no tag quality, ADR 0043), M13d (panning the zoomed frame in the full screen), M22.1 (two themes, one token palette; ADR 0044), M22.2 (the main screen after the mock-up: three cell views, 10 px gap, EXIF-oriented previews), M22.2a (the list view scales with the size slider and the slider never disappears), M22.2b (scrolling: a wheel notch is a third of the screen, a trackpad swipe coasts after the fingers lift) → done:M23.1 (English source: 206 identifiers renamed, the table inverted, `tests/source_language.rs` guards the rule) → next:M23.2 (L0/L1), then M22.3 (the left panel: years and tag chips); `plan/M22-ui-spec.md` §6; the owner's re-read of the M15.7 labels runs in parallel; SigLIP 2 stays opt-in and the default stays the multilingual CLIP. Interface language: chosen in Settings, detected from the system locale on the first launch, English when unknown; every visible surface of the window goes through `t()` and every CLI report prints the same language from `settings.lang`. Interface theme: dark by default, light and "follow the system" in Settings → Interface. The mock-up set outside the repository is digested in `plan/M22-ui-spec.md`; screens without a layout follow its rules. Renaming originals was rejected by the owner — §1 AGENTS.md stays unchanged.

Renaming originals was rejected by the owner — §1 AGENTS.md stays unchanged.

## Where we are

Since M23.1 the code speaks English as the repository always claimed: the 206 Cyrillic
identifiers (all test names, plus the album helpers of the `db` tests) are renamed, `PAIRS` is
English → Russian, `i18n::russian(en)` replaced `english(ru)`, `i18n::count` takes a noun key, and
the window shows exactly the same text as before in both languages. `tests/source_language.rs`
guards the rule — a Cyrillic identifier anywhere and a Russian comment or string in a converted
module fail — and its `PENDING` list names the 16 modules that still hold Russian (479 literals,
mostly error messages and data). ADR 0046 reverses ADR 0040's direction and keeps its mechanism.
Next: M23.3 (L2) — M23.2 (L0/L1) is done, and in L2 so far recycle, index, ocr, dupes, eval, faces, verify and organize are English; player, embed, app and main are left, M23.4 (`app.rs`), M23.5 (`main.rs`, tests, scripts), M23.6 (the
corpus pass).

Since M17 the owner chooses how he looks at the archive: "By months" (as in PhotoPrism) or "Flat",
and the order — date ↓ (default) / date ↑ / name / size. Both settings live in `settings`
(`grid_layout`, `grid_sort`) and survive a restart; the order is computed by one function
`App::ordered`, which the grid and the full screen flipping use (the M18 selection uses it too).
Since M18 actions apply to several files at once: Ctrl/Cmd+click adds and removes, Shift+click takes
a range in grid order, Ctrl+A — everything found, Esc and a click on empty space — nothing; a change
of view/filter/search/order drops the selection with the reason in the status. The selection lives
in memory only; it is not written to the settings.

Since M19 there are albums: a tree with subalbums and a file order, DB rows only. Since M20 a ready
album is exported to disk as **copies** of the files: the structure and the order repeat the tree,
nothing is overwritten, a repeat is idempotent. The originals do not change. The chip
"Album “Name” ×" opens and closes the set, "Album order" works only inside an album.
Since M9 there is the "Integrity" screen: two folders, a sha256 manifest and a check without a
terminal; the report is the same as the CLI's, the pass runs in the background and stops with
"Stop". Since M10 the app itself reminds about the due date: the banner "Time to check the archive"
with "Check" and "Remind later", but the heavy pass starts only on a click; the date, the result and
the report of the last pass live in `settings` and `<cache>/photocat/verify-last.txt`. Since M11
similar shots are searched above 20 thousand through a block index of dHash: on the corpus the same
103 groups in 0.017–0.025 s, 50 thousand synthetic — in 0.446 s.
Since M16 the GUI shell is rebuilt after the PhotoPrism model: side navigation in two groups with
counters, search in the top bar, a content panel with filter chips and a tile size, the grid
grouped by months from the newest, an inspector with the file name and "label — value" rows
(ADR 0033). Since M22 the shell is painted from the owner's mock-up (ADR 0044): one token palette with
two themes (dark by default, light and "follow the system" in the settings), radii 4/6/8, amber as the
only signal, monospace numbers, a 32 px top strip with the search and the state, a 28 px bottom strip
with the counters, the working folder moved into the left panel, and a tile card with a 22 px caption
band (name + date), and three cell views — the mock-up's 4:3 tile (default), proportion rows and a
list (M22). Since M13d a zoomed frame in the full screen is dragged with the mouse: at a scale above
100 % a detail is looked at, not the centre of the frame.

`photocat` can: index a folder (sha256 + dHash + sharpness + EXIF + preview), find exact copies and
similar shots, show them in the GUI, suggest the best frame per group, keep the decision
"keep / spare" in the DB and export a CSV plan, arrange by dates with links. Since M3 it also
computes CLIP embeddings locally, searches by meaning with English text ("cake", "sea", "document"),
shows tags in the card and filters by them, collects 30 clusters into a report for review.
Since M6 faces are found locally: SCRFD finds boxes and 5 points, ArcFace gives a 512-dimension
vector, aligned crops are grouped by cosine, a group can be named and split with the "not him"
button.
Since M7 the showcase is configurable: `organize --by day|month|event`, the name from the template
`{date} {time} {camera} {tag} {orig} {ext}` (or the original one), `--exclude-dup` leaves out the
files marked as spare; the originals still do not change — the showcase is a hardlink/copy/symlink.
Since M4 videos are no longer a blind spot: each has a 320 px cover frame, a dHash over the frame,
the duration and the resolution (with the phone rotation taken into account), re-encodes merge into
duplicates.
Since M4b there are "Settings": the model folder, the path and the size limit, consent to download
and a sha256 check; a second (English) model at 148.6 MiB; a tag can be removed from a photo (the
decision is in the DB); the calendar shows a month with day thumbnails and drills down through the
layers year → month → week → day on a double click. Videos now take part in the CLIP search too (the
cover frame).
Since M5 receipts, certificates and book pages are searched by words: `tesseract` walks over
documents and screenshots, the text lies in the DB and is searched by FTS5, a highlighted excerpt is
visible in the card. On the corpus 435 candidates were recognized in 106 s, a text search takes
2–3 ms. In the card a button recognizes only the selected shot; the batch pass is a separate button
with the number of candidates. Indexing, CLIP and OCR stop with the "Stop" button and continue from
the remainder; during a pass the window responds, because reading the DB no longer bumps into DDL
and the writer lock. Since M5c the paths to `ffmpeg`, `ffprobe`, `tesseract` and `curl` are fields
in "Settings": empty — from `PATH`, non-empty — the given file; the path is checked by running the
version and is not saved if it does not work. `doctor` shows the version, the file found, the source
of the path and what breaks without the program. Since M8 the archive can be compared with a copy:
`verify A B` compares sizes, and counts sha256 only on a match, `manifest` writes a manifest in the
`sha256sum` format, `verify --manifest` checks against it. Both commands only read the archive.

## M17 — the grid layout and the sort order (2026-11-23)

The owner asked for a switch "as now — by dates / a flat grid, as before" and for sorting.
Done in `src/app.rs` and `src/db.rs`; the DB schema did not change (`settings` is key/value), the
data and the originals were not touched.

- **Switches in the view header**, next to "Tiles N": "By months / Flat" and the "Sort" menu with
  "Date: newest first" (default), "Date: oldest first", "Name", "Size: largest first". The fifth
  option "Album order" appeared with albums (M19).
- **"Flat"** — one continuous grid without month headers. **"By months"** — the former headers
  "May 2024 · N files · size"; with "oldest first" the months run from the old to the new.
  "No date" is the last group in both directions.
- **The order is total and stable:** the key is the date (or the name, or the size), then `id`, so
  equal dates do not swap between frames. One function `App::ordered`; the grid and the flipping in
  the full screen take the same one, there is no second sorting place.
- **A semantic search sets the order itself:** while there are `semantic_hits`, sorting and grouping
  are muted with the hint "the search sets the order", the results are a flat list by relevance, the
  chip "by relevance: N files" with its "×" drops the search.
- **The settings survive a restart:** `settings.grid_layout` (`months|flat`),
  `settings.grid_sort` (`date_desc|date_asc|name|size_desc`); an unknown value from an old DB — the
  default value, without a crash. `SCHEMA_VERSION` was not bumped.
- **Documents:** `docs/UI.md` (the switches, the single order, unavailability during a search),
  `.pi/skills/add-view/SKILL.md` (the display settings — in `settings`, the data of a view — in
  `App`).

Checks: `cargo test` — 118 passed (was 112; the new ones: the flat order by date in both directions,
"No date" at the end, the reversal of the months, name and size, the order = relevance during a
search, the settings roundtrip in `db`, a window frame without month headers in "Flat"); `cargo
clippy --all-targets` — clean; `enforce-architecture` — 39 checks, 0 violations; `timeout 6
./target/release/photocat gui --db /tmp/photocat-m17.db` (a copy of the real DB, 2230 files) — code
124, no crashes. No logic measurements are required: the layout does not change indexing, duplicates,
CLIP, OCR and faces.

Tails: the month header is still not pinned while scrolling (sticky is a separate task), in the
calendar the order stayed chronological — the sorting concerns the "All files" view.
Multiple selection of tiles was a separate task then — done in M18.

## M18 — multiple selection of tiles (2026-11-23)

The owner: without selecting several files an album (M19) and any action over a group stay work on
one file at a time. Done only in `src/app.rs`; the DB schema, indexing, duplicates, CLIP, OCR,
faces, the player and the trash were not touched, the originals did not change.

- **Clicks:** a plain one — one file, Ctrl/Cmd — add/remove, Shift — a range by `App::ordered`
  (not by ascending id: the newest are on top on the screen), a click on empty space and Esc — nothing,
  Ctrl+A — everything found. The kind of a click is read by one function `pick_kind`.
- **Visible without hovering:** a picked tile has an accent outline and a tick in a circle; the count
  "picked N · size" — in the chips row (through `db::human()`), next to it "Pick all (M)" and
  "Drop the selection". In the inspector — the last clicked file and the line "N picked"; in the
  status on the right — "Picked N · last: name".
- **Another set drops the selection:** `PickScope` (view, query, type, tag, search, order, layout)
  is compared once per frame; a change of any field clears `picked` and writes the reason
  ("selection dropped: the path/date filter changed"). "Pick all" takes only what is visible now.
- **Memory only:** `App.picked: BTreeSet<i64>` + `pick_anchor`; nothing is written to `settings`,
  after a restart it is empty. `reload` keeps only the files that are still in the index.

Checks: `cargo test` — 124 passed (was 118; the new ones: Ctrl adds/removes, Shift takes a range in
order, a reversed range gives the same set, modifiers → action, "pick all" takes only what is
visible, Esc drops the selection through a headless window frame, a change of filter/order/view
drops it with a reason, the chips row and the card show the count); `cargo clippy --all-targets` —
clean; `enforce-architecture` — 39 checks, 0 violations; `timeout 6
./target/release/photocat gui --db /tmp/photocat-m18.db` (a copy of the real DB, 2230 files) — code
124, no crashes. No logic measurements are required: the selection does not touch indexing,
duplicates, CLIP, OCR and faces.

Tails: arrows/Home/End and a rubber-band rectangle with the mouse are not assigned (outside M18); the
selection lives only in the grid — "Duplicates", "Faces" and "Trash" kept their one-file actions.

## M19 — albums: a tree with an order, data in the DB only (2026-11-23)

The owner builds a tree of albums ("Dacha 2024 → May, June", "Lisa → 1 year") so that it turns into
a ready physical folder. Done in `src/db.rs`, `src/main.rs`, `src/app.rs`, `src/storage.rs`; schema
4 → 5, the data and the originals were not touched.

- **Schema:** `albums(id,parent_id,position,name,name_key,note,created_at,updated_at)` and
  `album_files(album_id,file_id,position,added_at)`; `parent_id=0` — the root, the name is unique
  among siblings case-insensitively, the order is dense 0..n−1 for files and subalbums, one snapshot
  is one row in an album (`PRIMARY KEY(album_id,file_id)`), different albums are allowed. Only
  `CREATE TABLE IF NOT EXISTS`, `SCHEMA_VERSION` 5.
- **`db`:** `create_album`, `rename_album`, `move_album` (a cycle is forbidden), `delete_album`
  (recursively, rows only), `add_to_album` (idempotent), `remove_from_album`,
  `move_in_album`, `albums_tree`, `album_items`, `albums_of_file`/`album_refs_of_file`,
  `album_path`. Deleting `files` rows (an index reset, "remove a folder") clears `album_files` and
  compacts the order — there are no dangling references, `PRAGMA foreign_keys` was not enabled.
- **CLI:** `albums list [--tree] | new | rename | move | add | remove | delete | show`; paths are
  the `files.path` key, files are not opened. `delete` without `--yes` shows the contents and exits
  with code 2; a cycle/a repeated name — code 1; success — 0.
- **GUI:** the view "Albums (N)" in the "LIBRARY" group, a tree with indents and triangles,
  "+ New album / + Subalbum / Rename / Move to… / Up / Down / Delete", tiles of subalbums and the
  `draw_cell` grid in the album order. "In the album / All files" — a temporary look to pick the
  shots (M18) and press "Add picked"; the chip "Album “Name” ×". In the card — the "Albums" row with
  "×" (removes the row only). "Export to disk…" was muted at the time of M19; since M20 it works
  (see § M20).
- **Order:** `db::GridSort::Album` ("Album order") is in the menu only inside an album; outside it
  the sorting returns to date ↓. The grid, the full screen and the selection take the order from one
  function `App::ordered`. A change of album/mode drops the selection with a reason.

Checks: `cargo test` — 135 passed (was 124; the new ones: a 3-level tree and the counters, the order
and the renumbering, uniqueness among siblings, a refusal on a cycle, recursive deletion,
idempotency, an empty/long/repeated name, the trash, the cleanup when a file is deleted, the 4→5
migration, the chip and the album order, a window frame with the tree and the buttons); `cargo
clippy --all-targets` — clean; `enforce-architecture` — 39 checks, 0 violations. Migration of a copy
of the real DB (2230 files): version 5, the files intact, the DB 25 403 392 → 25 411 584 B.
`albums add` of 500 files into a 3-level tree — 25 ms, a repeat — 19 ms ("500 were already there"),
the DB +28 KiB, the positions 0..499. `timeout 6 ./target/release/photocat gui --db
/tmp/photocat-m19-gui.db` — code 124, no crashes.

Tails at the time of M19: the export to disk — closed in M20; "Move to…" in the GUI puts the album at
the end of the list (`--position` exists only in the CLI); the order of subalbums on disk will be
alphabetical (the owner's decision, M20); drag&drop, "smart" albums by a filter and a handmade cover
were not done.

## The queue after M16 (2026-11-23)

The owner asked for: a switch "as now — by dates / a flat grid, as before", sorting by dates, his own
folders as data in the DB (not on disk) and an export of such a folder to disk with a decision on
name collisions. Then he specified: the folders are **"Albums"** with **nesting**, the purpose of an
album is a **physical folder with ordered data**, the export is **files, not links**, the structure
on disk repeats the album structure. The request about renaming files on disk was withdrawn by the
owner.
The owner's rule: **all operations are on data, not on files; files are touched only by special
tools** (a plan → a confirmation → a report).

| ID | What | Depends on | Document |
|---|---|---|---|
| M17 | "By months / Flat" and sorting (date ↓/↑, name, size) ✅ | — | `plan/BACKLOG.md` § M17 |
| M18 | Multiple selection of tiles: Ctrl/Shift, "pick all", a counter ✅ | M17 | § M18 |
| M19 | Albums in the DB: a tree with nesting, a file order, rows only | M18 | `plan/M19-albums.md` |
| M20 | Export of an album as a physical folder: the album structure, file copies, name collisions | M19 | `plan/M19-albums.md` §6 |

**Words.** The owner decided (2026-11-23): not "folders" but **"albums"** — "Folders (N)" in the
interface are already taken by the archive folders (`settings.roots`), hence `albums` in the code and
"Albums" on the screen. The old "album = a calendar day" is cancelled: a day stays a day, an album is
an explicit set of files.

**The export is copies, not links.** Links stay with `organize` (the showcase); `export` has no link
mode: the goal of an album is a ready folder that can be handed over or uploaded without the archive.

**Renaming originals is cancelled.** The owner first asked for renaming files on disk, then withdrew
it (2026-11-23): §1 AGENTS.md does not change, `fs::rename` is still absent from `src/`, no exception
appears in `check.sh`. A mass edit of names stays outside the application.

## Measured on real data (2026-11-22)

Corpus: `~/Documents/nasty/mail_media/Фотографии с телефона`, 2230 files, 21.2 GiB (1972 photos,
258 videos).

| Metric | Value |
|---|---|
| Indexing (`--jobs 16`, release) | 64.9–69.3 s, 0 errors (before M4) |
| Indexing with video (`--jobs 16`, release, M4) | 84.7 s (+20 s for 258 videos), 1 error — the known broken JPEG |
| Videos with a cover and a dHash | 258/258, total duration 3:19:46, ffmpeg failures 0 |
| DB without CLIP | 1.3 MB |
| DB with CLIP vectors and tags | 11 MB (vectors 3.85 MiB, tags 13 797 rows) |
| Previews (320 px) | 29 MB of photos + 4 MB of video covers = 33 MB |
| The CLIP model (4 files, sha256) | 217.5 MiB |
| CLIP embeddings of 1971 photos (`--jobs 8 --batch 8`) | 66.9 s (34 ms/photo), 0 errors |
| A repeated `embed` | 0.025 s, 1971 skipped by sha256 |
| A search over 20 120 vectors | 48–51 ms per query (including 40 MiB of BLOBs from SQLite) |
| Clusters (`--k 30`) | 1.6 s, 30 clusters × 12 examples |
| A confident tag (0.22 + a lead of 0.01) | for 1122 of 1971 photos: children 540, animals 202, document 170, screenshot 119, food 54, holiday 23, beach 14 |
| Duplicates/series at `--max-dist 6` | 103 groups, 238 files, 214.1 MiB "spare" |
| CLIP embeddings of photos + videos (`--jobs 8 --batch 8`) | 2229/2229 in 77.3 s (34.7 ms/file), 0 errors, 15 603 tags, the DB 12 MB |
| The search "cake" over 2229 vectors | 11 ms, videos and photos with a cake on top |
| Clusters of 2229 files, k=30 | 1.0 s |
| Downloading `clip-vit-b32-en` (148.6 MiB, 3 files) | 18.8 s, sha256 matched; the 100 MiB limit rejects both models, the 160 MiB limit picks the English one |
| Files without a dHash | 1 (the broken JPEG; videos now have a dHash) |
| Files without a date | 0 (EXIF 92%, the rest — the file name/mtime) |
| OCR candidates (`ocr --dry-run`) | 435 of 1972 shots (no EXIF camera or a confident "document"/"screenshot" tag), 1 ms |
| OCR of 435 candidates (`--jobs 8`, tesseract 5.5.4 rus+eng) | 106.2 s (0.24 s/file), 434/435 — 1 broken JPEG, 146 with text, 66 444 characters |
| OCR at `--jobs 4` | 169.8 s — 1.6 times slower, the same text |
| A repeated OCR | 0.0 s, 434 skipped by sha256 |
| A text search (434 texts) | 2–3 ms per query ("sberbank", "invoice", "2023") |
| The DB before / after OCR | 12.4 → 12.7 MB (+276 KiB: the text and FTS5) |
| OCR quality by eye | a Sberbank receipt, an invoice and a book page are read in full; a price list with glare — partially |
| `verify` of two copies of the corpus (`--jobs 8`) | 19.9 s, 2230/2230 matched, 2 × 21.2 GiB read, 0 errors |
| `verify` of a copy with 1 changed, 1 appended, 1 deleted and 1 extra file | 16.1 s; matched 2227, size 1, contents 1, only in A 1, only in B 1; code 1 |
| `manifest` of the corpus (`--jobs 8`) | 10.1 s, 2230 rows, 193 KiB, 0 errors |
| `verify --manifest` over the corpus | 11.4 s, 2230/2230 matched; `sha256sum -c` reads the same file |
| `verify --manifest` over a changed copy | 8.9 s; matched 2227, changed 2, missing in the folder 1, missing in the manifest 1; code 1 |

A search check by eye: "cake" → the first frame is a pie on a board; "document" → the first photo
of a page with the task text. The tags "children"/"animals" on the control frames matched the
contents.

## Interface (2026-11-23): the shell after the PhotoPrism model

The analysis of the current interface, the options and the choice — `plan/M16-interface.md`; the
decision — ADR 0033. Only `src/app.rs` changed; the DB schema, indexing, duplicates, CLIP, OCR,
faces, the player, the trash and the network were not touched.

What changed:

- **The frame has one style:** step 8×6, button 10×4, target 32×26, radius 4 px, shadows off, the
  palette comes from `visuals()`, the accent is the same. Panel sections and cards without
  `Frame::group` — flat.
- **The left panel:** two groups — "LIBRARY" (All files, Calendar, Faces) and "TOOLS" (Duplicates and
  series, Trash); an item has its number on the right (files, groups, faces, the trash). The
  "Filter" section: path/date and a single choice "All / Photos / Videos" instead of the "images
  only" checkbox. The tile slider moved from the panel to the view header — the panel no longer
  scrolls when the selection is empty (the budget is kept).
- **The content panel:** the view title 18 px, the facts 12 px, on the right "Tiles N" and the
  slider; below, a chips row of the active filters with "×" and "found N of M". A chip removes only
  its own filter; the row is always there, so the header does not jump.
- **The grid:** groups by months from the newest to the oldest, the header "May 2024 · 132 files ·
  5.4 GiB", inside a month the newest on top, "No date" is the last group. The tile step is 2 px,
  hovering gives an outline and a cursor. The order lives in the drawing: `filtered()` still returns
  row indices.
- **The inspector:** a preview, the file name 14 px, the path 11 px, the actions, then "label —
  value" rows (a 72 px column) instead of a stack of "label above value".

Checks without a window: `cargo clippy --all-targets` clean; `cargo test` — 112 passed (the new test
`сетка_группирует_по_месяцам_от_свежих` runs a full window frame through
`eframe::Frame::_new_kittest`, checks the order of the months, the order inside a month and "No
date"); `enforce-architecture` — 39 checks, 0 violations. A short run
`timeout 6 ./target/release/photocat gui --db /tmp/photocat-ui.db` (a copy of the real DB, 2230
files) — code 124, no crashes. At 1240 px: 1240 − 260 − 280 ≈ 700 px of the centre − 2×8 margins;
four 150 px tiles + three 2 px gaps = 606 px — 4 columns remain. No logic measurements are required:
the layout does not change indexing, duplicates, CLIP, OCR and faces.

Tails: the month header is not pinned while scrolling (`ScrollArea` in egui does not give a sticky
row without manual drawing); there is no multiple selection of tiles — that is a separate task, not
a hidden feature. The keyboard shortcuts (Ctrl+I, Ctrl+F, F5, arrows) are still in `docs/UI.md`
§Debt.

## Interface (2026-11-22): three columns and "Settings" in a dialog

Rebuilt after the owner's mock-up (`photocat_2.html`): the top bar in two rows (the folder,
"Index", "Refresh", "Settings"; the "Semantic / By text" switch, the query, "Find", "Reset"), on the
left — the application map (4 views, a filter, the view context, the model, the text, a pinned
"Summary"), in the centre — the grid with the view title and the counter, on the right — "File
details" with a 4:3 preview, metadata, tags and text, below — a status row with the selected file.
"Settings" became a 760×620 dialog with six sections (model, indexing, tags, faces, storage,
programs) instead of a separate view. The right panel is not drawn when the window is narrower than
1200 px — the card moves to the left so that the grid keeps 4 columns.

Checked by a run on the corpus (2230 files, the DB 22 MB): the grid, the selection, the card with a
preview, tags and the recognition button; the view "Duplicates and series" (103 groups, 214.1 MiB);
the "Settings" dialog with the sections; the mode without the right panel. `cargo clippy
--all-targets` — no remarks, `enforce-architecture` — 33 checks ok. No logic measurements are
required: the layout does not change indexing, duplicates, CLIP and OCR. The decision is recorded in
`docs/DECISIONS.md` 0024, the rules — in `docs/UI.md`.

Tails: in the mock-up "Settings" opened with a gear — the word "Settings" was kept, an icon without
a label is forbidden by the findability rules. The pass progress moved to the third row of the top
bar so that the buttons do not shift when it starts. The keyboard shortcuts (Ctrl+I, Ctrl+F, F5,
arrows) are still not done — they are in `docs/UI.md` §Debt.

## M13 — the owner's remarks after a review (2026-11-22)

Closed in `src/app.rs`, `db.rs`, `embed.rs`, `faces.rs`, the new `recycle.rs`/`storage.rs`.

- **The full screen**: a double click on a tile and a button in the card; a 1920 px frame is decoded
  in the background, ←/→, a 3 s slideshow, zoom, the whole OCR text next to it, "Watch the video" —
  ffplay with sound, "Stop the video" — close the player.
  Closing returns to the same view/filter/day/group; the history is not reset.
- **Faces**: a click on a face leads to the calendar day (or to all files with a selection if there
  is no date); "pick" on crops + "Name the picked people"; group checkboxes + "Photo where everyone
  is together (N)".
- **The right panel** closes with "Hide" and comes back with "Details"; the calendar is no longer
  cut.
- **The calendar**: "Year / Month / Week", navigation with the arrows, a click on a month/day changes
  the scale.
- **Duplicates/tags**: labels for "keep", "Export the plan", "Compute tags"; 14 CLIP hints
  (furniture, interior, clothes, nature, city, transport, plants); "Compute tags" from the ready
  vectors; own tags in the card and in the filter.
- **Folders**: the button "Folders (N)", the built-in folder picker, several roots in one indexing,
  "Remove from the index" without deleting files.
  a confirmation; the cleanup goes through the OS trash (`trash`), then the index rows.
- **Storage**: the sizes of the DB/previews/faces/models on a button; a cleanup of
  previews/faces/the index with a confirmation; `storage.rs` deletes only inside `<cache>/photocat/`.
- **Tiles/scrolling**: a 100–260 px slider, textures load 8 per frame, smooth scrolling animation is
  on.

Checked: `cargo test` — 79 passed, 3 ignored, `clippy` — clean, `check.sh` — 38/0. Migration of a
copy of the real DB (22 MiB, 2230 files) from schema 2 to 4: the data is intact, `trashed_at` and
`user_tags` appeared. `timeout 6 gui` on the copy — code 124, no crashes. The owner reviews the
pixels and the gestures.

Tails: above 100% zoom there is no panning; own tags are manual; video — the external player; there
were no separate albums then (an album = a calendar day); there is no model cleanup (consent only).
Albums as explicit sets — M19, `plan/M19-albums.md`.

## M13b — fixes after a review (2026-11-22)

The owner reviewed M13 and found: the full screen loads for seconds, videos have no preview and do
not play, the calendar is cut on the right, there are two tag buttons and photos disappeared by
tags, an empty day from "Faces", it is unclear how to index an added folder, indexing is slow.

- **The full screen.** The 320 px preview from the cache is shown at once (without waiting), the
  1920 px frame is loaded on top of it. The selected file is decoded in advance in the background
  (`prefetch_photo`), fast ←/→ drop the old stream and do not wait for it. A measurement on a
  108 MP JPEG (36 MB, 12032×9024): the former full decode + thumbnail — 1.33 s, the new TurboJPEG
  1/4 path — 0.54 s; the 320 px preview is visible at once. On 12–14 MB — about 0.2–0.3 s.
- **Video.** The old DB did not know about covers; the incremental indexing recomputes 258 videos and
  creates 258/258 covers + the duration in 10.2 s. In the full screen "Watch the video" opens
  `ffplay` — video and sound in a separate window, all ffmpeg codecs. There is no manual frame
  stream and no separate audio stream any more.
- **The calendar.** The cells were counted without `item_spacing`, so 7 columns ran over to the
  right and hid the counter. The grid step is fixed at 4 px, the width is counted from it; the week
  header too. The right edge and the file count are in place now.
- **Tags.** One button "Compute tags" is left: it computes the missing embeddings and then recomputes
  the labels over all ready vectors. The tag filter now takes **any** weight above the threshold,
  not only the top one: with 14 hints a photo with children no longer disappears from "children" if
  "clothes" turned out slightly higher.
- **Faces → the calendar.** The transition resets the old search/filter, which could hide the shot;
  the day opens with the photo.
- **Folders.** The field on top is filled with the first folder. The folder picker has "Add and
  index" and "Add without indexing"; after adding, the index starts by itself.
- **Indexing.** A repeated pass does not read unchanged files: the key is path+size+mtime, plus a
  recount of videos without a cover/duration. On a copy of the real DB: 1971 photos skipped, 259
  files processed in 10.2 s; a repeated pass — 0.024 s. The threads are rayon, the DB write is a
  separate writer, not on the UI thread.

Checked: `cargo test` — 79 passed, 3 ignored, `clippy` — clean, `check.sh` — 38/0. The measurements
are in the § below.
Left: panning above 100% zoom; video seeking and exact sound sync; models are not cleaned.

## M13c — smooth flipping and the slideshow (2026-11-22)

The owner: when flipping there is first a blurry version, then a sharp one; imv flips smoothly. Done:

- One background decoder with a queue: the current, the next, the previous and +1; a cache of up to
  4 RGBA frames, a queue of up to 8 jobs. When flipping, the next frame is usually ready.
- The blur is gone: the 320 px thumbnail is shown only in the centre and is not stretched; while
  there is no next frame, the **previous sharp** one stays on the screen with the note "loading the
  next one".
- Along the way: JPEG on Linux through the TurboJPEG DCT scale (108 MP: 1.33 s → 0.52 s), a preview
  at once, the selected frame is preloaded on a click/hover.
- The slideshow: an interval of 3/5/10 s, space — start/stop, in a loop, the next frame is flipped
  only after decoding, so the transitions are sharp. ←/→ reset the timer.
- Video: "Watch the video" launches `ffplay` — video and sound in a separate window; the codec and
  seeking inside ffplay, without a manual frame stream.

Checked: `cargo test` — 79 passed, 3 ignored, `clippy` — clean, `check.sh` — 38/0. The TurboJPEG
measurements are in § M13b and `docs/ARCHITECTURE.md`.

## M14 — the built-in video player (2026-11-23)

The owner demanded a built-in player: cross-platform and without external dependencies. ADR 0029
("video — the external ffplay") is cancelled by ADR 0030; the dependencies are recorded in ADR 0031.

**What was done.** `src/player.rs` — L2 without egui: demux `rff-format-mp4/mkv`, decode H.264
(`rusty_h264`), HEVC (`rusty_h265`), AAC (`rff-codec-aac`), sound `cpal`. Two threads: the decode
holds a queue of 3 frames, the presentation goes by the clock (the master is the sound, otherwise the
wall clock). The frame is converted to RGBA with integer arithmetic by the container matrix, with
rotation; in the GUI it is drawn as a texture in the full screen. "Watch the video", "Pause"/"Play",
"Stop the video", the "0:12 / 0:22" bar with seeking, the volume slider. `ffplay` stayed a fallback
if the player refused the codec.

**Checked on the corpus** (release; the machine under someone else's load, load ~10, governor
powersave — the numbers are understated):

| Metric | Value |
|---|---|
| Decoding 640×480 Baseline | 244.9 fps (conversion 0.8 ms/frame) |
| Decoding 1280×720 Baseline | 58.0 fps (1.6 ms) |
| Decoding 1920×1080 Baseline | 36.4 fps (4.6 ms) |
| Decoding 2520×1080 High | 23.6 fps (3.5 ms) |
| Decoding HEVC Main 1920×1080 | 26.3 fps (2.8 ms) |
| Showing 1920×1080 with sound | 29.5 fps at 30 fps of the source, the sound plays, no lag |
| Showing 1280×720 (a 9 fps source) | 9.2 fps — exactly real time |
| Showing 2520×1080 with sound | 11.6 fps: the sound is the master, frames are dropped (the decode of 23.6 < 30) |
| The same file without sound | 22.9 fps — a slowed video instead of a black screen |
| A seek to 10 s | the first frame 0.11 s, the position 10.01 s |
| Pixels against `ffmpeg` | orientation and colour matched: the mean \|Δ\| 0.63 per channel, max 3 |
| The first frame | 0.07–0.35 s depending on the size |

**Checks:** `cargo test` — 94 passed (15 new player tests: YUV→RGB, the range, rotation, the
resampler, the channels, the clock, the codec registry, the duration), `clippy --all-targets` —
clean, `check.sh` — 39/0 (the `player` module is registered in the layer map), `timeout 8 gui` —
code 124, no crashes. The sound was checked by the fact of playback (the PipeWire device opened,
`played` grows); the owner checks it by ear.

**Honest limits.** 4K H.264 does not play (it needs ~14 fps at 30). At 2520×1080 High the decode is
slower than real time — frames are dropped, and with a lag > 1.5 s the sound is switched off. The
`rff`/`rusty_*` crates are pre-1.0; H.264 Baseline/High, HEVC Main, AAC-LC were checked. Frame
formats other than 4:2:0 8/10 bit and the VP9/Opus codecs go to the external `ffplay`.

## M14b — the player reworked after the owner's review (2026-11-23)

The owner: "the video does not just stutter — it barely moves, the sound too; I pressed stop and
could not close the program". The causes were found and fixed.

- **The sound and the video shared one decode thread.** While a frame was decoded, the audio ring
  emptied, the sound choked, and the clock was counted by played samples — and slowed down together
  with the sound. The video waited for that clock: hence "barely moves" for both. Now three
  threads: video, sound (its own demuxer), presentation. The sound is the master only while there is
  data in the ring; otherwise the wall clock drives.
- **`stop()` waited for the threads (`join`) on the interface thread** — the window froze. Now a
  flag + the threads exit by themselves, without waiting: an exit after five frames — 0.42 s.
- **The frame is downscaled to 1280 px** before presentation: 2520×1080 into a texture is 10.9 MB
  per frame, 1280×548 — 2.8 MB. The texture upload in the GUI is limited to 30 times per second.
- **A lag no longer turns the video into a slideshow.** Without sound the clock follows the decoder
  (slowly, but smoothly). With sound the tolerance is 0.5 s, a catch-up by seeking no more than once
  in 1.5 s; two catch-ups in 4 s — the sound is switched off with an honest note in the status.
- **A seek requested before the threads started was lost** — the generation counter now starts from
  zero.

Measurements (a machine without someone else's load, load ~2):

| Metric | Value |
|---|---|
| Decoding 640×480 / 720p / 1080p / 2520×1080 | 331 / 72 / 50 / 27.7 fps |
| Decoding HEVC Main 1080p | 25.3 fps |
| Showing 1080p with sound, 900 frames | 29.8 fps — real time, the position 29.95 s in 30.2 s |
| Showing 720p (a 9 fps source) | 9.2 fps — real time |
| Showing 2520×1080 with sound | 22.9 fps (the decode 27.7), the lag ≤ 0.5 s, the sound plays |
| A seek to 10 s | the first frame 0.11 s, the position 11.96 s |
| An exit after 5 frames | 0.42 s |

- **`cargo run` is debug, and it was to blame for the "stutters" after delivery.** A pure debug
  profile decodes 1080p at 2.2 fps against 41.9 in release (19 times): the video turned into a
  slideshow, although the intermediate run went from the release binary. The dev profile in
  `Cargo.toml` is configured (dependencies `opt-level = 3`, our code `opt-level = 1`): `cargo run`
  now gives 39.0 fps of decoding and 29.2 fps of presentation with sound.

Checks: `cargo test` — 94 passed, `clippy --all-targets` — clean, `check.sh` — 39/0,
`timeout 8 gui` (release and debug) — code 124 (the window is alive). The owner checks the sound
and the gestures.

## M14c — flipping in the full screen and the playback panel (2026-11-23)

The owner: "when flipping forward-backward a video should change to the next file; now flipping
shows nothing. Add a playback panel with icon buttons and a big play button in the centre if the file
is a video".

The cause of "shows nothing" was found in the code, not in the player:

- **The former frame covered the video.** In the full screen the layer order was "the player frame →
  the full frame → the former sharp frame → the thumbnail". A video never has a full frame, so the
  former shot (`viewer_tex`) stayed on the screen always. Now the former frame is kept only for
  `kind == "image"`, and for a video the cover frame is shown.
- **Flipping did not stop the player.** The ←/→ buttons changed `idx`, but `video_id` stayed from
  the old file, and `poll_video` kept drawing the old video over the new one. Now there is one point
  of a file change — `App::viewer_step`: first `stop_video`, then `idx`. The player frame is drawn
  only if `video_id` matches the open file.
- **The slideshow got stuck on a video**: the readiness of the next frame was checked by
  `photo_cache`, and a video does not get there. Now a video is considered ready at once (it has a
  cover).
- **The playback panel.** ⏮ ▶/⏸ ⏭ — 32 px icon buttons drawn with geometry (`paint_icon`), not
  emoji; for a shot ▶ starts the slideshow, for a video — play/pause. Next to it: the time
  `0:12 / 0:22`, seeking, a sound icon and a slider, "Stop the video". Space on a video —
  pause/resume, as in players; on a shot it stayed the slideshow.
- **A big ▶ button in the centre of the video frame** (a 76×76 plate, the note "video · press ▶ to
  play"), a click on the frame — play/pause too; on pause the button comes back.
- **The end of a video opens the next file** (`video_ended`), except the last one in the list.

Checks: `cargo test` — 99 passed (was 94; 5 new: flipping in a loop, stopping the video on a file
change, flipping a single file, and two headless full screen frames through `egui::Context::run_ui`
— on a video the ▶ button note is drawn, the arrow opens the next file, the former shot is not
left), `clippy --all-targets` — clean, `check.sh` — 39/0, `timeout 6 gui` (release) — code 124. The
owner checks the pixels and the gestures: a headless test sees the text and the file change, but not
the taste.

## M14d — the tag filter does not stay silent (2026-11-23)

State: the changes are in the working tree, not committed; a separate M14d commit before M15.

The owner picked "furniture" and got zero for every tag. CLIP and the index were intact: the DB had
`tag_show = 0.50` (the right edge of the old slider), and the strongest weight of the corpus is 0.32,
so no tag passed the threshold. The threshold in the DB was returned to 0.22. On the corpus
"furniture" gives 717 shots at 0.22 and 0 at 0.50; at least one tag above 0.22 exists for 2162 of
2229 files.

- The threshold slider stands next to the tag picker and in "Settings → Tags": 0–100% of the
  strongest weight in the index. 100% = 0.32, 50% ≈ 0.16, the threshold 0.22 (default) ≈ 70%. At
  100% at least one tag remains; for another model and archive the maximum is different — 100% is
  equated to it.
- An old threshold above the maximum becomes 100% when the index is read, and the status says:
  "the tag threshold 0.50 is above the strongest weight 0.32 — set to 100%".
- In the filter each tag has the number of shots ("furniture · 717", "mine: dacha · 12"): a zero is
  visible before the click. (The number 717 is at the raw threshold 0.22; after M15.2 "furniture"
  has 669 at the 70th percentile.)
- An empty tag filter explains the threshold and offers "Reset the filter" instead of a bare
  "nothing found".

Checks: `cargo test` — 101 passed (was 100; the new one: percentages of the strongest weight are
converted into a cosine and back), `clippy --all-targets` — clean, `check.sh` — 39/0. The pixels and
the gestures are up to the owner.

## M15 — tags that work on any archive (plan, 2026-11-23)

The owner opened `8.jpg` — a vertical render of a wardrobe — and saw "document" on top, below it
"screenshot", "interior" and only then "furniture 0.255". The analysis:

- the gap "document 0.276 / screenshot 0.271" — 0.005, less than the confidence threshold 0.01:
  there is no accent on "document", the list is simply sorted; the right answer is "not sure";
- the 224×224 crop is one of the causes: a vertical frame keeps white shelves without a top, a
  bottom and context, for CLIP that is a sheet/an interface;
- a repeated measurement in M15.1 showed that the crop is not the only cause: the whole frame
  removes "document" from the leaders, but "interior" comes forward, and "furniture" stays fourth;
- over the corpus: in 337 of 717 shots with "furniture" ≥ 0.22, document/screenshot is higher, and
  in all 337 furniture is second. This is not an accident, but a crop + an overstated baseline of
  the general prompts.

The decision: **do not tune the prompts to this archive**. The plan: an ensemble of frames (the
centre + the whole frame); the threshold — a percentile per tag (100% = the maximum of the chosen
tag); an explicit "not sure"; an editable dictionary of labels/prompts in the DB with a recompute
over the stored vectors; common prompt templates without words from this folder; a check on two
independent folders; a larger model — after M12. In detail, with tasks, criteria and open questions
— `plan/M15-tags.md`, the short criteria — `plan/BACKLOG.md` § M15.

The global-maximum threshold from this step was replaced by a percentile one in M15.2 (see below):
the filter is now 0–100% per tag, and `tag_show` again means only the confidence.

M14d checks: `cargo test` — 104 tests, 101 passed, 3 `#[ignore]`; `clippy --all-targets` — clean;
`check.sh` — 39/0; release built, a GUI smoke test with a separate DB — code 124.

## M15.1 — the frame ensemble (2026-11-23) ✅

The CLIP input is two frames per shot: a centre crop and the whole frame with margins (letterbox, the
margins are the mean colour). `derive::clip_inputs` decodes the preview once, `embed::embed_thumbs`
averages the two vectors and normalizes again; the DB still holds one vector. `photocat embed
--refresh` resets the vectors of the active model — otherwise the sha256 key considers the old
computation current.

Measurements on the corpus (2229 files, `--jobs 8 --batch 8`):

| Metric | Value |
|---|---|
| The time of `embed --refresh` | 176.6 s (79 ms/file) against 77.3 s — 2.3×, within 2.5×; 0 errors |
| "furniture" ≥0.22 | 717 → 1029 shots (+43%) — the tag is found more often |
| top-1 "document" | 266 → 248 files; "screenshot" 195 → 211, "interior" 264 → 264 |
| A confident tag (0.22 + a lead of 0.01) | 1284 → 1285 — no regression |
| The stability of top-1 | in 1263 of 1284 former confident ones (98.4%) the tag is the same |
| The size of the vectors | 4.35 MiB — unchanged, the DB holds one vector per file |

`8.jpg` with the whole frame: the centre — screenshot 0.278 / document 0.275 / interior 0.271 /
furniture 0.257; the whole frame — interior 0.274 / furniture 0.262 / screenshot 0.261 / document
0.257; the ensemble — interior 0.278 / screenshot 0.275 / document 0.272 / furniture 0.265. **The
criterion "furniture first" is not reached:** the crop removes "document" from the leaders, but
"interior" comes forward. This is not a reason to bring the old input back: "furniture" is found
more often, there are no fewer confident tags. The right next step is the percentile threshold
(M15.2), "not sure" (M15.3) and common prompts (M15.5), not tuning the prompts to `8.jpg`.

Checks: `cargo test` — 108 tests, 104 passed, 4 `#[ignore]` (+ the diagnostic test `8.jpg`);
`clippy --all-targets` — clean; `check.sh` — 39/0; the search "cake" on the updated vectors — 11 ms.

## M15.2 — a percentile threshold per tag (2026-11-23) ✅

The filter threshold is no longer a raw cosine and no longer a global maximum.
`settings.tag_show_pct` is a percentile 0–100 per tag: 100% — the strongest shot of this tag, 0% —
all of them, 70% — the top 30%. The distributions are built once when the tags are loaded
(`embed::tag_distributions`), in the interface each tag carries the number of shots at its own
threshold. The tag confidence (the card, OCR, `{tag}` in the showcase) stayed separate:
`settings.tag_show` = 0.22 and a lead of 0.01, the filter slider does not move it.

Migration of an old DB: if `tag_show_pct` is not there yet, `tag_show` returns to 0.22 (the old
slider had moved it away), the filter is set to 70%. A folder of fewer than 100 files — the
percentile is noisy, the raw threshold is used, and the interface says why.

Measurements on the corpus (2229 files, the DB 25 MB):

| Tag | 0% | 70% | 100% |
|---|---|---|---|
| furniture | 2229 shots, the threshold 0.167 | 669, 0.229 | 1, 0.298 |
| document | 2229, 0.184 | 669, 0.240 | 1, 0.327 |
| children | 2229, 0.190 | 669, 0.248 | 1, 0.280 |
| interior | 2229, 0.182 | 669, 0.239 | 1, 0.306 |

At 70% at least one tag passes for 2129 of 2229 files (with the old 0.22 it was 2162). At 100% any
tag has a shot — the scale no longer runs into emptiness, and each tag has its own maximum.

Checks: `cargo test` — 111 tests, 107 passed, 4 `#[ignore]` (the new ones: the percentile keeps the
top share and at least one shot, the distributions are sorted, the filter goes by its own tag and a
small folder takes the raw threshold, the migration of the old `tag_show`), `clippy --all-targets` —
clean, `check.sh` — 39/0, `timeout 6 gui` (release) — code 124.

## M15.3 — an explicit "not sure" in the card (2026-11-23) ✅

The card no longer passes the first weight off as an answer. If the first tag is below the threshold
0.22 or leads the second by less than 0.01, the line "tag: not sure — the first weights are close"
appears, no tag is highlighted, the top-3 with the weight and the percentile are visible ("document
0.27 · 98%"), the rest of the weights are under the expanding line "all tags (14)". A confident tag
has the line "tag: furniture" and an accent. The percentile is the share of shots with a lower weight
of this tag; on a folder of fewer than 100 files it is not shown, only the weight remains.

`8.jpg` on the updated vectors: document 0.271 / interior 0.270 / screenshot 0.267 / furniture 0.261
— the lead of the first over the second is 0.001, so the card writes "not sure", not "document".
There is no highlight; the search and the filter still work: "not sure" is about the label, not about
the search.

Checks: `cargo test` — 115 tests, 111 passed, 4 `#[ignore]` (the new ones: the card writes "not
sure" and does not show the fourth tag without expanding; a confident tag is highlighted; the weight
percentile counts the share of the weakest; close weights give `None`, a lead — the tag), `clippy
--all-targets` — clean, `check.sh` — 39/0, `timeout 6 gui` (release) — code 124.

## M9 — the GUI for comparing copies and the manifest (2026-11-23) ✅

`verify` and `manifest` no longer live only in the CLI. The left panel got a sixth view,
"Integrity": the fields "Folder A (the archive)", "Folder B (the copy)", "The manifest file", the
checkbox "Show all paths", the buttons "Compare the copies", "Write the manifest" and "Check against
the manifest". The screen is a form, not a grid: it does not need tiles and the preview cache, so it
is drawn before `self.items`/`self.textures` are taken and leaves the frame by itself.

- **One set of functions for the CLI and the GUI.** `src/verify.rs` gained `verify_report`,
  `check_report` and `manifest_report`: the CLI prints them through `print!`, the screen shows the
  same text (monospaced, in a scroll area) and puts it into the buffer of "Copy the report". `app.rs`
  does not build a report itself — the logic and the words are in one place.
- **Background and "Stop".** The pass runs in a separate thread, the progress is in the top bar
  ("Integrity: 120/2230") with the same "Stop" button as indexing; a second pass does not start
  while the first is running. A cancel writes the verdict "stopped: not everything was checked",
  not "matches".
- **The manifest only on a confirmation.** An existing file is not overwritten by the first click:
  "Write the manifest" turns into "Overwrite the manifest" with a "Cancel", and editing the field
  resets the confirmation and the old result. An incomplete manifest (a cancel/a read error) does
  not reach the disk.
- **The paths are checked before the start:** the same path in A and B, a non-existent folder, a
  manifest file that is a folder or a missing manifest — a red line with the reason and zero records
  on the disk.
- **Documents:** `docs/UI.md` (the screen, the states, the metrics, "six views"),
  `docs/ARCHITECTURE.md` § "Integrity", the layer map of `check.sh` (`app` may `verify`).

Checks: `cargo test` — 165 passed, 4 `#[ignore]` (the new ones: the screen shows the fields
and the three buttons; two identical paths — a refusal without a thread; the background comparison
returns the report through `mpsc`; an existing manifest waits for a confirmation
and stays intact; the result is visible as the verdict and the same report line; the `verify` reports
name the counters, the paths and the list truncation, and a cancel is not passed off as a write),
`clippy --all-targets` — clean, `check.sh` — 42/0, `cargo run -- gui --db /tmp/photocat-m9.db` (a
copy of the real DB, 2230 files) — code 124, no crashes. The CLI on a test folder: `verify` with
divergences — code 1 and the path lists, `manifest` — code 0 and the file, `verify --manifest` —
code 0.

Next in order: M10 — the check reminder, then M11 — a BK-tree for duplicates above 20 thousand photos,
M12 — an extensible catalogue of CLIP models.

Tails of M8/M9: symlinks and special files are not dereferenced and do not enter the comparison (the
showcase `--symlink` is a bad candidate for verify: two real copies must be compared). The same file
that got into two copies as a hardlink is still read twice — the inode acceleration was not done.
An interruption in the CLI is `Ctrl-C`; in the GUI — the "Stop" button (checked on
`Progress::cancel`). The folder walk phase does not listen to a cancel: on millions of files "Stop"
works only with the start of sha256. Non-UTF-8 names lose bytes in the manifest through
`to_string_lossy` (ADR 0014), so such names are better checked by `sha256sum` directly. The manifest
and the folders are text only: there is no system folder dialog on this screen (as in M9: without a
new dependency).

Tails after M7: an event is the heuristic "day + a GPS cell of ~1 km", the place names are
coordinates, because reverse geocoding is the network (ADR 0022). In the corpus only 7 shots have a
GPS, so events barely differ from days: the owner should check on an archive with geotags. The
showcase lives only in the CLI: there is no arranging in the GUI. The code does not delete an
unsuccessful showcase — the owner removes it himself.

Tails after M6: 617 groups on the corpus are almost all without names — the owner has to name his
people by eye; the threshold 0.45 is chosen on the corpus, but false merges of relatives are possible
and are split with the "not him" button. The detector takes a screen, a statue or a picture on a wall
for a face; there are few such "faces", they are visible in the common grid and remain separate
groups. The crops of replaced files stay in the `faces/` cache (the key is sha256): the code does not
delete files even in the cache, the size grows only when the contents are replaced.

Tails after M5c: the path is checked on saving, not before every run — if the program was removed
after the setup, `doctor` and the pass error status will show it. The tesseract language packs are
still installed by the OS package manager: the app does not download them (ADR 0019).

Tails after M5b: the "Stop" button stops indexing, CLIP, OCR and faces; a model download cannot be
interrupted yet — a separate small step if wanted.
Tails after M5: of 435 candidates 288 have no text — false hits of the CLIP heuristic (shots of
furniture, animals). The candidates could be narrowed by the screen aspect ratio, but that saves only
the time of a one-off pass; for now it is left as is. M4b still knows two compatible models and the
fixed 7 tags — extending the catalogue and own tags are separate tasks.

## M10 — the reminder to check the integrity (2026-11-23) ✅

The backup is checked by a due date, not "when I remembered". Done in `src/db.rs`, `src/app.rs`,
`docs/`, `README.md`; the DB schema did not change (`settings` is key/value), the archive was not
touched.

- **The due date is data, not a timer.** `settings` holds `verify_root`, `verify_manifest`,
  `verify_interval_days` (30 by default, 0 — off), `verify_last_at` (RFC3339 UTC), `verify_last_ok`,
  `verify_last_report`. There is one calculation — `db::verify_due(&settings, now)`; `now` is passed
  explicitly, so "exactly due", "overdue", "too early" and "the clock went back" are checked without
  waiting. No manifest — we stay silent; an empty/broken date — "due"; a date in the future — not
  due.
- **The banner** "Time to check the archive" in the top bar: the manifest name, the date of the last
  pass and how many days overdue; the buttons "Check" and "Remind later". It starts nothing by
  itself: "Check" opens "Integrity" and starts the same background check against the manifest with
  progress and "Stop". "Remind later" — only until the next start, nothing is written to the DB.
- **The interval** is edited on the "Integrity" screen ("Remind me to check every [N] days") and is
  saved at once; the same place holds the date/the result of the last pass, the path of
  `verify-last.txt` and "Copy the last report".
- **The result.** A finished manifest or check writes the date, "ok" and the full report into the
  settings and into `<cache>/photocat/verify-last.txt`; into the archive folder — nothing. Writing a
  manifest sets the reference point: the archive has just been read in full. An error and a cancel
  of a pass do not touch the former successful report and date. The CLI `verify --manifest` for
  cron/systemd did not change, the app sets no system timers (an example is in the README).
- **Documents:** `docs/UI.md` (the banner, the reminder row, the states), `docs/ARCHITECTURE.md`
  § "The check reminder", ADR 0037, README (§ the schedule).

Checks: `cargo test` — 170 passed, 4 `#[ignore]` (the new ones: the due calculation in all six
cases, the settings roundtrip, the banner is visible and does not start a pass, "remind later" hides
it until a restart, a successful manifest and a check write the result and the file, a broken
manifest does not erase the former report); `clippy --all-targets` — clean; `enforce-architecture` —
42 checks, 0 violations. On a corpus of 2230 files (21.2 GiB): `manifest --out /tmp/…` — 8.9 s, 2230
rows, 196 789 B; `verify --manifest` — 10.0 s, 2230/2230 matched, code 0. The GUI on a copy of the
real DB with an overdue date under `timeout 6` — code 124, no crashes; the banner and "does not
start itself" are checked by a headless frame, because the owner sees it on the screen.

Tails of M10: "Remind later" acts until a restart — the insistence between starts is not
configurable; a check by cron does not update `verify_last_at` (the GUI does not know about it and
keeps reminding by its own date); the due date is recalculated at load, after a pass and when the
interval is edited, not at midnight — a session open for weeks will see the banner after "Refresh";
the banner has no progress of its own — it shows the common "Integrity" row.

## M11 — fast search of similar shots above 20 thousand photos (2026-11-23) ✅

The pairwise scan of dHash was O(n²): tolerable on 2230 files, on 20 thousand — hundreds of millions
of comparisons. Done in `src/dupes.rs`; the DB schema, indexing, CLIP, OCR, faces, the GUI and the
originals were not touched.

- **An index by bytes.** Above `INDEX_MIN = 20_000` hashes eight 8-bit buckets are built. If the
  Hamming distance is ≤ 7, at least one byte matches entirely (the pigeonhole principle); candidates
  from the eight buckets are deduplicated by a marker and checked by the full Hamming over `u64`
  before `union`. Below the threshold — the former scan, and for a radius of 8–64 — it too: the index
  has no guarantee there, and such thresholds glue almost everything.
- **A BK-tree was measured and rejected.** On 64-bit hashes at a radius of 6 almost all points lie at
  a distance of ~32, and the triangle inequality does not cut off the bulk: 20 thousand — 2.58 s,
  50 thousand — 17.2 s against the goal of 5 s. The decision and the numbers — ADR 0038; criterion 2
  in the BACKLOG was corrected so that the document does not lie.
- **The result is the same:** `find_groups_with(..., index_from)` gives the tests both paths on the
  same data; `union-find`, the frame advice, `plan_csv` and the CSV did not change; exact copies by
  `sha256` are still glued before the search of similar ones. There are no new dependencies.
- **Documents:** `docs/ARCHITECTURE.md` § "Duplicates: the block index" and § "Measurements",
  ADR 0038.

Checks: `cargo test` — 172 passed, 5 `#[ignore]` (the new ones: the index and the scan agree on an
empty set, one element, all identical, all far apart and random sets of 8/40/80/160 at `max_dist`
0–10; exact copies by sha256 are glued through the index; the synthetic measurement — ignored);
`clippy --all-targets` — clean; `enforce-architecture` — 42 checks, 0 violations. Release: the corpus
of 2230 files `dupes --max-dist 6` — 0.017–0.025 s, the same 103 groups, 238 files, 214.1 MiB;
synthetic 20 thousand — 0.082 s (the test RSS 27.1 MB), 50 thousand — 0.446 s (RSS 48.2 MB), which
fits into 5 s.

Tails of M11: the index is rebuilt on every `dupes` run and is not cached in the DB; at a radius of
8–64 the square remains (deliberately: such thresholds are not practical); eight buckets are a
compromise between the number of candidates and memory, other partitions were not measured.

## M12 — an extensible catalogue of CLIP models (2026-11-23) ✅

The dimension stopped being a constant, the catalogue became data, and the third model — SigLIP with
a different vector. Done in `src/embed.rs`, `src/derive.rs`, `src/db.rs`, `src/app.rs`,
`src/main.rs`; schema 6 → 7, the old vectors and tags were preserved.

- **`ModelEntry` is data.** `dim`, `image_side`, `max_tokens`, `pad_token`, `norm`
  (mean/std/a filter), `frames`, the ONNX output names, the licence. `DIM` is gone: the dimension
  comes from the model and is checked against the ONNX metadata at load and against the actual
  output; an error names both numbers. `TextKind`: `Projected` (CLIP + dense), `Direct`
  (`text_embeds`), `Pooler` (SigLIP `pooler_output`).
- **SigLIP base 224** (`siglip-base-224-en`): 768 features, 203.5 MiB, Apache-2.0, three files with
  pinned URLs and sha256. Its own preprocessing: mean/std 0.5, bicubic, the whole frame into
  224×224 (one frame, without a centre crop), the text is padded to 64 tokens with `</s>`.
- **Schema 7:** `tags` got the key `(file_id,tag,source)`; the migration rebuilds the table, the data
  is preserved, the sha256-change trigger now removes the CLIP tags of all models. The vectors were
  already split by `(file_id,model)` — switching the model does not erase the others.
- **Search/tags/clusters** read only the active model; `retag` recomputes only its dictionary.
  The GUI and `models list` show the features, the licence, the size and the status; the 250 MiB
  limit holds SigLIP (203.5), the former 160 — does not.
- **Documents:** `docs/ARCHITECTURE.md` § "The model catalogue", `docs/UI.md` (the model settings and
  the switch), `README.md`, ADR 0039.

Checks: `cargo test` — 176 passed, 6 `#[ignore]` (the new ones: the catalogue with unique
ids/folders, two dimensions and sha256; a dimension error names the numbers; a download refusal
without consent; the 6→7 migration and the tags of two models on one file; the SigLIP test with real
weights — ignored); `clippy --all-targets` — clean; `enforce-architecture` — 42 checks, 0 violations.
On a corpus of 2227 files (2 in the trash): downloading SigLIP — 36.7 s, 203.5 MiB, sha256 matched;
`embed --refresh` — 418.2 s (0.188 s/file), 0 errors, the vectors 6.8 MiB, the DB 25.4 → 40.1 MB;
`embed --retag` — 2.4 s; a search — 35–40 ms; `clusters --k 30` — 1.5 s. The old CLIP vectors
(2229) and tags (31 206) are in place next to SigLIP (2227 / 31 178).

Tails of M12: SigLIP understands only English queries and has a low cosine scale — a confident tag
of 0.22 almost never fires, a percentile filter or a lower threshold is needed; calibration and a
multilingual model — M15.6. `tag_hidden` hides a label on a shot in all models. The download progress
cannot "Stop" (as before). `embed` skips files by sha256 without regard to `dim`: if the dimension
is changed under the same id, `--refresh` is needed.

## Open questions to the owner

1. ~~Names in the showcase: the original ones or from the date?~~ Decided in M7: the original ones by
   default, the choice is the owner's — `--name` with substitutions or `--rename` (the former
   format).
2. ~~The structure of the showcase: by days or by months?~~ Decided in M7: `--by day` (default),
   `--by month`, `--by event` — all three are available, none is imposed.
3. ~~Video: a dHash over the cover frame or a separate class?~~ Decided in M4: a dHash over the
   cover, videos in the common duplicate groups.
4. ~~The confident tag threshold~~ Moved into "Settings" (0.22 + a lead of 0.01 by default): the
   owner can pick it himself, looking at the cards.
5. ~~The word for M19~~ Decided by the owner (2026-11-23): **"Albums"**.
6. ~~Renaming an original~~ The owner withdrew the task (2026-11-23): the originals are not renamed,
   M21 is cancelled, §1 AGENTS.md stays as it was.
7. ~~The export: links or copies; flat or by days~~ Decided by the owner (2026-11-23): **copies of
   the files, the structure repeats the album tree**.
8. ~~Numbers on subalbum folders~~ Decided: **without numbers**, the order of subalbums on disk is
   alphabetical.
9. ~~The default name template~~ Decided: **`{n}_{orig}`**; the width of the number by the number of
   files.
10. ~~One shot twice in one album~~ Decided: **once** (in different albums — allowed).

All decisions on M17–M20 are taken and closed; the M20 measurements are in § M20 and
`docs/ARCHITECTURE.md`.

## Known defects

- OCR: 288 of 435 candidates gave empty text — these are shots without captions (furniture, animals)
  where CLIP confidently put "document"/"screenshot", or photos without an EXIF camera. The empty
  string is stored, a repeated pass does not touch them.

- CLIP tags are approximate: 849 photos have the "not sure" tag — that is expected, not an indexing
  error.
- One photo of 1972 stayed without a preview, CLIP and faces (`P_20200724_154351.jpg` does not
  decode) — it is indexed, but does not take part in the passes; the error is visible in the counter
  (1), the face file is marked as "0 faces", so it is not scanned every time.
- The English models (`clip-vit-b32-en`, `siglip-base-224-en`) understand only English queries; the
  Russian tag labels remain, but the query "cake" is meaningless for them. The two multilingual
  models are 217.5 MiB (CLIP, the default) and 393.3 MiB (SigLIP 2, needs a 400 MiB limit); there is
  no compatible multilingual model under 100 MiB in the catalogue yet.
- Switching the model does not erase the old embeddings and tags: they are tied to `model` (and the
  calibration version to `source`), so the DB grows by ~4.4–6.9 MiB per model; the tags are shown only
  for the active model. Tags written before the M15.6 calibration of SigLIP are not shown until
  `embed --retag` — a state, not a mix.
- The owner needs to reindex the folder: the old DB does not know `duration_s` of videos (258 rows).
- Non-UTF-8 file names (Linux) are stored in the DB through `to_string_lossy` — see ADR 0014.
- `plan/M3-clusters.md` — a local report with archive paths, it does not get into git.
- CLIP tags on vertical/wide frames and renders: the "centre + the whole frame" ensemble removes
  "document" from the leaders, but "furniture" does not become first on `8.jpg` — the baseline of the
  general prompts is overstated even after the common templates of M15.5. The card honestly writes
  "not sure" (M15.3); a bigger model did not help either (M15.6: SigLIP 2 is worse on documents).
- After a schema update old DBs get an empty `sharpness` until a reindex; the duplicates panel warns
  about it.
- Faces: the threshold 0.45 is a compromise; relatives sometimes have a high cosine, so one group can
  contain two similar people. This is cured with "not him", not with an automatic threshold. The
  detector does not distinguish a face in a photo from a face on a screen/a statue. 21 faces of the
  corpus are smaller than 40 px — the embedding is weak, but such faces are not discarded if they
  passed the detection threshold of 0.5 and the minimum of 28 px.

## Checks

- `cargo test`: 155 tests, 151 passed, 4 require the model weights or the corpus (faces: the cosine
  components, NMS, similarity by 5 points, bilinear sampling,
  saving/splitting/naming and erasing faces on a sha256 change; paths to external programs:
  empty/an explicit path/a start error/the version flags;
  settings with paths survive a roundtrip; the schema of an old DB; decisions on duplicates; CLIP
  invalidation on a sha256 change;
  the sharpness advice; exact copies; the CSV plan; cosine; k-means; fitting a preview into a tile —
  portrait, panorama, square, zero size; the video duration; the ffprobe parsing with a 90/270
  rotation;
  the hidden tag disappears from the filter and is reset on a sha256 change; the settings survive a
  roundtrip;
  flipping months across the year boundary, the length of February, the first Monday of the calendar;
  drilling down the calendar year → month → week → day; skipping empty
  months while flipping and "Today" going to the nearest month with files;
  the comparison: different sizes are not hashed, an equal size is compared by sha256, only in A/B,
  a read error does not break the pass, a cancel does not write an incomplete manifest, escaping and
  parsing
  the manifest, a refusal on garbage, the roundtrip of the manifest and the check on the disk;
  the percentile keeps the top share and at least one shot, the tag distributions are sorted,
  the filter goes by its own tag, a small folder takes the raw threshold, the old `tag_show` migrates
  into a percentile; the card writes "not sure" with close weights and does not show the fourth tag
  without
  expanding, a confident tag is highlighted, the weight percentile counts the share of the weakest).
  Plus two `#[ignore]` tests: "the model on a real photo" and "CLIP and faces share one ONNX
  environment".
- A cancel and reading during a pass: `stats` and `search --text` during OCR — 11–15 ms (before the
  fix — 5 s and "database is locked"); the test "a cancel before the start does nothing";
  `OMP_THREAD_LIMIT=1` for tesseract. "Stop" buttons for indexing, CLIP, OCR and faces; "Resume
  (N)" — the remainder according to the plan.
- External programs: `photocat doctor` shows ffmpeg/ffprobe 9.0.2 and `/usr/bin/ffmpeg`
  (from PATH), tesseract 5.5.4, curl 8.23.0, the purpose and "without it" for each;
  `settings --ffmpeg /nonexistent/ffmpeg` refuses with code 2 and writes nothing,
  `settings --tesseract <path>` saves and doctor shows "set in the settings";
  `tesseract --list-langs` is parsed by a test.
- The showcase (`organize`): the depths `day`/`month`/`event`, the name template with all six
  substitutions, empty substitutions without `__`, the `-2` collisions, `--exclude-dup`, a refusal on
  an unknown substitution and on `--by foo` — 13 tests; on the corpus of 2230 files a dry run by days
  — 0.017 s and
  573 folders, by months — 76, by events — 576 (GPS in 7 shots); `--apply` — 2230 hardlinks
  in 0.065 s, the inode matches, 0 bytes; `--exclude-dup` on a copy of the DB with 135 marked —
  2095 included; the template with `{tag}` — 0.037 s, the tags are read only when it is there.
- The OCR pipeline: an FTS5 query and the excerpt highlighting, erasing
  the text on a sha256 change, the visible tags with weights, the selection of candidates by
  camera/tag, the parsing of
  `tesseract --list-langs`, the paths to the programs, `--all` without video);
  on the corpus: 435 candidates, `ocr --jobs 8` — 106.2 s, 434/435, a repeat — 0.0 s, `search --text`
  — 2–3 ms. `photocat doctor` sees tesseract 5.5.4 and the languages `rus, eng`.
- The album export (M20): 15 tests in `organize` (the tree, depth 3, the width of the number, the
  cleanup
  and the collisions of folder and file names, `{n}`/`{hash8}`, the trash and a long path, a copy is
  not a link,
  idempotency, `-2` on a taken name, a stale order without duplicates, an empty album,
  a missing file, a cancel, a refusal of an `--out` in the root) and a test of the GUI dialog. On the
  corpus: 500 files /
  2.4 GiB — a dry run 14 ms, `--apply` 2045 ms, a repeat 20 ms and "500 skipped"; 2230 files /
  21.2 GiB — 111 ms, a repeat 45 ms and "2230 skipped"; 5 copies — `sha256` matched, the inode
  differs.
- The layers: `check.sh` — 41 checks, 0 violations; `egui` only in `app.rs`; `organize` uses
  `derive::sha256_file` to compare the target; external processes
  are started only through `tools.rs`, `curl` — only in `embed.rs` and without sending data.
- The path from the settings is really used: a saved working link to ffmpeg → video indexing
  without errors; a link broken after saving → videos without a preview, `errors 1`, the rest of the
  pass
  continues; a restored link → `errors 0` again.
- Integrity (`verify`/`manifest`): on the corpus of 2230 files / 21.2 GiB `verify --jobs 8` of two
  reflink copies — 19.9 s, 2230/2230 matched; a copy with 1 changed, 1 appended, 1 deleted and
  1 extra file — 16.1 s, all four divergences found, code 1; `manifest` — 10.1 s, 2230
  rows, `sha256sum -c` reads the file; `verify --manifest` — 11.4 s, 2230/2230; on a changed copy
  — 8.9 s and "matched 2227, changed 2, missing in the folder 1, missing in the manifest 1". The
  codes 0/1/2 are checked.
- The CLI on the corpus: `embed`, `embed` again, `search` (cat/child/beach/cake/document),
  `clusters --k 30` — everything worked, 0 errors; `faces` — 1972/1972 shots in 630.2 s (1 error —
  the broken JPEG), 2498 faces,
  617 groups, 1 error (the known broken JPEG); `faces --list`, `--recluster`, `label`, `split` and
  `download --yes` are checked on a test DB: the name and the split survive a rebuild.
- Faces: the detection is checked by eye on 12 shots (the crops are aligned); the group 35/2990 of
  362 faces —
  the same child, the group 3070 of 252 faces — the same woman. `faces download --yes`
  on a clean folder — 182.4 MiB in 31.3 s, sha256 matched.
- GUI: `timeout 6 photocat gui` on the real DB (2498 faces) — the window is alive, the return code
  124, no crashes;
  "Settings" with the fields of the programs is checked by a headless build, the owner reviews the
  pixels.
  The owner has to look with his eyes at the calendar, "Settings", the download by consent and the
  "remove" button of a tag: the pixels are not covered by a headless check.
- Previews in the grid are no longer stretched into a square: the picture fits into a 150×150 tile
  keeping the proportions (`app.rs::fit_rect`), the margins are filled with the background; the
  tooltip is limited to
  400 px on the long side and does not distort the frame either. Portrait shots from a phone no
  longer look squashed; panoramas are visible in full.

## M20 — exporting an album as copies on disk (2026-11-23)

The owner built a tree of albums (M19) and wants to get a real folder out of it. Done in
`src/organize.rs` (`plan_album`, `apply_export`), `src/db.rs` (`album_subtree`,
`album_export_data`), `src/main.rs` (`export`), `src/app.rs` (the "Export to disk…" dialog).
The DB schema did not change; no archive file is opened for writing.

- **CLI.** `photocat export --album N --out DIR [--name TEMPLATE] [--apply]`. Without `--apply` —
  only
  a plan tree, nothing on the disk, not even a folder. `--out` must exist and not lie inside
  `settings.roots`; otherwise code 2 and an explanation. Codes: 0 — a plan/success, 1 — there were
  copy errors.
- **The structure.** `--out/<album>/<subalbum>/…` one to one with the tree; the folders of subalbums
  without
  numbers (alphabetical on the disk, the tree order stays in the app); an empty album — an empty
  folder. The file names `{n}_{orig}`: the position number in its own album, 2 digits up to 99, 3 up
  to 999;
  `{date} {time} {camera} {tag} {orig} {ext} {hash8}` are available.
- **Copies only.** `std::fs::copy`, the inode differs, the contents match by `sha256`; there is no
  link mode in
  `export`. The name sanitizer is shared with the showcase; folder names additionally get
  `_` instead of forbidden symbols, an empty name → `album-<id>`, Windows device names — with `_`.
- **Collisions.** Equal names in an album are separated by the number; a name taken by other
  contents gets
  `-2` (not an overwrite); the same `sha256` — a skip; the same shot under an old number — "the order
  in the
  target is stale N". The cache "what was exported" lies in `<cache>/photocat/exports/`, not in
  `--out`; there are
  no service files in the target.
- **GUI.** The folder, the template, "Show the plan" with the tree and a summary, "Export N files" in
  the background,
  "Export: 173/500" and "Stop" in the top bar, the report and "Copy the report". An error (no
  folder,
  it is inside the archive, an unknown substitution) is visible before the start.
- **A limit, honestly:** reordering an album does not move the already exported files to new
  numbers — the app does not rename files in the target. It is cured by a manual re-export; the plan
  and the report
  say so directly.

**Measurements** (a copy of the real DB; the measurement target — `/tmp` and a separate folder in
the home directory):

| Corpus | Operation | Result |
|---|---|---|
| a tree of 3 levels, 500 files, 2.4 GiB | `export` (dry run) | 14 ms, 4 folders, 0 errors |
| the same 500 files | `export --apply` | 2045 ms, 500 copies, 2.4 GiB; the inode differs, `sha256` matched |
| the same target | a repeat of `--apply` | 20 ms, exported 0, skipped 500, files 500 |
| 2230 files, 21.2 GiB, one album | `export --apply` | 111 ms, 2230 copies, 0 errors (the FS gave `copy_file_range`) |
| the same target | a repeat of `--apply` | 45 ms, exported 0, skipped 2230 |

Checks: `cargo test` — 155 tests, 151 passed, 4 `#[ignore]` (16 new: the tree and the position
number,
the width of the number, the cleanup and the collisions of folder names, the file collisions, the
template with `{n}`/`{hash8}`,
the trash and a long path, a copy is not a link + idempotency, a taken name `-2`, a stale order
without duplicates, an empty album, a missing file, a cancel, a refusal of `--out` in the root, the
depth three, the dialog
in the GUI); `cargo clippy --all-targets` — clean; `check.sh` — 41 checks, 0 violations (for
`organize` `derive` is allowed, it needs `sha256_file`); `timeout 6 gui --db /tmp/pc-m20.db` —
code 124, no crashes. The owner reviews the pixels of the dialog and the gestures.

Tails: the export does not rename or delete in the target (deliberately, §1 AGENTS.md) — a stale
order is cured by a new export; the cache trusts the size and mtime of the file in the target (if the
contents were replaced keeping both, a repeat may skip the recount — the same price as in indexing);
NFD/NFC names on macOS are not normalized.

## M15.4 — a dictionary of tags and prompts in the DB (2026-11-23)

Labels and prompts stopped being code constants: the owner adds "Wardrobe", edits prompts,
disables extra tags and recomputes weights without running CLIP over the photos again. Done in
`src/db.rs`
(schema 6, `tag_defs`, `TagDef`, select/write/replace, counters), `src/embed.rs`
(`DEFAULT_TAGS`, `ensure_tag_defs`, `Model::set_tag_defs`, JSON exchange), `src/app.rs`
(the editor in "Settings → Tags", the background button "Recompute tags"), `src/main.rs`
(`photocat tags …`). `TAG_LABELS`/`PROMPTS_RU`/`PROMPTS_EN` are gone; `ocr.rs` and `organize.rs`
read the labels from `tags` and did not change.

- **The schema.** `tag_defs(model, tag_id, label, prompts, enabled, position, min_pct)`; `label`
  is unique inside a model, `prompts` — one line per template (since M15.5 — 2–3 templates,
  the tag weight is the best of them). `SCHEMA_VERSION` 5 → 6; an old DB catches up in place,
  `files`/`embeddings`/`tags`
  are not rewritten (checked on a copy of the real DB: 2230 files and 2229 vectors in place).
- **The default.** 14 common tags (`embed::DEFAULT_TAGS`), no "wardrobe"/"dresser"/"drawing";
  the seeding happens only if the model has nothing, edits are not overwritten by a repeated run
  (`settings.tag_defs_version`). "Restore the defaults" and `tags --reset` — an explicit rollback.
- **A recompute.** `embed --retag` / the button: one text tower over the stored `embeddings`,
  `replace_tags` for each file. 2229 files in 1.0–1.2 s; the photos are not decoded.
- **The filter.** The list of tags in the filter comes from the dictionary (the enabled ones in
  order) plus the old labels
  that are no longer in the dictionary. A tag's `min_pct` raises its percentile threshold above the
  common
  slider. A disabled tag is not counted and is not shown.
- **The exchange.** JSON without paths and photos: `tags --export FILE` / `--import FILE`, in the
  GUI — the fields and buttons;
  by default `<cache>/photocat/tags.json`. An import replaces the model's dictionary and asks for a
  recompute.

**Acceptance on a copy of the real DB** (`/tmp/photocat-m154.db`, 2230 files): `tags` seeded 14 tags;
`tags --add "Wardrobe" --prompt "a photo of a wardrobe" --prompt "a photo of a wardrobe with
shelves"`; `embed
--retag` — 2229/2229 in 1.0–1.2 s. "Wardrobe": the weights 0.187–0.292, top‑1 for 218 shots, at the
70th
percentile the threshold 0.2454 and 669 shots — as many as the other tags have. The edit survived
three separate runs; `--export` → `--reset` → `--import` returned 15 tags. Tests: the JSON roundtrip,
the seeding once without overwriting edits, the uniqueness of a label inside a model, the order and
the languages
of the prompts, the 5 → 6 migration. `cargo test` — 156 tests, 152 passed, 4 `#[ignore]`;
`clippy --all-targets` clean; `check.sh` — 41 checks, 0 violations; `timeout 6 gui --db
/tmp/photocat-m154.db` — code 124, no crashes.

**Left:** the prompts are still one per label — common templates (2–3 per label) and
the decision about "Drawing/scheme" — M15.5; a check on a second independent folder — M15.7 (done).
The owner reviews the pixels of the editor: a headless build does not show how it looks.

## M15.5 — common prompt templates (2026-11-23)

There was one prompt per label and it described the thing by enumeration ("a photo of furniture, a
wardrobe, a table,
a sofa"), which is why the general categories "document"/"screenshot" pulled foreign shots. Done
in `src/embed.rs`: each of the 14 default tags now has 2–3 common templates in two languages;
the tag weight is the **best** of the templates, not the average (the average blurs a guess if one
formulation worked). `DEFAULT_TAG_VERSION` = 2; so that the new set reaches an old DB, a
dictionary fingerprint (`settings.tag_defs_sig`) was added: the default is updated only when the
fingerprint matches,
the owner's edits and an import (version 0) are not touched. "Drawing/scheme" was not added to the
default —
it is a niche, not a common category; the owner adds it through the dictionary (M15.4).

**A "before/after" measurement on two independent folders** (the same CLIP ViT-B/32 multilingual,
the same embeddings, only the prompts changed):

| Folder | Confident | The same top-1 | "document" in top-3 | A false "document/screenshot" |
|---|---|---|---|---|
| the phone corpus, 2229 files | 1285 (57.6%) → 1242 (55.7%) | 1855 (83.2%) | 804 → 534 | on photos with a camera and no text 270/1655 (16.3%) → 225/1655 (13.6%); of 145 shots with recognized text 123 (84.8%) → 123 (84.8%) |
| "birthday", 110 files | 66 (60.0%) → 69 (62.7%) | 95 (86.4%) | 38 → 21 | "screenshot" in top-3 39 → 38 |

"Furniture" in top-3 grew 392 → 407, "interior" 849 → 905 — the common templates returned weight
to the object categories. `8.jpg`: document 0.274 / furniture 0.273 / interior 0.270 — "not sure"
(a lead of 0.001), that is, the card does not pass the first weight off as an answer. The average
over the templates lost to the maximum on all four metrics (confident 1175, false 245, documents
120/145) — that is why the maximum was chosen.

Checks: `cargo test` — 157 tests, 153 passed, 4 `#[ignore]` (a new test of the default update:
an untouched dictionary catches up with the version, an edit and an import — do not); `clippy`
clean; `check.sh` — 41/0;
`timeout 6 gui --db /tmp/m155-a-final.db` — code 124, no crashes. There is still no labelled sample
of ≥ 100
shots: the accuracy of top-1/top-3 on it — M15.7 (done); here the stability of top-1 was measured
and the behaviour on a weak label (there is recognized text / a photo with a camera and no text).

## M21 — language policy, Apache-2.0 license, UI language switch (2026-11-24)

The owner asked to take `LICENSE` and `NOTICE` from the sibling project, replace the name, keep the
Russian documentation as the second variant, continue the project in English (docs, commits) and let
the interface language be chosen in the settings. Communication with the owner stays Russian.

**License.** `LICENSE` is the Apache License 2.0 text (the copyright holder line is kept),
`NOTICE` names `photocat`; `Cargo.toml` declares `license = "Apache-2.0"`; `README.md` §License
points at both files. A new dependency must be Apache-2.0-compatible (AGENTS.md §1).

**Repository language.** `README.md` and `AGENTS.md` are English now; the Russian originals live next
to them as `README.ru.md` and `AGENTS.ru.md`, both linked from the English files. ADR 0040 records the
decision. The section "Continue in a new session" was removed from the README. The Russian
`docs/*.md` and `plan/*.md` stay in place until their English translation lands (`plan/I18N.md`,
M21.7–M21.9); this file switches to English from this entry on.

**Interface.** New L0 module `src/i18n.rs`: `Lang` (`En` default, `Ru`), a Russian→English table and
`t(lang, "…")`. The language is `settings.lang` (`en`/`ru`), chosen in the new first tab of the
settings dialog "Interface" and saved immediately; `photocat settings --lang en|ru` does the same
from the CLI and `settings` prints it. The shell is translated: navigation, top bar, search mode,
progress rows, the archive-check banner and the whole settings dialog — 55 labels. Untranslated
screens fall back to Russian by design (`plan/I18N.md`).

**Checks.** `cargo test` — 182 passed, 6 `#[ignore]` (new: the English shell renders `All files`,
`Settings`, `LIBRARY`, `FILTER` headlessly; `settings.lang = ru` survives a reopen; the pair table
rejects empty, duplicate and non-Cyrillic keys); `cargo clippy --all-targets` clean;
`check.sh` — 44 checks, 0 violations (i18n registered as L0, allowed for `db`, `app`, `main`);
`timeout 6 ./target/release/photocat gui --db /tmp/photocat-m21.db` — exit code 124, no panic.

**Next.** `plan/I18N.md` lists the screens and documents in order; content panes (grid, card,
calendar, faces, trash, integrity, viewer) are M21.2–M21.5.

## M21.2 — the grid in English, language detected on the first launch (2026-11-24)

Owner's answers: choose the right default (English, detected from the system), no release exists so an
old DB may be re-indexed, history is not translated, CLI text follows the chosen language (M21.6), the
copyright holder stays, push.

**Language on the first launch.** `db::ensure_lang` looks at the locale — a set `LC_ALL` or
`LC_MESSAGES` decides on its own (`C`/`POSIX`/unknown → English), otherwise `LANGUAGE` and `LANG` —
stores the answer in `settings.lang`, and runs before the window opens (`app::run`) and before
`photocat settings` prints.
The owner's own choice in the dialog always wins because the row already exists.

**Grid (M21.2).** View title and counts (`4 files`, `Album “X”`, `3 groups · 1.2 GiB wasted`,
`5 people on 6 photos`), `Tiles`/`Sort`/`Flat`/`By months`, the sort menu labels with `now: …`,
month headers (`May 2024`, `No date`), the chips row (`path/date:`/`tag:`/`by relevance:`/`found 4 of
10`/`no filters`/`Select all (4)`/`Clear selection`/`2 selected · 40 B`), the empty state. Plural
forms live in `i18n::count` (Russian 1/2–4/5+, English 1/N); month names in `i18n::month_name`; sizes
in `db::human_in(lang, n)` (`КиБ`/`KiB`) while `db::human()` stays for CLI.

**Checks.** `cargo test` — 185 passed, 6 `#[ignore]` (new: locale parsing, plural counter, month
names; the grid tests now assert `May 2024`, `April 2024`, `No date`, `Select all (4)`, `2 selected ·
40 B`); `clippy --all-targets` clean; `check.sh` — 44 checks, 0 violations; `settings --lang en|ru`
and the first-launch detection checked on fresh DBs: `LANG=ru_RU.UTF-8` → `ru`; `LC_ALL=C` → `en`;
`LC_ALL=en_US.UTF-8 LANG=ru_RU.UTF-8` → `en`; `LANG=de_DE.UTF-8` and no locale at all → `en`.
`timeout 6 gui --db /tmp/m21g.db` — exit code 124, no panic.

**Next.** M21.3: file card and inspector (labels, date source, tags, OCR excerpt, decisions), then
M21.4 (duplicates, calendar, faces, trash, integrity) and M21.6 (CLI reports take `lang` in the
structs).

## M21.3 — the file card and the inspector in English (2026-11-24)

The owner asked for the next step of the interface translation: the right-panel inspector and the
file card are now fully translated, and the grid no longer has a Russian island beside it.

**What moved.** `details_panel` and `file_card` take `settings.lang` and pass it to `t()`. Translated:
the panel title `File details` and `Hide` with its hint; the empty state; the `▶ video` / `no preview`
placeholder; the `selected N · size` line; `Open full screen`, `To trash` and their hints; the status
messages after those actions; the metadata labels `Date`, `Camera`, `Size`; the date source
(`file name`, `file time`, `source not set`; `EXIF` stays a token); the `Albums` section with its
remove hint and status; `AI tags` with `tag: <name>`, `tag: not sure — the top weights are close`,
`all tags (N)`, `tag: not computed`, the `remove` button and the hidden-tag restore; `My tags` with
`none yet`, the input hint, `Add tag` and its statuses; `Text on the photo` with
`Recognize text`/`Recognize again`, the disabled-button note, `Show the full text` and hints.
Data stays as data: file names, paths, tag labels and OCR text are never translated; `GPS`/`SHA256`/
`EXIF` are tokens, not labels. `date_source(src, lang)` gained the language argument; `tag_row` does
the same for its `remove` button. Status strings are translated as a fixed part plus the dynamic
value, so the CLI screens that still speak Russian (M21.4) are untouched.

**Checks.** `cargo test` — 186 passed, 6 `#[ignore]` (new: `english_language_translates_the_file_card`
— a headless card frame with `File details`, `Date`, `Camera`, `Size`, `AI tags`, `My tags`, `Text on
the photo`, `Open full screen`, `To trash`, `tag: …`, `file name`, and no Russian card label left).
The three card tests that assert Russian now set `settings.lang = Ru` themselves, as the scope test
does. `cargo clippy --all-targets` clean; `check.sh` — 44 checks, 0 violations.

**Next.** M21.4: duplicates, calendar, faces, trash and integrity screens, their dialogs and statuses.

## M21.4a — the duplicates screen in English (2026-11-24)

The owner asked to continue the interface translation screen by screen: the duplicates view was the
next one after the file card. The panel of the view, the group cards and the labels painted on the
tiles no longer speak Russian under an English interface.

**What moved.** `dupes_controls` takes `settings.lang`: the header, the threshold line
(`Similarity threshold: 6 bits out of 64`), `Find groups` and its status
(`2 groups · 1.2 MiB wasted · 0.0 s`), the summary line, `Export the plan` / `Copy the list` with
their hints and their statuses, and the two explanatory paragraphs. The group card shows
`#1 — <reason> · 2 files · 1 KiB wasted` and `<decision|advice>: keep b.jpg — <advice>`. The tile
carries `spare`, `keep`, `no date`, `▶ video` and `no preview`: `Cells` got a `lang` field, so
`draw_cell` needs no extra argument and every view that draws tiles is covered at once. Sizes go
through `db::human_in(lang, n)`. Group reasons (`exact copies`, `similar`) and the advice text come
from the `dupes` module: they are data of the report and move with the CLI reports in M21.6.

**Checks.** `cargo test` — 187 passed, 6 `#[ignore]` (new:
`the_english_language_translates_the_duplicates_screen` — a headless duplicates frame with
`Duplicates & series`, `Find groups`, `Export the plan`, `Copy the list`, `#1 — … · 2 files`,
`Similarity threshold: 6 bits`, and no `Find groups`/`оставить`/`без даты` left in Russian);
`cargo clippy --all-targets` clean; `check.sh` — 44 checks, 0 violations.

**Next.** M21.4b: the calendar (scale tabs, month and weekday names in `i18n`, day headers,
navigation hints, the `no date` group).

## M21.4b — the calendar in English (2026-11-24)

The calendar was the second content screen: the scale tabs, the month grid and the day list spoke
Russian under an English interface, and month and weekday names lived in `app` as a Russian table.

**What moved.** The four scale tabs (`Year`, `Month`, `Week`, `Day`), `Today`, the
previous/next hints per scale, the month summary (`3 files this month, 1 KiB`), the empty-month and
empty-day notes, `Go to 2024-05`, the day headings and the `No date` collapsing group all go through
`t()`; sizes go through `db::human_in`. Month names now come from `i18n::month_name` instead of the
`MONTHS` table, and the new `i18n::weekday(lang, 0..6)` replaces the `WEEKDAYS` table for both the
weekday header row and the day/week titles — Monday first on both languages. Both old constants are
gone; a weekday index out of range gives Sunday instead of a panic. The `без даты` string stays in
the day map as a key only: keys are data, labels go through `t()`.

**Checks.** `cargo test` — 189 passed, 6 `#[ignore]` (new:
`the_english_language_translates_the_calendar` — a headless calendar frame with `Year`, `Month`,
`Week`, `Day`, `Today`, `May 2024`, `Mon`, `Sun`, `1 file this month`, and no `Год`/`Месяц`/`Пн`/
`Сегодня` left; plus `weekdays_in_both_languages` for the table); `cargo clippy --all-targets` clean;
`check.sh` — 44 checks, 0 violations.

**Next.** M21.4c: the faces screen (threshold, scan/recluster, the group list, face tiles with
`not him`/`pick`, the name and batch-assign fields).

## M21.4c — the faces screen in English (2026-11-24)

The faces view was the third screen. The panel, the group list and the face tiles now follow the
interface language.

**What moved.** The threshold line, the `face model not downloaded` note with `Face settings`, the
counters (`2 people on 1 photo, 1 group`, `no faces yet: 2 of 3 photos`), `Resume`/`Find faces` with
the remaining count, `Rebuild groups`, the group list (`All people · 2`, `no name · 1 photo`), the
selected-group block (`Person's name:`, `Save the name`, the pinning note), the batch fields
(`People picked: 0`, `name for the picked people`, `Name the picked people`, `Unpick`), the empty
states and the tile labels (`not him`, `pick`, `no frame`, `no date`). `draw_face_cell` gained a
`lang` argument — it is a free function and has no `Cells`-like struct; `draw_cell` already gets the
language from the `Cells` field added in M21.4a. The scan, cluster, label, split and batch-assign
statuses go through `t()`; the counters use `i18n::count`. Statuses printed from the module reports
(`faces::Stats::to_string()`) stay Russian until M21.6, when the reports themselves take `lang`.
A test helper `window_frame_at` was added: a taller headless frame, because the face-group list in
the left panel is otherwise clipped out of the drawn text and cannot be asserted on.

**Checks.** `cargo test` — 190 passed, 6 `#[ignore]` (new:
`the_english_language_translates_the_faces_screen` — a headless faces frame with `People`,
`Resume (2)`, `Rebuild groups`, `Groups`, `All people · 2`, `no name · 1 photo`, `Person's name:`,
`Save the name`, `Name the picked people`, `Unpick`, `not him`, `pick`, `no frame`, `no date`, and no
Russian face label left); `cargo clippy --all-targets` clean; `check.sh` — 44 checks, 0 violations.

**Next.** M21.4d: the trash (screen, confirm dialog, restore and empty statuses), then the
integrity screen. The left panel blocks (summary, model and tag filter, stats) and the settings
dialog panes are recorded as M21.4f: they still speak Russian and are visible on every screen.

## M21.4d — the trash in English (2026-11-24)

The app trash was the fourth screen: the note about the OS trash, the buttons, the marks on the
tiles and the confirmation dialog — the one place where a wrong word costs files.

**What moved.** The screen note, `The trash is empty.`, the count (`1 file · 1 B`), `Restore all`,
`Empty the trash (N)` with its hint, the tile marks `in trash` and `restore` (with the hint that the
file on disk was untouched), the statuses `restored from the trash: N`, `file restored from the
trash`, `file not restored`, `trash not emptied`, and the confirmation dialog: `Empty the trash?`,
`Moved to the OS trash: 1 file, 1 B.`, the irreversible note, `Move to the OS trash`, `Cancel`.
The Russian screen text had a doubled gap in the middle of the sentence (a broken literal) — it is a
single sentence again. Sizes go through `db::human_in`.

**Checks.** `cargo test` — 191 passed, 6 `#[ignore]` (new:
`the_english_language_translates_the_trash` — a headless trash frame with an open confirmation
dialog: `Restore all`, `Empty the trash (1)`, `in trash`, `restore`, `Empty the trash?`,
`Move to the OS trash`, `Cancel`, `1 file · `, and no Russian trash label left; the modal needs two
frames, as the viewer test does); `cargo clippy --all-targets` clean; `check.sh` — 44 checks,
0 violations.

**Next.** M21.4e: the integrity screen (three buttons, fields, verdict and report labels, statuses),
then M21.4f — the left panel blocks and the settings dialog panes, still Russian and visible on
every screen.

## M21.4e — the integrity screen in English (2026-11-24)

The last of the five content screens in `plan/I18N.md` M21.4. The screen that decides whether the
archive still matches the backup could not stay Russian while everything around it speaks English.

**What moved.** The title, the read-only note, the three fields (`Folder A (archive)`,
`Folder B (copy)`, `Manifest file`), `Show all paths`, the three buttons with their hints
(`Compare the copies`, `Write the manifest`, `Overwrite the manifest`, `Check against the manifest`),
the running note, the reminder row (`Remind me to check every N days`, `0 — do not remind`), the
last-pass line (`last pass: 2026-11-24 10:00 · matched`), the next-check line, the verdicts
(`the copies match`, `manifest written: 2 files (20 B) → …`, `the folder matches the manifest`, the
stopped and error forms with their counters), `Copy the report` / `Copy the last report` and every
plan error (`please give folder A (archive)`, `folder A and folder B are the same path; nothing to
compare`, `the manifest already exists: …`). `IntegrityJob::title`, `IntegrityOutcome::verdict` and
`IntegrityOutcome::status` now take `lang`; `integrity_plan` and `integrity_start` build their
messages on the same language. The report body is still the text `verify` prints for the CLI: it is
the CLI's output, and M21.6 gives those report functions a `lang` field — the test asserts both
(the labels are English, the report line is still Russian).

**Checks.** `cargo test` — 192 passed, 6 `#[ignore]` (new:
`the_english_language_translates_the_integrity_screen` — a headless integrity frame with a finished
verdict: `Integrity`, `Folder A (archive):`, `Folder B (copy):`, `Manifest file:`, `Show all paths`,
`Compare the copies`, `Write the manifest`, `Check against the manifest`, `Remind me to check every`,
`0 — do not remind`, `the copies match`, `Copy the report`, and no Russian label left; the four
integrity tests that assert Russian set `settings.lang = Ru` themselves, as the card tests do);
`cargo clippy --all-targets` clean; `check.sh` — 44 checks, 0 violations.

**Next.** M21.4f: the left panel blocks (`Сводка`, the model and tag filter, `все теги`) and the
settings dialog panes (model, indexing, tags, faces, storage, tools) — they are still Russian and
visible on every screen. After that M21.5 (viewer, player, folder picker, storage modal) and M21.6
(CLI reports).

## M21.4f — the left panel and the settings panes in English (2026-11-24)

The last M21.4 block: the two places that stay on screen whatever view is open. The left panel sits
next to every screen, and the settings dialog is one click from the top bar — leaving them Russian
while the screens around them speak English would have made the whole effort pointless.

**What moved.** The left panel: the model block (`AI model`, `Compute tags` with its hint, `with a
confident tag: 12`, the free-word-search note, the tag filter with `tag: …` / `mine: …` / `all tags`,
`Filter threshold`, the “no tag passes the threshold” note, `search: 3 files`), the text block
(`Text on the photo`, `with text: 146 of 435 photos`, `candidates: 146, left: 289`,
`Recognize documents (N)`, the tesseract-missing note and the long-pass hint) and the summary
(`Summary`, `Files:`, `Size:`, `No date:`, `No preview:`, `Videos without a cover:`).
The settings dialog: all six panes — model (state, `Check the model`, folder, download limit,
`Network consent` and the download/`Compute tags` buttons, the progress line), indexing
(`Indexing threads`, `Duplicate threshold`, `bits out of 64`), tags (both thresholds, the dictionary
editor with `Remove from the dictionary`, `Add to the dictionary`, `Recompute tags`, export/import and
`Restore the defaults`, plus every dictionary status), faces (threshold, consent, download/check,
progress), storage (`Index`, `DB`, `Cache`, `Models`, `People`, `Compute the used space`, the three
cleanup buttons and the “never writes into the archive” note) and external tools (the PATH note,
`empty — from PATH`, `without it: …`, `Check and save the programs` plus the tool states).
`tools::Tool::purpose`, `Tool::without` and `Tools::source` stay Russian in the module (they are also
`doctor` output) — the GUI wraps them in `t()`; M21.6 will give the CLI the same language. Sizes in
the touched panes go through `db::human_in`, so `KiB/MiB` follow the language. Labels with a number
inside the sentence (`files: 3/9`, `with text: N of M photos`, the dictionary export/import statuses)
are built per language, as M21.4c did; the tag labels, model titles, prompts and paths are data and
stay as they are.

**Checks.** `cargo test` — 194 passed, 6 `#[ignore]` (new:
`английский_язык_переводит_левую_панель` — a headless window frame: `AI model`, `model not
downloaded`, `Model settings`, `Text on the photo`, `candidates: 0, left: 0`, `Summary`, `Files:`,
`Size:`, `No date:`, `No preview:`, and no Russian label left — it needs two frames, because the
bottom summary panel takes its height from the previous frame; and
`английский_язык_переводит_разделы_настроек` — six headless pane frames: model, indexing, tags with
an empty dictionary, faces, storage and tools, including `dims`, `without it: …` and the translated
ffprobe description; the Russian list checks all panes at once); `cargo clippy --all-targets` clean;
`check.sh` — 44 checks, 0 violations; the release build and a 6 s `photocat gui` smoke test — no panic.

**Next.** M21.5: the full screen viewer and built-in player controls, the folder picker (`roots_modal`,
`picker_modal`) and the storage confirmation modal — the last Russian surfaces inside the window.
After that M21.6 gives the CLI reports a `lang` field.

## M21.5 — the full screen viewer, the player, the folder dialogs and the storage modal in English (2026-11-24)

The last Russian surfaces inside the window. After M21.4f the left panel and the settings spoke
English, but the full screen viewer, the player controls, the folder dialogs and the storage
confirmation still answered in Russian — the owner would have hit the half-translated state exactly
where the archive is looked at.

**What moved.** The viewer header (`Close`, the `1 of 3` counter through `i18n::position`, the
`no date` fallback). The playback row: the `previous file — ←` / `next file — →` hints, the play
button hints (`play the video — space` / `pause and resume — space`), `ffplay in its own window`,
`no device`, `Stop the video` with its hint. The slideshow row: `Slideshow` / `Stop the slideshow`
with the “walks the current list” hint, `interval:` and the `3 s` / `5 s` / `10 s` choices through
`i18n::seconds`. The zoom row: `zoom out`, `Fit`, `zoom in`, `zoom 100%`, `Text on the photo`,
`In the system viewer`. The frame itself: the poster overlay `video · press ▶ to play`, `the full
frame is loading…` and `loading the next frame…`; the OCR column with the “no text” note. Every
status the viewer can write: frame not decoded, file not found in the index, opening in the system
player, the built-in player falling back to `ffplay` and then to the system player, `ffplay: the
video is closed`, the video finished, the decoder lagging behind the sound.
The archive folders dialog (`Archive folders`, the note about indexing several folders, the empty
state, `Remove from the index` with its hint, `Add a folder…`, `Index all`, `Close`, the
removed/not-removed statuses), the folder picker (`Add a folder`, `Up`, `Add and index`, `Add
without indexing`, `Cancel`, `No subfolders.`, `cannot read …`, `folder added: …`) and the storage
confirmation (three titles, three texts, three buttons, `Cancel`, `index reset; index a folder now`,
`cache cleared: …`, `cleaning failed: …`) moved in one commit: the picker sits on top of the folders
dialog, and a translated picker over a Russian parent is a half-translated flow.

**Shape.** `viewer_modal`, `start_video_playback`, `poll_video`, `roots_modal`, `picker_modal` and
`storage_modal` read `self.settings.lang` once and pass it down; `draw_viewer_image` and
`paint_play_overlay` take `lang` as an argument, as `draw_face_cell` does since M21.4c. Two new
helpers keep numbers out of the sentence: `i18n::position(lang, i, n)` and `i18n::seconds(lang, n)`.
Dynamic fragments elsewhere are translated as parts (`format!("{}: {path}", t(lang, "folder added"))`).
File names, paths, dates, durations and the video frame stay as data.

**Checks.** `cargo test` — 198 passed, 6 `#[ignore]` (new:
`английский_язык_переводит_полный_экран` — a headless viewer frame for a photo and for a video:
`Close`, `Slideshow`, `Fit`, `interval:`, `3 s`, `Text on the photo`, `In the system viewer`,
`1 of 1`, `the full frame is loading…`, `press ▶ to play`, and no Russian label left;
`английский_язык_переводит_каталоги_и_выбор_папки` — two headless modal frames through the new
`modal_frame` helper: `Archive folders`, the empty state, `Add a folder…`, `Index all`, `Close`, then
`Add a folder`, `Up`, `Add and index`, `Add without indexing`, `Cancel`, `No subfolders.`;
`английский_язык_переводит_подтверждение_очистки` — all three storage confirmations with their
titles and buttons. The two viewer behaviour tests that assert the Russian `нажмите ▶` label now set
`settings.lang = Ru` themselves, as the integrity tests did; and a unit test for `i18n::position` /
`i18n::seconds`: `3 of 12` / `3 из 12`, `5 s` / `5 с`). `cargo clippy --all-targets` clean;
`check.sh` — 44 checks, 0 violations; release build and a 6 s `photocat gui` smoke test on a copy of
the real DB (30 MB, 2230 files, twice) — code 124, no panic.

**Next.** M21.6: the CLI reports get a `lang` field set from `settings.lang` so `index`, `stats`,
`dupes`, `organize`, `verify`, `embed`, `faces`, `ocr`, `doctor` and `settings` print English too —
no global language variable. Then M21.7 (`docs/`), M21.8 (`plan/`), M21.9 (`.pi/`) and M15.6/M15.7.

## M21.6 — the CLI reports in English (2026-11-24)

The last surface of M21: after the window spoke English, `index`, `stats`, `dupes`, `organize`,
`export`, `verify`, `manifest`, `embed`, `tags`, `faces`, `ocr`, `search`, `clusters`, `models`,
`settings`, `albums`, `doctor` and `--help` still answered in Russian. Now they print the language of
`settings.lang`, and the numbers stay numbers.

**How the language travels.** Report structs gained a `lang` field (`index::Stats`, `ocr::Stats`,
`faces::{Stats, ClusterStats, DownloadReport}`, `embed::{EmbedStats, RetagStats, DownloadReport}`,
`verify::{Report, CheckReport, ManifestStats}`, `organize::ExportReport`). The caller sets it before
printing: `main` from `language(&args)` (one `db::open` + `db::ensure_lang` per command), `app` from
`self.settings.lang` when a background pass returns. `Display` impls build the sentence with
`i18n::fill(lang, template, values)` — the translation owns the word order, the code only supplies
pre-formatted numbers. Sizes go through `db::human_in`, so `ГиБ`/`GiB` follow the language.

**Data versus words.** `dupes::Group` kept the reason and the advice as ready Russian strings, so a
group found before a language change would have stayed Russian. `Reason` (`Exact`/`Similar`) and
`Advice { same_content, parts: Vec<AdvicePart> }` now hold the facts, and `label(lang)` builds the
words at display time; the CSV plan and the screen use the same function. `--help` is one translated
block (`i18n::USAGE`), so the menu cannot drift between languages. `tools::Tool::purpose`/`without`
and `Tools::source` take `lang` — the GUI and `doctor` now call one function instead of the GUI
wrapping a Russian string in `t()`.

**Honest tail.** Runtime diagnostics from L1/L2 (`anyhow::bail!` inside `db`, `derive`, `embed`,
`faces`, `verify`, `organize`) still speak Russian: they are error text, not a report, and moving
them would mean threading `lang` through every fallible helper. Recorded in `plan/I18N.md`.

**Checks.** `cargo test` — 205 passed, 6 `#[ignore]` (was 198; new: `fill` substitutes values by
order and keeps `{}` when a value is missing; every translation keeps the same number of `{}` as the
original; a source scan proves every literal passed to `i18n::t`/`i18n::fill` in the reporting
modules exists in `PAIRS`; English `index`/`ocr`/`faces`/`embed`/`organize`/`tools` reports;
`dupes::Reason`/`Advice` on both languages; the three `verify` report bodies; the English `--help`
without a single Cyrillic letter). The four screens' English tests that already existed now assert
the English report body too, and the reminder test sets `settings.lang = Ru` because it checks the
Russian report file. `cargo clippy --all-targets` clean; `check.sh` — 52 checks, 0 violations
(`i18n` allowed for `tools` and the reporting L2 modules; the layer map in the script is the source
of truth).

**On the corpus** (copy of the real DB, 2230 files, release): `stats` prints 2230 files, 21.2 GiB,
258 videos, 3:19:46 — the same numbers in both languages, only the labels and units change;
`dupes --max-dist 6` — 103 groups, 238 files, 214.1 MiB, 0.023 s, the same groups as before the
change; `doctor`, `models list`, `tags --list`, `albums list`, `ocr --dry-run` (384 of 1970),
`faces --dry-run`, `organize` dry run (571 showcase folders), `export` dry run and
`verify`/`manifest` on temporary folders all print English with correct exit codes (1 on a
difference, as before). `timeout 6 ./target/release/photocat gui --db /tmp/m216.db` — exit code 124,
no panic.

**Next.** M21.7: translate `docs/ARCHITECTURE.md`, `docs/UI.md`, `docs/DECISIONS.md` and move the
Russian originals to `*.ru.md`. Then M21.8 (`plan/`) and M21.9 (`.pi/`), then M15.6/M15.7.

## M21.7a — the architecture document in English (2026-11-24)

`docs/ARCHITECTURE.md` now speaks English; the Russian original moved to `docs/ARCHITECTURE.ru.md`
with a link from both files (`README.md`/`README.ru.md` keep the same shape). The document is a
translation, not a rewrite: the layer table, the indexing flow, the SQL schema block, the video,
export, integrity, reminder, duplicate-index and model-catalogue sections, and all **89 measurement
rows** are the same, with dates and numbers untouched. Real paths and names stay as data
(`Фотографии с телефона`, `8.jpg`, `Дача 2024 → Май → Неделя 1`, the Russian search queries).

**Renamed headings.** «Замеры» → "Measurements", «Схема БД» → "DB schema", «Поток индексации» →
"Indexing flow", «Долгие проходы» → "Long passes", «Кросс-платформенность» → "Cross-platform".
`AGENTS.md` already cites § "Measurements", so the English heading is the one the constitution
expects. The Russian plan documents that point at these sections were retargeted to
`docs/ARCHITECTURE.ru.md` in the same change, so no link now leads to a heading that does not exist.
Units in the tables are `MB`/`MiB`/`GiB`, as in `README.md`; the one place where both spellings
matter (`stats` in two languages) keeps `ГиБ`/`GiB` as the observation.

**Checks.** Both files have 89 measurement rows and the same section count; no code was touched, so
`cargo test`/`clippy`/`check.sh` are unaffected by this commit.

**Next.** M21.7b: `docs/UI.md`.

## M21.7b — the interface document in English (2026-11-24)

`docs/UI.md` is now English; the Russian original is `docs/UI.ru.md`, and both files link to each
other. Everything that makes the document normative survived: the three rule blocks ("Rules that are
not up for debate", "Layout", "Interaction"), the "Findable in a second" table, the 27-row metrics
table, the screen table, the long state list, the keyboard table, the wording rules and the debt
list. The pointer to the normative sources (`.pi/skills/ui-design/references/*.md`) is unchanged.

**Labels quoting the UI.** The English variant quotes the labels the owner sees in English mode
(`Index`, `By months / Flat`, `LIBRARY`/`TOOLS`, `Keep`/`spare`, the integrity verdicts) and says at
the top of § Wording that the code keeps the Russian string as the translation key (`i18n::PAIRS`,
`plan/I18N.md`). Screen names are the view names (`Grid`, `Duplicates`, `Calendar`, `Faces`,
`Albums`, `Trash`, `Integrity`), so a section reads the same as the `View` enum.

**One stale item fixed in both variants.** The debt list claimed "there is no multi-selection of
tiles" — false since M18, which the same document describes. Item 9 now says what is actually
missing: decisions on duplicates are still made one tile at a time; the multi-selection works in the
grid, in albums and in the trash, and batch actions over duplicate groups are a separate task.

**Links.** Russian plan documents that cited `docs/UI.md` §Долг/§Взаимодействие were retargeted to
`docs/UI.ru.md`, so a Russian section name never points into the English file. Both UI files have the
same 67 table rows; no code was touched.

**Next.** M21.7c: `docs/DECISIONS.md`.

## M21.7c — the decisions log in English (2026-11-24)

`docs/DECISIONS.md` is now English; the Russian original is `docs/DECISIONS.ru.md`, both files link
to each other. All **40 ADRs** are there with the same ids, the same "forbids" lines, the same tables
(0001 frameworks, 0019 programs, 0031 player crates) and the same measurements (0027, 0030, 0034,
0035, 0036, 0038, 0039).

**Two things changed beyond translation.** 0009 ("Russian UI, English identifiers") carries a
"Superseded" note pointing at 0040: the window has been bilingual since M21, the identifiers stay
English and `db::human` is joined by `db::human_in(lang, n)` for the GUI — the note is in both
variants, because the old wording alone now contradicts `docs/UI.md`. Inside the Russian variant the
cross-references point at `docs/UI.ru.md` §Метрики and `docs/ARCHITECTURE.ru.md` §Замеры, so a Russian
section name never points into an English file; `AGENTS.ru.md` was retargeted the same way.

**Checks.** Both files have 40 `## NNNN.` headings, one `### ` subheading (0001), and the ADR ids
match one to one. `check.sh` greps `docs/DECISIONS.md` and `docs/ARCHITECTURE.md` for every Cargo
dependency; the English files keep every crate name (`rff-*`, `rusty_h264`, `cpal`, `ort`,
`turbojpeg`, `trash`), so the dependency check still finds its evidence. No code was touched.

**Next.** M21.8: `plan/STATUS.md`, `plan/BACKLOG.md` and the remaining `plan/*.md`; then M21.9
(`.pi/skills`, `.pi/prompts`), then M15.6/M15.7.

## M21.8a — the status log in English (2026-11-24)

`plan/STATUS.md` is English now; the Russian original is `plan/STATUS.ru.md`, both files link to each
other. All **43 sections** are there with the same dates, the same tables (the corpus measurements,
the player numbers, the tag percentiles, the M20 export) and the same numbers; the code references
(`src/*.rs`, `settings.*`, `SCHEMA_VERSION`), the test names and the corpus path stay as data.

**What changed beyond translation.** Entries from M21 on were already written in English and are kept
verbatim in both variants (the Russian file says so in its header): the history above them was
translated by hand, entry by entry, in chronological order. References to the behaviour documents
were retargeted — `docs/UI.ru.md`/`docs/ARCHITECTURE.ru.md`/`docs/DECISIONS.ru.md` became
`docs/UI.md`/`docs/ARCHITECTURE.md`/`docs/DECISIONS.md`, because a Russian section name must not
point into an English file; `plan/STATUS.ru.md` keeps the Russian ones. Quotes («») became "…" and
the Russian labels that appear as evidence (the `сетка_группирует_по_месяцам_от_свежих` test, `КиБ`
against `KiB`, the LOOKING-FOR strings of a translated screen) stayed Russian: they are data.

**Not translated.** `plan/M3-clusters.md` — a local report with archive paths, it is in `.gitignore`
and is not documentation.

**Checks.** Both files have 43 `## ` sections and one `# `; the English file has 23 lines with
Cyrillic left and every one of them is a quoted string or a corpus path. `plan/I18N.md` moved the
M21.8 row from `todo` to done in three steps (`8a` STATUS, `8b` BACKLOG, `8c` the remaining
`plan/*.md`). No code was touched: `cargo test`, `clippy` and `check.sh` are unaffected.

**Next.** M21.8b: `plan/BACKLOG.md`.

## M21.8b — the backlog in English (2026-11-24)

`plan/BACKLOG.md` is English now; the Russian original is `plan/BACKLOG.ru.md`, both files link to
each other. All **26 task blocks** are there with the same ids, the same acceptance criteria and the
same measurements (indexing 64.9–69.3 s, 103 duplicate groups, 2498 faces in 630 s, the M20 export
2045 ms for 500 files, `dupes` 0.023 s after M21.6); the paths, the crate names, the CLI commands and
the corpus folder stay as data. The M21 block was already English and is kept verbatim in both
variants; references to the behaviour documents were retargeted from `docs/*.ru.md` to `docs/*.md`.
Two criteria said "the texts in Russian" (M9, M17) — they now say "the texts in English", because the
window has been bilingual since M21; the criteria themselves are unchanged.

**Checks.** Both files have 26 `## ` blocks and one `# `. `plan/I18N.md` moved M21.8b to done. No
code was touched.

**Next.** M21.8c: `plan/SESSION.md`, `plan/M15-tags.md`, `plan/M16-interface.md`,
`plan/M19-albums.md`.

## M21.8c — the plan documents in English (2026-11-24)

The remaining plan documents are English now, each with its Russian original next to it and a link
from both sides: `plan/SESSION.md` + `.ru.md` (the M13–M14c log), `plan/M15-tags.md` + `.ru.md`
(the tag diagnosis, the five done steps and the three left), `plan/M16-interface.md` + `.ru.md`
(the analysis that led to ADR 0033) and `plan/M19-albums.md` + `.ru.md` (the album schema and the
collision table of the export). All 7 `#`/`##`/`###` headings, all the tables (the `8.jpg` weights,
the collision table, the M19/M20 measurements) and every number are kept; the example album names
in the CLI snippets are English (`Dacha 2024`), while the corpus folder stays as it is on disk.

**Not translated.** `plan/M3-clusters.md` — a local report with archive paths; it is in `.gitignore`
and is data, not documentation.

**Cross-references.** Inside the Russian variants every reference to a plan document that has a
pair now points at the `*.ru.md` file — in `AGENTS.ru.md`, `docs/*.ru.md`, `README.ru.md` and the
Russian plan files themselves — so a Russian reader never lands in an English document. The English
files point at the English one, and `plan/M3-clusters.md` keeps its single name in both.

**Checks.** The heading trees of every pair match (`SESSION` 7/7, `M15-tags` 17/17,
`M16-interface` 8/8, `M19-albums` 15/15); the English files keep only the link line and the corpus
path in Cyrillic. `plan/I18N.md` closes the M21.8 row and the "Repository" bullet lists all the plan
pairs. No code was touched.

**Next.** M21.9: `.pi/skills/*` and `.pi/prompts/*`.

## M21.9 — the repository skills in English (2026-11-24)

The last Russian layer of the repository is English now. Eight documents: the four
`.pi/skills/*/SKILL.md` (`add-view`, `corpus-bench`, `enforce-architecture`, `ui-design`) and the
four normative references of `ui-design` (`verstka`, `vzaimodeystvie`, `nayti-za-sekundu`, `revu`).
Each has a Russian original next to it as `*.ru.md`, both files link to each other; the Russian
variants point at `docs/*.ru.md`, `plan/STATUS.ru.md`, `AGENTS.ru.md` and the Russian
`references/*.ru.md`, the English ones at the English documents. All headings, tables, numbers and
code references are kept (indexing 84.7 s, 103 duplicate groups / 238 files / 214.1 MiB, 2229
embeddings in 77.3 s, 435 OCR candidates, the 12 review checks, the 4 px grid and the 150+26 tile).

**The prompt and the scripts.** `.pi/prompts/next-task.md` was already English, so it has no pair;
only its line about an untranslated document now says "If `plan/I18N.md` marks a document as still
Russian, the Russian original is the source of truth." The two scripts — `check.sh` and `bench.sh` —
are code, not documentation: their comments and runtime messages (`ok`, `ERROR`, `WARNING`,
`RESULT: boundaries are in order (52 checks)`, `indexing time`, `clustering time`) are English now,
while the checks and thresholds are unchanged. `bash -n` passes on both; `check.sh` — 52/0.

**Made true by the translation (both variants).** The placement map says seven views, not "four";
the navigation rule lists `All files`, `Albums`, `Calendar`, `Faces`, `Duplicates`, `Integrity`,
`Trash`; the review's honest list dropped "multi-selection does not exist" (M18 added it) and keeps
the remaining five items; the interaction rules and the review rubric name the two real confirmations — "Empty the trash"
(the OS trash) and cleaning the cache/index — instead of "there are no confirmations". The
references still describe the interface of their last revision; the current behavior document is
`docs/UI.md`.

**Checks.** Heading trees match in every pair (`verstka` 9/9, `vzaimodeystvie` 12/12,
`nayti-za-sekundu` 9/9, `revu` 7/7; `ui-design` 8/8, `add-view` 7/7, `corpus-bench` 7/7,
`enforce-architecture` 12/12). The English markdown keeps Cyrillic only in the link line and in the
corpus path. No code was touched: `cargo test`, `clippy` and `check.sh` are unaffected.
`plan/I18N.md` closes the last row and records the skill pairs.

**Next.** M21 is done. The queue returns to M15.6 (a multilingual model and calibration) and M15.7
(a check on two folders).

## M15.6 — a multilingual model and the calibration of the tag weight (2026-11-24) ✅

M12 left the tail: "SigLIP understands only English and has a low cosine scale — a confident tag of
0.22 almost never fires". The task was a fourth model (SigLIP 2 multilingual) plus calibration of the
thresholds. Done in `src/embed.rs` (catalogue entry, `pad_left`, `score_calib`, `score_version`,
`tag_source`); `derive.rs`, `db.rs`, `app.rs` and the schema did not change, the originals were not
touched.

**What appeared.**

- **The fourth model.** `siglip2-base-224-multi`: SigLIP 2 base patch16-224
  (`onnx-community/siglip2-base-patch16-224-ONNX`, Apache-2.0), 768 features, 393.3 MiB (vision 90.2,
  text 270.3, tokenizer 32.8 — the text tower carries a 256k-token Gemma vocabulary), three files with
  pinned URLs and sha256, read through the existing `TextKind::Pooler`. The ONNX graph was checked
  first: `pixel_values → pooler_output` 768, `input_ids → pooler_output` 768, no attention mask.
- **The padding rule is data.** The SigLIP 2 tokenizer pads to 64 on the right with `<pad>` itself,
  and the pooler reads the last token — so every text collapsed into one vector (cosine "a dog"/"a
  cat" 0.976) and the tags stopped differing at all: 0 confident tags, "документ" on a photo of cats.
  `pad_left` strips that tail and pads on the left so the EOS stays last; with it the same four test
  frames (lake, car, desk, portrait) are matched correctly 4/4, whereas the right-padded variant gets
  0/4.
- **Calibration.** The threshold must mean the same thing on every model, so the stored tag weight is
  mapped by the model's own affine map `cos * scale + shift` into the CLIP scale; the coefficients
  put the median best tag (0.263) and the median lead (0.0118) of CLIP where they are: SigLIP 2
  `(1.42, 0.14)`, SigLIP 1 `(0.55, 0.23)`, CLIP identity. The percentile filter is untouched by it
  (a monotone map), and `tags.source` carries `@v2` for a calibrated model, so rows of an older
  calibration are never mixed in; `embed --retag` rewrites them in 2–4 s without reading a photo.
- **The old tail is fixed.** `siglip-base-224-en` had 0 % confident tags at 0.22; after the
  calibration it has 55.6 % — the same as CLIP — at the price of a lower document recall (53.5 %).
- **SigLIP 2 is opt-in.** The default model stays `clip-vit-b32-multilingual`: SigLIP 2 does not fit
  the 250 MiB limit (the owner raises it to 400), it is 2.7× slower per file, and it is worse on the
  document category. `models download` refuses it at 250 MiB naming both numbers; nothing is
  downloaded without a click.

**Measurements** (`/tmp` DB, release, the phone corpus 2229 files + "день рожденья" 111, 23.3 GiB;
the full table — `docs/ARCHITECTURE.md` § Measurements):

| Model | `embed` | best tag, median | lead, median | confident | documents with text caught | false "document" without text |
|---|---|---|---|---|---|---|
| CLIP ViT-B/32 multilingual | 147 s (0.063 s/file) | 0.263 | 0.0118 | 55.6 % | 85.9 % | 3.3 % |
| SigLIP 2 base 224 multilingual | 398.8 s (0.170 s/file) | 0.263 | 0.0118 | 55.3 % | 69.0 % | 9.7 % |
| SigLIP base 224 English | 404.5 s (0.173 s/file) | 0.262 | 0.0119 | 55.6 % | 53.5 % | 1.0 % |

The picture repeats in each folder separately (CLIP 55.4 % / 61.3 % confident, SigLIP 2 54.8 % /
64.0 %), so the calibration is a property of the model, not of a folder. Ground truth for the document
category is the OCR text (373 candidates, 142 with text, 68 218 characters, 83.1 s). Where both models
are confident (815 files) their labels agree in 629 cases (77.2 %). The vectors of four models cost
18.4 MiB and the DB 62.8 MB.

**Not done, deliberately.** A comparison with CLIP ViT-L/14 (over 500 MiB, over any sane limit); the
fp16 SigLIP 2 text tower (538.7 MiB) to separate int8 damage from the model itself — that question was
later closed by measurement (§ M15.6 tail below: int8 costs no quality). (The labelled check these
numbers were waiting for was done in M15.7 below.)

**Checks.** `cargo test` — 207 passed, 6 `#[ignore]` (new: the catalogue of four models with unique
ids/folders, sha256, licence and identity-only-for-CLIP calibration; the calibration puts both SigLIP
medians onto CLIP's; `tags.source` carries `@v2`; the padding test keeps the EOS last and reproduces
the right padding for CLIP); `cargo clippy --all-targets` — clean; `check.sh` — 52 checks, 0
violations; `embed --retag` on the bench DB reproduces the measured numbers; `models list` shows four
models with sizes, licences and the limit.

## M15.7 — a blind labelled check of the tags on two folders (2026-11-24) ✅

The tag work of M15.1–M15.6 was argued with proxies: the share of confident tags, the leading label
on `8.jpg`, the agreement of two models, OCR text as ground truth for documents. All of them can
rise while the tags get worse. This task replaced the proxies with a sample a human labelled, scored
by code, on two independent folders fixed before the labelling.

**What appeared.** A new L2 module `src/eval.rs` and the `photocat eval` command, no DB change, no
new dependency, no touching the archive:

```
photocat eval --folder DIR --out OUT [--sample 120]   # sample, labels.csv, guesses.csv, sheets
photocat eval --labels OUT/labels.csv                 # the numbers
```

- **The sample is reproducible and blind.** The folder's files (preview + tags, not in the trash)
  are sorted by `sha256` and every k-th is taken — content, not names or dates. `labels.csv` holds
  only `n,path,label`; the model's top-3 go to a separate `guesses.csv`, so nothing in the tile
  being labelled shows the prediction.
- **Contact sheets** (`sheet-NN.png`, 16 tiles of 320 px, the number burned in with a 3×5 bitmap
  font — no font dependency) are built from the previews already in the cache; re-running the export
  keeps the labels already filled in.
- **The labels** are dictionary ids (`furniture`, `children`, …), `other` (no dictionary label is
  the subject — an adult portrait, a household detail), `skip` (cannot judge), or two ids joined by
  `|` when both are equally fair for a boundary shot. Unknown ids stop the scoring with the list of
  valid ones.
- **The report** counts top-1/top-3 over the shots a dictionary label fits, the coverage itself, the
  accuracy of a *confident* tag (the same `tag_show` + `tag_margin` rule the card uses for
  "not sure"), the "confidently wrong on `other`" count, a per-tag table, and the furniture recall at
  the 100 %/70 % percentile filter.

**The numbers** (CLIP ViT-B/32 multilingual, the default; 120 shots per folder, 21.2 GiB + 49 MiB,
`/tmp` DBs, release, 2026-11-24). Folder A — `Фотографии с телефона` (2230 files, 1969 with tags);
folder B — `фото с Wats app от Лизы` (289 files):

| | A | B |
|---|---|---|
| labelled / `other` / skipped | 82 / 38 / 0 | 86 / 34 / 0 |
| dictionary covers | 68.3 % | 71.7 % (a second label allowed on 22 shots) |
| top-1 | 70 (85.4 %) | 71 (82.6 %) |
| top-3 | 80 (97.6 %) | 84 (97.7 %) |
| confident tag right | 55 of 69 (79.7 %) | 49 of 62 (79.0 %) |
| confidently wrong on `other` | 14 of 38 (36.8 %) | 13 of 34 (38.2 %) |
| furniture at 70 % / 100 % | 13 of 13 / 0 of 13 | — (no furniture in the sample) |
| `embed --refresh` | 126.1 s for 2231 files (56.5 ms/file) | 16.7 s for 289 (57.8 ms/file) |
| indexing | 84.7 s (the corpus-bench baseline of M4) | 1.8 s for 289 files |
| DB after CLIP | 30 MB (with OCR and faces) | 2.3 MB |

Renders and screenshots are in both samples: 3 documents + 1 screenshot on A, 2 documents + 3
screenshots on B, plus the watermarked furniture renders that make up most of A's furniture and
interior shots. The furniture percentile row at 100 % is a sanity check and reads 0 of 13 by
construction — the threshold is the single strongest shot of the whole index (0.306); at 70 %
(0.235) the filter shows 676 shots of 2229 and keeps all 13 labelled furniture shots.

**The honest costs.** A third of both samples is `other`: 14 common labels cannot name an adult
portrait or a household detail. On a third of those the model is not unsure but **confidently
wrong** — the card highlights a wrong label, which is exactly what the "not sure" rule (M15.3) was
meant to limit; the rule fires when the lead is small, not when the model is confidently off. The
per-tag table shows where the weight sits: "children" 29 of 32 on A and 42 of 45 on B, "animals" 19
of 19 on A, while "furniture" is 8 of 13 top-1 (12 of 13 top-3) — the renders are still the weak
spot from §3.

**Not done, deliberately.** The labels are one reviewer's reading of 320 px previews, so the owner's
re-read is the next honest step; the precision of the furniture filter needs a labelled top-N of the
tag (a sample chosen by the model's own weights is not random) and was not attempted.

**Checks.** `cargo test` — 216 passed, 6 `#[ignore]` (new: the sample stride is even and repeatable,
a path with a comma and a quote survives the CSV roundtrip, label parsing of `other`/`skip`/`a|b`,
the metrics of a synthetic set including the confident-wrong count and the per-tag table, an unknown
label stops with the list of valid ones, the percentile recall on a known distribution, the digit
printer stays inside the sheet); `cargo clippy --all-targets` — clean; `check.sh` — 52 checks, 0
violations; the same labels re-scored after `embed --refresh` reproduce the table.

**Next.** `plan/SESSION.md` — the queue: the owner's re-read of the labels; the fp16 tail of M15.6 is
closed (see below) and SigLIP 2 stays opt-in.

## M15.6 tail — the fp16 towers against int8 (2026-11-24) ✅

M15.6 left an honest doubt: SigLIP 2 caught 69.0 % of the shots with recognized text where CLIP caught
85.9 %, and `onnx-community` ships fp16 towers next to the int8 ones — part of the loss could have been
the 8-bit quantisation of the text tower. The question was answered by measurement, not by an opinion,
and the answer is no.

**How.** A bench copy of the model folder (`/tmp`, symlinks) and a temporary edit of the catalogue's
file list for the length of each run; no catalogue entry was added and no default changed. The same
2231 files, the same tag dictionary, the same `tag_show` 0.22 / `tag_margin` 0.01 rule, the same
calibration protocol as M15.6 (raw cosines inverted with the coefficients in force, medians mapped to
CLIP's 0.263 / 0.0118):

| towers (vision + text) | model | confident tags | documents with text caught | false “document” without text | `embed --refresh` |
|---|---|---|---|---|---|
| int8 + int8 (the catalogue entry) | 393.3 MiB | 55.2 % (1231) | 69.0 % (100/145) | 10.0 % (183/1828) | 510.0 s |
| int8 + fp16 text | 661.6 MiB | 55.8 % (1246) | 64.8 % (94/145) | 12.7 % (232/1828) | — (`--retag` 8.3 s) |
| fp16 + fp16 | 748.9 MiB | 55.4 % (1235) | 49.0 % (71/145) | 3.6 % (65/1828) | 528.7 s |

**The answer.** int8 costs no tag quality. The confident share is the same on all three variants; the
fp16 text tower alone is *worse* on documents (64.8 % against 69.0 %) and raises false “document”
labels (12.7 % against 10.0 %); the fully fp16 model keeps the share but trades document recall for
precision (49.0 % caught, 3.6 % false) — it is less document-biased, not better at finding documents.
The document weakness belongs to SigLIP 2 itself, not to the 8-bit files, so the M15.6 conclusion
stands: the default stays the multilingual CLIP and SigLIP 2 stays an opt-in entry.

The three calibrations come out as one would expect from a working map — `(1.4434, 0.1381)` for int8,
`(1.1803, 0.1528)` for int8 vision + fp16 text, `(0.7827, 0.1902)` for fp16 — and each puts the medians
on CLIP's; the catalogue's `(1.42, 0.14)` for int8 reproduces 55.2 % / 69.0 % / 10.0 % — M15.6 had
54.8 % confident on this folder, 69.0 % of the documents caught and 9.7 % false on the combined set.
The price of fp16 would
have been 1.9× the weights on disk and in RAM (748.9 against 393.3 MiB) and a slower model load for
no gain; a process-level `search` (model load + the sha256 of every weight file included) takes
2.8–5.1 s on every variant, the text encoding itself is milliseconds.

**What was tried and not kept.** ORT on CPU does run the fp16 graphs; the only code needed was a
15-line reader of an `f16` output tensor (`ort` feature `half` + `half::f16`). Since no variant won,
the reader, the dependency and the temporary catalogue data were reverted — the source tree is
unchanged by this task. The weights (Apache-2.0) were fetched by hand, outside the app and outside the
repository, into `/tmp`: `vision_model_fp16.onnx` 186 039 516 B, sha256
`a1959f7bd3993a607e48839f6d01e25b876fe76afda301b028b78eef68aabd95`; `text_model_fp16.onnx`
564 862 230 B, sha256 `711da56ada0a4aa11c7dd3320df741081a3cae4f0ae1b5e5c6d5b294738d0eb0`; both from
`onnx-community/siglip2-base-patch16-224-ONNX`. They are not in the catalogue, so `models list` still
shows four entries and nothing new is downloaded by the app.

**Checks.** The bench DB reproduces every number above (`/tmp/m156/bench.db`, a copy of the M15.7 DB);
`cargo test` — 216 passed, 6 `#[ignore]`; `cargo clippy --all-targets` — clean; `check.sh` — 52 checks,
0 violations, run after the revert. ADR 0043.

## M13d — panning the zoomed frame in the full screen (2026-11-24) ✅

The last item of the viewer debt: above 100 % the frame was centred and cropped, so a face in a
group photo or a line of a document could not be looked at. Now the frame moves under the cursor.

**What appeared.** `Viewer::pan` keeps the shift in screen points; `draw_viewer_image` takes it and
the frame size, applies the drag delta of the left button and clamps the result with the new
`clamp_pan`: the frame never leaves the area, and a side that is fitted stays centred even when the
other one is zoomed. The cursor is a hand (`Grab`, `Grabbing` while dragging); at 100 % there is
nothing to drag, so the hand does not appear. A new file (`viewer_step`) and "Fit" reset the shift,
so another snapshot does not open cropped. The hint "перетаскивание — сдвиг кадра" /
"drag to move the frame" is drawn next to the zoom percentage only above 100 % and goes through
`i18n::t`; a video frame keeps its click (start/pause) and is not panned. `draw_viewer_image` now
takes the area as one `Vec2` (seven arguments), so the clippy `too_many_arguments` warning stays
away.

**Checks.** `cargo test` — 219 passed, 6 `#[ignore]` (new: the clamp on a fitted, a doubled and a
one-axis-zoomed frame; a file change resets the shift; the hint appears only above 100 %);
`cargo clippy --all-targets` — clean; `check.sh` — 52 checks, 0 violations;
`timeout 6 ./target/release/photocat gui --db /tmp/photocat-m13d.db` — code 124, the window lived
to the timeout, no crash and no output in the log. `docs/UI.md` lost debt item 5 (the rest are
renumbered, the reference in the `ui-design` skill follows), and the metric row, the screen row and
the pointer table describe the drag.

**Not done, deliberately.** No wheel zoom, no double-click "fit ↔ 100 %", no panning of the video
frame: the video is fitted by design, and no gesture is promised that the code does not do.

## M22.2b — scrolling with a trackpad and the wheel (2026-11-24) ✅

The owner's remark after the main screen: on a trackpad the grid was heavy to scroll, and the wheel
was slow. Both were true and both had the same cause — nobody had ever set the profile: egui's default
wheel line is 40 px (50 notches to cross a month on a 2000-row grid), and on Linux and Windows winit
reports `MouseWheelSource::Unknown`, so a precision trackpad got no inertia at all and every swipe
died the moment the fingers lifted.

**What appeared.** One profile, in two places. `apply_theme` sets `line_scroll_speed = 280 px` — a
third of the 840 px default window per wheel notch — raises the scroll-to animation to 2400 pts/s over
0.10–0.30 s and puts `kinetic.decay_time` at 0.35 s, so the same numbers drive egui's own coasting on
macOS. `Scroll` (`src/app.rs`) does the trackpad half that egui cannot: it estimates the gesture
velocity from point deltas (smoothed over 50 ms), and once no gesture event has arrived for 80 ms it
keeps adding `v · dt` to `smooth_scroll_delta` with exponential decay until the remaining path is
under 6 px. A new touch, the opposite direction or a wheel notch drops the coast in the same frame, so
a direction change never jumps. Idle frames request nothing (`pump` returns `false`). The model
deltas on Wayland (physical pixels ÷ pixels-per-point) are one-to-one with the fingers, so only the
coast is added on top. A source-scan test forbids `wheel_scroll_multiplier` and per-view
`scroll_source`, so the grid, the left panel, the card and the dialogs move the same way.

**Measured** (a copy of the real DB, 2232 files, release, 2026-11-24). Trackpad flick of 1200 px/s
(6 frames × 20 px): the coast carries the grid 274 px in 81 frames (≈1.35 s) and stops on its own; a
wheel notch is 280 px (a third of the window), so a full-height screen is three notches. 160 headless
frames of continuous scrolling over the grid (80 wheel notches, then an 80-frame gesture with the
coast), 1240×840: p50 6.4–6.7 ms (149–157 fps), p95 6.6–6.9 ms (145–152 fps), p99 6.7–7.4 ms over
six runs — the 16.7 ms budget of 60 fps is never touched, and the preview decode (8 textures per
frame) is included. The full table row is in `docs/ARCHITECTURE.md` § Measurements.

**Checks.** `cargo test` — 233 passed, 7 `#[ignore]` (new: the wheel notch is a third of the window
and the profile is the same in both themes; a flick coasts 200–340 px and stops; a new gesture and a
wheel notch cancel the coast without a jump; Ctrl/Cmd + gesture is a zoom and never coasts; nothing is
requested in idle; no `ScrollArea` sets its own multiplier; an end-to-end frame test proves the grid
itself moves — with the injection removed it fails, and with the default 40 px notch it fails too, so
both halves are really checked; the `#[ignore]` `замер_прокрутки_сетки` reproduces the frame numbers
from `PHOTOCAT_BENCH_DB`).
`cargo clippy --all-targets` clean; `check.sh` — 52 checks, 0 violations; `timeout 6
./target/release/photocat gui --db /tmp/m222b.db` — code 124, no panic. `docs/UI.md` § Interaction
carries the numbers.

**Not done, deliberately.** No smooth zoom with the wheel, no page-key scrolling, no per-region
speed: the profile is one for the window, as `docs/UI.md` requires.

## M22.2a — the list view scales with the size slider (2026-11-24) ✅

The owner's remark after the main screen: the slider must not disappear in the list view. Hiding it
made the panel header jump on every switch, and a list of 1976 rows has as much to scale as a grid.

**What changed.** The "Tiles" slider, its label and the three view buttons are now drawn in **every**
cell view — one block of constant height, so the right-hand group of the header never shifts; only
the tooltip follows the view (`tile_hint`: tile width / row height / list row size, three sentences
in `i18n::PAIRS`). `draw_list` takes its metrics from the same `settings.tile_size` and a single
clamp 100–260 px: the row is `24 + 0.16 × value` high (40 px at 100, 65.6 px at 260), the thumbnail
`row − 8` high in 4:3 (42.7×32 → 76.8×57.6), the name 12 → 14 px and the mono date · size 10 → 12 px
(`list_row_height`, `list_name_px`, `list_meta_px`). The two text lines are centred as one block, the
kind/duration rides the name's top line and the keep/restore button is centred, so nothing is clipped
at either end of the slider.

**Numbers (headless frames, release, 2026-11-24).** Two rows of `draw_list`: 90 px at 100 and
141 px at 260 — the 6 px frame cursor is constant, the difference is exactly 2 × 25.6. The header
frame draws "Плитки 200" in the tile, rows and list views. No corpus pass: the change touches layout
only, no decoding, no DB column and no file operation.

**Checks.** `cargo test` — 225 passed, 6 `#[ignore]` (new: the list scales with the slider, its
headless frame measures the row height, and the slider is visible in all three views); `cargo clippy --all-targets` — clean; `check.sh` — 52 checks, 0
violations; `timeout 6 ./target/release/photocat gui` — the window lived to the timeout, no panic.
`docs/UI.md` § Layout and § Metrics carry the new list row and the always-present slider.

**Not done, deliberately.** The slider's minimum and maximum stay 100–260 px (a larger thumbnail is a
separate decision on the corpus), the columns of the list are unchanged, and the trackpad/wheel
smoothing is M22.2b.

## M22.1/M22.2 — two themes and the main screen after the mock-up (2026-11-24) ✅

The owner brought a mock-up set made outside the repository: seven HTML screens, a dark and a light
design system, and the rule "for screens without a layout, build by the same rules". The digest is
`plan/M22-ui-spec.md`; the decision is ADR 0044.

**What appeared.** `app::Theme` with two palettes (`DARK` — cool graphite `#0c0e12`/`#1a1c20`, amber
`#f59e0b`; `LIGHT` — slate `#f8fafc`/`#ffffff`, amber `#d97706`), `settings.theme` (`dark | light |
system`, key/value, `SCHEMA_VERSION` unchanged), `apply_theme` filling `egui::Visuals` from the tokens
and setting the mock-up's radii (4/6/8 px), the type scale 10/11/12/13/14/18 and the mono/caps
helpers; "Settings →
Interface" has "Interface theme". The main screen follows `photocat_1`: a 32 px top strip (brand,
search mode, query, Find/Reset, the archive state, Folders/Details/Settings) and a 28 px bottom strip
(the last fact, the counters and the selection in monospace); the folder field and "Index / Refresh"
move into the left panel above the navigation; navigation rows have a 2 px accent edge and the card
tone when active; the content panel is the canvas tone and its title is 18 px; a tile is an 8 px rounded card
with a 1 px frame and a 22 px caption band (the file name in mono 10 clipped against the date, the
date in `text_dim`); selection is a 2 px accent line plus a square check flag; video keeps the
`▶ M:SS` strip. Red (`danger`) appears only on "To trash".

**Checks.** `cargo test` — 222 passed, 6 `#[ignore]` (new: the palette switches, the radii are 4/6/8, the theme
survives a settings roundtrip, the proportion rows pack four 4:3 frames into 700 px at the target
height and keep a lonely panorama at it, an EXIF-rotated JPEG is stored and previewed in the displayed
orientation, the 7 → 8 migration keeps rows and clears only image previews); `cargo clippy --all-targets` — clean; `check.sh` — 52
checks, 0 violations; `timeout 8 ./target/debug/photocat gui` — the window lived to the timeout, no
panic and no output. No file operation, no network call and no schema change: `theme` is a settings
row like `lang`.

**Owner's review on the same day.** The mock-up's tile is back as the **main cell view**
(`settings.cell_style = tiles`): one width from the "Tiles" slider, the columns share the width, a 4:3
frame and a 22 px caption band, and the snapshot is *fitted inside whole* — no crop, no stretch. The
proportion rows became a **second view** (`rows`: the row height from the slider, the row fills the
width, the tile width follows the frame, no margins) and a **third, `list`**, was added: a 44 px row
with a 58×40 thumbnail, the name, the date and the size. All three share one entry point
(`draw_files`) and the same click/selection rules. The grid gap grew to the mock-up's **10 px**
(`gap-2.5`), month groups are separated by 24 px.

Previews now follow the **EXIF orientation** (ADR 0045): `analyze_image` decodes through
`load_oriented`, so a phone portrait is upright and `files.width/height` are the displayed dimensions;
the cached preview is overwritten when its own dimensions do not match the expected ones. On the
corpus this showed on `IMG_20240822_124033.jpg` (orientation 6): now 1792×3904 in the DB and a 147×320
preview (it was 3904×1792 and a sideways 320×147 before). Re-indexing 1972 images / 3.8 GiB took
252 s in the dev profile (release is about four times faster); 258 videos were skipped, one broken JPEG
(`P_20200724_154351.jpg`) is still reported as before. `SCHEMA_VERSION` 7 → 8 clears `files.thumb`
for images once, so an old DB cures itself on the next `index`; videos, tags and albums are untouched.

The radii came back: the rendered mock-up screens round their
controls, navigation rows and cards, so `apply_theme` now sets 4 / 6 / 8 px by role (the design-system
prose claimed 0 px; the screen wins, as with the palette), and the preview corners are cut to the card
radius. The answers to the disputed questions are recorded in `plan/M22-ui-spec.md` §5: amber stays,
no bundled fonts (the built-in families, the binary must not grow), previews keep the fit, favourites
become a later task, albums keep the nested tree, the selection stays in the chips row.

**Not done, deliberately.** Screens other than the main one keep the old visual details until their
task (M22.3–M22.11); the sticky month heading is M22.4; bundled fonts, cropped 4:3 previews, a
floating selection bar, favourites, ratings, share/delete and the mock-up's non-existent EXIF, GPU,
tray and update functions are refused (§5 of the spec, with the recommendation to the owner).
