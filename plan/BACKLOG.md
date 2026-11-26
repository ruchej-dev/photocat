# BACKLOG

*Русская версия: [BACKLOG.ru.md](BACKLOG.ru.md).*

One task — one block: the goal, the acceptance criteria, the files. The order = the order of work.
Take a task — move it to `plan/SESSION.md` and put `STATE: doing:<ID>` into `plan/STATUS.md`.

---

## M1 — indexing, duplicates, arranging, GUI ✅ (done)

sha256 + dHash + EXIF + previews → SQLite; duplicates and series; a date plan with hardlinks; a GUI
on egui.

---

## M2 — acting on duplicates ✅ (done)

**Goal.** Turn the report "here are 103 groups" into a decision: what to keep, what to mark as
spare, and why.

**Acceptance criteria.**
1. In the "Duplicates" view every file of a group has a "keep" button (one per group), the rest are
   marked as spare; the state lives in the DB (`files.decision`) and survives a restart.
2. For each group an advice is computed: sharpness (the variance of the Laplacian over grey), the
   resolution, the file size; "keep" is offered to the best one by default, the advice is labelled
   with words.
3. The "Export the plan" button writes a CSV: `group,keep_path,drop_path,reason,wasted_bytes` —
   without deleting files. A separate "Copy the list to the clipboard" button.
4. A counter in the panel: "in groups N, marked spare M, X will be freed" — recomputed on the fly.
5. `photocat dupes --csv` learns to print the `decision` and `sharpness` columns.
6. The measurement on the corpus of 2230 files is written to `docs/ARCHITECTURE.md`.

**Files.** `src/derive.rs` (sharpness), `src/dupes.rs` (a field in `Group`), `src/db.rs`
(`ALTER TABLE files ADD COLUMN decision TEXT`, `sharpness REAL`), `src/app.rs` (the "Duplicates"
view).

---

## M3 — CLIP: semantic tags and search ✅ (done)

**Goal.** The search line "cake", "sea", "document" works by the contents, not by the file name.

**Acceptance criteria.**
1. Embeddings are computed over a 224 px preview, an ONNX model (CLIP ViT-B/32 or SigLIP), the CPU
   through `ort`; the weights are downloaded once into `~/.cache/photocat/models/` with a sha256
   check.
2. The vectors lie in the DB (`embeddings(file_id, model, vec BLOB)`) as f16/f32; a repeated
   indexing does not recompute unchanged files (the key is sha256).
3. The search: a text query → a cosine → the top-N previews in the grid, the response time < 200 ms
   for 20 thousand photos.
4. A ready list of prompts (beach, document, screenshot, food, animals, children, holiday)
   gives tags; the tags are visible in the file card and filter the grid.
5. Clustering of the embeddings (k-means or HDBSCAN) collects 20–40 clusters; the report
   "cluster → 12 examples" is written to `plan/M3-clusters.md` for the owner to review.
6. A measurement: how many seconds per 1000 photos, how many MB of weights and vectors.

**Not doing.** No network calls: a local model only.

---

## M4 — previews and duplicates of videos ✅ (done)

**Goal.** 258 videos stop being a blind spot.

**Acceptance criteria.**
1. A cover frame through `ffmpeg -ss <10%> -frames:v 1` (a process, not a library), a preview in the
   cache.
2. A dHash over the frame → videos take part in duplicate groups; equal re-encodes are glued.
3. The duration and the resolution from `ffprobe` in the DB.
4. An ffmpeg error does not break indexing: the file stays without a preview, the error counter
   grows.

---

## M4b — settings, models, tags and the calendar ✅ (done)

**Goal.** The owner chooses the model and the place where it lies himself; tags can be corrected;
the calendar shows a month, not collapsing days.

**Acceptance criteria.**
1. The "Settings" section: the model folder, the path, the size limit, consent and download, the
   sha256 check, the model switch; everything is saved in the DB and survives a restart.
2. The network only for weights and only with consent; photos, paths and queries are not sent
   (ADR 0015, 0016). There is a mirror and a pick of a compatible model within the limit.
3. A second, smaller model (`clip-vit-b32-en`, 148.6 MiB) works through the same interface; the
   catalogue is extended by data.
4. `tag_hidden`: "remove" at a tag hides it on the photo, "restore the hidden tags" brings it back;
   the filter and the counters take it into account.
5. The calendar: a month with navigation, day cells with thumbnails and counters, the picked day as
   a grid.
6. Measurements on the corpus: embed of photos+videos, the model download, the limits; written to
   `docs/ARCHITECTURE.md`.

**Files.** `src/db.rs` (settings, tag_hidden), `src/embed.rs` (the catalogue, the downloader,
TextKind), `src/app.rs` (settings, the calendar, tags), `src/main.rs` (`models`, `settings`).

---

## M5 — OCR of documents and screenshots ✅ (done)

**Goal.** Receipts, certificates, screenshots are searched by text.

**Acceptance criteria.**
1. `tesseract` (rus+eng) as a separate pass over the files marked as a document/screenshot (the
   heuristic: no EXIF camera, a screen resolution, a CLIP tag from M3).
2. The text in the DB (`ocr(file_id, text)`), an FTS5 search, the highlighting of a match in the
   card.
3. The originals are not overwritten, the text is not written next to the file.

**Done.** `src/ocr.rs` (candidates, a rayon pass, writing to the DB), `db` (the `ocr` table, FTS5
`ocr_fts`, the triggers, `fts_query`/`search_ocr`), `derive::ocr_image` (EXIF rotation, PNG into
stdin), the CLI `ocr [--jobs N] [--all] [--dry-run] [--file PATH]` and `search --text`, the GUI: the
"by text on the photo" checkbox, the "Recognize text" button for the selected shot and a separate
batch pass "Recognize documents (N)", an excerpt with highlighting in the card. On the corpus: 435
candidates of 1972 shots, `ocr --jobs 8` — 106.2 s, 434/435 (1 broken JPEG), 66 444 characters in
146 files, the text is searched in 2–3 ms, the DB +276 KiB. A Sberbank receipt, an invoice and a book
page are read in full.

**Files.** `src/ocr.rs`, `src/derive.rs`, `src/db.rs`, `src/main.rs`, `src/app.rs`.

---

## M5b — cancelling and resuming long passes ✅ (done)

**Goal.** During OCR/CLIP/indexing the window responds, a pass can be stopped and finished later.

**Acceptance criteria.**
1. Reading the DB during a run is not blocked: `db::open` does no DDL while `SCHEMA_VERSION` matches
   (the measurement: `stats` during OCR — 11 ms instead of 5 s and "database is locked").
2. A "Stop" button for indexing, CLIP and OCR; a file already started is finished, the result is
   committed.
3. After a stop the button shows "Resume (N)"; a repeated run skips what was done by `sha256` (for
   OCR — also the files with empty text).
4. `tesseract` is started with `OMP_THREAD_LIMIT=1`, so that the workers do not take all the cores.
5. The one-off pass over the selected shot (M5) is not affected.

---

## M5c — paths to external programs in the settings ✅ (done)

**Goal.** On Windows and macOS the programs are often not in `PATH`; the owner has to give the path
himself, without rebuilding the app.

**Acceptance criteria.**
1. Fields in "Settings" for `ffmpeg`, `ffprobe`, `tesseract`, `curl`; empty — taken from `PATH`.
2. `photocat doctor` shows where each program came from and what breaks without it.
3. The path is checked on saving: a version run, the error — in the status, not in the console.
4. A measurement is not needed (it is a setting), but a GUI smoke test is mandatory.

**Files.** `src/tools.rs` (a new module), `src/db.rs` (settings), `src/derive.rs`/`src/embed.rs`
(running by a path), `src/app.rs`, `src/main.rs`.

**Done.** `tools::Tools` keeps four paths (`settings.tool_ffmpeg` and so on), empty — the name from
`PATH`; the lookup, the run and `CREATE_NO_WINDOW` are in one place. Saving checks the path by
running `-version`/`--version`: the GUI writes the result into the status, the CLI
`settings --ffmpeg PATH` refuses with code 2. `doctor` shows the version, the file found, the source
(the settings/`PATH`), the purpose and what breaks without the program; `settings` prints the same
list. Tests: `tools` (empty/an explicit path/a start error/the version flags), the settings roundtrip
in the DB. A GUI smoke: the window lives 6 s, no crashes.

---

## M6 — faces ✅ (done)

**Goal.** "Show me all photos with Lisa".

**Acceptance criteria.**
1. Detection and embedding of faces (ONNX: SCRFD + ArcFace), clustering, naming clusters by hand.
2. The similarity threshold is configurable, false glues are visible and are split with a button.
3. No sending of faces outside; the model is local.

**Done.** `src/faces.rs`: SCRFD (a 640×640 detection, three steps, NMS) + ArcFace (alignment by
5 points, 112×112, 512 f32), the crops in `<cache>/faces/`, the tables `faces`, `face_clusters`,
`face_scans`, `settings.face_threshold`. The pass skips shots by `sha256`, stops with the "Stop"
button; the clusters are a union-find by cosine, the names and the manual splits are pinned. GUI: the
"Faces" view, the group list, the crops, "not him", "Save the name"; the CLI
`faces [--dry-run|--list|--recluster|download|label|split]`. On the corpus: 1971/1972 shots in 630.2
s (`--jobs 4`), 2498 faces on 856 shots, 617 groups, a rebuild in 1.2 s, the model 182.4 MiB.
Nothing goes to the network except the weights, with consent.

**Files.** `src/faces.rs`, `src/db.rs`, `src/app.rs`, `src/main.rs`, `src/derive.rs`.

---

## M7 — the showcase: extend organize ✅ (done)

**Goal.** The owner chooses how the showcase looks: by days, by months or by events; the names are
built from the date/time/camera/tag; the files marked as spare do not get into the showcase.

**Acceptance criteria.**
1. `--by day` (default) — `YYYY/YYYY-MM-DD/`; `--by month` — `YYYY/YYYY-MM/`;
   `--by event` — `YYYY/YYYY-MM-DD/` for shots without a GPS and
   `YYYY/YYYY-MM-DD/<LL.LLN_LL.LLE>/` for shots with coordinates (a cell of ~1 km).
2. `--name "{date}_{time}_{orig}"` — a name template with the substitutions `{date}`, `{time}`,
   `{camera}`, `{tag}`, `{orig}`, `{ext}`; `--rename` — the short form `{date}_{time}_{orig}`;
   without a template the file name stays the original one.
3. `--exclude-dup` leaves out the files with `decision='drop'`; the counter of the excluded ones is
   visible in the plan.
4. An empty substitution leaves no "garbage" separators (`__`, a trailing `_`), the symbols not
   allowed in a name are replaced, collisions in a folder get `-2`, `-3`.
5. An unknown substitution in the template and an unknown `--by` — a refusal with code 2 and the
   list of the allowed ones.
6. Without `--apply` nothing is created; the originals are not renamed and not deleted.
7. The measurement on the corpus (a dry run and an application into a separate folder) is written to
   `docs/ARCHITECTURE.md`.

**Done.** `src/organize.rs`: `Layout` (`day`/`month`/`event`), `Options`, `gps_cell`, the name
template (`PLACEHOLDERS`, `tidy`/`component`, the length ≤120, the `-2` collisions),
`validate_template`, the plan counters (`no_date`, `excluded`, `dirs`). `--by event` — the day + a
coordinate cell of ~1 km; without a GPS the file stays in the day folder. `--exclude-dup` relies on
`files.decision`; the tags for `{tag}` are read only if the template asks for them. On the corpus of
2230 files: a dry run 0.017–0.037 s,
`--apply` (hardlink) — 2230 links in 0.065 s, 0 errors, 0 bytes; `--exclude-dup` on a copy of the DB
— 135 excluded. 13 new tests, `cargo test` — 64, `check.sh` — 0 violations. ADR 0022.

**Files.** `src/organize.rs`, `src/main.rs`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`.

---

## M8 — integrity and backup ✅ (done)

**Goal.** Replace a manual `cmp` of two disks with one command and get a manifest of the archive
against which, half a year later, one can check that nothing was swapped.

**Acceptance criteria.**
1. `photocat verify <a> <b>` compares the trees by relative paths: files only in A, only in B,
   different sizes, different contents, matched. With different sizes sha256 is not computed; with
   equal ones — it is computed and compared. A report with numbers and example paths.
2. The progress is visible, `--jobs N` sets the number of threads; `Ctrl-C` is safe because the
   command only reads. The return code: 0 — the copies match, 1 — divergences, 2 — an argument
   error.
3. `photocat manifest [DIR] --out FILE` writes a manifest in the `sha256sum` format (relative paths,
   the escaping of `\\`, `\n`, `\r`), including all regular files; without `--out` it prints to
   stdout. On a read error or a cancel the file is not written: an incomplete manifest is worse than
   a missing one.
4. `photocat verify --manifest FILE DIR` checks the manifest: a missing file, an extra file, changed
   contents, matched; the same progress and return codes.
5. No archive file is deleted, renamed or changed; the only new file is the manifest written by an
   explicit `--out`. Symlinks and special files are skipped with a counter.
6. The measurement on the corpus of 2230 files / 21.2 GiB: `verify` of two copies, `manifest` and a
   manifest check; the numbers are written to `docs/ARCHITECTURE.md`.
7. Tests: a comparison by size and contents, only in A/B, the escaping and parsing of the manifest,
   a refusal to write a manifest on an error, a cancel.

**Done.** `src/verify.rs`: a walk of two trees by relative paths, size → sha256 only for the files
of equal size, the report (`only_a`, `only_b`, `size_diff`, `content_diff`, `same`), progress and a
cancel through `db::Progress`, `rayon` over `--jobs`. `manifest` writes a manifest in the `sha256sum`
format (relative paths, the escaping of a backslash, `\n`, `\r`), the manifest file itself is
excluded from the walk; an incomplete manifest is not written. `verify --manifest FILE DIR` finds the
missing, the extra and the changed files. The CLI: `verify`, `manifest`; the codes 0/1/2. Neither
deletion, nor renaming, nor writing into the archive folder — only an explicit `--out`. On the corpus
of 2230 files / 21.2 GiB: `verify` of two copies — 19.9 s (2230/2230), `manifest` — 10.1 s (2230
rows, 193 KiB), `verify --manifest` — 11.4 s; a changed copy is found in 16.1 s (4 divergences), a
manifest check — in 8.9 s (changed 2, missing 1, extra 1). 10 new tests, `cargo test` — 74,
`check.sh` — 33 checks, 0 violations. ADR 0023.

**Files.** `src/verify.rs`, `src/main.rs`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`.

---

## M13 — a revision of the owner's interface ✅ (done)

**Goal.** Close the remarks after a review of the archive: a full screen view, understandable
duplicates, a calendar by years/months/weeks, a closable right panel, several folders, a trash, the
disk usage, own tags and a search for people together.

**Done.**
1. **The full screen.** A double click on a tile (and a button in the card) opens a modal window over
   the interface: a 1920 px frame is decoded in the background, ←/→, a slideshow every 3 s, zoom, the
   full OCR text next to it, "Watch the video" — in a window, "With system sound" — the system
   player. Closing returns to the same view, filter, day and selection. `App::viewer_modal`,
   `Viewer`.
2. **Faces.** A click on a face leads to the calendar day that holds the shot (if there is no date —
   to all files with a selection). The crop got "pick" and "Name the picked people": several faces
   go into a group with one name at once (`db::group_faces`). Groups can be ticked and "Photo where
   everyone is together (N)" pressed — a selection of only those shots that have a face from each
   group.
3. **The right panel** closes with the "Hide" button and comes back with the "Details" button; the
   calendar and the grid get all the room. The width of a month cell no longer has a hard minimum of
   82 px. (Since M22.16 the close target is the mock-up's "×" in the panel's header; "Details" in the
   top bar stays the worded way back.)
4. **The calendar** got the scales "Year / Month / Week" with navigation; a click on a month in a
   year switches to the month, a click on a day in a week — to the day.
5. **Duplicates and the model.** "Keep", "Export the plan", "Compute tags" got explanation labels
   right in the panel: what is written, that the files are not deleted, that the CSV is a report.
   There are 14 CLIP tags now (`furniture`, `interior`, `clothes`, `nature`, `city`, `transport`,
   `plants`), a "Compute tags" button appeared that works over the ready vectors without a repeated
   CLIP. Own tags are added by hand in the card (`user_tags`) and take part in the filter.
6. **Folders.** The "Folders (N)" button opens the list and the built-in folder picker (without a new
   dependency). Several folders are indexed with one button; "Remove from the index" deletes only DB
   rows, the files on disk are intact.
7. **Trash.** "To trash" writes `files.trashed_at` and hides the file from the normal views; the
   "Trash" view offers "Restore" and "Empty the trash (N)". The emptying happens only after a
   confirmation and only into the OS trash (`trash`), then the index rows are deleted. There is no
   permanent deletion in the app.
8. **Storage.** "Compute the used space" shows the DB, the previews, the face crops, the models and
   the whole cache. "Clear the previews", "Clear the face crops", "Reset the index" — with a
   confirmation; `storage.rs` deletes only inside `<cache>/photocat/`, it does not touch the archive.
9. **Tiles and scrolling.** The "Tile size" slider 100–260 px (150 by default, kept in the settings).
   Textures load in portions of 8 per frame, the smooth scrolling animation is on: a fast scroll does
   not decode hundreds of JPEGs on the UI thread.

**Checked.** `cargo test` — 79 passed, 3 ignored, `cargo clippy --all-targets` — no remarks,
`check.sh` — 38 checks, 0 violations. Migration of a copy of the real DB (22 MiB, 2230 files) from
schema 2 to 4: the data is intact, `trashed_at` and `user_tags` appeared. A GUI smoke on a copy of
the DB: the window is alive, code 124 on the timeout. The owner reviews the pixels and the gestures
(a double click, the trash, the calendar).

**Left (deliberately).**
- Above 100% zoom the frame in the full screen is centred and cropped: there is no panning yet.
- Own tags are manual; CLIP does not create them. A thing outside the 14 hints is found by a
  semantic query.
- There were no separate "albums" then: an "album" meant a calendar day. Since M19 albums are
  explicit sets of files in the DB (`plan/M19-albums.md`), and a day stays a day.
- Video is played by the system player, not inside the window.
- Clearing the models is not done: they weigh hundreds of MiB and are downloaded only with consent.

**M13b — fixes after the owner's review.** The full screen shows the 320 px preview at once, the full
1920 px frame — from the background cache/stream; the selected file is decoded in advance. Old videos
without covers are recomputed by the incremental indexing (258/258 covers in 10.2 s). Video plays in
the window: a stream of RGBA frames from `ffmpeg` 720×480, 15 fps, without sound; the sound — by the
system player. The calendar counts the width with a 4 px step and is no longer cut. There is one tag
button: the missing embeddings + a recompute of the labels; the filter takes any weight above the
threshold. The transition from "Faces" resets the old filter. A folder is indexed by itself after
being added; the field on top is filled. A repeated indexing skips unchanged files
(path+size+mtime): on the corpus 1971 photos were skipped, 259 processed in 10.2 s, a repeat —
0.024 s.

**Files.** `src/app.rs`, `src/db.rs`, `src/embed.rs`, `src/faces.rs`, `src/index.rs`,
`src/tools.rs`, `src/recycle.rs`, `src/storage.rs`, `Cargo.toml`, `check.sh`, `docs/UI.md`,
`docs/ARCHITECTURE.md`, `docs/DECISIONS.md` (0025, 0026, 0027).

---

## M9 — GUI: comparing copies and the manifest ✅ (done 2026-11-23)

**Goal.** The owner checks the backup and makes a manifest without a terminal: pick two folders, see
the divergences as a list and stop a long check without closing the window.

**Acceptance criteria.**
1. A new `View::Integrity` and the item "Integrity" in the left panel; the screen per the `add-view`
   skill, the texts in English.
2. The fields: folder A, folder B, the manifest file; the "show all paths" checkbox; the buttons
   "Compare the copies", "Write the manifest", "Check against the manifest". The paths are entered as
   text, without a new dependency for a system dialog.
3. The operations run in a background thread through `db::Progress` and `mpsc`; "N of M" is visible,
   there is a "Stop"; the window responds. The result — the counters and the list of paths, as in the
   CLI, plus "Copy the report".
4. The manifest is written only by a button and an explicit path; if the file already exists — an
   overwrite confirmation. During a comparison and a check nothing is written into the archive.
5. The GUI and the CLI use the same `verify` functions; the report and the codes are the same, the
   logic is not duplicated in `app.rs`.
6. `docs/UI.md` describes the screen, the states, the empty values and the behaviour of "Stop"; a GUI
   smoke run without crashes; `cargo test`, `clippy`, `check.sh` — green.

**Files.** `src/app.rs`, `src/verify.rs` (public reports), `docs/UI.md`.

**Done.** The `View::Integrity` view with the "Integrity" item; the fields A/B/the manifest, the
"Show all paths" checkbox, the buttons "Compare the copies", "Write the manifest"/"Overwrite the
manifest",
"Check against the manifest"; a background `mpsc` thread + `db::Progress`, the progress and "Stop" in
the top bar;
the verdict in words, the common report `verify_report`/`check_report`/`manifest_report` and "Copy
the
report". Overwriting the manifest — only by a second click. Nothing is written into the archive. The
checks and
the tails are in `plan/STATUS.md` § M9.

---

## M10 — the reminder to check the integrity ✅ (done 2026-11-23)

**Goal.** The backup is checked not "when I remembered" but by a due date: the app reminds, but does
not start the heavy pass by itself.

**Acceptance criteria.**
1. `settings` holds `verify_manifest`, `verify_interval_days` (0 — off),
   `verify_last_at`, `verify_last_ok`, `verify_last_report`; the values survive a restart.
2. When the GUI starts, if a manifest is set and the date is due, the banner "Time to check the
   archive" is shown with the buttons "Check" and "Remind later". There is no automatic start: the
   check begins only on a click.
3. The check runs in the background with progress and a "Stop", writes the result into the settings
   and the report into `<cache>/photocat/verify-last.txt`; nothing is written into the archive folder.
4. For cron/systemd the CLI path remains: `photocat verify --manifest FILE DIR` with the codes 0/1;
   the README has a ready timer example. The app does not set system timers itself.
5. Tests: the calculation of "due/not due" (no manifest, an interval of 0, exactly due, overdue, the
   clock went back), the banner does not start a pass; a check error does not erase the former
   successful report.
6. Without the network; everything locally.

**Done.** `db::Settings` got `verify_root`, `verify_manifest`, `verify_interval_days`
(30 by default, 0 — off), `verify_last_at` (RFC3339 UTC), `verify_last_ok`,
`verify_last_report`; `db::verify_due(&settings, now)` — the single calculation of "due/not due"
(no manifest, off, exactly due, overdue, early, the clock went back). In the top bar — the banner
"Time to check the archive" with "Check" and "Remind later"; the pass does not start by itself,
"Check" opens "Integrity" and starts the same background check. On the "Integrity" screen — the
interval, the date and the result of the last pass, the path of `verify-last.txt` and "Copy the last
report". A successful manifest and check write the date, "ok" and the full report into `settings` and
`<cache>/photocat/verify-last.txt`;
an error and a cancel do not erase the former report. The CLI did not change, the README got a
systemd timer example.
The checks and the numbers are in `plan/STATUS.md` § M10.

**Files.** `src/db.rs` (settings), `src/app.rs`, `src/verify.rs`, `README.md`,
`docs/ARCHITECTURE.md`.

---

## M11 — fast search of similar shots above 20 thousand photos ✅ (done 2026-11-23)

**Goal.** The current pairwise comparison of dHash is O(n²): on 2230 files that is 0.03 s, on 20
thousand already
hundreds of millions of comparisons. An index over the 64-bit Hamming distance must keep the result
and remove the
square.

**Acceptance criteria.**
1. `dupes::find_groups` gives **the same** set of groups as the current scan, on the corpus and on
   synthetic sets at `max_dist` 0–10. Exact copies by sha256 are glued as before.
2. Above the threshold (20 000 hashes) an index is built, below — the former scan. The threshold is
   a named
   constant with a comment, not a magic number in a loop.
3. The distance is the Hamming over `u64`; the index is built once, the union-find and the group
   advice do not change. There are no new dependencies.
4. A measurement in release: the corpus of 2230 files — no worse than 0.03 s; synthetic 20 thousand
   and 50 thousand hashes —
   the time and the memory written to `docs/ARCHITECTURE.md`; 50 thousand at `--max-dist 6` fits
   into 5 s.
5. A test "the index and the scan agree" on random sets (including empty, one element,
   all identical, all far apart); an empty dHash does not take part.

**A correction during the work.** A bare BK-tree was measured and rejected: on 64-bit hashes at a
radius of
6 almost all points lie at a distance of ~32, the triangle inequality cuts nothing off —
20 thousand in 2.58 s, 50 thousand in 17.2 s (worse than the goal of 5 s). Instead — an eight-block
index
(8 pieces of 8 bits): at a distance below 8 at least one piece matches entirely (the pigeonhole
principle), the candidates are checked by the full Hamming. The details and the numbers — ADR 0038,
`docs/ARCHITECTURE.md` § "Duplicates: the block index".

**Done.** In `src/dupes.rs` — `INDEX_MIN = 20_000`, `INDEX_BLOCKS = 8`, `INDEX_MAX_RADIUS = 7`,
`union_similar_indexed` (buckets by 8-bit pieces, a marker instead of a HashSet for deduplicating
the candidates) and the former `union_similar_pairwise`. `find_groups_with(..., index_from)` gives
the tests
both paths on the same data; the public `find_groups` picks by the threshold. For a radius of 8–64
the
scan remains: the eight blocks have no guarantee, and such thresholds anyway put almost everything
in one group.

**Files.** `src/dupes.rs`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`.

---

## M12 — an extensible catalogue of CLIP models ✅ (done 2026-11-23)

**Goal.** Add models by data, not by editing the code: the owner picks a model by size and
language, the vectors of different models are not mixed.

**Acceptance criteria.**
1. The embedding dimension is a property of `ModelEntry` (now `DIM = 512` is hardcoded in
   `embed.rs`);
   the load checks that the vision and the text towers gave one dimension; the search, the clusters
   and
   the saving into the DB take `dim` from the model/the row, not from a global constant.
2. The catalogue has at least one new model with pinned URLs, `sha256`, a size, a licence and a
   hint; the ids and the folders are unique, adding data does not require editing the logic. Models
   with different
   dimensions are allowed, but are compared only inside their own model.
3. `models list`, `models check`, `settings`, `doctor` and the GUI "Settings" show the new models,
   their size, the status "in place/not downloaded" and the `max_model_mb` limit; a download — only
   with consent,
   with a sha256 check and a mirror.
4. Switching the model does not erase the old vectors and tags and does not mix them: the search and
   the tags go
   only by the active model; the tags of other models are visible only after switching back.
5. Tests: the uniqueness of id/dir, `fits` by the limit, the pick of a compatible model when one is
   unavailable,
   a dimension error with a clear text, a download refusal without consent, empty hints.
6. A measurement on a clean folder: downloading the new model (the size, the time, sha256), `embed`
   on the corpus,
   the search and `clusters`; the numbers in `docs/ARCHITECTURE.md`.

**Not doing.** User tags, an automatic search for models on the network without consent,
telemetry.

**Done.** `DIM` is gone; `ModelEntry` keeps `dim`, `image_side`, `max_tokens`, `pad_token`,
`norm` (mean/std/a filter), `frames` and the ONNX output names. `siglip-base-224-en` was added:
Xenova SigLIP base patch16-224, 768 features, Apache-2.0, 203.5 MiB, the sha256 pinned.
`TextKind::Pooler` reads `pooler_output`, the text is padded to 64 tokens with `</s>`, the frame is
stretched
into a square with mean/std 0.5. Schema 7: `tags` with the key `(file_id,tag,source)`, the migration
preserves
the data, the sha256-change trigger removes the tags of all models. The GUI and `models list` show
the features,
the licence and the status; the search/tags/clusters read only the active model. The measurements —
in
`docs/ARCHITECTURE.md` § "The model catalogue", the decision — ADR 0039.

**Files.** `src/embed.rs`, `src/derive.rs`, `src/db.rs`, `src/app.rs`, `src/main.rs`,
`docs/ARCHITECTURE.md`, `docs/DECISIONS.md`.


## M13c — smooth flipping and the slideshow ✅

**Goal.** Remove the "blur → sharp" while flipping and make the slideshow a tool, not a button.

**Done.** One background decoder with a queue (the current, the next, the previous, +1), a cache of
4
RGBA frames; when flipping, the next one is usually ready. While there is no frame — the previous
sharp one is on the screen,
the 320 px thumbnail is not stretched. JPEG on Linux through the TurboJPEG DCT scale: 108 MP —
0.52 s
instead of 1.33 s. The slideshow: an interval of 3/5/10 s, space, in a loop, waits for a ready
frame; ←/→ reset
the timer. Video opens ffplay: video and sound in a separate window.

**Checked.** `cargo test` — 79 passed, 3 ignored, `clippy` — clean, `check.sh` — 38/0; the measurement
of TurboJPEG on a 108 MP file.

---

## M13d — panning the zoomed frame in the full screen ✅ (done 2026-11-24)

**Goal.** At a zoom above 100 % the owner looks at a detail — a face in a group photo, a line of a
document, the number on a receipt — and not only at the centre of the frame.

**Acceptance criteria.**
1. With the left button pressed the zoomed frame moves under the cursor; the cursor is a hand
   (`Grab`, `Grabbing` while dragging); the frame never leaves the area, so no empty background
   shows at an edge.
2. At 100 % ("Fit") there is nothing to drag: the frame stays centred and the hand cursor does not
   appear. A side that is fitted stays centred even when the other one is zoomed.
3. A new file and "Fit" reset the shift: another snapshot does not open cropped for no reason.
4. The hint "drag to move the frame" goes through `i18n::t` and is visible only above 100 % — the
   drag is discoverable without a tooltip.
5. A video frame is not panned: a click on it still starts and pauses the video.
6. `docs/UI.md` §Debt loses the item, the metric row and the pointer table describe the drag;
   `cargo test` covers the clamp and the reset; `clippy` and `check.sh` green; a GUI smoke run.
7. No new dependency, no DB change, the archive is untouched.

**Done.** `Viewer::pan` and `clamp_pan` in `src/app.rs`; `draw_viewer_image` applies the drag delta
of the left button, clamps the shift and paints the hand cursor (`Grab`/`Grabbing`) only while the
frame is zoomed; `viewer_step` and "Fit" reset the shift. The hint "перетаскивание — сдвиг кадра" /
"drag to move the frame" is new in `i18n::PAIRS` and appears only above 100 %. `docs/UI.md` debt item
5 is gone, the rest are renumbered. `cargo test` — 219 passed, 6 `#[ignore]` (3 new); clippy clean;
`check.sh` — 52/0; the GUI lives 6 s on a copy of the real DB, no crash. The numbers and the
deliberate omissions are in `plan/STATUS.md` § M13d.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`.

## M14 — the built-in video player ✅

**Goal.** Video plays in the app window, the same on all OSes, without external programs.

**Done.** `src/player.rs` in pure Rust: `rff-format-mp4/mkv` + `rff-codec-h264`/`hevc`/`aac`
(+`rusty_h264`/`rusty_h265`), the sound `cpal`. A decode thread and a presentation thread, a queue of
3 frames, the clock (the sound is the master), seeking by the sample index, rotation and the colour
matrix from the container, RGBA into a texture.
GUI: "Watch the video", "Pause"/"Play", "Stop the video", a bar with seeking, the volume.
`ffplay` — a fallback only. ADR 0030/0031, the measurements — in `docs/ARCHITECTURE.md`
§Measurements.

**Acceptance criterion.** The video of the corpus plays in the window with sound; 1080p holds real
time; seeking and pause work; `play --decode-only` gives numbers without a screen; the tests, clippy
and the layers are green.

**Not doing.** 4K, hardware decoding, the built-in player as a dependency on `ffmpeg`/`mpv`.

## M14c — full screen flipping and the playback panel ✅

**Goal.** Flipping in the full screen shows the next file, even if it is a video; the player has
icon buttons, a video has a big ▶ button in the frame.

**Done.** `App::viewer_step` — the single point of a file change: first `stop_video`, then
`Viewer::idx`. The former shot no longer covers a video (`stale` only for shots), the player frame
is tied to `video_id`. The panel: ⏮ ▶/⏸ ⏭ (32 px, `paint_icon`), the time, seeking, the sound,
"Stop the video"; for a shot ▶ — the slideshow. A big ▶ 76×76 in the centre of the video frame and a
click on the frame. Space on a video — pause, on a shot — the slideshow. The end of a video opens the
next file.

**Acceptance criterion.** ←/→ and the buttons change the file even in a list with videos; a big ▶ is
visible on a video; pause/resume and seeking work; after the end of a video the next file opens; the
tests, clippy and the layers are green.

---

## M15 — tags that work on any archive ✅ (1–5 done 2026-11-23, 6–7 done 2026-11-24)

**Goal.** The tags should work from the first launch on any archive, without tuning the prompts to
`Фотографии с телефона`; 100% of the scale always leaves something; the owner can add his own labels
and prompts. The detailed analysis, the `8.jpg` diagnostics, the order of tasks and the open
questions — `plan/M15-tags.md`.

**The order and the acceptance criteria.**

1. ✅ **The crop.** A centre crop + the whole frame with margins, averaging of the vectors,
   `embed --refresh`.
   Done: 176.6 s for 2229 files (2.3×), "furniture" ≥0.22 for 717 → 1029, confident tags 1284 →
   1285, in 98.4% of the old confident ones top-1 is the same. `8.jpg`: "document" leaves the leaders,
   but the first
   becomes "interior", not "furniture" — the criterion was moved to item 3 and item 5, without tuning
   the prompts.
2. ✅ **A percentile per tag.** `tag_show_pct` 0–100 per tag; at 100% exactly 1 shot
   for any tag, at 0% — all, at 70% — 669 (the thresholds 0.229–0.248). The confidence (`tag_show`
   0.22
   + a lead) stayed separate for the card, OCR and `{tag}` in the showcase; an old DB migrates
   (confidence 0.22, the filter 70%), a small folder takes the raw threshold.
3. ✅ **"Not sure".** With a lead below 0.01 the card writes "tag: not sure", shows the top-3
   with the weight and the percentile, without highlighting "document"; the rest — under "all tags
   (N)". `8.jpg`
   (document 0.271 / interior 0.270) shows "not sure".
4. ✅ **The dictionary.** `tag_defs` in the DB per model: the label, the prompts, enabled, the order,
   the minimum
   percentile. The default is common, seeded once, edits are not overwritten. "Wardrobe" is added,
   the tags are recomputed over the stored vectors (2229 files in 1.0–1.2 s), the filter sees it;
   JSON import/export; the roundtrip tests. ADR 0036.
5. ✅ **The prompts.** 2–3 common templates per label, the tag weight — the best of the templates
   (the average
   blurs); there is no "wardrobe with shelves" in the default; "Drawing/scheme" — not a common one,
   only through the dictionary.
   A measurement before/after on two folders: a false "document/screenshot" on a photo 16.3% →
   13.6%,
   documents with text 123/145 without losses. plan/STATUS.md § M15.5.
6. ✅ **The model and the calibration.** SigLIP 2 base 224 multilingual
   (`onnx-community/siglip2-base-patch16-224-ONNX`, Apache-2.0, 393.3 MiB, 768 features) is the
   fourth catalogue entry, opt-in: it does not fit the 250 MiB default limit, the owner raises it to
   400. The tag weight is calibrated per model into the CLIP scale (`score_calib`, ADR 0041), so the
   confident threshold keeps its meaning on every model; `tags.source` carries the calibration
   version. The padding rule became data (`pad_left`): without it SigLIP 2's texts collapse into one
   vector. Measured on two folders: SigLIP 2 55.3 % confident tags (CLIP 55.6 %), documents with text
   caught 69.0 % (CLIP 85.9 %), false "document" on photos without text 9.7 % (CLIP 3.3 %), 0.170 s
   per file against 0.063 — so the default stays the multilingual CLIP. `plan/STATUS.md` § M15.6.
   **The tail is closed** (2026-11-24): the fp16 towers were measured on the same 2231 files. int8
   costs no tag quality — 55.2 % confident and 69.0 % of the documents with text caught, the M15.6
   picture for this folder; the fp16 text tower alone is worse on documents (64.8 %), and the fully
   fp16 model trades recall for precision (49.0 % caught, 3.6 % false) at 748.9 MiB. Nothing was kept:
   no fp16 entry, no dependency, the tree is unchanged. ADR 0043, `plan/STATUS.md` § M15.6 tail.
7. ✅ **A check on two independent folders with a labelled sample** (≥100 shots including
   renders/screenshots): top-1/top-3, how many "furniture" shots at the 100 %/70 % percentile, the
   embedding time and the DB size — with the date, the folder and the model in
   `docs/ARCHITECTURE.md` § Measurements. Done 2026-11-24: `photocat eval`, 120 shots per folder,
   blind labels; top-1 85.4 % / 82.6 %, top-3 97.6 % / 97.7 %, confident tag 79.7 % / 79.0 %,
   furniture 13/13 at 70 % and 0/13 at 100 % (by construction), 126.1 s / 16.7 s to embed, DB
   30 MB / 2.3 MB. ADR 0042; `plan/STATUS.md` § M15.7. Open: the owner's re-read of the labels and
   a labelled top-N for furniture precision.

**Not doing.** Hardcoding prompts/thresholds for the current archive; auto-tuning for one folder;
the network for prompts; deleting/renaming files; promising an object detector.

**Files.** `src/derive.rs`, `src/embed.rs`, `src/db.rs`, `src/app.rs`, `src/ocr.rs`,
`src/organize.rs`, `plan/M15-tags.md`, `docs/UI.md`, `docs/ARCHITECTURE.md`,
`docs/DECISIONS.md`.

---

## M16 — the interface after the PhotoPrism model (done 2026-11-23)

**Goal.** The shell the owner knows from PhotoPrism: side navigation in groups with
counters, search as the main element of the top bar, a content panel with the tile size and
filter chips, the grid grouped by months, an inspector with the file name and "label —
value" rows. The detailed analysis, the options and the choice — `plan/M16-interface.md`, the
decision — ADR 0033.

**Acceptance criteria.**
1. The left panel: the groups "LIBRARY" and "TOOLS", the items with real numbers on the right; at
   840 px the navigation and the filter are visible without scrolling.
2. The top bar — two control rows, "Index" — the only accent button;
   the progress does not shift the grid by more than its own row.
3. The grid: months in descending order with the header "May 2024 · N files · size", inside — the
   newest on top;
   a step of 2 px; hovering gives an outline and a cursor; 4 columns at 1240 px.
4. Above the grid: chips of the active filters with "×", "found N of M", the "Tiles" slider.
5. The inspector: the file name 14 px, the path 11 px, the metadata as "label — value" rows, the
   actions before
   the metadata.
6. Panel sections and cards without `Frame::group`; one style for the whole GUI.
7. `cargo clippy --all-targets` clean, `enforce-architecture` without violations, `cargo test` green,
   `timeout 6 photocat gui --db <a copy of the real DB>` does not crash.
8. The documents are updated by the same change: `docs/UI.md`, `docs/DECISIONS.md` 0033,
   `.pi/skills/ui-design/references/verstka.md`, `plan/STATUS.md`.

**Files.** `src/app.rs`, `docs/UI.md`, `docs/DECISIONS.md`, `plan/M16-interface.md`,
`plan/STATUS.md`, `plan/BACKLOG.md`, `.pi/skills/ui-design/references/verstka.md`.

**Not doing.** A light theme, icons without labels, favourites, a map; changing
the DB schema, indexing, duplicates, CLIP, OCR, faces, the player and the trash.

---

## M17 — the grid layout and sorting by date ✅ (done 2026-11-23)

**Goal.** The owner chooses how he looks at the archive: as now — groups by months — or
a flat grid without grouping, as it was before M16; and in what order the files go.

**Acceptance criteria.**
1. In the view header, next to "Tiles N", two switches: "By months / Flat" and "Sort"
   with the options "Date: newest first" (default), "Date: oldest first", "Name", "Size:
   largest first". The words in English, there is no icon without a label. The fifth option —
   "Album order" —
   appeared with albums (M19) and works only inside an album.
2. "Flat" — one continuous grid without month headers; "By months" — the former headers
   "May 2024 · N files · size", inside a month — by the chosen sort order.
3. "No date" — the last group in both modes, including with "oldest first". The order is
   total and stable: the key `(taken_at, id)`, equal dates do not swap between frames.
4. A semantic search sets the order itself: while there are `semantic_hits`, the sort switch is muted
   with the hint "the search sets the order", the chip "by relevance" with "×" drops the search.
5. The layout and the sort order survive a restart: `settings.grid_layout` (`months|flat`),
   `settings.grid_sort` (`date_desc|date_asc|name|size_desc`); an unknown value — the
   default value, without a crash. `SCHEMA_VERSION` is not bumped: `settings` is key/value.
6. The order is computed by one function (`App::ordered`); the grid, the flipping in the full screen
   and the selection
   (M18) take the same one. There is no second sorting place; the mechanics "source row indices" of
   ADR 0033
   is kept.
7. Tests without a window: flat — by date; "oldest first" reverses the months and the flat list;
   "No date" at the end in both directions; the name and the size are sorted; during a search the
   order = relevance;
   the settings roundtrip.
8. `docs/UI.md` (the switches, the single-order rule, unavailability during a search), a line in
   `.pi/skills/add-view/SKILL.md` about the fact that the view settings (`tile_size`, the layout,
   the sort order) live in `settings`, and the view data — in `App`; `plan/STATUS.md`.

**Files.** `src/app.rs`, `src/db.rs`, `docs/UI.md`, `.pi/skills/add-view/SKILL.md`, `plan/`.

**Not doing.** A second grid renderer, a sticky month header (a separate task), sorting by
GPS/tags/camera, writing the order into the DB.

---

## M18 — multiple selection of tiles ✅ (done 2026-11-23)

**Goal.** Actions over several files at once: without this an album (M19) and any action over a
group stay work on one file at a time.

**Acceptance criteria.**
1. A click — one file (as now); Ctrl+click adds/removes; Shift+click — a range by the current
   order (M17); a click on empty space drops the selection.
2. In the chips row: "picked N · X MiB" (through `db::human()`), the buttons "Pick all (M)" and "Drop
   the selection". Esc drops, Ctrl+A picks what was found.
3. The inspector shows the last picked file and the line "N picked"; picked tiles have
   an outline and a tick — visible without hovering.
4. A change of the view, the filter, the query or the sort order drops the selection and writes into
   the status why
   ("selection dropped: the filter changed"), so that an action does not touch invisible files.
5. The selection is an `App` field (`BTreeSet<i64>` + an anchor), it is not written to the settings,
   after a restart
   it is empty.
6. Tests without a window (headless egui): Ctrl+click adds, Shift+click gives a range, Esc and a
   change of
   the filter drop it, "pick all" takes only what is visible.
7. `docs/UI.md` §Interaction — the rules of the selection; `plan/STATUS.md`.

**Files.** `src/app.rs`, `docs/UI.md`, `plan/STATUS.md`.

**Not doing.** A rubber-band rectangle with the mouse, a selection in duplicates/faces/the trash
(separately), drag&drop.



---

## M19 — albums: a tree with an order, data in the DB only ✅ (done 2026-11-23)

**Goal.** The owner builds a tree of albums with a file order — "Dacha 2024 → May, June", "Lisa →
1 year" — so that a ready physical folder comes out of it. For now these are only rows in the
database: no
file on disk is created, changed or moved.

**Acceptance criteria.**
1. The "Albums" view in the "LIBRARY" group (the number of albums on the right): a tree with indents
   and collapsing,
   a node with "N files · M subalbums · size"; "+ New album", "+ Subalbum", "Rename",
   "Move to…" (its own descendants are unavailable), "Up / Down", "Delete the album". Inside — a row
   of subalbums and a grid of files (`draw_cell`), the actions "Add picked" (M18), "Remove from
   the album", "Up / Down" for a file, "Export to disk…" (M20).
2. The schema: `albums(id,parent_id,position,name,name_key,note,created_at,updated_at)` and
   `album_files(album_id,file_id,position,added_at)`; `parent_id = 0` — the root; the name is unique
   **among siblings** case-insensitively (`parent_id, name_key`), not globally; `SCHEMA_VERSION`
   4 → 5 only through `CREATE TABLE IF NOT EXISTS`; a migration on a copy of the real DB (2230 files)
   without
   losing rows.
3. The order is part of the data: `position` is dense 0..n−1 for files and for subalbums, `db`
   renumbers
   after an insert/a move/a delete; inside an album the sort order by default is "Album order"
   (a new option in the M17 list, available only inside an album).
4. Data only: creating, renaming, moving, filling, deleting — `INSERT/UPDATE/DELETE`
   in `db.rs`; `delete_album` recursively cleans the subtree (the confirmation shows the contents),
   deleting
   `files` rows (an index reset, "remove a folder") cleans `album_files`, because
   `PRAGMA foreign_keys` is not enabled in `open`; tests for all the paths.
5. A cycle is forbidden: `move_album` refuses if the new parent is the album itself or its descendant;
   an idempotent addition (a repeated `add` does not multiply rows and returns "N were already
   there"); one file
   can be in several albums and in different branches of the tree, but **not twice in one album**
   (the key `PRIMARY KEY(album_id, file_id)`, decided by the owner); `albums_of_file` for the
   inspector.
6. A file in the app trash stays an album row, but is hidden in the grid; the counter "in the trash
   N";
   the file is not in the index — the row is deleted, there are no dangling references.
7. A CLI without graphics: `albums list [--tree]|new|rename|move|add|remove|delete|show`; the paths
   are treated
   as the `files.path` key, the files are not opened; the codes 0/1/2.
8. Tests: a tree of 3 levels, the order and the renumbering, uniqueness among siblings (in different
   branches equal names are allowed), a refusal on a cycle, recursive deletion does not touch the
   files on
   the disk, idempotency, an empty/long/repeated name, the 4→5 migration, a repeated opening of the
   DB
   writes nothing.
9. A measurement: `albums add` of 500 files (the time), a tree of 3 levels on the corpus, the DB size
   before/after;
   `docs/ARCHITECTURE.md` §Measurements.
10. Documents: `docs/UI.md` (the view, the nesting, the order), `docs/ARCHITECTURE.md` §Schema,
    ADR 0034 "Albums in the DB: a tree and an order, separate from the folders on disk",
    `plan/M19-albums.md`,
    `plan/STATUS.md`.

**Files.** `src/db.rs`, `src/app.rs`, `src/main.rs`, `docs/UI.md`, `docs/ARCHITECTURE.md`,
`docs/DECISIONS.md`, `plan/M19-albums.md`, `plan/STATUS.md`.

**Not doing.** Files on disk, copies, "smart" albums by a filter, a handmade cover, syncing with
folders; calling an album a "folder" — "Folders (N)" is already about the archive folders.

---

## M20 — exporting an album as a physical folder ✅ (done 2026-11-23)

**Goal.** A built album turns into a real folder with ordered files: copies
(not links), the structure repeats the album tree, the order is visible by the numbers in the names,
equal
`IMG_0001.jpg` do not overwrite each other.

**Acceptance criteria.**
1. `photocat export --album N --out DIR [--name TEMPLATE] [--apply]`; without `--apply` — only
   a plan tree, nothing is created on the disk, not even a folder.
2. The structure on disk is one to one with the album tree: `--out/<album>/<subalbum>/…`, each
   folder has its own files; an empty album gives an empty folder; the folders of subalbums
   **without numbers**:
   on disk they go alphabetically (the file system does not know `position`), the tree order stays
   in the
   app.
3. **Copies, not links**: the inode and the size differ from the original, the contents match by
   `sha256`;
   there is no `--mode` in `export` — links stay with `organize` (the showcase), it is another
   tool.
4. The order: the name by default `{n}_{orig}`, `{n}` — the `position` in its own album, the width by
   the number of
   files (2 digits up to 99, 3 up to 999); `{date} {time} {camera} {tag} {orig} {ext} {hash8}` are
   available
   in the template.
5. Collisions: equal names in an album are separated by the position number; a name taken by other
   contents
   gets `-2`, not an overwrite; the same `sha256` — a skip ("already exported", a run is idempotent);
   after a cleanup of the FS the subalbum names are compared case-insensitively on Windows/macOS, the
   second one gets
   `-2`; the length of a component ≤ 120, the full path ≤ 240 — otherwise a skip with a reason. One
   shot twice
   in one folder is impossible: in an album it is one row (`PRIMARY KEY(album_id, file_id)`).
6. No existing file is overwritten, deleted or renamed; `--out` does not exist,
   `--apply` without `--out`, `--out` inside any `settings.roots` — a refusal with code 2.
7. The report: copied / skipped / errors / bytes written / "the order in the target is stale N"; an
   error
   of one file does not break the pass. Reordering does not move the already exported files (one
   cannot
   rename in the target) — the plan and the report say so directly, it is cured by a manual re-export.
8. GUI: "Export to disk…" — the folder and the template; first a plan tree, then "Export N files";
   the progress and "Stop" in the top bar.
9. Tests: five kinds of collisions, idempotency, a copy (not a link), an empty album, a depth of 3,
   a refusal of an `--out` in the root, a cancel in the middle (the created ones are listed), a
   missing source,
   a long path.
10. A measurement on the corpus: a tree of 3 levels, 500 files — the time and the volume of the
    copy; a repeat — the time and 0
    new; 2230 files — the time and the volume; `docs/ARCHITECTURE.md` §Measurements.
11. ADR 0035 "The album export: the album structure, file copies, nothing is overwritten",
    `docs/UI.md`, `plan/M19-albums.md`, `plan/STATUS.md`.

**Files.** `src/organize.rs` (`plan_album`, `apply_copy`), `src/db.rs` (reading the tree),
`src/main.rs`,
`src/app.rs`, `docs/`, `plan/`.

**Not doing.** Links and `--mode` in `export`, service files in `--out`, a "sync" and deleting
the extra in the target, an export by a schedule or at start, indexing `--out` without asking.

**Done.** `organize::plan_album` builds the tree `--out/<album>/<subalbum>/…` with the names
`{n}_{orig}`; `organize::apply_export` copies the files (`std::fs::copy`, not links), compares
the existing ones by size and `sha256`, moves a taken name to `-2`, skips what was already exported,
and a file
under an old number counts as "the order in the target is stale". The export cache — in
`<cache>/photocat/exports/`,
there are no service files in `--out`. `export` in the CLI (the codes 0/1/2) and the dialog "Export
to disk…" in the GUI with
a plan tree, progress and "Stop". The structure: `db::album_subtree`/`album_export_data`;
`organize` got `derive` (only `sha256_file`). On the corpus: 500 files / 2.4 GiB — 2045 ms and
a repeat 20 ms with 0 new; 2230 files / 21.2 GiB — 110 ms and a repeat 45 ms; 16 new tests,
`cargo test` — 155 (151 passed, 4 `#[ignore]`), `clippy` and `check.sh` — clean. ADR 0035.

---

## M21 — language of the project and of the interface ✅ (done)

**Goal.** The repository speaks English (docs, commits) while the Russian documentation stays as the
second variant; the interface language is chosen in the settings: English is the main language,
Russian the second. Details and the screen-by-screen order — `plan/I18N.md`.

**Acceptance criteria.**
1. `LICENSE` (Apache-2.0) and `NOTICE` (photocat) in the repository root; `Cargo.toml` declares the
   license; the README has a license section that points at both files.
2. `README.md` and `AGENTS.md` are English; `README.ru.md` and `AGENTS.ru.md` keep the Russian text
   and both directions are linked; no Russian documentation is deleted.
3. "Settings → Interface" offers English and Russian; the choice is `settings.lang` in the DB, saved
   on the spot and restored on the next start; `photocat settings --lang en|ru` does the same.
4. `src/i18n.rs` (L0) holds `Lang`, the Russian→English table and `t(lang, "…")`; an untranslated
   string is shown in Russian, never as a key and never empty.
5. A translated screen is fully translated: a headless frame test finds the English labels and no
   Russian ones for the labels covered by that commit.
6. `plan/I18N.md` keeps the coverage table up to date; ADR 0040 explains the lookup-table choice.
7. `cargo clippy --all-targets`, `cargo test`, `check.sh` clean; the GUI smoke test (`timeout 6`) has
   no panic.

**Files.** `src/i18n.rs`, `src/app.rs`, `src/db.rs`, `src/main.rs`, `Cargo.toml`, `LICENSE`,
`NOTICE`, `README.md`, `README.ru.md`, `AGENTS.md`, `AGENTS.ru.md`, `docs/`, `plan/`, `.pi/`.

**Do not.** Add a translation framework or a second source of UI strings; translate identifiers or
DB enums; ship a half-translated screen and call it done; drop the Russian variant.

**Done (M21.1/M21.2).** Apache-2.0 license and notice; bilingual README/AGENTS; `src/i18n.rs` with the
`Lang` type and the pair table; `settings.lang` + "Interface" settings tab + CLI flag; translated
shell (55 labels); ADR 0040; `plan/I18N.md`; tests for translation, persistence and table sanity.
`cargo test` — 182 passed, 6 `#[ignore]`; `clippy` clean; `check.sh` — 44/0.
M21.2: language detected from the system locale on the first launch (`db::ensure_lang`, English when
unknown); the grid is translated (title, counts, month headers, sort/layout, chips, empty state,
`db::human_in` units, `i18n::count` plurals) — `cargo test` 185 passed.
M21.3: the file card and the inspector are translated (panel title, empty state, preview placeholder,
selection line, `Open full screen`/`To trash` and statuses, `Date`/`Camera`/`Size`, date source,
`Albums`, `AI tags` with hidden-tag restore, `My tags`, `Text on the photo`) — `cargo test` 186
tests include a headless English card frame; `check.sh` 44/0.
M21.4a: the duplicates screen is translated (threshold, `Find groups`, summary, plan export/copy,
group header, `decision`/`advice`, tile labels `spare`/`keep`) — `cargo test` 187 tests include a
headless English duplicates frame; `check.sh` 44/0.
M21.4b: the calendar is translated (scale tabs, `Today`, navigation hints, month summary, empty
notes, day headings, `No date`); month and weekday names moved into `i18n` — `cargo test` 189 tests
include a headless English calendar frame and a weekday test; `check.sh` 44/0.
M21.4c: the faces screen is translated (threshold, model note, counters, `Resume`/`Find faces`,
`Rebuild groups`, group list, name and batch fields, tile labels `not him`/`pick`); `draw_face_cell`
takes `lang` — `cargo test` 190 tests include a headless English faces frame; `check.sh` 44/0.
M21.4d: the trash is translated (screen, count, `Restore all`, `Empty the trash`, tile marks, the
confirmation dialog and the statuses) — `cargo test` 191 tests include a headless English trash
frame with the open dialog; `check.sh` 44/0.
M21.4e: the integrity screen is translated (fields, three buttons with hints, reminder row, verdicts,
copy buttons, plan errors); `title`/`verdict`/`status` take `lang`, the report body stays with the
CLI reports (M21.6) — `cargo test` 192 tests include a headless English integrity frame; `check.sh`
44/0. Four integrity tests that assert Russian set `settings.lang = Ru` themselves.
M21.4f: the left panel (model block with the tag filter, the text block, the summary) and all six
settings panes are translated; `tools::Tool::purpose`/`without`/`Tools::source` are wrapped in `t()`
by the GUI and stay Russian for `doctor` until M21.6 — `cargo test` 194 tests include a headless
English left-panel frame and six settings-pane frames; `check.sh` 44/0.
M21.5: the full screen viewer and the built-in player controls are translated (header, playback and
slideshow rows, zoom row, the frame overlays and every player status), together with the archive
folders dialog, the folder picker and the storage confirmation — the last Russian surfaces inside the
window; `i18n::position`/`i18n::seconds` build the counter and the interval per language,
`draw_viewer_image`/`paint_play_overlay` take `lang` — `cargo test` 198 tests include a headless
English viewer frame (image and video), the two folder dialogs and all three storage confirmations;
`check.sh` 44/0.

M21.6: the CLI reports are translated (`index`, `stats`, `dupes`, `organize`, `export`, `verify`,
`manifest`, `embed`, `tags`, `faces`, `ocr`, `search`, `clusters`, `models`, `settings`, `albums`,
`doctor`, `--help`). Report structs carry a `lang` field set by the caller; the sentences are built
with `i18n::fill(lang, template, values)` so the translation owns the word order; `db::human_in`
gives `KiB`/`КиБ`. `dupes` keeps the reason and the advice as data (`Reason`, `AdvicePart`) and
builds the words at display time, so a group found before a language change is not stale. `--help` is
one translated block (`i18n::USAGE`); `tools::Tool::purpose`/`without`/`Tools::source` take `lang`.
Runtime diagnostics from L1/L2 still speak Russian (recorded in `plan/I18N.md` as the tail).
`cargo test` 205 tests (5 new: `fill`, placeholder parity, the source scan that proves every `t`/`fill`
literal is in `PAIRS`, English CLI reports on both languages, English `--help`); `cargo clippy` clean;
`check.sh` 52/0; on the corpus copy `dupes` stays 103 groups / 238 files / 214.1 MiB in 0.023 s.
M21.7a–c and M21.8a–c: the behavior documents and the plan are English, each with a Russian
`.ru.md` variant and links both ways (details in `plan/STATUS.md`). M21.9: the eight repository
skills and references (`.pi/skills/*/SKILL.md`, `.pi/skills/ui-design/references/*.md`) are English
with Russian `.ru.md` originals; the prompt was already English, the two scripts speak English too;
`check.sh` — 52/0.

## M21.8b — the backlog in English (2026-11-24)

`plan/BACKLOG.md` is English now; the Russian original is `plan/BACKLOG.ru.md`, both files link to
each other. All **26 task blocks** are there with the same ids, the same acceptance criteria and the
same measurements (indexing 64.9–69.3 s, 103 duplicate groups, 2498 faces in 630 s, the M20 export
2045 ms for 500 files, `dupes` 0.023 s after M21.6); the paths, the crate names, the CLI commands and
the corpus folder stay as data.

**What changed beyond translation.** The M21 block was already English (the task list is written in
English since M21) and is kept verbatim in both variants. References to the behaviour documents were
retargeted from `docs/*.ru.md` to `docs/*.md`; `docs/UI.md` §Interaction and §Debt are named
in English, while the Russian file keeps the Russian section names. Two acceptance criteria
were made honest by the translation itself: the M9 and M17 blocks said "the texts in Russian",
which is no longer true of the window since M21, so they now say "the texts in English"; the
M3 prompt list is named without a language, because the defaults have been bilingual since
M15.5. The criteria themselves are unchanged — only the wording follows the interface.

**Checks.** Both files have 26 `## ` blocks and one `# `; the English file has 2 lines with Cyrillic
left (the `Фотографии с телефона` corpus folder and the `КиБ`/`KiB` pair quoted as evidence). No code
was touched: `cargo test`, `clippy` and `check.sh` are unaffected.

**Next.** M21.8c: `plan/SESSION.md`, `plan/M15-tags.md`, `plan/M16-interface.md`,
`plan/M19-albums.md`; `plan/M3-clusters.md` stays uncommitted.

---

## M22 — the desktop UI specification: two themes and screen-by-screen conformance ✅

**Goal.** The owner's mock-up set (`docs/references/mockups/`) becomes the interface: one token palette
with a dark and a light theme, the layout of the main screens, and the same rules for the screens the
mock-up has no layout for.

**Source of truth.** `plan/M22-ui-spec.md`: the token table, the layout metrics, the screen-to-mock-up
map, the rules for an unsupported screen, the disputed questions and the full task list (M22.1–M22.11).

**Done so far.** M22.1 (two themes, the token palette, ADR 0044), M22.2 (the main screen after the
mock-up, three cell views) — 2026-11-24; M22.2a (the list view scales with the size slider, the slider
is always visible) — 2026-11-24; M22.2b (scrolling: a wheel notch is a third of the screen, a trackpad
swipe coasts after the fingers lift) — 2026-11-24; M22.3 (the left panel: the five newest years and the
tag chips, the panel budget measured at 840 px) — 2026-11-25; M22.4 (the month heading pinned while its
group is on screen, which folds in the former M13e) — 2026-11-25; M22.5 (the calendar after
`photocat_2`/`photocat_12`: the year layer of twelve month cards with counters, covers and sizes, the
month layer with the caps weekday row and the volume bars, the day list with the time, name and size,
the accent frame on the picked cell) — 2026-11-25; M22.6 (albums: the view heading "Albums", the open
album's card header at 14 px with its facts in mono 11, sub-album cards with a 16:10 cover, the name and
the counts from the "Tiles" slider, one scroll area so the cards never push the grid out) — 2026-11-25;
M22.7 (the full-screen viewer: the header and the tools row with words, "1:1" and a turned view, the
series strip of the day, the face chips over the frame, the inspector with the same file card) —
2026-11-25; M22.8 (the settings dialog after `photocat_4` — a caps section list with icons and the
accent edge of the active row — and the mock-up's icons as drawn geometry on every button of the
window, ADR 0048) — 2026-11-25; M22.9 (duplicates, faces, trash and integrity built from the parts of
the main screen: a `card_block` per duplicate group, the face tile's caption in mono 10, the one
`danger` word on "Empty the trash", the integrity form in the content panel with mono path fields and
one accent) — 2026-11-25; M22.10 (dialogs and reports: one `dialog_frame` — the card tone, a 1 px
`border`, the 8 px radius, 16 px of air — for all seven dialogs, 14 px titles inside the type scale,
exactly one accent button per action dialog and `danger` on the OS-trash confirmation, a facts row in
mono 11 for the numbers, the export report in mono 11 with a copy button, and the last two English
words of the Russian window through `t()`) — 2026-11-25; M22.11 (the light theme pass: the WCAG 2.1
ratio of every pair the screens paint is a test now, the light accent became the design system's
`amber-focus` `#b45309` — 2.91:1 → 4.58:1 on the panel — the active navigation row moved to the
`card` tone with a 2 px accent edge, the two `danger` values rose one step, and every screen renders
in both palettes without a colour of its own) — 2026-11-25; M22.12 (the right panel keeps its
280 px: the own-tag row takes the width the button leaves and no longer widens the panel past the
window, a headless frame of the whole window fails on any rectangle outside it, and the first status
line is built from the settings language) — 2026-11-25; M22.13 (every action button carries an
icon: six chained `.button(` calls became `WordButton`s, the one menu of the window is a `WordButton`
over `Popup::menu` with the `sort` glyph, and the guard now fails on a chained button or a word-only
menu) — 2026-11-25; M22.14 (a tile is a card again — the preview on the `field` well, the body on
`card`, the caption on `panel` — and the file card of `photocat_1`: the preview card with all four
corners cut, the identity row with the name and the size, the "Shooting params" block, album and tag
chips, the engine row; `card_block` counts its own 1 px line, so no block is wider than its panel) —
2026-11-26; M22.15 (the tile preview fills the whole 4:3 frame with a centring crop — `cover_rect`,
`ThumbFit` — instead of the letterbox; the calendar, the series strip, the album cover and the file
card keep the fitted whole preview at that point — the album cover dropped it in M22.29) — 2026-11-26;
M22.16 (the right panel after `photocat_12`: the
panel's frame with no margin of its own so its 280 px and the 1 px line are its own, a 44 px header
band with the mock-up's "×", the card on 12 px of air, the format badge of the preview, the pin of
the location row, the folder glyph of an album chip and the framed "`#`" own-tag row; the round
icon-only actions, the album "+ Add" chip and the 320 px width stay refused) — 2026-11-26;
M22.17 (the word of a card chip acts and only the "×" removes: a click on a tag filters the grid by
it and the accent marks that filter instead of "the confident tag", a click on an album chip's word
opens the album; `ChipClick` splits every card chip into the word and the "×") — 2026-11-26;
M22.18 (the own-tag row: the word button became a bare 22 px "+" with the word as its hint, so the
field takes ~190 px of the 255 px card instead of 81; the field and the "+" are one 22 px line with
the text centred, the row 28 px) — 2026-11-26; M22.19 (the top strip after `photocat_1`: the mark,
one search field with the mode switch, the "Tiles" pill, the state pill, the lit details toggle and
"Settings"; the content is one 44 px ribbon with the chip row only while a filter is on; the
selection floats at the bottom edge of the grid; the footer carries the counters and the selected
file; the years take three columns of two rows) — 2026-11-26; M22.20 (the albums after
`photocat_3`: the root is the mock-up's card grid with the counters over the cover and "size ·
date" under it, the control row filters the album names, orders the cards and switches them to a
list, the breadcrumb and the tree live inside an album, `AlbumNode::updated_at` gives the card its
date, and the "add photos to an album" dialog has three entrances — the card's dashed "+ Add", the
selection bar and "Add the picked ones", with a new album made in the same press; rows only) —
2026-11-26; M22.21 (the album card opens the album from the root — the action used to be dropped
while no album was open — and its cover is a mosaic of up to four of its first photos,
`AlbumNode::covers`; one photo whole, two in columns, three as a half and a right stack, four as a
2×2 grid with 2 px seams; every cell is filled and cropped since M22.29) — 2026-11-26; M22.22 (the albums root hides the ribbon's dead
"Tile / Rows / List" switch — the root has no snapshot grid — so the "Cards / List" of the album
control row is the only look switch there, and the switch now has a click test in both languages) —
2026-11-26; M22.23 (on the whole Albums screen the ribbon's cell views are hidden and its "Cards /
List" drives both the album cards and the album's photos — `App::grid_cell_style`, the global cell
view of the other screens stays; an album without sub-albums no longer looks dead) — 2026-11-26;
M22.24 (every album tree row carries a 14 px folder glyph and is one click target, the glyph
included; an album without a preview gets the real folder icon instead of the "▸" mark on its card
and in the list row) — 2026-11-26; M22.25 (a long file name is cut to the room the size leaves, so
the identity row cannot widen the 255 px card and the right panel stays 280 px; the grid keeps its
tile width when a photo is selected, and the album's "All files" switch is translated) — 2026-11-26;
M22.26 (the People screen after the owner's look: the tile's "pick" and "not him" became round icon
targets, the group list moved into the right panel with the best face of each group as its icon, the
dead "Tile / Rows / List" switch and the details toggle left the screen) — 2026-11-26; M22.27 (the
trash's per-tile "restore" became a round 24 px icon target on the tile and in the list row, the word
moved into the translated hint) — 2026-11-26; M22.28 (the full screen: the header ends with a 22 px
"×" that closes the screen, the face boxes moved behind the "Show faces" tool with the face glyph,
and a click on a box opens the 280 px right panel with every photo of that face, oldest first;
`db::FaceBox` carries the face id and its group) — 2026-11-26; M22.29 (an album cover has no margins:
every cell of the mosaic is filled and cropped by `ThumbFit::Filling` instead of the fitted whole
preview, `paint_cover_mosaic` cuts the four corners back to the 4 px radius with `round_preview`, and
the card and the list row share that call) — 2026-11-26.

**Acceptance criteria (whole block).**
1. `app::Theme` is the only colour source: `DARK` and `LIGHT`, `settings.theme`
   (`dark | light | system`, key/value, `SCHEMA_VERSION` unchanged), "Settings → Interface → theme".
   Radius 4/6/8 px (controls; navigation and menus; cards and dialogs), no shadows, the type scale
   10/11/12/13/14/18, monospace numbers, caps labels.
2. The main screen follows `photocat_1`: a 32 px top strip and a 28 px bottom strip, the mark and
   one search field in the strip, the working folder in the left panel; one 44 px ribbon above the
   grid, the chips only while a filter is on, the selection in the floating bar (M22.19); three cell views — the mock-up's 4:3 tile (default) with the preview
   filling the frame (a centring crop, M22.15) and a 22 px caption band, the `field` well of a
   `card` under it; proportion rows, and a list; a 10 px gap, accent selection.
   Previews follow the EXIF orientation and a stale preview is rewritten when its dimensions differ.
3. Calendar (`photocat_2`, `photocat_12`), albums (`photocat_3`) and the viewer (`photocat_5/6`) are
   brought to the same tokens; the albums keep the tree (an owner's decision is asked in the spec §5)
   inside an album, while the root of the view is the mock-up's card grid (M22.20).
4. Duplicates, faces, trash and integrity have no mock-up: they are built from the same parts, with
   one accent and red only on the OS-trash path.
5. The settings dialog gets the section list and the theme control; the mock-up's non-existent
   functions (autostart, tray, GPU, updates, metadata backup) are not added.
6. The sticky month heading (the former M13e) is closed in M22.4. ✅
7. The list view keeps the size slider and scales with it (row, thumbnail, name); the header never
   jumps when the cell view changes — M22.2a ✅. Trackpad and wheel scrolling is smooth and fast in
   every region — M22.2b ✅ (one profile in `apply_theme` + `Scroll`, 280 px per wheel notch, a
   274 px coast after a 1200 px/s flick, 145–152 fps at p95 on the corpus).
8. Every task ends with a measurement or a look at the result, `cargo test`, `clippy`, `check.sh`;
   `docs/UI.md` § Layout, § Metrics and § "What the interface will not have" follow the code.
9. Every screen reads in both themes: body text and the accent words ≥ 4.5:1, dim hints ≥ 3.0:1
   (checked by `both_themes_pass_the_contrast_ratio`), no screen writes a colour of its own in
   either palette (checked per screen), the light ground is neutral, and dark stays the default —
   M22.11 ✅.

**Files.** `src/app.rs`, `src/db.rs`, `src/i18n.rs`, `docs/UI.md`, `docs/DECISIONS.md`, `plan/`.

**Not doing.** A third theme, an accent picker, a "compact mode", bundled fonts until the owner
decides, cropped previews, a floating selection bar, favourites/ratings/share/permanent delete, the
mock-up's EXIF/GPU/tray/update functions, a network call, a file operation.


---

## M23 — English is the language of the code ✅ (M23.1–M23.6 done)

**Goal.** The repository is English in the code as well as in the documents: identifiers, comments,
logs, errors and the source text of the interface. Russian survives only as a translation
(`src/i18n.rs`, `*.ru.md`, the model prompts). ADR 0046; the direction is the reverse of ADR 0040.

**Acceptance criteria.**
1. No Cyrillic identifier in `src/`: `cargo test --test source_language` fails on one. ✅ (M23.1)
2. `PAIRS` is English → Russian; `t(lang, en)` gives Russian for `Lang::Ru` and the English source
   otherwise; `count(lang, n, noun)` holds both plural sets. ✅ (M23.1)
3. The window and the CLI show the same text as before for the same language — the inversion
   changed no behaviour. ✅ (M23.1: `cargo test` 234 passed)
4. Every module is converted: `PENDING` in `tests/source_language.rs` is empty, so a Russian
   comment or string in `src/` fails the guard (the model prompts of `embed.rs` stay exempt).
   ✅ (M23.4: `PENDING` is empty; comments and literals in `src/` are English)
5. `plan/I18N.md`, `docs/ARCHITECTURE.md` and `AGENTS.md` §6 describe the new direction. ✅ (M23.1)
6. Test messages and the shell scripts are English too: no Russian assertion message, `panic!`,
   `expect` text or `#[ignore]` reason in `src/`, and `scripts/*.sh` print English. The guard fails
   on a Russian message while what a test compares stays data (a Russian label, an album name, OCR
   text). ✅ (M23.5)
7. The archive is walked again after the conversion: a fresh index of the 2230-file corpus, `stats`,
   `dupes`, `organize`, `embed`, `ocr`, the GUI smoke test and every report in both languages give
   the numbers of 2026-11-22 (§ Measurements); a number that moved is explained or fixed. ✅ (M23.6:
   101 duplicate groups instead of 103 — the oriented dHash of M22.2, not the translation; and a real
   regression fixed: the OCR candidate heuristic had lost its content branch and proposed 203
   candidates instead of 357)

**Steps.** M23.1 — identifiers, the inverted table, the guard, the documents (done). M23.2 — L0/L1:
`i18n.rs`, `derive.rs`, `tools.rs`, `db.rs`, `storage.rs` (done: no Cyrillic left, the size units
moved into `i18n::units`, `db::human` speaks English and the GUI uses `human_in`). M23.3 — L2:
`dupes`, `eval`, `ocr`, `faces`, `organize`, `verify`, `recycle`, `index`, `player`, `embed` (done;
the CLIP prompts stay Russian as data, the default tag labels became English and reach the Russian
window through `i18n::word`). M23.4 — `app.rs` (done: 720 comment lines, 200 literals through the
table, the 40 hand-written `match lang` blocks replaced by one English source and one translation).
M23.5 — `main.rs`, the test messages and `scripts/` (done: 205 Russian message strings in ten
modules and the nine messages of `scripts/fetch-model.sh` became English; the guard checks message
positions, the data — album names, queries, expected Russian UI text — stays). M23.6 — the corpus
pass and the numbers in `docs/ARCHITECTURE.md` (done: 67.7 s indexing, 101 duplicate groups / 234
files / 210.3 MiB, 357 OCR candidates → 356 files and 66 584 characters in 138, both languages
printing the same numbers; `db::text_tag_labels` restored the tag branch of the OCR heuristic —
M23.3 made the labels English and the lookup asked an empty `settings.model_id`).

**Not doing.** Translating the owner's own data (album and face names, user tags, `labels.csv`);
a third language; gettext/fluent; changing the language mechanism decided in ADR 0040.

**Files.** `src/*.rs`, `tests/source_language.rs`, `docs/DECISIONS.md`, `plan/I18N.md`.

---

## M24 — a clean install and one binary 🔄 (M24.1–M24.2 done)

**Goal.** A person who has never had photocat: no DB, no previews, no models, no settings. Everything
the app needs appears on its own or is asked for with a button, and the weights come only from the
binary itself — the repository ships no shell scripts, because a cross-platform application is one
file.

**Acceptance criteria.**
1. `scripts/` holds no downloader: `models download --id ID --yes` and the "Download model" button
   are the only way to the weights, and the messages that used to point at `scripts/fetch-model.sh`
   name the models folder and the command instead. ✅ (M24.1)
2. A clean cache (`XDG_CACHE_HOME=$(mktemp -d) ./target/release/photocat gui`): the window opens,
   `<cache>/photocat/photocat.db` and `thumbs/` are created, the language comes from the locale, the
   screen says that no folder is indexed yet, the model block says "not downloaded", the faces and
   text blocks are disabled with an explanation, and no pass starts by itself.
3. The first folder: adding and indexing it from the window works with **no model at all** — stats,
   duplicates, the calendar, the grid and the full screen work without CLIP; the tags and the search
   say "download the model" instead of failing.
4. The model: the consent tick plus "Download model" fetches 217.5 MiB with the sha256 matched, then
   "Compute tags" works; with the network down the message says what failed and the rest keeps
   working.
5. Absent system programs: with no `ffmpeg` and `tesseract` (the fields cleared and the names out of
   `PATH`) the app still starts and indexes photos, downloads a model and runs OCR's disabled state,
   and each block says what is lost — one honest line, no crash, no dialog loop.
6. One binary: `cargo build --release` gives `target/release/photocat`, `doctor` lists the external
   programs as optional, and copying that file to another machine of the same OS is enough (the DB
   and the models travel in the cache). The weights are fetched by the binary itself, with no
   external program at all. ✅ (M24.2)
7. The pass is recorded in `plan/STATUS.md` with numbers: what a clean start costs in bytes and
   seconds, and what the first index of a 120-file folder takes.

**Steps.** M24.1 — the shell script is gone, the messages name the binary (done 2026-11-24; the
smoke of a clean start also caught the GUI not storing the first-launch language — fixed in
`app::first_lang`). M24.2 — the downloader is the binary's own HTTPS client, `ureq` + `rustls`, and
`curl` is gone (done 2026-11-24: 148.6 MiB in 22.7 s with the sha256 matched; ADR 0047). M24.3 — the
clean-install pass itself: an empty cache, no models, no external programs, then a model download
with consent.

**Not doing.** Bundling the weights into the binary (218–393 MiB, and they change independently of
the code); bundling `ffmpeg`/`tesseract` (ADR 0023); a portable single-file package (AppImage and
alike) — the target stays a release binary plus the cache.

**Decided (the owner, 2026-11-24).** The external `curl` is gone in favour of an in-process HTTPS
client (ADR 0047): the program must be able to download data itself, and "one binary" is strict. The
dependency is `ureq` + `rustls`, the binary grew by 2.4 MB.

**Files.** `src/embed.rs`, `src/tools.rs`, `src/app.rs`, `README.md`, `docs/DECISIONS.md`,
`plan/STATUS.md`.

---

## M25 — the left panel, the calendar and the card after the owner's review (planned 2026-11-26)

**Goal.** The owner read the window of M22.29 and wrote down what does not speak for itself. The
left panel must explain itself without a manual: a filter accumulates, every reset stands where its
filter was set, the numbers say what they count, and every action carries a hint. The calendar
becomes four period layers instead of one mixed layer, and the photos under the period obey the
cell view. The answers to the questions the owner asked stand first, next to the tasks that build
on them.

**Answers (what the questions were about).**
1. "Search by path or date" is literal, not smart: a case-insensitive substring of the path or of the
   ISO date (`2024-05` — May 2024, `vacation` — a folder or a file whose name contains it). Search
   by the content of a photo is the top bar's field with the "Semantic / By text" mode. Today a
   year click writes the year into this field, so the two look like one control — M25.3 separates
   them.
2. "No preview" is a file whose preview could not be built or is no longer in the cache: HEIC/AVIF
   (`image` cannot read them), a broken JPEG/PNG, a video without a cover (no `ffmpeg` or an ffmpeg
   error), or previews cleared in "Settings → Storage". The tile says "no preview". The summary
   counts `files.thumb IS NULL`, which stops being true after a cache cleanup — M25.4 makes the
   count honest and lets a re-index bring the previews back.
3. "Synced / Syncing" is not a synchronisation: it says that a background pass is running (indexing,
   CLIP, OCR, faces, the trash, an export, an integrity check) or that none is. Nothing is sent
   anywhere. It says nothing about the archive, so it moves into the status bar with a tooltip that
   names the passes — M25.8.
4. "Start indexing" walks the folders, reads only new and changed files (path + size + mtime decide)
   and writes to the cache DB and `thumbs/`; "Refresh" re-reads the DB into the window and reads no
   file; neither touches the archive. Both get a tooltip that says exactly this — M25.7.
5. A place name is not in a photograph: EXIF carries the coordinates only. A name needs a reverse
   geocoder — the network (forbidden by §1) or an offline data set (a new dependency and an ADR).
   The card shows the coordinates; M25.9 adds "copy" and records the choice.

**Tasks.** M25.1 (done 2026-11-26) the accumulating tag filter; M25.2 (done 2026-11-26) the year cloud (the five newest years, "…" for
  the older ones, the kept "All years"); M25.3 (done 2026-11-26) the path/date field; M25.4 (done 2026-11-26) "No date" and "No preview"
  as filters; M25.5 (done 2026-11-26) the summary leaves; M25.6 (done 2026-11-26) the pinned threshold and no "Model settings";
  M25.7 (done 2026-11-26) the two archive buttons explain themselves; M25.8 (done 2026-11-26) the state word moves into the status bar;
  M25.9 (done 2026-11-26) the location row; M25.10 (done 2026-11-26) the year and month layers; M25.11 the week and day layers;
  M25.12 the photos of the period follow the cell view; M25.13 the text block moves into Settings.

**M25.1 — the tag filter accumulates.**
**Goal.** A click adds a tag, a second click removes it; a reset is one click on the picked tag or
one reset control, not a permanent "all tags" cell.

**Acceptance criteria.**
1. `tag_filter: Option<String>` becomes `tag_filters: Vec<String>`, sorted and without duplicates;
   an empty set means "no tag filter". A file passes only when it carries **every** picked tag above
   that tag's threshold (AND); a word that exists both as a CLIP tag and as an own tag keeps
   matching both.
2. One rule in every tag cloud: the left panel chips, the AI tags and the own tags of the file card
   (M22.17), and the filter chips row above the grid — one chip per tag there.
3. The "all tags" chip is gone. A caps heading "Tags" carries a bare 20 px `Icon::Close` target at
   its right with the hint "reset the tag filter", drawn only while at least one tag is picked
   (the folder block's "+" pattern).
4. A chip's number keeps its meaning — how many photos carry this tag above its threshold, not the
   size of the intersection — so the promise "this click shows N photos" stays true and the row
   never moves on a click.
5. The card chip's hint becomes "add this tag to the filter" / "remove this tag from the filter".

**Acceptance.** "sea" then "document" → both chips lit, the grid holds only photos with both; a
second click on "sea" leaves "document"; the reset clears the set; the chips row shows both tags
and each "×" removes exactly one; one test covers the panel cloud and one the card.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/I18N.md`.

**Done (2026-11-26).** `App::tag_filters: Vec<String>` — sorted, no duplicates — with `tag_picked`,
`toggle_tag` and `clear_tag_filters`; `filtered()` demands every picked tag above its own threshold
(an own tag still matches the same word), and `PickScope.tag` became `Vec<String>`, so a change of
the set drops the selection as before. The "all tags" chip is gone: the cloud sits under a caps
"Tags" heading with a bare 20 px "×" (`tags_heading`) that clears the whole set and is drawn only
while a tag is picked; a click on a chip of the panel, of the AI tags or of the own tags adds its
tag and a second click removes it, and the card chips say "add this tag to the filter" / "remove
this tag from the filter". The chips row draws one chip per tag and each "×" removes exactly one,
while "Reset" clears them all; the count on a chip still means the photos that carry this tag alone.
The "all tags" percentile went with the chip: `embed::max_score` and `top_score_distribution` and
`App::top_scores` are removed, and the empty grid names one tag as before or says that no photo
carries all the picked tags. Tests: `the_panel_tag_cloud_accumulates_the_filter` (a click adds, a
second click removes, the AND narrowing, the two "×" of the chips row and "Reset"),
`the_reset_beside_the_tags_heading_clears_the_filter`, the extended
`a_click_on_a_tag_in_the_card_filters_the_photos` and the updated
`the_tag_chips_do_not_jump_when_a_tag_is_picked`. `cargo test` 313 passed + 3 guard tests,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**M25.2 — the year cloud: the newest years, "…" for the rest, and the kept "All years".**
**Goal.** The cloud stays short and dense — the five newest years, an ellipsis that filters the
older ones, the counts in the hint, and "All years" as the way back (the owner's word of
2026-11-26: "keep the All years button and add an ellipsis that works as a filter — all the rest of
the years").

**Acceptance criteria.**
1. Every drawn year is a wrapped chip instead of the 3-column 20 px rows: mono 11, 18 px, the width
   from the text, 4 px gaps — the geometry of a tag chip, so the block is dense.
2. The cloud holds the five newest years. A sixth chip "…" appears only when the index has older
   years and narrows the grid to exactly those older years; the year filter becomes
   `year_filter: Option<YearPick>` — one year or "older than the oldest year in the cloud" —
   matched against the prefix of `taken_at`. The "…" is a filter, not an expander: the cloud never
   grows.
3. No number is drawn inside a chip; the hover hint says "2019 · 1 234 files" through `i18n::count`;
   the "…" hint says "older than 2019 · N files" and names the range of the old years.
4. "All years" stays, drawn only while a year or the "…" is picked (as in M22.3): it clears the year
   filter. No second reset stands next to the "Years" heading — the owner kept this chip as the way
   back, and two controls that clear the same filter are one too many.
5. The year no longer writes into the path/date field (M25.3); the chips row shows "year: 2019 ×" or
   "older than 2019 ×".
6. `year_counts` returns every year of the index; the block takes the five newest for the cloud and
   knows whether the rest exists.

**Acceptance.** An archive of 2004–2024 draws five years and "…"; the "…" shows only the files of
2004–2019; a second click on the picked year or on "…" clears the pick; "All years" clears either;
the count lives only in the hint; a typed path word survives a year click; a headless 1240×840
frame draws the cloud without growing the pinned part; the M22.3 panel budget is measured again and
written to `plan/STATUS.md`.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/STATUS.md`.

**Done (2026-11-26).** The year block is a wrapped cloud of 18 px chips (`cloud_chip`, `year_chip`,
`YEARS_CHIP_H`): the five newest years and, when the index holds older ones, a sixth "…" chip. The
year filter is its own field — `App::year_filter: Option<YearPick>` (`YearPick::Year` /
`YearPick::Older(bound)`), matched against the prefix of `taken_at` — and `set_year` replaced
`pick_year`, so the path/date field keeps its word (the M25.3 half of the split is done here too).
`year_counts` returns every year; the cloud takes the five newest and knows whether the rest exists
(`years_range` names "2004–2019" in the "…" hint). No number is drawn in a chip: the hint says
"2019 · 1 234 files" through `i18n::count` (the "…" hint adds "older than 2020 · N files ·
2004–2019"); "All years" is drawn only while a pick is on and clears either kind of pick. The chips
row shows "year: 2019 ×" or "older than 2020 ×"; `FilterClear.year` and `PickScope.year` follow.
Tests: `the_years_cloud_keeps_five_and_filters_the_older_ones` (five chips and "…", no 2019/2004 in
the cloud, no number inside a chip, the "…" narrows to 2004–2019, the chips row, a second click, "All
years"), the reworked `a_year_click_narrows_the_date_filter` (every year counted, the year is its own
filter, a typed word survives the click) and the updated block and budget tests. Measured on the
corpus (2232 files, 10 years, 1240×840 headless frame): the "Years" heading at y 593, the first chip
at 618.5, the "…" at 648.5, the cloud ends at 666.5 and the pinned "Summary" starts at 692 — every
chip is seen without a scroll. `cargo test` 314 passed + 3 guard tests, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**M25.3 — the path/date field explains itself.**
**Goal.** The owner knows what the field compares before typing into it, and the year stops living
in it.

**Acceptance criteria.**
1. The label stays "Search by path or date"; the field carries a tooltip: the typed text is a
   case-insensitive substring of the path and of the ISO date, with the two examples "2024-05" and
   "vacation", plus where the content search lives ("Semantic / By text" in the top bar).
2. The year no longer writes into `query` (M25.2): `pick_year` becomes a setter of `year_filter`, and
   the chips row shows the year and the path word as two chips with two "×".
3. The hint text stays "2024-05, vacation"; `docs/UI.md` § Filters describes the field and says that
   a word search by content is another field.

**Acceptance.** A test types "vacation" and picks a year: both filters apply together, both chips
are drawn, each "×" removes exactly one; the tooltip is in `PAIRS` and reads in both languages.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`.

**Done (2026-11-26).** The split itself came with M25.2: `set_year` writes `App::year_filter`, not
`query`, and the chips row draws a year chip and a path chip with two "×". This task adds the field's
tooltip (`PATH_DATE_HINT`, one source for the window and the test): the typed text is a
case-insensitive part of the path or of the ISO date, with the two examples, and the content search
is "Semantic / By text" in the top bar. The hint stays `2024-05, vacation`. `docs/UI.md` gained a
§ Filters subsection that says the same and names the other field. Test:
`the_path_word_and_the_year_are_two_filters_with_two_crosses` (the word and the year apply together —
only `vacation-c.jpg` remains —, each chip's "×" removes exactly its own filter, and the tooltip is in
`PAIRS` and reads in Russian). `cargo test` 315 passed + 3 guard tests, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**M25.4 — "No date" and "No preview" are filters.**
**Goal.** The two numbers of the summary become two filters, and the preview count stops lying after
a cache cleanup.

**Acceptance criteria.**
1. Two toggle chips in the left panel filter block, under the kind row: "No date · N" and "No
   preview · N", the 20 px chip of a tag, the picked one on the accent, dimmed and disabled at
   zero.
2. They narrow the grid together with the rest and with each other: "no date" = `taken_at IS NULL`;
   "no preview" = `files.thumb IS NULL` **or the preview is not on disk** (the check runs once per
   `reload`, not per frame).
3. `storage::clear_thumbs` also runs `UPDATE files SET thumb = NULL`: a cleared cache must be
   healed by the next index. Today the skip rule (`can_skip` asks `thumb`) sees the old path, skips
   the file and the preview never comes back — `docs/ARCHITECTURE.md` § Indexing flow says "until a
   re-index", which is not true yet. No new column, so `SCHEMA_VERSION` is not bumped.
4. The hint of "No preview" names the four causes (HEIC/AVIF, a broken file, a video without a
   cover, cleared previews) and the way back ("index the folder again").
5. The counts are taken over the whole index, not over the current filter.

**Acceptance.** On a folder with one HEIC and a video indexed without `ffmpeg` the chips show the
count and a click shows exactly those files; after "Settings → Storage → Clear the previews" the
count equals the number of files, and one `index` pass nulls to zero and rebuilds every preview;
tests cover `clear_thumbs` + `can_skip` and the filter.

**Files.** `src/app.rs`, `src/storage.rs`, `src/index.rs`, `src/i18n.rs`, `docs/UI.md`,
`docs/ARCHITECTURE.md`.

**Done (2026-11-26).** Two 20 px toggle chips under the kind row (`App::only_no_date`,
`App::only_no_preview`, `toggle_chip`): "No date · N" and "No preview · N", the picked one on the
accent, dimmed and dead at zero. The counts are over the whole index, and "no preview" is honest:
`files.thumb IS NULL` or the preview named by it is not on disk — `App::previews_missing` is filled
by `refresh_missing_previews` once per `reload`. The two narrow the grid with the rest and with each
other (`filtered()`), appear in the chips row as "no date ×" / "no preview ×"
(`FilterClear.no_date` / `no_preview`), and `PickScope` carries both, so a change drops the
selection. `storage::clear_thumbs(db_path)` now also calls `forget_thumbs`, which nulls the `thumb`
column: `index::can_skip` no longer skips a file whose preview was cleared, so the next pass
rebuilds it (the `docs/ARCHITECTURE.md` § Indexing flow claim is true now). The summary stops
printing "No date:" and "No preview:" — their numbers are the chips now, and a second number
computed differently would drift; the remaining summary goes in M25.5. Tests:
`the_no_date_and_no_preview_chips_filter_the_grid` (the counts, a click, the two together, both
"×", a dead chip at zero) and
`a_cleared_preview_makes_the_file_processed_again` (`forget_thumbs` + `can_skip`). `cargo test` 317
passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors. The two new chips cost the pinned part 28 px: at 840 px the
last control of the duplicates block needs a scroll, so the duplicates translation test draws a
1240×1000 frame (M25.5 gives the panel 40 px back).

**M25.5 — the summary leaves the left panel.**
**Goal.** No doubled numbers and no block that only repeats the status bar.

**Acceptance criteria.**
1. "Summary" is removed from the left panel in both layouts: the pinned bottom of a wide window and
   the block after the card in a narrow one.
2. Files and size are already in the status bar (`N files · X`), so nothing moves there; the other
   three numbers (no date, no preview, videos without a cover) become the counts of the M25.4 chips;
   "videos without a cover" is a subset of "no preview" and is named in that chip's hint.
3. "press “Index” — ffmpeg will create the covers" moves into the same hint.
4. The panel budget test and the translation test of the panel are updated; the frame gains the
   40 px the summary took.

**Acceptance.** "Summary" appears in neither language; the status bar keeps one row; no number is
printed twice; a 1240×840 frame shows the tag cloud, the years and the model block with the summary
gone.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`.

**Done (2026-11-26).** `stats_block` is gone with both of its callers: the pinned `Panel::bottom
("left-stats")` of a wide window and the block after the file card in a narrow one. Files and size
were already in the status bar; the two "missing" numbers became the M25.4 chips, and "videos
without a cover" plus "press “Index” — ffmpeg will create the covers" moved into the "No preview"
hint. The strings `Summary`, `Files:`, `Size:`, `No date:`, `No preview:`, `Videos without a
cover:` and the ffmpeg line left `PAIRS`; `stat_row` stays for the storage pane. Measured on the
corpus (2232 files, 1240×840 headless frame, release): the years heading at y 629, the first chip
at 654.5, the "…" at 684.5, the cloud ends at 702.5, the model block starts at 717 and the bottom
strip at 812 — the 40 px the summary took went to the scrolling part, so the model block is now on
screen instead of hiding under the pinned block. The tag cloud inside the model block is still
~13 px below the strip: M25.6 makes the cloud the first block of the scroll, which brings it into
the frame (its acceptance names exactly that). `cargo test` 317 passed + 3 guard tests,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors;
`panel_years_measurement` prints the numbers.

**M25.6 — the threshold is pinned; the model is set in Settings only.**
**Goal.** The threshold slider never scrolls away, and the left panel stops being a second settings
screen.

**Acceptance criteria.**
1. The tag threshold (the slider, its percentage, the "few tags" and "no tag passes the threshold"
   notes) leaves `model_block` and is drawn in the pinned part of the left panel, under the kind and
   the two new filter rows and directly above the scroll.
2. The tag cloud becomes the first block of the scroll, so the threshold stands next to the chips it
   cuts; the years and the model block follow. M22.3's "the years open the scrolling part" is undone
   on purpose, and its measurement is redone.
3. The "Model settings" button leaves the panel: the model's path, size limit and choice live only
   in "Settings → AI model". The model state and "Compute tags" stay in the block.
4. The threshold's tooltip keeps the percentile explanation and says what the slider does to the
   cloud below it.

**Acceptance.** At 840 px the slider is visible without a scroll and stays put while the cloud
scrolls; no "Model settings" in the panel; the settings dialog is the only route to the model; the
panel budget is measured and recorded.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/STATUS.md`.

**Done (2026-11-26).** `threshold_block` draws the slider, its percentage, the "few tags" note and
"no tag passes the threshold — lower the slider" in the pinned part of the panel, under the two
"missing" chips and directly above the scroll; the tooltip keeps the percentile explanation and
adds what the slider does to the cloud below it. The scroll now opens with the tag cloud (the
`Tags` heading and its reset, then the chips), and the years and the model block follow — M22.3's
"the years open the scroll" is undone on purpose. `model_block` keeps the model state, "Compute
tags", the confident-tag line and the free-search line; the cloud, the threshold and the "Model
settings" button left it, so the model's path, size limit and choice live only in "Settings → AI
model". Tests: `the_left_panel_pins_the_threshold_above_the_tag_cloud` (at 840 px the threshold and
the cloud are on screen, the cloud below the slider, no "Model settings" in either language) and
`the_threshold_stays_put_while_the_tag_cloud_scrolls` (24 frames with the wheel over the panel: the
threshold keeps its y, the years under the cloud move up); the years tests point the models folder at
a never-downloaded one so the cloud does not shift them, and a `fake_model_dir` helper makes the
cloud test independent of the machine. Measured on the corpus with
`panel_years_measurement`: 1240×840 — the pinned threshold at y 621, the "Tags" heading at 685.5,
the years and the model block below the strip; 1240×1021 (the owner's height) — the "Years" heading
at 941, the strip at 993. `cargo test` 318 passed + 3 guard tests, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**M25.7 — the two archive buttons explain themselves.**
**Goal.** "Start indexing" and "Refresh" say what they do before a click.

**Acceptance criteria.**
1. "Start indexing"'s tooltip: it walks the folders, reads only new and changed files (path + size +
   mtime), computes the hash, the date, the dimensions, the sharpness and a preview, gives a video a
   cover through `ffmpeg`, writes to the cache only, can be stopped and resumed, and changes nothing
   on disk.
2. "Refresh"'s tooltip: it re-reads the database into the window, reads no file, indexes nothing and
   changes nothing; it is for a change made by the CLI or in another window.
3. Both strings go through `t()`; `docs/UI.md` names both in the controls.

**Acceptance.** Both tooltips read in both languages and match the code (`index::can_skip`,
`App::reload`); `cargo test --test source_language` passes.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`.

**Done (2026-11-26).** `INDEX_HINT` and `REFRESH_HINT` are one source for the window and the test;
"Start indexing" carries the first and "Refresh" the second through `on_hover_text`/`t()`. The
index hint names the skip rule (path + size + mtime, what `index::can_skip` checks), the hash, the
date, the dimensions, the sharpness and the 320 px preview, the video cover through ffmpeg, that
the pass writes to the cache only and never to the archive, and that it can be stopped and resumed;
the refresh hint says it re-reads the database, reads no file and changes nothing, and when to use
it (a change from the CLI or another window). Test:
`the_two_archive_buttons_carry_their_hints` (both strings in `PAIRS`, both read in Russian, and the
wording still names the skip rule, the untouched archive and the read-free refresh).
`cargo test` 319 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors; `cargo test --test source_language` 3.

**M25.8 — the state word moves from the strip into the status bar.**
**Goal.** "Synced" stops looking like a synchronisation feature and says where it belongs.

**Acceptance criteria.**
1. The "Synced / Syncing" pill leaves the top strip; the width it took goes to the search field.
2. The status bar carries the state: its dot and word name the running pass ("Indexing", "CLIP",
   "OCR", "People", "Trash", "Export", "Integrity"); two or more passes show "N passes"; when
   nothing runs the last status message stays as it is.
3. The hint lists all seven passes that `busy()` counts and says that their progress rows are in the
   top strip under the search field.
4. `sync_word` becomes the status bar's word; the top-strip width test and the translation test
   follow.

**Acceptance.** The strip has no state pill at 1240 px; the bottom bar names the running pass within
one frame; the hint names all seven; the search field is wider by the pill's former width.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`.

**Done (2026-11-26).** The "Synced / Syncing" pill left the top strip, and
`SEARCH_MAX_W` grew from the mock-up's 576 to 646 px — the English pill's ~70 px go to the field.
`App::running_passes` names the seven passes with the same strings their progress rows use
(`Indexing`, `People`, `Trash`, `Export`, `Integrity` through `t()`; `CLIP` and `OCR` are names), and
`state_word` returns one name, `{} passes` for two or more, or `None` — then the status bar keeps the
last message exactly as before. The dot and the word carry `PASS_HINT`, which lists all seven and
says their progress rows and "Stop" are in the top strip under the search field. `busy` and
`sync_word` are gone with the pill, and `Synced`/`Syncing` left `PAIRS`. Tests:
`the_status_bar_names_the_running_pass` (no pill; one channel → "Indexing"; two → "2 passes";
nothing → the last message) and `the_search_field_took_the_room_of_the_state_pill` (a 646 px field
frame in the strip, no state pill), plus the updated `english_translates_the_window_shell`.
`cargo test` 321 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

**M25.9 — the location row: the coordinates, and the place-name question.**
**Goal.** The card shows what a photo itself knows about its place, and the missing half is named
instead of faked.

**Acceptance criteria.**
1. The "Location" row of the card keeps the EXIF coordinates (5 decimals) and gains a copy target
   with the hint "copy the coordinates".
2. The hint says that a place name is not in the file, and why: a name needs a reverse geocoder —
   the network is forbidden by §1 of `AGENTS.md`, an offline data set is a dependency and an ADR.
3. The choice is recorded in `docs/DECISIONS.md` as an open question for the owner: no place name /
   an offline data set (size, license, what it covers — country, region or city) / a lookup by an
   explicit click, which sends the coordinates and therefore needs a change of the rule.
4. No map link and no network call in this task.

**Acceptance.** A photo with GPS shows "Location", the coordinates and a working copy; a photo
without GPS shows no row; nothing opens a browser; a test covers both rows.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `docs/DECISIONS.md`.

**Done (2026-11-26).** The "Location" row keeps the two EXIF coordinates to five decimals and gains a
20 px copy target (the hint `copy the coordinates`); a press copies "55.75580, 37.61730" to the
clipboard and says `coordinates copied` in the status bar. The row and its pin carry `LOCATION_HINT`,
which says that EXIF holds the coordinates only, that a place name needs a reverse geocoder, that the
network is forbidden by §1 and an offline data set is a dependency and an ADR. ADR 0049 is that
question for the owner: no place name (today), an offline set (size, licence, coverage) or a lookup
by an explicit click (which sends the coordinates and needs a change of the rule). No map link and no
network call. Test: `the_location_row_shows_and_copies_the_coordinates` (GPS → the row, the five
decimals and a working copy; no GPS → no row). `cargo test` 322 passed + 3 guard tests,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**M25.10 — the calendar's year and month layers.**
**Goal.** "Year" shows years, "Month" shows the months of the chosen year.

**Acceptance criteria.**
1. "Year" draws the years of the archive as tiles, newest first: the year, the count, the size, the
   volume bar against the fullest year and the year's first snapshot as the cover; a click picks a
   year, a double click opens its months.
2. "Month" draws the twelve month cards of the picked year — the layer that "Year" draws today
   (count, size, cover, volume, empty months dimmed, "Go to the nearest month with files").
3. The arrows step by the layer's own unit (a year, a month with files); "Today" picks today's year
   and month and lands on the layer it can keep.
4. The ribbon's facts name the layer: the years of the archive, "2024", "May 2024", "12–18 May".

**Acceptance.** Each tab draws the tiles of its own period; the number of tiles matches the period; the picked one is on the accent; a double click drills one
layer down; a headless frame and the M22.5 measurement are redone on the corpus.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/STATUS.md`.

**Done (2026-11-26).** The "Year" tab now draws the archive's years, newest first, as cards of the
month card's height (`paint_arch_card`, shared by both layers): the year at 12 px, its counter, a
16:10 cover of the year's first snapshot fitted whole, a volume bar against the fullest year and the
size; a click picks a year, a double click opens its months. The twelve month cards moved to "Month"
(the layer "Year" drew before), and the month's day grid left with them; the week and day layers keep
their old code until M25.11. The ribbon facts name the layer (`the years of the archive`, `2024`), the
arrows step by the layer's own unit (a year inside the archive's range, a month with files), and
"Today" lands on the months of today's year. An empty year says "No files in this year." and offers
"Go to <the nearest month with files>". Two fixes came with it: `i18n::count` gained the `year` noun,
and `tag_counts` now computes one threshold per label instead of twice per frame (the pinned
threshold of M25.6 and the cloud both call it) — the measurement below shows the win. Tests:
`the_years_layer_draws_the_archive_years_and_drills_on_a_double_click` (three years newest first, the
click picks, the double click opens the months), the reworked
`the_month_layer_draws_twelve_months_with_counts_and_sizes`,
`the_picked_month_and_day_are_framed_in_the_accent` (a year card on the years layer, a month card on
the months layer, a day cell on the week layer), `an_empty_year_offers_the_nearest_month_with_files`
and `english_translates_the_calendar`. Measured on the corpus (2232 files, 10 years, 1240×840):
years layer p50 4.63 ms (216 fps), p95 4.91; months p50 4.69 (213 fps), p95 5.17; the oldest card at
y 436; the grid scroll again p50 5.72 ms (175 fps), p95 6.14 — better than M22.2b's 6.4–6.7 ms.
`cargo test` 323 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

**M25.11 — the calendar's week and day layers.**
**Goal.** "Week" shows the weeks of the chosen month, "Day" the days of the chosen week.

**Acceptance criteria.**
1. "Week" draws the weeks that touch the picked month as 4–6 tiles: the date range (a week that
   starts in the previous month says so), the count, the size, the first snapshot as the cover and
   the volume bar.
2. "Day" draws the seven days of the picked week: the weekday and the date, the count, up to three
   thumbs and the volume bar; a double click on a day ends the drill-down.
3. The arrows step by week and by day; "Today" lands on today's week and day.

**Acceptance.** The week tiles cover the month exactly once; the day tiles are Monday–Sunday of the
picked week; a click picks, a double click opens the photos below; one test per layer.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`.

**M25.12 — the photos of the period obey the cell view.**
**Goal.** The "Tile / Rows / List" switch works on the calendar, on the photos under the period.

**Acceptance criteria.**
1. Under the tiles, the photos of the picked period are drawn by `draw_files` with the ribbon's cell
   view: the year for the year layer, the month for the month layer, the week and the day; the
   heading "May 2024 · 132 files · 5.4 GiB" stands above them.
2. The fixed `day_strip` and the day-only footer are gone; the switch now changes the calendar
   visibly.
3. "No date" stays a collapsible group at the bottom of the year layer.
4. One `ScrollArea` for the calendar as before, the photos follow the tiles inside it.

**Acceptance.** Switching the cell view changes the photos of every layer in the same frame; the
photos are exactly the files of the period (a test per layer); the year layer over the corpus stays
inside a 60 fps frame, and the M22.5 measurement is redone with the photos in it.

**Files.** `src/app.rs`, `docs/UI.md`, `plan/STATUS.md`, `docs/ARCHITECTURE.md` § Measurements.

**M25.13 — "Text on the photo" moves where OCR is expected.**
**Goal.** The left panel stops carrying a feature that is used once a month.

**Acceptance criteria.**
1. The OCR block (the tesseract state, the recognized counts, the candidates left, "Recognize
   documents / Resume (N) / Stop") leaves the left panel.
2. A new "Settings → Text on photos" pane holds the state and the run, and the `tesseract` path and
   the language packs move there from "External tools", which keeps `ffmpeg`/`ffprobe`; the pane
   explains the candidate rule and says that without tesseract the button is disabled.
3. The per-photo button in the file card stays. The "By text" mode of the top search, when nothing
   is recognized, points at the pane instead of saying "no recognized text yet" alone.
4. The "OCR" progress row and its Stop behave as before.
5. The new strings are in `PAIRS`, the settings tab list and `docs/UI.md` § Settings.

**Acceptance.** The left panel has no OCR block; the new pane starts, stops and resumes a pass; a
clean cache disables the button with one honest line; the 357-candidate corpus pass runs from the
pane.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/I18N.md`.

**Not doing.** Renaming or deleting originals; a network geocoder or a map link; tag filters that
combine with OR; a permanent "all tags" or "all years" cell; a third theme; sending coordinates
anywhere.

**Checks for every task.** `cargo test`, `cargo clippy --all-targets`,
`cargo test --test source_language`, `bash .pi/skills/enforce-architecture/scripts/check.sh`, a
headless 1240×840 frame of the changed screen, and a number in `plan/STATUS.md` when the task
changes a measurement.

**Files.** `src/app.rs`, `src/storage.rs`, `src/index.rs`, `src/i18n.rs`, `docs/UI.md`,
`docs/DECISIONS.md`, `docs/ARCHITECTURE.md`, `plan/`.
