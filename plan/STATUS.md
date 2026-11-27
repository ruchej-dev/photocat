# STATUS

*Русская версия: [STATUS.ru.md](STATUS.ru.md).*

> From M21 on this file is written in English; the older entries were translated from the Russian
> original in M21.8 and the numbers were kept. See `plan/I18N.md`.

STATE: done:M21.1 (mechanism, shell), M21.2 (grid), M21.3 (file card and inspector), M21.4a (duplicates screen), M21.4b (calendar), M21.4c (faces), M21.4d (trash), M21.4e (integrity), M21.4f (left panel blocks, settings panes), M21.5 (viewer, player controls, folder dialogs, storage confirmation), M21.6 (CLI reports), M21.7a (the architecture document), M21.7b (the interface document), M21.7c (the decisions log), M21.8a (`plan/STATUS.md`), M21.8b (`plan/BACKLOG.md`), M21.8c (the remaining `plan/*.md`), M21.9 (the repository skills), M15.6 (the multilingual SigLIP 2 model and the calibration of the tag weight), M15.7 (a blind labelled check on two folders; ADR 0042), M15.6 tail (the fp16 towers against int8 — int8 costs no tag quality, ADR 0043), M13d (panning the zoomed frame in the full screen), M22.1 (two themes, one token palette; ADR 0044), M22.2 (the main screen after the mock-up: three cell views, 10 px gap, EXIF-oriented previews), M22.2a (the list view scales with the size slider and the slider never disappears), M22.2b (scrolling: a wheel notch is a third of the screen, a trackpad swipe coasts after the fingers lift), M22.3 (the left panel: the five newest years and tag chips), M22.4 (the month heading pinned while scrolling), M22.5 (the calendar after `photocat_2`/`photocat_12`: the year and month layers, the day list) → done:M23.1 (English source: 206 identifiers renamed, the table inverted, `tests/source_language.rs` guards the rule), M23.2 (L0/L1 modules), M23.3 (every L2 module, the default tag labels in English), M23.4 (`app.rs`: comments, literals, the `match lang` blocks), M23.5 (the test messages and `scripts/`; the guard checks message positions), M23.6 (the corpus pass: the numbers on 2230 files and the OCR candidate regression it caught), M24.1 (the model download is the binary's business: `scripts/fetch-model.sh` is gone), M24.2 (the binary downloads the weights itself: `ureq` + `rustls` in place of `curl`; ADR 0047), M22.0 (the owner's mock-ups are in `docs/references/mockups/`), M22.6 (the albums: the card header, the sub-album cards with covers, one scroll area for them), M22.7 (the full-screen viewer: the header and the tools row with words, 1:1 and a turned view, the series strip of the day, the face chips, the inspector inside), M22.8 (the settings dialog after `photocat_4`, and the mock-up's icons as drawn geometry on every button; ADR 0048), M22.9 (the screens without a mock-up: duplicates, faces, trash and integrity built from
the parts of the main screen), M22.10 (dialogs and reports: one card frame, one accent button, mono
numbers), M22.11 (the light theme pass: the palette's contrast is a test, the light accent is `#b45309`, every screen renders in both themes without a colour of its own), M22.12 (the right panel keeps its 280 px: the own-tag row fits, no frame leaves the window, and the first status line follows the settings language), M22.13 (every action button carries an icon: six chained `.button(` calls and the one menu became `WordButton`s, and the guard now fails on a chained button or a word-only menu), M22.14 (a tile is a card again — the preview on the `field` well, the body on `card` — and the file card follows `photocat_1`: the preview card, the identity row, the "Shooting params" block, album and tag chips, the engine row; `card_block` counts its own line), M22.15 (the tile preview fills the whole 4:3 frame with a centring crop instead of the letterbox: `cover_rect`; every other preview stays fitted whole), M22.16 (the right panel after `photocat_12`: its own 280 px without a side-panel margin, a 44 px header band with the mock-up's "×", the card on 12 px of air, the format badge, the location pin, the folder glyph of an album chip and the framed "#" own-tag row), M22.17 (the word of a card chip acts and only the "×" removes: a tag click filters the grid by that tag and the accent marks the filter, the album word opens the album), M22.18 (the own-tag row: the word button became a bare 22 px "+", so the field is ~190 px of the 255 px card instead of 81), M22.19 (the top strip, the one-row content ribbon and the floating selection bar after `photocat_1`: the mark, one search field, the "Tiles" pill, the state, the icon details toggle and "Settings"; the chip row only while a filter is on; the years in three columns of two rows), M22.20 (the albums after `photocat_3`: the card grid at the root, the control row with the filter, the order and the Cards/List look, the breadcrumb and the tree inside an album, the card date from `updated_at`, and the "add photos to an album" dialog with three entrances), M22.21 (the album card opens from the root — the open-album action used to be dropped there — and its cover is a mosaic of up to four of its first photos: `AlbumNode::covers`, `cover_cells`, `paint_cover_mosaic`), M22.22 (the ribbon at the albums root hides the dead "Tile / Rows / List" switch), M22.23 (on the whole Albums screen the ribbon's cell views are hidden and the "Cards / List" switch drives both the album cards and the album's photos: `App::grid_cell_style`; the global cell view of the other screens stays), M22.24 (every album tree row carries a folder glyph and is one click target; an album without a preview gets a real folder icon instead of the "▸" mark), M22.25 (a long file name is cut to the room the size leaves, so the right panel keeps its 280 px and the grid no longer jumps when a photo is selected; the "All files" switch of an album is translated), M22.26 (the People screen after the owner's look: the tile's "pick" and "not him" are round icon targets, the group list moved into the right panel with the best face of each group as its icon, and the dead "Tile / Rows / List" switch and details toggle left the screen), M22.27 (the trash's per-tile "restore" became a round icon target, the word lives in the hint), M22.28 (the full screen: a "×" closes it, the face boxes moved to the "Show faces" tool, and a click on a face opens the right panel with all of its photos), M22.29 (an album cover has no margins: every cell of the mosaic is filled and cropped, the 4 px radius survives the crop) → done:M25.1 (the tag filter accumulates: `tag_filters: Vec<String>`, AND over every picked tag above its own threshold, no "all tags" chip but a caps "Tags" heading with a bare 20 px reset) → M25.2 (the year cloud: the five newest years as 18 px chips and a "…" that filters exactly the older ones, the year a filter of its own — `YearPick`, `App::year_filter` — instead of the path field, and the pins at 666.5 / 692 px on the corpus) → M25.3 (the path/date field explains itself: its tooltip says the text is a part of the path or of the ISO date and points at "Semantic / By text", and the year is a filter of its own with its own chip) → M25.4 ("No date · N" and "No preview · N" as two honest toggle chips over the whole index; clearing the previews nulls `files.thumb`, so the next index rebuilds them) → M25.5 (the summary leaves the left panel: the numbers live in the status bar and the two "missing" chips, and the frame gains the 40 px — measured on the corpus, cloud ends 702.5 px, model block 717 px, strip 812 px) → M25.6 (the threshold is pinned above the scroll and the tag cloud opens it; "Model settings" left the panel, the model lives only in Settings; measured: threshold 621 px, cloud 685.5 px at 840, years 941 px at the owner's 1021) → M25.7 (the two archive buttons explain themselves: the index tooltip names the skip rule, the cache-only write and the stop/resume; "Refresh" promises no file read) → M25.8 (the state word moves into the status bar: the pill left the top strip and the search field grew to 646 px, the bar names the running pass or "N passes" with a hint that lists all seven) → M25.9 (the location row: the five-decimal EXIF coordinates and a copy target, no place name and no network — ADR 0049 asks the owner which of the three options he wants) → M25.10 (the calendar's years and months layers: one card per year of the archive and the twelve month cards under it; measured at 4.63 / 4.69 ms p50 on the corpus) → M25.11 (the week tiles of the month and the seven days of the week, the photos below on every layer; measured at 4.44 / 4.61 ms p50) → M25.12 (the photos of the period stand under the tiles of every layer in the ribbon's cell view; measured with them: years 4.62, months 4.83, weeks 4.66, days 13.66 ms p50) → M25.13 (the text block moves into "Settings → Text on photos", with the `tesseract` path; the panel stops carrying a once-a-month feature) → M24.4 (the Windows release: an MSVC build from Linux, the VC++ runtime and DirectML in the folder, the esaxx fast path off — ADR 0050 — and the folder picker with drive buttons and a path field) → done:M26.12 (the Lumix `P1050016.MOV` is MJPEG/PCM and now plays in the built-in player; when a file is refused nothing is started — the viewer names the reason and offers `ffplay`/the system player only by an explicit press; measured 30.1 fps in the Windows exe under Wine) → M26.1 (an archive folder is switched off without losing its rows: `settings.roots_off`, one filter in `App::reload` before the rows reach `items`, a tick box in the folder list, `stats` naming the hidden folders; on the corpus 2234 files the switch hides 4 files / 4.8 MiB and the shown set is 2230) → M26.2 (a warning mark for a limited feature and its OS-specific info dialog: `tools::installer()`, `tools_missing` filled by the probe of `refresh_tool_states`, the amber `Warning` target in the status bar, `App::limited_modal` with the command to copy and no network) → next:M26.3 ("Find the programs": one press instead of typing paths) from the owner's review of the Windows build, 2026-11-26 — thirteen tasks in `plan/BACKLOG.md` § M26: the folder switch, a warning mark for a limited feature with an OS-specific info dialog, "Find the programs" in the settings, the too-tall Windows top strip (screenshot `~/Pictures/Screenshots/Screenshot from 2026-11-26 22-18-14.png`), the face settings moving to the People screen, a layout that survives a narrow window, the catalog pill with its dropdown after `photocat_1` (the mock-up's "Рабочий каталог" block), the database vacuum, per-catalog passes (faces, tags, text), the People screen's split that keeps the group open (with a multi-split for the ticked faces), the automatic match of a new face to an already named person in every catalog, the built-in video player as the only player (the codec-out-of-scope offer is explicit, not an automatic ffplay window), and the settings dialog that keeps one size in every section (measured: at 1240×840 the frame is 794×654 in seven sections and 966×654 in "Tags", where the dictionary's action row asks for 716 px inside the 520 px column); then next:M24.3 (a clean install: an empty cache, no models, no external programs) — the M25 review is closed, all thirteen tasks done (the years, the path/date field, the two "no" filters, the summary, the pinned threshold, the two archive buttons, the state word, the location row, the four calendar layers and the text block); M24.3 (a clean install: an empty cache, no models, no external programs) stays queued; `plan/M22-ui-spec.md` §6; the owner's re-read of the M15.7 labels runs in parallel; SigLIP 2 stays opt-in and the default stays the multilingual CLIP. Interface language: chosen in Settings, detected from the system locale on the first launch, English when unknown; every visible surface of the window goes through `t()` and every CLI report prints the same language from `settings.lang`. Interface theme: dark by default, light and "follow the system" in Settings → Interface. The mock-up set is in `docs/references/mockups/` and digested in `plan/M22-ui-spec.md`; screens without a layout follow its rules. Renaming originals was rejected by the owner — §1 AGENTS.md stays unchanged.

Renaming originals was rejected by the owner — §1 AGENTS.md stays unchanged.

## Where we are

M26.2 is the second task of the review: a limited feature says so before the owner hunts for the
reason. `tools::installer()` names the package manager of the running system — by the target, and on
Linux by what is really installed (`/etc/arch-release` or `pacman` — Arch, `apt` — Debian/Ubuntu),
with the official pages named when nothing is recognised — and holds the command, the page list and
the language-pack note per system. `App::refresh_tool_states` fills `tools_missing` from the same
probe `doctor` runs, so no frame starts a process; while that list is not empty the status bar draws
the amber `Warning` mark 16 px after the state word (the hint "some features are limited — click for
the details"), and a click opens `App::limited_modal`: one row per program with its state line and
what is lost without it, "How to install on <system>:" with the command in mono on the `field` tone,
the note about the language packs, the download pages as plain words, the honest line that the app
installs and downloads nothing and opens no browser, where the path goes in "Settings", the accent
"Copy the commands" and "Close". Nothing is installed and nothing is opened: the command goes to the
clipboard and the status says so. The mark and the dialog leave in the same frame the program starts
— a saved or a corrected path — and with all three found there is no mark. Measured: `cargo test`
337 passed (the new `a_missing_program_leaves_a_warning_mark_and_its_dialog` with a broken tesseract
path, and the `tools::the_install_lines_follow_the_running_system` unit test), `cargo test --test
source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics` 0 errors; the dialog is 674×333 px at 1240×840 with its card inside the frame, and the
frame test reads it in both languages. The sentence that points at "Find the programs" waits for
M26.3, when the button exists: today the dialog points at the path fields, which is the truth of this
build.

M26.1 is the first task of the owner's Windows-build review: an archive folder can be switched off
without losing its rows. `settings.roots_off` holds the switched-off paths in the same
newline-separated form as `roots` (no schema change, no `SCHEMA_VERSION` bump), the folder list gives
every root a tick box (ticked — shown; cleared — the path dimmed, the word "off", the hint
"off: its files are hidden from every view; the rows, the previews and the files on disk stay"),
and one filter in `App::reload` drops the rows of a switched-off root before they reach `items` —
so the grid, the calendar, the albums, the duplicates, the faces, the trash, the semantic search, the
tag counts and the status bar see one visible set. The counters that used to come from the DB alone
(face photos, OCR files, the trash) are recomputed over that set in the same pass; the per-root counts
come from the DB (`db::root_file_counts`) so a switched-off folder still shows how much it holds.
Indexing skips a switched-off root, and the face, tag and text passes skip it too; switching it back
on reads nothing from disk. `stats` names the switched-off folders with their own count and prints
the shown set apart from the total; `index` says it skipped a switched-off folder and exits 0.
Measured on a copy of the real DB (2234 files, two roots, 21.2 GiB): switching
`~/Pictures/wallpapers` off names it in `stats` with 4 files / 4.8 MiB, the shown set becomes 2230
files, the total stays 2234, and `index` on that root writes no row. `cargo test` 335 passed,
`cargo test --test source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics` 0 errors. The switch is about showing, not deleting: `stats` and the
status line say so, and the docs (`docs/UI.md` § Screens and § States, `docs/ARCHITECTURE.md`
§ Data) state it.

Since M23.1 the code speaks English as the repository always claimed. M23.1 renamed the 206
Cyrillic identifiers and inverted the table (`PAIRS` is English → Russian, `i18n::russian(en)`
replaced `english(ru)`, `i18n::count` takes a noun key); ADR 0046 reverses ADR 0040's direction and
keeps its mechanism. M23.2–M23.4 converted every module: `i18n`, `derive`, `tools`, `db`, `storage`,
`dupes`, `eval`, `ocr`, `faces`, `organize`, `verify`, `recycle`, `index`, `player`, `embed`, `main`
and `app.rs` — 720 comment lines and 200 literals in the GUI alone, and the 40 hand-written
`match lang` blocks are gone: one English source string, one Russian translation. Progress rows and
refusals follow `settings.lang`, the byte units live in `i18n::units`, the default tag labels are
English and reach the Russian window through `i18n::word`. `tests/source_language.rs` guards the
rule with an empty `PENDING` list: a Russian comment or literal in `src/` fails, only the CLIP
prompts, the `labels.csv` keywords and the `--by` aliases stay exempt. Measured: `cargo test` 236
passed, `cargo clippy` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors; with the
owner's database `photocat stats` prints Russian under `lang=ru` and English after
`settings --lang en`, 2234 files either way.
M23.5 finished the code: 205 Russian message strings inside `#[cfg(test)]` in ten modules became
English (assertion messages, `expect` texts, four `#[ignore]` reasons, a bench `println!`) and
`scripts/fetch-model.sh` speaks English, comments and all nine runtime messages; what a test compares
stays data — the Russian label the window must show, album and tag names, OCR text, a query. The
guard grew a third check, `test_messages_are_english`: a Russian literal in a message position of
`assert!`/`assert_eq!`/`assert_ne!`/`debug_assert*!`/`panic!`/`bail!`/`anyhow!`, in `.expect("…")` or
in `#[ignore = "…"]` fails; the arguments a test compares are not messages and are left alone.
Measured: `cargo test` 236 + 3 guard tests passed (the same 236 as M23.4 — only messages changed),
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors; one
injected Russian message and one injected Russian `expect` text were both caught.
M23.6 ran the pass the acceptance rule asks for; the numbers are in `docs/ARCHITECTURE.md`
§ Measurements: indexing 67.7 s, 101 duplicate groups / 234 files / 210.3 MiB, `embed` 160.6 s
(126.1 s on a quiet machine), 31 206 tags, `ocr` 357 candidates into 356 files with 66 584
characters in 138, and every report printing the same numbers under both languages. Two findings:
(1) the duplicate count fell from 103 to 101 because of M22.2's oriented dHash — `51d6d78`, the code
from before M22.2, still gives exactly 103/238/214.1 MiB on a fresh index, and the two lost pairs are
portrait shots of one field taken 4 s apart, where the 9×8 gradient grid lands on near-flat bands;
(2) a real regression the pass caught — the OCR candidate heuristic had lost its content branch,
because the tag dictionary was looked up by `settings.model_id`, empty until the owner picks a model.
`db::text_tag_labels` now reads the labels of the `document`/`screenshot` ids from every dictionary,
so 357 candidates are proposed again instead of 203
(`a_confident_document_tag_counts_without_a_saved_model_choice` fails on the old code).

M22.14 is the owner's review of the right panel and the tile view after M22.13. A tile takes the
`card` tone with the preview on a `field` well (the mock-up's `surface-container-highest`): the well
used to be the `canvas` tone of the grid itself, so a fitted snapshot had no visible tile around it
and read as a photograph at full size with a caption — the preview is still fitted whole. The file
card follows `photocat_1` block by block: the info glyph, the preview as a 4:3 card with all four
corners cut and the duration badge of a video, the identity row (name in the numeric family in the
accent, size on the right), the mono path, a `card_block` "Shooting params", album chips, AI tags as
a caps row with `CLIP <weight>%` and chips "tag 30% ×" (the weight and the percentile in the hint),
own tags as chips, the engine name next to "Text on the photo". The exposure badges and the
share/favourite/delete circles stay refused. A defect of `card_block` surfaced with it: the block
forgot its own 1 px line and was 2 px wider than the 280 px panel, pushing the panel past the window
(M22.12's rule) — counted now. `the_light_ground_is_neutral_and_the_tile_well_is_the_field` and
`the_file_card_fits_the_right_panel` cover both; 279 tests, `cargo test --test source_language` 3,
`clippy` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors.

M22.15 answers the owner's next look at the tile: "in tile mode the photograph is shown in its own
proportions and fitted into the tile square; it must fill the whole tile, as it used to be". The
fitted letterbox is gone: `cover_rect` scales the preview by the larger ratio and centres it, so the
photo covers the 4:3 frame edge to edge — the proportions are kept, nothing is stretched, and the
sides that do not fit are cut (a portrait above and below, a panorama at the sides). This is the
mock-up's own crop that §5 q. 4 had refused. Only the grid tile fills: `draw_cell` takes a
`ThumbFit`, the tile view passes `Filling`, the proportion rows `Whole`, and the calendar, the
series strip, the album cover and the file card keep `paint_thumb`'s fit — a preview is never
cropped where the whole shot matters. The cell-view tooltip says so: "a 4:3 frame, the snapshot
fills it, the sides are cut". `a_portrait_covers_the_tile_and_keeps_its_proportions`,
`a_panorama_covers_the_tile_by_height`, `a_square_covers_the_whole_tile`,
`a_zero_size_does_not_break_the_cover` and `the_tile_draws_the_preview_over_the_whole_frame` (a real
160×320 preview in a 200×150 frame draws a 200×400 picture, no margin) cover it; 284 tests,
`cargo test --test source_language` 3, `clippy` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics` 0 errors.

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
Since M22.3 the left panel says where the files are, not only how to filter them: the five newest
years with their counts open the scrolling part right under the pinned filter (a 20 px cell in two
columns of three rows, mono 11 on the left, mono 10 at the right edge, the picked year on the accent
fill), a click writes the year into the date filter and a second click — or "All years", drawn only
while a year is picked — clears it. The block is drawn on the grid and the calendar only: the
duplicates, faces, trash and integrity screens have a context of their own. The tag menu became
chips: 20 px, mono 11, the word with the count of snapshots after a middle dot, the picked chip in
the accent, the width following the text and never the state, so a click never moves the row. The
rows of the pinned "Summary" went from 26 to 16 px, which is where the years live. Measured with a
headless frame at 1240×840: the scrolling part starts at 560 px, the years end at 680 px, the pinned
"Summary" starts at 692 px — every year is seen without a scroll, and a pick moves nothing. Four
tests: the year click narrows the filter (and drops a semantic search), the drawn rows carry the
counts while an empty year is absent, all five rows stand above the summary in a frame at 840 px, and
two chip frames hold the same words in the same places.
Since M22.5 the calendar is drawn from the mock-ups `photocat_2`/`photocat_12`: the year layer is
twelve month cards (4×3, 92 px, a 6 px gap) with the mono number and the month's name, the counter,
a 16:10 cover of the month's first snapshot fitted whole, the volume bar and the size, above a caps
line "12 MONTHS OF 2024" with the facts of the year in mono on the right; the month layer is the
caps weekday row and 96 px day cells with the number in mono 11, the counter as a badge on
`accent_soft`, up to three thumbnails and a bar against the busiest day. A cell that holds files
keeps the card tone and a 1 px line, an empty one the panel tone, a picked one the `accent_soft` fill
and a 1 px accent frame — one helper (`cal_cell`) owns all three states, so the old `faint_bg_color`,
the `from_gray` numbers and the blue 2 px stroke are gone. Under the year, month and week layers the
selected day is a list with the time, the name and the size (the shared `draw_list` with
`Cells::time_only`), and an empty month leads to the nearest month with files. Measured on the real
DB (2232 files, release): the year layer p50 5.56 ms (180 fps), the month layer p50 5.85 ms (171 fps),
and the whole year layer fits a 1240×840 window.
Since M22.6 the albums screen follows `photocat_3` inside its tree (the owner's decision, spec §5 q. 7):
the view heading is "Albums" with the count, and the open album has its own card header — the name at
14 px with "N files · M sub-albums · size" in mono 11 and the app trash after it — over the same action
row; the primary "+ New album" carries the accent. A sub-album is a card of the width of the "Tiles"
slider: a 16:10 canvas cover with the sub-album's first snapshot fitted whole (a folder mark when it has
none), the name at 13 px, the count of files and the size in mono 10, the card tone with a 1 px border
and the accent line under the cursor. The cards scale with the slider, so they take one row, two at
most, the rest scrolls, and the snapshot grid keeps 220 px — measured before the cap: eight cards pushed
the grid out of a 840 px window. The album chip stays in the chips row and "Album order" keeps its
place in the Sort menu.
Since M22.20 the albums screen is the mock-up's card grid, and photos have a way into an album that is
not a trip through the tree. The root of the view is the cards of the top level albums — a 16:10 cover
with the first snapshot fitted whole, "N sub-albums" at the top left and "N files" at the bottom right
on black 60 % plates, the name at 13 px and "size · date" in mono 10, 10 px of air on every side; the
open album keeps the 2 px accent frame and the accent name. The control row under the ribbon filters
the album names, orders the cards ("Album order" by default — the order of the export stays data —, "By
update", "By name", "By file count", "By size"), switches to a 38–57 px list row and carries the
primary "+ New album"; `settings.album_sort` and `settings.album_list` keep the choice (two key/value
rows, no schema change). The tree stays inside an album under the breadcrumb "All albums › … › here",
because there the nesting matters, and the root no longer repeats it. The date on a card is the day of
the last change: `AlbumNode` gained `updated_at`, and `add_to_album`, `remove_from_album` and
`move_in_album` touch it. The way into an album is one dialog with three entrances — the mock-up's
dashed "+ Add" chip of the file card (one photo), "Add to an album… (N)" in the floating selection bar
(the marked photos) and "Add the picked ones (N)" inside an album (straight into the open album): it
lists the albums with their paths and counts, filters by name and has a field for a new album, so a
name there makes a root album and fills it in the same press; rows only, a repeat does not duplicate.
Refused: the mock-up's per-card "⋮" (an icon-only target; the actions belong to the open album) and its
floating album bar ("Open · Export · Delete" duplicates the action row).
Since M22.21 a click on a card at the root opens its album: the "open the album" action used to be
thrown away while no album was open yet, so the grid of the top level was a dead end. The cover of a
card is a mosaic of up to four of the album's first photos: `AlbumNode::covers` carries four previews
in the album order (`db::ALBUM_COVERS`, `albums_tree` reads them in one pass and skips a file without a
preview or in the trash), and the card paints one photo whole, two in columns, three as a left half
with a right stack, four as a 2×2 grid with 2 px seams (`cover_cells`, `paint_cover_mosaic`); the list
row uses the same mosaic in its 16:10 well, and the tree hint says "the cover shows its first photos
in order". The mosaic keeps `paint_thumb`'s fitted whole preview, so no snapshot is cropped on an album
card (M22.15's rule). Measured on a copy of the real DB (2232 files, 1240×840 headless frames): eight
sub-album cards with four real previews each — p50 7.38 ms, p95 9.87 ms; the same screen with one
preview per card before the change gave 7.41 / 12.40 ms, so the three extra cells cost nothing
measurable. `cargo test` 305 passed (four new tests), `cargo test --test source_language` 3,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors; the
window lives through `timeout 6`.
The owner's next report: "не работают кнопки переключения в список и плитку альбомов". The album
look switch did not act on the photos either — it changed only the sub-album cards, and an album
without sub-albums looked dead. M22.22 first hid the ribbon's grid switch at the albums root only;
the owner's screenshot from inside an album ("Садик", four files) showed the bottom "Список /
Плитки" still doing nothing to the four snapshots. M22.23 finishes it: the ribbon's cell views are
hidden on the whole Albums screen, and its "Cards / List" drives both the album cards and the photos
(`App::grid_cell_style`: Cards → tiles, List → list, while `settings.cell_style` of the other screens
stays untouched). Tests: `the_album_look_switch_changes_the_photos` (inside an album with two photos
a click on "Список" stores the choice and makes the grid a list, "Плитки" brings the tiles back, the
global Rows view stays) and `the_albums_view_holds_one_look_switch` (one "Список" at the root and
inside an album, the ribbon's "Плитка / Ряды" gone). `cargo test` 307 passed, `cargo test --test
source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics .` 0 errors.
The owner asked why the album lists have no icons. The tree rows were bare selectable labels, and an
album without a preview drew a "▸" glyph in its card or list-row well. M22.24 gives every tree row a
14 px folder glyph (`album_tree_row`), makes the whole row one click target — the glyph is not a dead
spot — and draws the real folder icon in an empty album card or list row. The open tree row keeps the
`accent_soft` fill and the 2 px accent edge. The test
`a_click_on_the_folder_glyph_of_a_tree_row_opens_the_album` clicks 12 px left of the name and lands
in the sub-album. `cargo test` 308 passed, `cargo test --test source_language` 3, `cargo clippy
--all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.
The owner's next report: the right panel on the Albums screen does not match its mock-up; in the
"All files" look of an album, clicking a photo makes the tile sizes jump and the panel is cut off.
The cause was one row of the file card: the identity row printed the whole file name in a horizontal
`Label`, so a long name demanded more than the 255 px card, the ScrollArea widened to 306.5 px and
the right panel grew past the window edge (1691 px) — and because the panel's width feeds the central
panel, the grid recomputed its columns and the tiles jumped on the next click. M22.25 cuts the name
to the room the size leaves (`shorten`, the full name in the hint) and the "All files" switch of the
album now goes through `t()` instead of a hard-coded English string. Measured on a copy of the real
DB at 1691×1021: before the fix the panel rect was 1412–1718.5 (306.5 px), the tiles 176 → 181 px
after selecting a file; after it the panel is 1412–1691 (279 px) and the tiles stay 176 px. Test
`a_long_file_name_does_not_widen_the_right_panel` (a long name, the panel at the window edge, no
frame or word past the window, the tile widths before and after the selection equal). `cargo test`
309 passed, `cargo test --test source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55
checks, `rust-analyzer diagnostics .` 0 errors.
Since M22.7 the full screen is the mock-up's `photocat_5/6`: a 58 px chrome of the header ("Back to the
grid", the name at 14 px, the date and the facts in mono 11, "N of M" in the accent) and the tools row,
where every tool is a word with a hint — "Zoom out" · the percentage · "Zoom in" · "Fit" · "1:1" ·
"Rotate" · "Text on the photo" · "In the system viewer" · "Inspector". "1:1" shows one image pixel per
screen pixel (so the ceiling went to `MAX_ZOOM` 800 %); "Rotate" turns the *view* by 90° through a
rotated mesh and the file on disk, the DB and the preview stay as they are, the next file opening
unturned. Under the frame the series strip names the day ("SERIES · 2019-12-17 · 16 files") and draws
a window of up to sixteen 64×48 neighbours of that day with the position inside the day, the current one
framed in the accent; a click jumps there (`viewer_goto`). A window, not a scroll area — one scroll per
region holds. The face chips mark every face the model found (`Lisa (92%)` in a dark plate over a 1.5 px
accent box, turned with the view) and are read once per file through `db::load_face_boxes`. "Inspector"
— or I — opens the same `file_card` in a 280 px right column, so the card has a second caller and not a
second copy. Measured on the real DB (2232 files, release, 1240×840 headless frames) with sixteen
neighbours and a face chip: p50 6.91–8.65 ms (116–145 fps), p95
7.73–10.45 ms; with the inspector on p50 6.89–7.89 ms, p95 9.47–11.46 ms over four runs — the
series is computed on a file change, not every frame (it cost 9.66 ms before).
Since M22.8 the settings dialog is the mock-up's `photocat_4` and the window has the mock-up's icons.
The section list sits under a caps 10 px line "SETTINGS SECTIONS": seven rows of 28 px with a 14 px
icon, the 13 px label and a mono 10 value at the right edge that shows the state before a click (the
interface language, the model's dimensions, the files in the index, the enabled tags, the face
threshold, `2/3` of the external programs); the active row carries the `accent_soft` fill and a 2 px
accent edge on the left, and a value is dropped when it would touch its word (the widest Russian
label keeps its room). The panes keep their content — the mock-up's autostart, tray, GPU and backup
are still refused (spec §5 q. 5) — and "Interface" keeps the language and the theme with the hint
that the theme changes neither files nor the database. The owner asked for the mock-up's icons on
every button: `paint_icon` grew from the five player signs to 62 glyphs of the Material Symbols
family by name, drawn as geometry — no icon font, no image, the binary does not grow (ADR 0048). Every
action button is one `WordButton`: a 14 px icon, 6 px, the word, the accent on the one main action of
a screen and the danger border on the OS-trash path. The kinds of 20 px target that cannot hold both
keep their word alone (per-tile `keep`/`restore`, the "×" of a chip, the album tree's triangle, the
"×" of an album row), the two round targets on a face tile are icon-only (M22.26), and menus keep
words; a source test fails on a button built
outside `WordButton`. The wider buttons pushed the controls of the content panel into the heading —
right-aligned they began at x 288 and ran over "All files · 2232 files" on a 1240 px window with the
details panel open — so the heading keeps its row (title, facts, "Tiles" slider on the right) and the
controls (three cell views, "By months / Flat", "Sort") take a second row of 20 px buttons that wraps
instead of clipping, with a constant height across views (M22.2a holds). Measured on the real DB
(2232 files, release, 1240×840): the settings dialog p50 6.89–8.01 ms (125–145 fps), p95 ≤ 8.13 ms
over the seven sections; the grid pass p50 6.73–6.84 ms (146–149 fps), unchanged against M22.2b's
6.4–6.7 ms.
Since M22.10 every dialog of the window is the same card: the `card` tone, a 1 px `border` line, the
8 px radius and 16 px of air inside — a modal used to be egui's `Frame::popup`, the frame of a menu
with the 6 px `menu_corner_radius` and 6 px of air — and its title is a 14 px subheading, because the
type scale has no 16. The five action dialogs carry exactly one accent button (the export dialog's
"Export N files" was a plain button and is the main action now); the trash confirmation, the one
button that touches the disk, wears the `danger` line and no accent. A number of a dialog lives in a
facts row of its own in mono 11 ("2 files" per folder root, "1 file · 18.4 MiB", "2 albums · 2 files",
"export: 3/8") instead of inside a translated sentence, and the export report is a mono 11 body in a
scroll area with a copy button. Measured on a copy of the real DB: folder list p50 0.03 ms, export
with a 500-line plan p50 0.18 ms, the other dialogs 0.02–0.08 ms.
Since M22.11 both themes are measured, not eyeballed. The light palette took the drawn screen's amber
`#d97706`, which reads 2.91:1 on the panel and 2.86:1 on the selection fill — under the active
navigation row, a picked chip, a picked year and a confident tag, all body text. The light accent is
now the light design system's own `amber-focus` `#b45309` (4.58:1 on the panel, 5.02:1 on the card,
4.51:1 on the selection fill) with `#92400e` for its hover, and the active navigation row sits on the
`card` tone with a 2 px accent edge — the tone the specification's §2.5 always named — instead of the
`field` tone (4.07:1 → 5.02:1). The two `danger` values rose one step for the same reason (dark
`#ef4444` was 4.34:1 on the card, now `#f45c5c` 5.07:1; light `#dc2626` 4.41:1 on the panel, now the
design system's error `#ba1a1a` 5.90:1). Everything else held: every screen writes no colour of its
own in either palette, the light ground is the cool `canvas-base` `#f8fafc` with no warm cast (a tile's
frame is painted in `canvas`), dark stays the default and the choice survives a restart. Three tests
hold it: `both_themes_pass_the_contrast_ratio` (WCAG 2.1 over every pair the screens paint),
`the_light_ground_is_neutral_and_the_tile_frame_is_the_canvas`, and
`every_screen_paints_only_the_tokens_of_its_theme` (grid, four calendar layers, albums, full screen —
plus the four screens without a mock-up and all seven dialogs in their own tests).
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
vector, aligned crops are grouped by cosine, a group can be named and split with the crossed-head
button on a face tile.
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
and are split with the crossed-head button of a face tile. The detector takes a screen, a statue or a
picture on a wall for a face; there are few such "faces", they are visible in the common grid and
remain separate groups. The crops of replaced files stay in the `faces/` cache (the key is sha256): the code does not
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
11. ~~Downloading the weights without `curl`~~ Decided by the owner (2026-11-24): **the program
    downloads the data itself**. `ureq` + `rustls` inside the binary, `curl` removed altogether —
    ADR 0047, M24.2. The clean-install pass (M24.3) checks the download on a machine with nothing
    installed.

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
  contain two similar people. This is cured with the crossed-head button, not with an automatic
  threshold. The
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

## M22.3 — the left panel: the years and the tag chips (2026-11-25) ✅

`plan/M22-ui-spec.md` §5 question 9 decided the panel budget: the year list and the tag chips come in,
the folder tree and the storage quota do not. This change adds both and measures the budget.

**What appeared.** The years open the scrolling part of the panel, right under the pinned filter
block: a 20 px cell in two columns of three rows, mono 11 on the left and the count of files in mono
10 at the right edge, the picked year on the accent fill and in the accent text, and "All years" as
the first cell only while a year is picked — a way back, not a permanent row, and six cells are still
three rows, so the block never changes height. A single column of five years (130 px) pushed the
model block out of the default 840 px window, which is why the list is two columns wide. A click
writes the year into the date filter (the `query` field the filter block shows, so the owner sees
what happened) and drops a semantic search and the text snippets, because a year is a date and not
words; a second click on the picked year clears it. The block is drawn on the grid and the calendar
only: the duplicates, faces, trash and integrity screens have a context of their own and the date
filter does not drive them — drawing it there took room from the duplicates screen and pushed "Copy
the list" out of the panel, which an existing test caught. Counts come from `taken_at`, five rows at
most, and a year with zero files does not exist. The rows of the pinned "Summary" went from 26 to
16 px (`stat_row` is a pair of labels, while `horizontal` assumes an interactive row): four rows cost
48 px less, and that is the room the years took.
The tag menu of the model block became chips: the word with the count of snapshots after a middle
dot, 4 px apart, wrapping, idle `field` with `text_med`, picked — accent fill, accent 1 px border and
accent text. The width follows the text and never the state, so a click moves no chip
(`tag_chip`); the percent slider sits right under the row and the model block still warns "no tag
passes the threshold". Two new strings, "YEARS" and "All years", went through `i18n::PAIRS`.

**Measured** (a headless frame of the whole window, 1240×840, 2026-11-25): the scrolling part starts
at 560 px, the year block ends at 680 px and the pinned "Summary" starts at 692 px — every year is
seen without a scroll and stands 12 px above the summary; a picked year adds "All years" without
moving either.

**Found while looking at the result.** A tile caption wrapped a long file name onto a second line
inside a 22 px band, so the first line was painted over the preview: `draw_cell` laid the name out
with `layout`, which wraps, and clipped only the sides. `caption_name` now lays the name out as one
row with an ellipsis at the edge of the date (the rule `docs/UI.md` already stated), and
`a_long_caption_name_is_cut_to_one_line` holds one row and the width of the place for three names.
The owner's archive has such names by the dozen, so it was visible on the very first screen.

**Checks.** `cargo test` — 244 passed, 7 `#[ignore]` (new: `a_year_click_narrows_the_date_filter` —
counts, the five-row cap, the year written into the filter, the semantic search dropped, the way
back; `the_years_block_draws_the_newest_five_with_counts` — the rows and their counts are drawn, an
empty year and "All years" are not, "All years" appears once a year is picked;
`the_left_panel_fits_the_years_above_the_summary` — the place of every word in a frame at 840 px;
`the_tag_chips_do_not_jump_when_a_tag_is_picked` — two chip frames hold the same words in the same
places and the row keeps its size). and `a_long_caption_name_is_cut_to_one_line` — a caption name stays on
one row and inside its place). `cargo clippy --all-targets` — clean; `check.sh` — 55 checks, 0
violations; `cargo test --test source_language` — 3 passed. `docs/UI.md` § Layout, § Interaction, §
Metrics and § Screens carry the new rows.

**Not done, deliberately.** No folder tree and no storage quota in the panel (question 9), no
per-folder years, no year list on the screens whose context is not time.

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
both halves are really checked; the `#[ignore]` `grid_scroll_measurement` (then `замер_прокрутки_сетки`) reproduces the frame numbers
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
task (M22.5–M22.11); bundled fonts, cropped 4:3 previews, a floating selection bar, favourites,
ratings, share/delete and the mock-up's non-existent EXIF, GPU, tray and update functions are refused
(§5 of the spec, with the recommendation to the owner).

## M23.6 — the corpus pass after the English conversion (2026-11-24) ✅

The point of the pass is not a pretty table: the conversion touched every module, so the archive has
to be walked again and every number compared with the baseline of 2026-11-22. A release build, a
`/tmp` DB, the real 2230 files from `Фотографии с телефона`, the archive read only.

**Numbers (all in `docs/ARCHITECTURE.md` § Measurements).** `index --jobs 16` 67.7 s, 1 error (the
known broken JPEG), DB 1.4 MB, 2229 previews 28.1 MiB; a repeated index 0.029 s. `stats` 2230 files,
21.2 GiB, 258 videos, 3:19:46, 1 without a dHash, 0 without a date. `dupes --max-dist 6` 0.023–0.026 s,
101 groups / 234 files / 210.3 MiB. `organize` dry runs 0.021–0.023 s, 573 / 76 / 576 folders. `embed`
CLIP multilingual 160.6 s (an unrelated build held the machine at load 9–14; the same operation took
126.1 s the day before), vectors 4.4 MiB, 31 206 tags; `search` 10–11 ms, `clusters` 0.9 s. `ocr` 357
candidates, 70.3 s, 356/357, 66 584 characters in 138 files; `search --text` 1 ms. DB with CLIP and OCR
16.0 MB. GUI: `timeout 6` opens on the new DB and lives to the timeout. Both languages print the same
numbers in the same order — only the labels and the units differ (`КиБ`/`KiB`).

**Finding 1 — the duplicates are 101, not 103.** The cause is not the translation but M22.2 (ADR 0045):
the dHash is now computed on the EXIF-oriented frame, and two pairs of portrait shots of one field
taken 4 s apart fall out at `max-dist 6` — their oriented 9×8 gradient grid samples near-flat bands and
the pairs drift by 14 and 7 bits instead of 5 and 3 on the sideways frame. Proven by index: the code
from before M22.2 (`51d6d78`) on a fresh index gives exactly 103 / 238 / 214.1 MiB with the same 2230
files. Nothing else moved. This is a recall loss for portrait bursts and is now in AGENTS § 8, the
baseline of the `corpus-bench` skill and the measurements table; fixing the 9×8 grid for portrait
frames is not part of M23.6.

**Finding 2 — a real regression, fixed.** `ocr --dry-run` proposed 203 candidates instead of 357: the
content branch of `wants_text` (a confident `document`/`screenshot` tag) never fired. M23.3 made the
default tag labels English, so the old hard-coded check `Some("документ" | "скриншот")` had to become
a dictionary lookup — and that lookup asked for `settings.model_id`, which stays empty until the owner
picks a model, so it returned nothing on every default DB. `db::text_tag_labels` now reads the labels
of the `document` and `screenshot` ids from every dictionary (`enabled` only), so a renamed tag in a
second model still counts and the OCR module needs neither the model nor its catalogue. The cost is
unchanged: the pass reads the labels, not the photos.

**Checks.** `cargo test` — 238 passed, 7 ignored (new: `text_tag_labels_come_from_every_dictionary` and
`a_confident_document_tag_counts_without_a_saved_model_choice`, the second fails on the old code);
`cargo test --test source_language` — 3 passed; `cargo clippy --all-targets` — clean; `check.sh` — 55
checks, 0 violations; `rust-analyzer diagnostics` — 0 errors; the OCR tests now carry the English
default labels as data. No file of the archive was touched: the index wrote to the cache and the DB.

**Not done, deliberately.** The dHash grid on portrait frames (a candidate for a later task, the
numbers are in the measurements table); the OCR text of the 154 newly proposed shots was computed here
but not yet reviewed by the owner — the card shows it as soon as the pass runs on his DB; the `--all`
path, the faces pass and the model downloads were not re-measured because M23 did not touch them.

## M24.1 — the model download is the binary's business (2026-11-24) ✅

The repository still carried `scripts/fetch-model.sh`, the manual fallback that curled four CLIP files
into `<cache>/photocat/models/clip-vit-b32/`. It was the last piece of the model path that was not the
application: bash, `sha256sum`, `du`, a Unix-only path — a cross-platform application has to be one
binary, and the same four files with the same sha256 already arrive through `models download` and the
"Download model" button, mirror (hf-mirror) and retry included.

**What changed.** The script is deleted. The two messages that pointed at it name the two real ways
instead: `embed::check_model_in` prints the folder and `photocat models download --id ID`, and the
`curl` row of the external-programs table says "the weights have to be copied into the models folder
by hand — a download needs curl" (both languages, `PAIRS` updated in the same commit). `README.md`
and `.ru.md` install the model with the command instead of the script; `AGENTS.md` § 4 and 5, ADR 0013
(network), ADR 0014 (cross-platform boundary) and the external-programs table in `docs/DECISIONS.md`
say the same thing; `plan/I18N.md` records that the script is gone.

**Why it is not a loss.** The script did nothing the binary cannot: same URLs, same pinned sha256,
same two towers. What the script *could* do and the binary cannot is work on a machine without `curl`
— but that machine also could not run the script, which called `curl` itself. So the honest fallback
is unchanged: copy the four files into `<models_dir>/<id>/` by hand. Whether to drop `curl` as well,
with an in-process HTTPS client, is the owner's open question (STATUS § Open questions, #11).

**Checks.** `cargo build --release` and `cargo test` — 238 passed, 7 ignored; `check.sh` — 55 checks, 0
violations (it fails on a `src/` file that deletes files, and on a missing licence note — neither
applies); `rust-analyzer diagnostics` — 0 errors; a grep for `fetch-model` in the repository finds only
the dated M23.5 records, which stay as written.

**The clean-start smoke (the first step of M24.2).** An empty cache (`XDG_CACHE_HOME=$(mktemp -d)`, no
DB, no models, no settings) opens the window: `<cache>/photocat/photocat.db` appears (176 KiB with the
seeded 14-tag dictionary) together with `thumbs/`, `stats` says 0 files, `models list` says "not
downloaded" for all four, `ocr --dry-run` finds 0 of 0 candidates, no pass starts by itself. The smoke
found one defect: the GUI read the default English and stored nothing, while the CLI detected the
locale, so a fresh install would show two languages at once and never remember the first-launch guess.
`app::first_lang` now calls `db::ensure_lang` before the window opens, so `lang=ru` comes from the
locale and is written once, and the owner's later choice still wins — the numbers of the run are in
`docs/ARCHITECTURE.md` § Measurements.

**Not done, deliberately.** The rest of the clean-install pass — M24.3: a folder indexed with no model
at all, the consent download of 217.5 MiB, and the run with `ffmpeg`/`tesseract` cleared out.

## M24.2 — the binary downloads the weights itself (2026-11-24) ✅

The owner's answer to the open question of M24.1: "the program must be able to download data itself".
`curl` is gone — not called, not configured, not listed — and the downloader is now `ureq` 3.5 with
`rustls` and the bundled WebPKI roots, inside `src/embed.rs` (ADR 0047). One GET of a catalogue file
into the models folder: no second client, no request body, no header carrying anything about the
owner. The mirror (hf-mirror) and the three attempts stay, the sha256 check stays, the consent tick
stays.

**What was removed.** `Tool::Curl`, `settings.tool_curl`, the `--curl` flag of `settings`, the field
in "Settings → External programs", the `doctor` line and the "without it" hint; the "the weights have
to be copied by hand — a download needs curl" message; the `curl` row of the ADR 0019 table. The GUI
now draws one line per `tools::ALL`, and the array of states became `Vec` so removing a tool cannot
index out of bounds again (a test caught exactly that).

**How it behaves.** `http_get` streams the body to the file in 64 KiB chunks (a 400 MiB SigLIP 2 must
not sit in memory), prints a line every 16 MiB ("Model: text.onnx — 48.0 MiB of 61.5 MiB"), refuses a
body that ended earlier than `Content-Length` promised, retries three times with a short pause, and
falls back to the mirror. Timeouts: 20 s to connect, 30 minutes for the whole transfer.

**Measured (a temp DB, a temp models folder, real network).** `models download --id clip-vit-b32-en
--yes` — 148.6 MiB in 22.7 s, three files, sha256 matched (the check would have failed otherwise);
the same download through `curl` on 2026-11-22 was 18.8 s, so the difference is the network, not the
client. The binary grew 85.1 → 87.6 MB (`rustls` + `ring` + roots). `check.sh` §4 was rewritten for
the new rule and finds no violation; a second HTTP client, `ureq` outside `src/embed.rs` or a
`.post()` in the downloader is an error.

**Checks.** `cargo test` — 239 passed, 7 ignored; `cargo clippy --all-targets` clean;
`rust-analyzer diagnostics` 0 errors; `check.sh` 55 checks, 0 violations.

**Not done, deliberately.** Byte-level progress in the GUI progress bar (the row shows the message and
a tick per file); a "Stop" for a download in flight (the request cannot be cancelled mid-body yet —
the same limitation as before); the clean-install pass itself — M24.3.

## M22.0 — the mock-up set lives in the repository (2026-11-24) ✅

The owner moved his design set from `~/Documents/stitch_photocat_desktop_ui_specification` into
`docs/references/mockups/`: seven HTML screens (`photocat_1`, `_2`, `_12`, `_3`, `_4`, `_5`, `_6`),
four `screen.png` screenshots and two design-system files (`functional_precision_desktop`,
`functional_precision_light`), 2.7 MB in all. Until now the layout source sat on one machine and the
documents named it by an absolute home path — a path that means nothing on another machine and that
AGENTS §1 forbids in code and UI.

**What is in the folder.** A `README.md` (and `.ru.md`) that states the one thing a future session must
not get wrong: this is a **reference, not a build step** — nothing here is compiled, loaded, bundled
or shipped, and no webview enters the application (AGENTS §1). It also maps each folder to the screen
it shows and to the task that took it, and repeats `plan/M22-ui-spec.md` §5: a function the mock-up
shows and the spec refuses (autostart, GPU settings, backups, ratings, sharing, cloud sync) is not to
be built because "it is in the picture".

**What was fixed with it.** The absolute path is gone from `plan/M22-ui-spec.md`/`.ru.md` (both now
point at `docs/references/mockups/`), `plan/BACKLOG.md` M22 no longer says "outside the repository",
ADR 0044 says where the set went, and the `STATE` line names the folder. The mock-ups themselves are
committed as exported: they are the owner's design and are not edited to match the code — the
difference is recorded in the spec, not smoothed out of the picture.

**Checks.** `check.sh` — 55 checks, 0 violations (the HTML lives in `docs/`, no `src/` module sees it);
`cargo test` unchanged at 239 passed; the folder is documentation, so nothing in the binary or the
schema moved.

## M22.4 — the month heading pinned while scrolling (2026-11-25) ✅

The owner's remark after M22.3 and the debt item 7 of `docs/UI.md`: in the grouped grid the month is
named only at the start of its group, so after a screen of scrolling there is no telling which month
the snapshots belong to. M22.4 pins that heading; it also closes the former M13e, which was folded
into it.

**How.** The grid loop records where every group heading landed (`MonthHead`: the top in window
coordinates, the height with its spacing, the key, the counts). After the single `ScrollArea` returns
its viewport, `draw_sticky_month` takes the last heading above the top edge, fills a band on the
canvas tone over the tiles — opaque, so nothing shows through the words — and draws the same row
through `month_labels`/`month_row`, shared with the group heading so the two cannot drift apart. The
row lives in a child UI that does not advance the grid's cursor, and it slides up by the overlap when
the next group's heading arrives (`y = min(top, next_top − height)`); before the group's own heading
has gone above the edge nothing is pinned. No nested `ScrollArea`; "Flat" and a semantic search have
no groups, so nothing is pinned there either.

**Checks.** Two headless tests. `the_month_heading_stays_pinned_while_its_group_is_on_screen` runs the
whole window at 1240×840 over two months of 25 snapshots with the wheel turned over the grid: after
one notch the top caption has moved yet exactly one "May 2024" is drawn at the y it had before the
scroll, and after four more notches "April 2024" stands there while "May 2024" is gone (with the pin
disabled the test fails, as it should).
`the_next_month_heading_pushes_the_pinned_one_out` draws the row alone with synthetic headings: on the
top edge when the next heading is far below, and exactly 6 px higher when it overlaps by 6 px.
`cargo test` 246 passed (244 + 2), `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

**Measured** (`docs/ARCHITECTURE.md` § Measurements). On the real DB (2232 files, release) the group
heading "октябрь 2026" is drawn at y 132 on the first screen and at y 132 after a wheel notch — pinned
at the top edge of the grid area; the owner's settings keep "Flat", so the bench switches to "By
months" itself. The 160-frame scrolling measurement of M22.2b repeated on the same DB: p50 6.9–7.0 ms
(142–148 fps), p95 8.9–9.6 ms, p99 11.2–13.7 ms, against p50 6.77 ms / p95 9.75 ms / p99 12.24 ms on
the stashed code before the change — one rect and one text row per frame are below the noise.

**Documents.** `docs/UI.md` § Layout and § Metrics describe the pinned row, debt item 7 is closed and
the list renumbered (the duplicated entry 8 became 7); `docs/UI.ru.md` and the honest list of the
`ui-design` skill (`revu.md`/`.ru.md`) follow; `plan/M22-ui-spec.md` §2.5, §3 and §6 M22.4 carry the
result and the numbers. No new interface strings, so `plan/I18N.md` does not change.

## M22.5 — the calendar after `photocat_2`/`photocat_12` (2026-11-25) ✅

The calendar was the last screen still painted in the pre-M22 way: `faint_bg_color` cells, `from_gray`
numbers, a 2 px blue selection stroke and a `gamma_multiply(0.25)` fill — colours that the token
palette replaced everywhere else in M22.1–M22.4. M22.5 brings the year, month, week and day layers to
the mock-ups' rules and builds the day list the month layer was missing.

**How.** `cal_cell` is the one way to paint a calendar cell: the card tone and a 1 px line while the
cell holds files, the `panel` tone while it is empty (the month stays a grid — the gaps of the archive
are seen, not searched for), the `accent_soft` fill and a 1 px accent frame while it is picked; it
returns the palette, so the number, the counter and the bar inside are drawn in the same tones.
`cal_bar` draws the volume share of the busiest month of the year or the busiest day of the month. The
**year layer** gets the mock-up's line — a caps "12 MONTHS OF 2024" with the facts of the year in mono
10 — and 4×3 cards of 92 px with a 6 px gap: the mono number and the month's name, the counter at the
right edge, a 16:10 cover of the month's first snapshot fitted whole, the volume bar and the size. The
**month layer** gets the caps weekday row and cells of 96 px: the day number in mono 11, the counter as
a badge (accent on `accent_soft`), up to three thumbnails and the bar, and the picked day is framed in
1 px of accent. The **day list** under the upper layers is the shared `draw_list` with
`Cells::time_only`: a row reads "18:27 · 1.6 MiB" instead of repeating the same date on every line
(`caption_date` also does it in a tile caption on the "Day" layer), under a heading "Wed, 01.05.2024"
at 14 px with the facts in mono (`day_heading`, `day_title`). An empty month now leads to the nearest
month with files — `nearest_file_month` had been dead code since M21.4b and points the button
"Go to April 2024" — and the arrows still skip empty months. The view's facts name the layer (a year, a
month, a week or a day) instead of always the same month.

**Checks.** Four headless tests. `the_year_layer_draws_twelve_months_with_counts_and_sizes` (the caps
line, the facts of the year, all twelve month names, ten empty zeros, the sizes of the months with
files); `the_picked_month_and_day_are_framed_in_the_accent` (a stroke scan of the frame: exactly one
1 px accent rect in the content area with a month picked and exactly one with a day picked, none with
nothing picked — that is how the old blue stroke would have failed); `the_day_strip_shows_the_time_name_and_size`
("09:10 · 2.0 KiB", "14:23 · 3.0 KiB", the names, the day heading, and no repeated "2024-05-01 09:10");
`an_empty_month_offers_the_nearest_month_with_files` (backwards first, then forwards). `cargo test`
250 passed (246 + 4), `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics .` 0 errors.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, the real DB of 2232 files, release, headless
frames at 1240×840): the year layer p50 5.56 ms (180 fps), p95 and p99 5.65; the month layer p50
5.85–5.90 ms (169–171 fps), p95 6.26–7.39 ms — twelve covers, 31 day cells and the preview budget stay
inside the 16.7 ms of 60 fps. The whole year layer fits a 1240×840 window (December's card at y 442); a
month grid of five weeks is 510 px tall, so its day list sits below the fold and is reached by the same
scroll as the grid.

**Looked at.** Both layers were opened in the window on the owner's screen for a few seconds and
screenshotted: the 4×3 cards with the accent frame on the picked month, the caps weekday row, the
counter badges, the thumbnails and volume bars, and the day list under the month grid. The first look
found the empty cells invisible on the dark canvas, which is why an empty cell now keeps the panel
tone.

**Documents.** `docs/UI.md` § Metrics (the calendar cell and the new "Calendar day list" row), §
Screens, § States, and the stale paragraph claiming that colours come from `ui.visuals()` with a blue
`rgb(90,170,255)` accent — a defect left from M22.1, now the tokens only. `plan/M22-ui-spec.md` §3 and
§6 M22.5 carry the result and the numbers, `plan/BACKLOG.md` counts M22.5 as done,
`docs/references/mockups/README.md` marks `photocat_12` as built, and `plan/I18N.md` gets the new
labels. Still open, outside this task: `highlight_job` (the OCR match in the card and the search
snippet) still paints the old blue — it belongs to M22.9's pass over the screens without a mock-up
(closed there: the match takes the theme's accent).

## M22.6 — the albums' cards inside the tree (2026-11-25) ✅

The albums screen was the last one of the M22 set with the old look: the open album lost its name into
the heading of the grid and repeated it in the tree, the sub-albums were 96 px folder plates painted
from `ui.visuals()` with a `selected_fill` band and no preview at all. M22.6 gives the screen the
mock-up's card (spec §6 M22.6) and keeps the tree — the owner's answer to spec §5 q. 7.

**How.** The view heading stays "Albums" with the count of albums; the open album carries its own card
header — `album_header` draws the name at 14 px strong and "N files · M sub-albums · size" in mono 11
next to it, with " · N in the trash" when the app trash is not empty — and the action row follows it
unchanged ("+ Sub-album", "Rename", "Move to…", "Up / Down", "Delete the album"). "+ New album" is the
view's main action and carries the accent (`primary_button`, the mock-up's `+`). A sub-album is a card
(`draw_album_cell`, `album_card_size`): the width from the "Tiles" slider, a 16:10 `canvas` cover with
the sub-album's first snapshot fitted whole (`paint_thumb` with the shared texture cache and the decode
budget — no second loader), a folder mark when it has no snapshot, the name at 13 px cut with an
ellipsis and clipped to the card, "N files" on the left and the size on the right in mono 10, the card
tone, a 1 px `border`, the accent line under the cursor and a pointing hand. The sub-album count cannot
fit a 100 px card, so it lives in the tree and in the card's hint. The cards scale with the slider, so
`ui.available_height() − 220` (never less than one card row, never more than two, plus the 10 px gap)
bounds a scroll area: the album's snapshots keep 220 px and are never pushed off the window. The album
chip stays in the chips row — there is no floating selection bar — and "Album order" keeps its place in
the Sort menu.

**Checks.** Three headless tests: `the_album_card_follows_the_tiles_slider` (100 → 105 px,
260 → 205 px, the 16:10 cover inside); `the_album_card_draws_at_the_model_height` (the same numbers on
the real drawing code, as M22.2a did for the list); `the_open_album_gets_a_card_header` ("Albums" and
"2 albums" in the heading, the name printed once, "3 files · 1 sub-album · 2.0 MiB" next to it, "May",
"12 files" and "2.0 MiB" on the card) and `many_sub_albums_keep_the_grid_visible` (twelve cards at
260 px: the snapshot names are still drawn — without the cap the grid was pushed out, which the test
reproduces). `cargo test` 254 passed (250 + 4), `cargo clippy --all-targets` clean, `check.sh` 55
checks, `rust-analyzer diagnostics .` 0 errors.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, a copy of the real DB of 2232 files, release,
1240×840 headless frames): the Albums view with eight sub-album cards over real previews — p50
6.94–7.51 ms (133–144 fps), p95 7.18–7.99 ms over four runs; the cards end above the grid and three
snapshot tiles are drawn under them. `docs/UI.md` § Metrics and § Screens follow (the "Album header" and
"Sub-album card" rows), `plan/M22-ui-spec.md` §3 and §6 M22.6 carry the result, `plan/BACKLOG.md`
counts M22.6 as done and `plan/I18N.md` gets the one new label.

## M22.8 — the settings dialog and the mock-up's icons (2026-11-25) ✅

Two things in one change, because the owner asked for both: the settings dialog after `photocat_4`
(spec §6 M22.8) and the icons of the mock-up on every button of the window. The dialog had its panes
but no mock-up look; the window had no icons at all, only the five player signs drawn by `paint_icon`.

**The section list.** Above the list a caps 10 px line "SETTINGS SECTIONS"; each section is a 28 px row
with a 14 px icon (`SettingsTab::icon` — `settings`, `psychology`, `folder_supervised`, `label`,
`face`, `inventory_2`, `terminal` of the mock-up), the 13 px label and a mono 10 value at the right
edge. The value is the state before a click, as the mock-up's "ViT-B/32" and "4.8 GB": the interface
language, the model's dimensions, the files in the index, the enabled tags, the face threshold, the
programs found. The active row sits on the `accent_soft` fill with a **2 px accent edge** on the left
(the mock-up's 3 px bar rounded down to the line of our layout) and its value in the accent. The value
is dropped when `10 + 14 + 6 + label + 8 + value + 8` does not fit the 200 px list, so the widest
Russian label ("Индексация и дубли") never collides with its counter. The pane title stays 16 px over a
1 px rule, the fields stay 520 px, and the groups the mock-up shows but photocat does not have
(autostart, tray, GPU, update checks, metadata backup) are not added — spec §5 q. 5 holds.

**The icons.** `Icon` went from 5 variants to 62 and `paint_icon` from 110 lines to ~600 of geometry:
the Material Symbols of the mock-up by name, drawn with lines, closed paths, circles, blocks and
arrowheads in an `s` box (14 px in a button, 12 px in a compact one, a 1.5 px stroke — not 1.0, which
the calendar check reads as a selection frame). A 62-cell contact sheet was rendered from the shapes
(as SVG, then PNG) and looked at twice: the first pass showed a gear that reads as a sun at 14 px, so
`Settings` became the mock-up's `tune` (three rails with a knob), and the face had no visible eyes;
both were fixed before the sweep. One widget replaced four builders: `WordButton` with `Normal`,
`Primary`, `Danger` and `picked` roles, a 20 px compact variant for a strip switch and a 20 px small
one for a dense row. It keeps the geometry of `egui::Button` (26 px, 10 px padding, 4 px radius, the
style's interact tones) and adds the icon 6 px before the word; the icon takes the colour of the word,
so both themes and the disabled state need no second drawing. 97 call sites moved: the top strip, the
left panel (the folder block, seven navigation rows with icons and the year/tag blocks untouched), the
content header (three cell views, "By months / Flat"), the duplicates, faces, trash and integrity
panes, the albums action row, the calendar ("Today", the chevron arrows), the folder list and the
picker (folder glyphs on the rows), the storage, export and album dialogs, the settings panes, the
inspector, the full-screen header, tools and playback row, and the face naming row. Four kinds of
20 px target keep their word alone — the per-tile actions placed into a fixed rect (`keep`,
`restore`, `pick`, `not him`), the "×" of a filter chip, the album tree's collapse triangle and the
"×" of an album row — and menus ("Sort", the model and language pickers) keep words: an egui menu
button takes text or a texture, and a texture is the second pipeline ADR 0048 refuses.

**Checks.** Six new tests, all headless. `every_icon_paints_inside_its_box` paints all 62 glyphs and
asserts a shape inside the 14 px box; `the_icon_inventory_covers_the_enum` reads the enum out of the
source and compares it with the list, so a new glyph cannot stay untested;
`a_word_button_draws_the_icon_before_the_word` (the word is drawn, the icon is left of it and inside
the button); `the_settings_sections_are_caps_with_icons_and_an_accent_edge` (the caps line, four
section labels, exactly one 2 × 28 px accent fill in the dialog — the modal fades in, so the hue is
compared and not the alpha); `the_settings_section_list_holds_both_languages` (all seven labels inside
the 200 px list under both languages); `every_button_of_the_window_has_an_icon` scans the 16 000 lines
of `src/app.rs` for `ui.button(`, `.small_button(`, `egui::Button::new(`, `primary_button(` and
`danger_button(` and fails on any hit outside the four documented exceptions. The seventh covers the
regression the icons caused: `the_content_controls_keep_their_own_row` draws the whole window at
1240×840 and checks that the five controls are one row below the title, that no control runs over the
title or past x 900 into the details panel, and that the size slider stays in the heading row.
`cargo test` 268 passed (261 + 7), `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, the real DB of 2232 files, release, 1240×840
headless frames): the settings dialog p50 6.94 ms (Interface), 6.94 (AI model), 6.90 (Indexing),
8.01 (Tags — the dictionary list is the heaviest pane), 6.89 (People), 7.06 (Storage), 6.92 (External
programs), p95 ≤ 8.13 ms; the section list's caps line stands at y 168. The M22.2b grid pass repeats
after the icons over four runs: p50 6.73–6.84 ms (146–149 fps), p95 ≤ 7.18 ms against 6.4–6.7 ms
before — below the noise of a 160-frame run (one run with a busy machine gave p95 8.50 ms).

**Documents.** `docs/UI.md` § Layout gets the icons paragraph (the set, the size, the colour, the four
exceptions, the player) and loses the old "player exception"; § Metrics gets "Action button" and
"Icon" rows, the new look of the navigation item, of the settings dialog and of its section list, and
the two rows of the content panel; § Screens follows. `docs/DECISIONS.md` ADR 0048 records the decision and what we would do without it (the
same sentence as the fonts: nothing is bundled). `plan/M22-ui-spec.md` §5 q. 11 gets the owner's answer,
§3 and §6 M22.8 the result; `plan/I18N.md` gets the one new label ("Settings sections"); this file and
`plan/BACKLOG.md` count M22.8 as done.

## M22.9 — the screens without a mock-up (2026-11-25) ✅

The four screens the mock-up set has no layout for — duplicates, faces, trash, integrity — were the
last ones built by hand: the duplicates' groups were egui `Frame::group` boxes on a tone that only
happened to match the tiles, `draw_face_cell` still painted the old blue `rgb(90,170,255)` on a picked
face and a selected file and kept grey 190/150 captions in a proportional font, the trash's empty
button was the accent one, and the integrity form began at the raw edge of the central panel. Spec
§6 M22.9 asks for the same parts as the main screen: the header, the mono numbers, one accent, red
only on the OS-trash path, and no local colour.

**How.** A new `card_block` draws a group of content as a card of the palette — the `card` tone, a
1 px `border`, an 8 px radius, 8 px inside — and the duplicate screen uses it per group: "#N" in mono
11 `text_dim`, the reason at 13 px strong, the facts "N files · X spare" in mono 11 at the right edge,
the advice at 11 px, the tiles on the card tone (`cells.bg` is `theme(ui).card` there now, so the
previews' corner cuts cannot drift from the card). The face tile keeps its metrics 150×150 + 26: the
`card` tone with the 8 px radius instead of a 3 px one on `extreme_bg_color`, the caption in mono 10
(the date `text_med` on the left, the detection score `text_dim` on the right — the caption band of a
file tile), and both the picked face and the selected file carry the 2 px accent. The group's name
became a 14 px subheading with the facts in mono 11, and "Photos with everyone together" dropped from
18 px to 14 px, so the 18 px view title stays alone. The trash's counter reads in mono 11 and "Empty
the trash" moved from `primary` to `danger` — the one word of the window that changes the disk, and
the only place red is allowed. The integrity screen became the content panel's form (canvas tone, a
12/6 margin, so the title stands where the grid's title stands): facts in mono 12, the three path
fields in the numeric family, "Compare the copies" the only accent fill. `highlight_job` — the OCR
match in the card and the search snippet, a defect left from M22.5 — lost the same old blue and takes
the theme's accent.

**Checks.** Four headless frame tests, one per screen, each checking the labels the screen had and the
tokens it now uses; a shared helper walks every drawn word with its font and colour (resolving the
galley's unset colour through the shape's fallback and override) plus every fill and stroke, and fails
on a colour outside the palette; the grey-235/210 of text on a dimmed photograph stays the documented
exception. The duplicates test asserts the group's card fill across the content panel and that no
`danger` appears on the screen; the faces test the mono 10 caption and the 2 px accent frame; the trash
that "Empty the trash (1)" is `danger` and "Restore all" is not; the integrity that its title sits at
the content panel's margin and that the form holds exactly one accent fill. `cargo test` 272 passed
(268 + 4), `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0
errors, `cargo test --test source_language` 3 passed.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, the real DB of 2232 files, 52 duplicate groups
and 2498 faces, release, 1240×840 headless frames, 40 frames per screen): duplicates p50 5.61–5.71 ms
(175–178 fps), p95 ≤ 7.21; faces p50 8.07–8.49 ms (118–124 fps), p95 ≤ 10.63 (the heaviest — every face
tile decodes its crop inside the 8-per-frame budget); trash p50 5.08–5.13 ms (195–197 fps), p95 ≤ 6.96;
integrity p50 4.77 ms (210 fps), p95 ≤ 7.29.

**Documents.** `docs/UI.md` § Layout gets the paragraph on the screens without a mock-up and loses the
stale "radii over 4 px" line that contradicted the 4/6/8 scale; § Metrics gets rows for the face
tile's caption, a duplicate group and the trash, and the integrity row now names the form's frame and
the numeric path fields; § Screens follows. `plan/M22-ui-spec.md` §3 marks the four screens as built,
§6 M22.9 carries the result and the numbers; `docs/ARCHITECTURE.md` § Measurements gets the row; this
file and `plan/BACKLOG.md` count M22.9 as done.

## M22.10 — dialogs and reports (2026-11-25) ✅

Spec §6 M22.10 asks for one look for the dialogs the mock-up set has no layout for: the card tone, a
1 px border, the 8 px radius, mono numbers, one accent button, and report bodies in mono 11 with a
copy button. Every dialog of the window was a `Frame::popup` — egui's frame for a *menu*: the
`menu_corner_radius` of 6 px, the `menu_margin` of 6 px, and the 16 px titles the settings dialog took
along were outside the type scale (10/11/12/13/14/18).

**How.** One `dialog_frame(ctx)` gives a modal the card — the `card` tone, a 1 px `border`, the 8 px
radius, 16 px inside — and every modal uses it: the folder list, the picker, the three storage
confirmations, the album dialog, the export dialog, the trash confirmation and the settings dialog
(whose content keeps its 760×620 and now sits 16 px inside the card). A dialog's title is a 14 px
subheading — the settings pane's heading too, so the whole window lives on the scale.

A number inside a translated sentence cannot be moved into the numeric family (the translation owns
the word order), so the three dialogs that had numbers grew a facts row of their own, exactly like the
album header of M22.6: the folder list counts the indexed files of each root ("2 files", mono 11
`text_med`), the trash confirmation reads "1 file · 18.4 MiB" above the sentence "Will be moved to the
OS trash." and the album delete shows the names, then "2 albums · 2 files", then the hint that the
rows go and the files stay. The export's progress ("export: 3/8") and its report body are mono 11; the
report moved into its own scroll area, as the integrity report already was. The folder counts live in
`App::root_counts`, refreshed when the index or the folder list changes: counting 2232 paths per frame
cost 0.84 ms, and the dialog is open for minutes.

Each of the five action dialogs has exactly one accent button: "Export N files" was `WordButton::new`
and is `WordButton::primary` now, next to "Index all", "Add and index", the cleaning confirmation and
the album's confirm. The trash confirmation is the one dialog whose button moves files: it wears
`WordButton::danger` and no accent (M22.9). Along the way the last two English words of the Russian
window — "Close" and "Copy the report" in the export dialog — went through `t()`; both were in the
table already, the call was missing.

**Checks.** `every_dialog_is_a_card_with_one_accent_button` walks all seven dialogs after the fade-in
(a modal's palette arrives with a multiplied alpha while it animates, so the frame helper runs the
clock): the card frame is found by the tone, the 1 px `border` and the 8 px radius together, the five
action dialogs hold exactly one accent button, the settings dialog none (every change is saved at
once) and the trash confirmation none but a `danger` line; no 16 px word is left, and every drawn
word, fill and line is a token of the palette.
`the_dialogs_put_their_numbers_in_the_numeric_family` holds "2 files", "2 albums · 2 files",
"1 file · 1 B", "export: 3/8" and the report body in mono 11; `the_export_dialog_speaks_the_chosen_language`
fails on the old code with "Close" in the Russian window. `cargo test` 275 passed (272 + 3), `cargo
clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors,
`cargo test --test source_language` 3 passed.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, a copy of the real DB of 2232 files with an album
of 500 files so that the export dialog draws a 500-line plan, release, 1240×840 headless frames, 16
frames per dialog after four warm-up frames): folder list p50 0.03 ms, p95 0.04; picker p50 0.08 ms;
storage 0.02 ms; album delete 0.02 ms; export p50 0.18 ms, p95 0.32; trash 0.02 ms; settings p50
0.06 ms, p95 0.08. The folder list cost 0.84 ms per frame before its counters were cached.

**Documents.** `docs/UI.md` § Layout gets "a dialog is a card" (the frame, the 14 px title, one accent
button, the facts row, the report body); `plan/M22-ui-spec.md` §2.4 names the dialog frame and §6
M22.10 carries the result; `docs/ARCHITECTURE.md` § Measurements gets the row; this file and
`plan/BACKLOG.md` count M22.10 as done.

## M22.11 — the light theme pass (2026-11-25) ✅

Spec §6 M22.11 asks for the pass that was never done after the palettes arrived in M22.1: every screen
looked at in both themes, body text and dim hints above the AA floor, an untinted preview ground, and
dark still the default. The pass found two genuine failures — both in the palette, none in a screen
(no screen wrote a colour of its own in either theme, which is what §6 M22.10's and M22.9's checks
had already bought).

**The light accent.** The light palette took the drawn mock-up's amber `#d97706`; on the light slate it
reads 2.91:1 on the panel, 3.19:1 on the card and 2.86:1 on the selection fill. That tone colours the
active navigation row, a picked chip, a picked year and the confident tag — body text at 11–13 px — and
it also fills a primary button whose word is white on it (3.19:1). The same light `DESIGN.md` names
`amber-focus: #b45309`; that tone reads 4.58 / 5.02 / 4.51:1, and white on it 5.02:1. The light accent
is `#b45309` now, `accent_hover` `#92400e` (white on it 7.09:1). The active row of the left panel moved
from the `field` tone (4.07:1 under the light accent) to the `card` tone with a 2 px accent edge —
the tone spec §2.5 always named and the mark the settings section list already carried — so the whole
navigation reads the same in both themes.

**The danger word.** "Empty the trash" and "To trash" are body text too: the dark red `#ef4444` read
4.34:1 on the card, the light `#dc2626` 4.41:1 on the panel. They are `#f45c5c` (5.07:1 on the card,
5.31 on the panel, 6.01 on the canvas) and the light design system's error `#ba1a1a` (5.90–6.46:1).
`danger` still means only the path to the OS trash, and still no more than one such word per screen.

**The neutral ground.** The light `canvas` was already the cool `canvas-base` `#f8fafc`, not the warm
Material `background` `#fff8f5` of the same file (ADR 0044 chose that deliberately). The pass makes it
a rule with a test rather than a note: every ground token keeps `r ≤ g ≤ b`, and a tile's frame is
painted in `canvas`, so the letterbox around a photograph is the neutral ground and the only warm tone
on screen is `accent_soft`, which always means "selected".

**Checks.** `both_themes_pass_the_contrast_ratio` computes the WCAG 2.1 luminance ratio over the pairs
the screens actually paint: `text`/`text_med` ≥ 4.5 and `text_dim` ≥ 3.0 on `canvas`/`panel`/`card`/
`field`; the accent ≥ 4.5 on canvas, panel, card and the selection fill; the danger word ≥ 4.5 on its
three grounds; `on_accent` ≥ 4.5 on the idle and hovered accent fill. The dark selection fill is
composited (egui keeps colours premultiplied: amber 15 % over the card), which is 4.59:1 under the
accent. `the_light_ground_is_neutral_and_the_tile_frame_is_the_canvas` holds the ground and the frame
in both themes; a fresh install opens dark (`db::Settings::default`). `every_screen_paints_only_the_
tokens_of_its_theme` renders the grid, all four calendar layers, the albums and the full screen in
both palettes and fails on a word, fill or line off the palette; the four screens without a mock-up
and all seven dialogs check both themes in their own tests (`assert_palette` and `dialog_parts` now
take the theme from the app, the dialog helper applies it itself).

**Measured** (WCAG 2.1, both palettes, the numbers also in `docs/ARCHITECTURE.md` § Measurements):
`text` 14.97/13.22/12.64 dark and 17.06/16.30/17.85 light on canvas/panel/card; `text_med`
8.32/7.35/7.03 and 7.24/6.92/7.58; `text_dim` 4.88/4.31/4.12 and 4.55/4.34/4.76; the accent on the
panel/card/selection 7.94/7.60/4.59 dark and 4.58/5.02/4.51 light; the danger word 5.31/5.07 dark and
5.90/6.46 light on panel/card; `on_accent` 8.66 idle and 5.84 hovered in dark, 5.02 and 7.09 in light.
`cargo test` 278 passed, `cargo test --test source_language` 3 passed, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. On the real DB of 2232 files
the four screens without a mock-up cost the same in the light palette as in the dark one (p50
duplicates 5.93 vs 5.72 ms, faces 8.46 vs 8.41, trash 5.25 vs 5.23, integrity 4.88 vs 4.90, 40 frames
each), and no word, fill or line fell off either palette on real previews, tags and faces.

**Documents.** `docs/UI.md` § Layout gets the contrast rule with its numbers, the "looked at in both
themes" rule, the neutral-ground rule and the navigation row in the metrics table;
`plan/M22-ui-spec.md` §2.1 carries the new token values and §6 M22.11 the result;
`docs/DECISIONS.md` ADR 0044 gets the revision paragraph (the palette change and the test that now
guards it); `docs/ARCHITECTURE.md` § Measurements gets the row; this file and `plan/BACKLOG.md` count
M22.11 and the M22 block as done.

## M22.12 — the right panel keeps its width (2026-11-25) ✅

The owner's review after M22.11: the inspector's card came out of its 280 px panel. The first letters
of "Сведения о файле", "Дата", "Камера", "Размер" and the other captions were cut on the left, the
preview and "Добавить тег" ran over the window's right edge, and the bottom status line stayed in
English inside a Russian window.

**The cause of the shift.** The row of an own tag was a fixed 150 px field plus the word button —
13 px wider than the card. egui widened the right panel to fit the row, the panel's stored size kept
the wider frame, the panel grew past the window edge and the central panel painted its canvas over
the panel's left margin: the whole card was shifted right and clipped on both sides. The wider left
border looked like a broken layout, not like a too-wide row, which is why it survived the mock-up
pass unseen.

**The fix.** The field takes what the button leaves (240 px reserved for it in the card, never below
60 px), so it shrinks with the panel and the button always ends inside the card; the row keeps its
26 px height in both languages. The load path also builds the first status line from the settings it
has just read (`settings.lang`), not from the previous frame's language: a fresh Russian window now
opens with "в индексе 2232 файла, 21.2 ГиБ" instead of "2232 files in the index".

**Checks.** `the_file_card_fits_the_right_panel` draws the whole window at 1240×840 with one file
selected in Russian and asserts three things: the own-tag button stays inside the panel, the panel
title stands on the panel's left edge (not shifted), and no painted frame leaves the window. On the
old code it fails with a frame at x 1253.5 in a 1240 px window — the 13.5 px that were visible on the
owner's screen. Two test helpers are new: `collect_text_rects` (a galley may be anchored by its right
edge in a right-to-left row, so the recorded position is not always the top-left) and
`collect_rect_bounds`/`window_rects_at`, which collect every painted rectangle.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, the real DB of 2232 files, headless frames):
the farthest painted edge is 1240.0 in a 1240 px window and 1691.0 in a 1691 px one — nothing is
painted outside the window in either; on the old code the card's frame reached 1253.5 at 1240. The
same frame keeps the field and the button on one line. `cargo test` 279 passed,
`cargo test --test source_language` 3 passed, `cargo clippy --all-targets` clean, `check.sh` 55
checks, `rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Layout gets the rule (a row never widens its panel; a long field takes
what the button leaves) and § Metrics the own-tag row; `plan/M22-ui-spec.md` §6 M22.12 carries the
result; `docs/ARCHITECTURE.md` § Measurements the row; this file and `plan/BACKLOG.md` count M22.12.

## M22.13 — every action button carries an icon (2026-11-25) ✅

The owner's question after the layout pass: "all icons placed?" The answer was no. The source guard
`every_button_of_the_window_has_an_icon` looked for the literal `ui.button(`, and a button chained as
`ui\n.button(…)` slipped past it: six action buttons were still word-only — "Check now" and "Remind
me later" in the integrity banner, "Open full screen" and "Show the full text" in the file card,
"restore hidden tags (N)" and "Go to <month> <year>" in the calendar.

**Done.** Each is a `WordButton` now: `Verified` and `Close` in the banner, `FitScreen` and
`TextFields` in the card, `Restore` for the hidden tags and `CalendarToday` for the empty month. The
last plain menu of the window, "Sort", became a `WordButton` with the mock-up's `sort` glyph (three
rails) that opens the same list through `Popup::menu`; the current order is still shown inside the
list, so no behaviour changed. The guard now also fails on a chained `.button(`, on
`egui::Button::new(` (any path) and on `ui.menu_button(`. The icon set grew to 64 glyphs.

**What keeps words, by design.** A `ComboBox` is a select, not a button — the language, the model,
the theme and the album parent stay words, exactly as the mock-up draws them as `<select>`. The
calendar layer switch and the slideshow interval are segments and values; the four per-tile targets,
the "×" of a chip and of an album row and the album-tree triangle live in 20 px rects; the calendar's
prev/next chevrons are icon-only in the mock-up too and carry hints.

**Checks.** `every_button_of_the_window_has_an_icon` fails on the pre-M22.13 code with the six
offenders and passes now; `the_icon_inventory_covers_the_enum` passes with the new `Sort` glyph.
`cargo test` 279 passed, `cargo test --test source_language` 3 passed, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. On the screen the "Sort" control
shows its glyph before the word and "Open full screen" its fullscreen frame.

**Documents.** `docs/UI.md` § Layout gets the `ComboBox`/menu rule and the new guard in § "Where a
button keeps its word alone"; ADR 0048 gets the revision paragraph; `plan/M22-ui-spec.md` §6 M22.13
carries the result; this file and `plan/BACKLOG.md` count M22.13.

## M22.14 — the tile card and the file card after `photocat_1` (2026-11-26) ✅

The owner after M22.13: "the right panel does not match the mock-up; in tile mode you again made the
photographs full size, but they must be fitted into the tile".

**Symptoms.** The grid drew the whole tile — the well of the preview and the caption strip — on two
tones: `canvas` and `panel`. `canvas` is the tone of the grid itself, so a photo that did not fill
the 4:3 frame had an invisible letterbox and no visible card: the snapshot floated on the canvas at
full size with a caption under it. The right panel was a plain list of label-value rows: no preview
card, no identity row with the size, no grouped shooting parameters, no chips.

**Done.** `draw_cell` fills the card body with `card` and the preview well with `field` (the
mock-up's `surface-container-highest`); the fitted preview is unchanged. `file_card` is rebuilt after
`photocat_1`: `round_preview` cuts all four corners for a card, the preview gets an 8 px radius and a
1 px `border` and a video a duration badge; the name is mono 12 in the accent with the size at the
right edge and the path in mono 10; the shooting parameters live in a `card_block` with caps labels
("Shooting params", "Date and time", "Resolution", "Location") and the checksum under a hairline;
albums and tags are `card_chip`s with a count in a `caps_row`, the confident tag in the accent, the
weight and the percentile in the hint, "all tags (N)" for the unsure case; own tags are chips too;
"Text on the photo" carries the engine name at the right. The header gains the info glyph.

**A defect found on the way.** `card_block` set its content to `available − 16` and forgot the 1 px
line painted on both sides: every block was 2 px wider than its column, and inside the 280 px right
panel that widened the panel and pushed a frame past the window — the very failure M22.12 banned.
The line is counted now; no other screen changes its look, only by 2 px.

**Checks.** `the_file_card_fits_the_right_panel` (M22.12) fails on the old `card_block` with a frame
at x 1242 in a 1240 px window and passes now; the palette test is renamed to
`the_light_ground_is_neutral_and_the_tile_well_is_the_field` and looks for `field` wells. `cargo
test` 279 passed, `cargo test --test source_language` 3 passed, `cargo clippy --all-targets` clean,
`check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. On the screen at 1240×840 the tiles are
cards with the photo inside and the file card shows the mock-up's blocks.

**Documents.** `docs/UI.md` § Layout and § Metrics (the tile well), the file-card row of § Screens;
`plan/M22-ui-spec.md` §2.4, §2.5 and §6 M22.14; `plan/I18N.md` (the new labels); this file and
`plan/BACKLOG.md` count M22.14.

## M22.15 — the tile preview fills the frame (2026-11-26) ✅

The owner after M22.14: "in tile mode the photograph is shown in its own proportions and fitted
into the tile square. The photograph must fill the whole tile, as it used to be".

**Symptom.** Every tile carried the letterbox of `fit_rect`: a portrait stood in the middle of the
4:3 frame with two `field` strips at its sides, so the tile read as a small photo on a plate rather
than as a filled cell of the grid.

**Done.** `cover_rect` scales the preview by the larger of the two ratios and centres it, so the
snapshot covers the 4:3 frame edge to edge; the sides that do not fit are cut evenly (a portrait
above and below, a panorama at the sides), the proportions are kept and nothing is stretched. The
painter clips the picture to the frame. Only the grid tile fills: `draw_cell` takes a `ThumbFit`
(the tile view passes `Filling`, the proportion rows `Whole`), and the calendar, the series strip,
the album cover and the file card keep `paint_thumb`'s fit — a preview is never cropped where the
whole shot matters. The cell-view tooltip now says "a 4:3 frame, the snapshot fills it, the sides
are cut" instead of promising a fitted whole snapshot.

**Checks.** `a_portrait_covers_the_tile_and_keeps_its_proportions`,
`a_panorama_covers_the_tile_by_height`, `a_square_covers_the_whole_tile` and
`a_zero_size_does_not_break_the_cover` pin the maths; `the_tile_draws_the_preview_over_the_whole_frame`
draws a real 160×320 preview into a 200×150 frame and reads the mesh back: 200×400, the top above and
the bottom below the frame, no margin. The scroll measurement of 2026-11-24 repeats on the real DB of
2232 files, six runs: p50 6.55–6.64 ms (151–153 fps), p95 6.72–7.19 ms — the clip rect and the
larger source area cost nothing measurable. `cargo test` 284 passed, `cargo test --test
source_language` 3 passed, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors; the window lives through `timeout 6`.

**Documents.** `docs/UI.md` § Layout, § Metrics and the tile rule; `plan/M22-ui-spec.md` §2.4,
§2.5, §5 q. 4, §6 M22.15 and §7; `docs/ARCHITECTURE.md` § Measurements; this file and
`plan/BACKLOG.md` count M22.15.

## M22.16 — the right panel after `photocat_12` (2026-11-26) ✅

The owner after M22.15: "make the layout of the right panel after the mock-up" (`photocat_12`).

**Symptoms.** The panel never owned its 280 px: egui's default side-panel frame keeps an 8 px margin
on each side, so the 1 px line painted by `panel_edge` stood 9 px inside the panel — a strip of panel
tone between the line and the grid — and the file card lived on 263 px. The header was two loose
rows in the top corner of the scroll area with a word button "Hide", the preview and the blocks
touched the panel's lines from edge to edge, and the own-tag field, the album chips and the location
row were farther from `photocat_12` than from it.

**Done.** The right panel now takes `Frame::NONE` with the panel tone: the line stands on its left
edge and the whole width is the content's. The mock-up's header is a 44 px band on the `card` tone
with a 1 px line under it — the info glyph, "File details" at 12 px strong and the mock-up's "×" as
a 22 px icon button 12 px from the right edge, with the hint "free up room for the grid and
calendar"; the worded way back is still "Details"/"Hide details" in the top bar, so an icon is never
the only path (docs/UI.md § Layout). The card is drawn inside 12 px of air (`PANEL_PAD`) on every
side — the mock-up's `p-3.5` — so the preview and every block share one column, and the full-screen
inspector keeps the same air. The details the window can carry came with it: the corner badge of the
preview shows the format of a still ("JPG", "HEIC") next to the video's "▶ M:SS"; the location row
gets the mock-up's pin (a new `Icon::Location`); an album chip gets the folder glyph before the name;
the own-tag row becomes one framed field with the mock-up's "`#`" and the "Add tag" button inside it,
still never wider than the card (M22.12); the card's two actions take the 20 px button so "Open full
screen" and "To trash" stay on one row in both languages. Refused: the round icon-only actions over
the preview (the words stay), the album "+ Add" chip (a flow of its own — adding a file to an album
from the card), the exposure badges and the RAW badge (not indexed, `plan/M22-ui-spec.md` §5 q. 5),
the place name (a geocoder; the network is for models only), the 320 px panel width (the owner's
280 px from M22.12) and the `rounded-full` tag pills (the 4/6/8 radius scale).

**Measured** (`docs/ARCHITECTURE.md` § Measurements, the real DB of 2232 files, 1240×840 headless
frames): the header band is 279×44 px at y 31–75, the "×" 22×22 px at x 1206–1228, the file card
255 px wide at x 973–1228 — 12 px of air on both sides — and no rectangle or word of the panel leaves
the window in either language; the Russian action row of the card is one line now (it used to wrap
over 255 px). `cargo test` 285 passed (one new test), `cargo test --test source_language` 3 passed,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Layout (the right panel, the "×" in the list of targets that keep their
word alone), § Metrics (the right panel, the preview in the card, the own-tag row) and the file-card
row of § Screens; `plan/M22-ui-spec.md` §2.5, §3 and §6 M22.16; `docs/references/mockups/README.md`;
`plan/I18N.md`; `docs/ARCHITECTURE.md` § Measurements; this file and `plan/BACKLOG.md` count M22.16.

## M22.17 — the word of a chip acts, the "×" removes (2026-11-26) ✅

The owner after M22.16: the tag cloud of the card highlights one tag as "chosen", yet a click on any
other tag deletes it from the photo; a click on a tag must not delete anything — removal has the "×"
of the chip; and the highlight must mean the tags the photos are filtered by. Either a click does
something meaningful (a search of the shared photos) or nothing at all, and then nothing is
highlighted.

**Symptom.** `card_chip` was one 20 px target: the word "tag 30% ×" was painted as a single galley
and every click on the box hid the tag from the photo (`db::hide_tag`). The accent marked the
*confident* tag, so the chip that looked "chosen" was not the chip the grid was filtered by; the
albums chips worked the same way — a click anywhere removed the photo from the album.

**Done.** A chip is now two targets in one box, `app::ChipClick` (`None`, `Body`, `Remove`): the "×"
owns the glyph, the 4 px gap before it and 2 px around it — 16 px of the chip's right edge — turns
`danger` under the pointer and carries the removal hint, while the word carries the action hint. A
click on the word of a tag chip sets `tag_filter` (the same filter as the left panel's tag chips), a
second click on the highlighted chip clears it, and the accent marks exactly that filter; the
confident tag keeps its place in the caps row ("CLIP 84%") and in the line "tag: <name>". The word of
an album chip opens the album in the "Albums" view (the path of a click in the tree); only the "×"
removes the photo from it. My tags follow the same rule: the word filters, the "×" removes the own
tag. The left panel already worked this way and is untouched.

**Checks.** `the_word_of_a_card_chip_acts_and_the_cross_removes` clicks the chip at 8 px of its left
edge (`Body`) and at 8 px of its right edge (`Remove`); `a_click_on_a_tag_in_the_card_filters_the_photos`
clicks a tag in the whole window frame: the filter becomes `beach`, the two tags of the photo stay, a
second click clears the filter; `the_word_of_an_album_chip_opens_the_album_and_the_cross_removes_the_row`
checks the album word (the view becomes "Albums", the album is picked) and the "×" (the view stays, the
rows of the file are reloaded). `cargo test` 288 passed, `cargo test --test source_language` 3
passed, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0
errors; the window lives through `timeout 6`.

**Documents.** `docs/UI.md` § Interaction (the chips of the file card), § Metrics (the card chip) and
§ Findable in a second (a filter by a tag seen on a photo); `plan/M22-ui-spec.md` §2.5 and §6 M22.17;
`plan/I18N.md` (three labels); this file and `plan/BACKLOG.md` count M22.17.

## M22.18 — the own-tag field gets its room back (2026-11-26) ✅

The owner after M22.17: the "add tag" field is short and the button is far too big — drop the word
and keep only the "+".

**Symptom.** The own-tag row reserved 140 px for a word button ("Add tag", 118 px in Russian) inside
a 237 px framed row: the field got `available − 140 − 16` = 81 px, less than the 164 px hint of the
field, and the button outweighed the input it serves.

**Done.** The button is an icon-only 22 px square (`icon_button` with the `add` glyph, the word
`Add tag` as its hint) inside the framed row; the field takes `available − 16 − 22 − 8` — about
190 px of the 255 px card in either language, and the Russian hint fits whole. The reserve is
counted, so the row still never widens the panel (M22.12), and the field never falls below 40 px.
The owner's second look at the same row ("the input is short and sticks to the upper edge of the
shared field") found the cause: egui styles a text edit as a button — `min_size.y` is the 26 px of
`interact_size` and the text is aligned `LEFT_TOP`, so inside the 22 px row the input sat at the top
and read as short and crooked. The row now sets `interact_size.y = 22` and centres the text
(`TextEdit::align(LEFT_CENTER)`): "`#`", the tag and the "+" are one 22 px line, the row is 28 px
(the mock-up's `h-7`).
`docs/UI.md` § Layout names the "+" among the targets that keep no word, next to the panel's "×":
the owner's request outranks the general rule, and the "`#`", the hint and the "My tags" heading say
what the square does.

**Checks.** `the_own_tag_row_keeps_a_wide_field_and_a_bare_plus`: the word "Добавить тег" is not
drawn, the hint of the field is whole, the "+" is a 22×22 rect at least 150 px after the text and
inside the window, and the vertical centres of the "#", the hint and the "+" agree within 1 px; the
test fails on the 140 px reserve (103 px to the button) and on the theme's top-aligned text (3.5 px
off) and passes now.
`the_file_card_keeps_the_mock_ups_band_and_air` still keeps the "+" inside the card's 12 px air;
`the_file_card_fits_the_right_panel` now looks for the field's hint (the row has no word button any
more). `cargo test` 289 passed, `cargo test --test source_language` 3 passed, `cargo clippy
--all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors; the window lives
through `timeout 6`.

**Documents.** `docs/UI.md` § Layout (the "+" in the list of targets that keep no word), § Metrics
(the own-tag row) and the file-card row of § Screens; `plan/M22-ui-spec.md` §2.5 and §6 M22.18; this
file and `plan/BACKLOG.md` count M22.18.

M22.19 is the owner's next look at the window: "the top part does not match the mock-up, and other
elements either". A frame at 1440×875 beside the rendered `photocat_1` named the differences: the
strip was eight word buttons without a mark, the content had three rows where the mock-up has one,
the selection sat in the chip row, and the footer repeated the "Details" word that the icon toggle
now carries. The strip now follows the mock-up group by group: the photo-library mark and "photocat",
one search field in the middle (mode switch, query, "Search", "×") at most 576 px and centred on
measured widths, and on the right the "Tiles" pill (a compact slider, the value in the hint), the
state pill (a dot and "Synced"/"Syncing", the counters in the hint), the lit details toggle and
"Settings". "Folders (N)" became the bare "+" of the "Catalog" heading; the facts moved into the
state hint and the footer. The content is one 44 px ribbon — title 18 px and facts 12 px mono on the
left, the cell views, "By months / Flat" and "Sort" on the right behind 1 × 16 px dividers — and the
chip row appears only while a filter is on. The selection is the mock-up's floating pill at the
bottom edge of the grid: a dot, "N selected · size" in mono 11 in the accent, "Select all (M)" and
"Clear selection", drawn in a child ui at a fixed rect after the scroll area (an `egui::Area` was
refused: its first frame is a sizing pass that paints nothing). The footer is a dot, the last fact
and the counters on the left, the selected file "name · frame · size" on the right; the five years
went from two columns of three rows to three of two, so the fifth year no longer falls under the
pinned summary at 840 px. Refused: the "Filter" menu, the per-month "Select all", the 224/320 px
panels and the storage bar (the reasons are in §6 of the spec).
**Measured.** At 1440×875: the mark at x 12–112, the search frame at 273–860, the "Tiles" pill at
1030–1150, the toggle and "Settings" to the right edge; the ribbon is one row, y 85–129; with two
files picked the bar sits at y 751–783 and the chip row is not drawn. `cargo test` 290,
`cargo test --test source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors. A stray `git checkout src/app.rs` during the pass wiped the
uncommitted M22.16–M22.18 work; it was replayed from that pass's session log — the 289 tests of
M22.18 passed again — and M22.19 was then re-applied on top.

**Documents.** `docs/UI.md` § Layout, § Metrics (the top strip, the ribbon, the years, the selection
and the footer) and the Grid row of § Screens; `plan/M22-ui-spec.md` §2.4, §2.5, §3 and §6 M22.19;
this file and `plan/BACKLOG.md` count M22.19.

M22.26 is the owner's look at the "People" screen. Four things were wrong: the face tiles carried two
worded buttons where the owner asked for icons (an empty tick circle for "pick", a crossed head for
"not him"), the group list sat in the left panel while the right panel showed the file card that this
screen does not need, the group rows had no picture of the person, and the ribbon's "Tile / Rows /
List" switch did nothing because `draw_face_cell` never reads `CellStyle`. The tile now has two round
24 px targets drawn over a dark scrim so they read on any crop: the tick circle fills with the accent
and shows the tick once the face is picked, the crossed head is drawn only inside one group — in "All
people" there is nothing to split off — and both carry their translated hint instead of a word
(`tile_icon_target`, `Icon::Pick`, `Icon::NotHim`). The right panel of the screen is the group list now
(`faces_panel`): the same 44 px header band as the details panel, the face glyph and "Groups" but no
"×" — the list is content, not a toggle — then one scroll with "All people · N" and a row per group:
the "together" tick, the best face of the group as its icon (`face_avatar`, the same texture cache the
tiles use, the face glyph while a crop is missing) and the name with its photo count. The list left
the left panel, which keeps the threshold, the pass, "Rebuild groups" and the together search; below
1200 px the list falls back into the left panel the way the file card does. The details toggle of the
top strip is not drawn on this screen and the ribbon keeps no cell-view switch there. `cargo test`
310, `cargo test --test source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors; the window lives through `timeout 6`. On a copy of the real DB
(2232 files, 2498 faces in 617 groups) the screen costs p50 10.27 ms / p95 12.16 ms in the dark palette
and 9.61 / 11.08 ms in the light one — up from the 8.07–8.49 ms of M22.9, which is the price of the
617-row list with a face icon on every row, still inside the 16.7 ms of a 60 fps frame. The new test
`the_faces_screen_keeps_its_list_in_the_right_panel` checks that "Groups" and "All people · 2" are
drawn at x ≥ 960 of a 1240 px window and that "Tile", "Rows", "List" and "Details" are not drawn on
the screen at all; the two existing screen tests lost the "not him"/"pick"/"picked" words and now
check that those words are gone.

**Documents.** `docs/UI.md` § Layout (the ribbon, the right panel, the one-scroll rule), § Metrics
(the face tile), § Screens (the People row), the false-merge bullet and the wording list; `plan/I18N.md`
(the face tile labels are hints now); `docs/ARCHITECTURE.md` § Measurements (the M22.26 row);
`plan/M22-ui-spec.md` §6 M22.26; this file and `plan/BACKLOG.md` count M22.26.

M22.27 is the owner's word about the trash: the "restore" button on a trashed tile should be an icon.
The tile and the list row now carry the same round 24 px target a face tile has — the dark scrim, the
accent on hover, the restore glyph (`tile_icon_target`, now shared with M22.26, and the thin
`restore_icon` wrapper) — and the word moved into the hint, translated: "Restore: back into the usual
views — the file was untouched on disk". The rest of the screen did not change: "Restore all" keeps
its word, "Empty the trash (N)" keeps the one `danger` border, and a click on the target no longer
selects the row or the tile as well. The name row of the list gained the room the 70 px word took:
`right` is 40 px in the trash instead of 96. `cargo test` 310, `cargo test --test source_language` 3,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors; the
window lives through `timeout 6`. The trash tests now check that the word "restore" is not drawn on
the screen at all, and the button guard lost its exception for it.

**Documents.** `docs/UI.md` § Layout (the word-only targets), § Metrics (the Trash row),
§ Screens (the Trash row) and the wording list; `plan/I18N.md` (the trash hint); `plan/M22-ui-spec.md`
§6 M22.27; this file and `plan/BACKLOG.md` count M22.27.

M22.28 is the owner's look at the full screen: no visible close target, the face boxes always on, and
a face told nothing about its other photos. The header now ends with the mock-up's 22 px "×" right of
the position counter (`Icon::Close`, the hint "close the full screen — Esc"); "Back to the grid" stays
as the worded way back. The boxes became a tool: "Show faces" in the tools row with the face
silhouette, off by default, disabled with the hint "no faces found in this file" while the model found
none. Every box is a click target now — `paint_face_chips` returns the pressed face id and
`db::FaceBox` grew `id` and `cluster_id` — and a click opens the 280 px right column: the same 44 px
band with the face glyph, the name of the group and a "×", the count "N photos", then every photo of
that face, oldest first (`App::face_focus_for`), as 64×48 rows with name, date and size; the frame on
screen carries a 1.5 px accent. A click on a row opens that photo in place (or rebuilds the viewer
when the current walk does not hold it), a click on the empty part of the frame or the panel's "×"
closes it, and paging closes it too — another file is another face. An ungrouped face lists only the
frame and says "This face is not in a group yet." `cargo test` 311, `cargo test --test source_language`
3, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors;
the window lives through `timeout 6`. The old test `the_full_screen_marks_the_faces_over_the_frame`
became `the_full_screen_hides_the_face_boxes_until_asked` (off by default, the word turns them on,
the turn keeps them); `a_picked_face_lists_its_photos_in_the_right_panel` checks the chronological
list of a group, the panel texts and the ungrouped case.

**Documents.** `docs/UI.md` § Screens (the full-screen rows, the new face panel), § Interaction (the
two new ways into faces) and the counter list; `plan/I18N.md` (the viewer strings of M22.28);
`plan/M22-ui-spec.md` §6 M22.28; this file and `plan/BACKLOG.md` count M22.28.

M22.29 is the owner's look at an album cover: the mosaic of previews must not show a photo at its own
size with margins around it — the previews should fill their cell. The mosaic kept `paint_thumb`'s
fitted whole preview (M22.21, M22.15's rule), so a 16:10 cover of 4:3 photos left a band inside every
cell at once. `paint_cover_mosaic` now paints through `paint_thumb_as(..., ThumbFit::Filling)`: the
preview covers its cell edge to edge and the sides that do not fit are cut evenly — the centring crop
of the grid tile (`cover_rect`), no stretch and no margins, the `canvas` showing only through the 2 px
seams. The crop would square the corners of the well, so the mosaic ends with
`round_preview(ui, rect, RADIUS_CTRL, bg, true)` — `bg` is the tone under the well, `card` on an album
card, `canvas` in the list row — and both looks share the one call. Refused: a square cover, a "cover
crop" setting, more than four cells; the fitted whole preview stays everywhere else — the calendar,
the series strip and the file card. `cargo test` 311, `cargo test --test source_language` 3,
`cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors; the
window lives through `timeout 6`.
`the_album_cover_paints_four_previews` also checks that each of the four painted rectangles covers
its cell, so the margins cannot come back unnoticed.

**Documents.** `docs/UI.md` § Interaction (the Albums bullet) and § Metrics (the album card and the
album list row); `plan/M22-ui-spec.md` §6 M22.29 and §7; this file and `plan/BACKLOG.md` count M22.29.

## M25.1 — the tag filter accumulates (2026-11-26)

The owner's first task of the M25 review: the tag filter must accumulate, and a reset stands where the
filter is set instead of a permanent "all tags" cell. Done in `src/app.rs` and `src/i18n.rs`; the DB
schema, the archive and the originals are untouched.

- **The set.** `App::tag_filter: Option<String>` became `tag_filters: Vec<String>`, kept sorted and
  without duplicates (`tag_picked`, `toggle_tag`, `clear_tag_filters`). `filtered()` demands every
  picked tag above that tag's own threshold (AND); a word that exists both as a CLIP tag and as an own
  tag still matches both. `PickScope.tag` became `Vec<String>`, so a change of the set drops the
  selection with the same reason in the status.
- **One rule in every cloud.** The panel chips, the AI tags and the own tags of the file card and the
  chips row above the grid all add on the first click and remove on the second. The card chips say
  "add this tag to the filter" / "remove this tag from the filter"; the panel and the chips row carry
  one chip per tag.
- **The reset.** The "all tags" chip is gone. The cloud sits under a caps "Tags" heading whose right
  edge carries a bare 20 px `Icon::Close` target ("reset the tag filter"), drawn only while at least
  one tag is picked — the folder block's "+" pattern (`tags_heading`). Each "×" of the chips row
  removes exactly its own tag, "Reset" clears them all.
- **The numbers.** A chip still counts the photos that carry this tag alone above its threshold, not
  the size of the intersection — "this click shows N photos" stays true and the row does not move on
  a click. The "all tags" percentile left with the chip: `App::top_scores`, `embed::max_score` and
  `embed::top_score_distribution` are removed, and the empty grid names one tag as before or says
  that no photo carries all the picked tags.

Checks: `the_panel_tag_cloud_accumulates_the_filter` (a click adds, a second click removes, the grid
keeps only the photo with both tags, the two "×" of the chips row and "Reset"),
`the_reset_beside_the_tags_heading_clears_the_filter` (no 20 px target while the filter is empty, a
press clears the set), the extended `a_click_on_a_tag_in_the_card_filters_the_photos` (the second tag
joins the filter, the second click removes one) and the updated
`the_tag_chips_do_not_jump_when_a_tag_is_picked` (the same chips in the same places, no "all tags");
`cargo test` 313 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors. A headless 1240×840 frame draws both chips and the chips row
in the test.

**Documents.** `docs/UI.md` § Interaction (the card chips and the left panel), the grid row of
§ Screens and § Metrics; `plan/M22-ui-spec.md` §6 M22.17; `plan/I18N.md`; this file and
`plan/BACKLOG.md` count M25.1 done and M25.2 next.

## M25.2 — the year cloud: five chips, a "…" for the older years (2026-11-26)

The owner's second task: the years must stay short and dense, the "…" must be a filter and not an
expander, and "All years" must stay. Done in `src/app.rs` and `src/i18n.rs`; the DB schema, the
archive and the originals are untouched.

- **The filter is its own.** `App::year_filter: Option<YearPick>` (`YearPick::Year` /
  `YearPick::Older(bound)`) is matched against the prefix of `taken_at`; `set_year` replaced
  `pick_year`, so the year no longer writes into the path/date field — a typed word survives a click
  (the M25.3 half of the split is done here). `PickScope.year` follows, and a change of the pick
  drops the selection with "the year filter changed".
- **The cloud.** `year_counts` returns every year of the index; the block takes the five newest as
  18 px chips (`cloud_chip`/`year_chip`, the tag chip's geometry with `YEARS_CHIP_H`) and, only when
  older years exist, a sixth "…" chip that filters exactly those years (`YearPick::Older`, the
  oldest year of the cloud as the boundary). No number is drawn in a chip: the hint says
  "2019 · 1 234 files" through `i18n::count`, and the "…" hint adds "older than 2020 · N files ·
  2004–2019" (`years_range`). "All years" is drawn only while a pick is on and clears either kind;
  a second click on the picked chip clears it.
- **The chips row.** "year: 2019 ×" or "older than 2020 ×" (`FilterClear.year`), each "×" clears the
  year only and leaves the path word and the tags alone.

Measured on the corpus (2232 files, 10 years, a 1240×840 headless frame, release): the "YEARS"
heading at y 593, the first chip at 618.5, the "…" at 648.5, the cloud ends at 666.5 and the pinned
"Summary" starts at 692 — every chip is seen without a scroll, and the cloud is 13.5 px shorter than
M22.3's 680 px year block. The measurement is repeatable:
`PHOTOCAT_BENCH_DB=~/.cache/photocat/photocat.db cargo test --release -- --ignored
panel_years_measurement --nocapture`.

Checks: `the_years_cloud_keeps_five_and_filters_the_older_ones` (five chips and "…", no 2019/2004 in
the cloud, no number inside a chip, the "…" narrows the grid to 2004–2019, the chips row names the
pick, a second click clears it, "All years" is drawn only while a pick is on and clears it), the
reworked `a_year_click_narrows_the_date_filter` (every year counted, the year is its own filter, a
typed path word survives the click) and the updated
`the_years_block_draws_the_newest_five_with_counts` / `the_left_panel_fits_the_years_above_the_summary`;
`cargo test` 314 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Interaction (the left panel), the "One year" row of § Findable, the
grid row of § Screens and the "Year chip" row of § Metrics; `plan/M22-ui-spec.md` §6 M22.3
(superseded in part); `plan/I18N.md`; this file and `plan/BACKLOG.md` count M25.2 done and M25.3 next.

## M25.3 — the path/date field explains itself (2026-11-26)

The owner's third task: know what the field compares before typing into it, and let the year stop
living in it. The split itself landed with M25.2 (`set_year` writes `App::year_filter`, not `query`,
and the chips row draws the word and the year as two chips with two "×"); this task adds the words.
Done in `src/app.rs`, `src/i18n.rs` and `docs/UI.md`; no schema, no archive, no originals.

- **The tooltip.** `PATH_DATE_HINT` — one constant used by the field and by the test — says the typed
  text is a case-insensitive part of the file path or of the ISO date, with the two examples
  "2024-05" and "vacation", and that a search by what is on the photo lives in the top bar under
  "Semantic / By text". The hint `2024-05, vacation` stays; the label stays "Search by path or date".
- **The document.** `docs/UI.md` gained a § Filters subsection: the field is literal, the year cloud
  is its own filter, and the content search is another field.

Check: `the_path_word_and_the_year_are_two_filters_with_two_crosses` — with "vacation" typed and
2024 picked only `vacation-c.jpg` passes, both chips are drawn ("path/date: vacation ×", "year: 2024
×"), each "×" removes exactly its own filter, and the tooltip is in `PAIRS` and reads in Russian.
`cargo test` 315 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Filters and the "Filter by path/date" row of § Findable; `plan/I18N.md`;
this file and `plan/BACKLOG.md` count M25.3 done and M25.4 next.

## M25.4 — "No date" and "No preview" are filters (2026-11-26)

The owner's fourth task: the two numbers of the summary become two filters, and the preview count
stops lying after a cache cleanup. Done in `src/app.rs`, `src/storage.rs`, `src/index.rs` and
`src/i18n.rs`; no schema change (`SCHEMA_VERSION` is not bumped), no archive, no originals.

- **Two chips.** Under the kind row, "No date · N" and "No preview · N" — the 20 px chip of a tag
  (`toggle_chip`), the picked one on the accent, dimmed and dead at zero (`Sense::hover` and
  `text_dim`). Their counts are over the whole index, never over the current filter.
- **Honest "no preview".** A file passes when `files.thumb IS NULL` **or** the preview it names is
  not on disk: `App::previews_missing` is filled by `refresh_missing_previews` once per `reload`,
  so the filter does not stat the cache on every frame. The hint names the four causes (HEIC/AVIF,
  a broken file, a video without a cover, cleared previews) and the way back (index the folder
  again).
- **The filters narrow together.** `filtered()` applies both on top of the rest; the chips row
  draws "no date ×" / "no preview ×" and each "×" removes only its own; `PickScope` carries both
  fields, so a change drops the selection with "the no-date and no-preview filters changed".
- **A cleared cache heals.** `storage::clear_thumbs(db_path)` now calls `forget_thumbs`, which runs
  `UPDATE files SET thumb = NULL`: `index::can_skip` asks that column, so the next pass reads the
  file again and rebuilds the preview — the `docs/ARCHITECTURE.md` § Indexing flow promise is true
  now instead of "until a re-index".
- **The summary stops repeating them.** "No date:" and "No preview:" left `stats_block`; their
  numbers are the chips now, and the old "No preview:" counted `thumb IS NULL` only, which would
  have drifted from the honest chip. The rest of the summary goes in M25.5.

Tests: `the_no_date_and_no_preview_chips_filter_the_grid` (the counts over the whole index — one
file without a date, two without a preview, one of them a preview missing from the disk —, a click
narrows, both together leave only the file with neither, each "×" of the chips row removes its own,
a chip at zero does nothing) and `a_cleared_preview_makes_the_file_processed_again`
(`forget_thumbs` then `can_skip`). `cargo test` 317 passed + 3 guard tests, `cargo clippy
--all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

The two chips cost the pinned part 28 px. The panel budget test (`the_left_panel_fits_the_years_
above_the_summary`) still holds, but at the 840 px minimum the last control of the duplicates block
needs a scroll, so the duplicates translation test now draws a 1240×1000 frame; M25.5 gives the
panel 40 px back.

**Documents.** `docs/UI.md` § Filters (the two chips and the four causes), § States (the chips row),
§ Metrics (the filter toggle chip); `docs/ARCHITECTURE.md` § Indexing flow and the `thumb` column;
`plan/I18N.md`; this file and `plan/BACKLOG.md` count M25.4 done and M25.5 next.

## M25.5 — the summary leaves the left panel (2026-11-26)

The owner's fifth task: no doubled numbers and no block that only repeats the status bar. Done in
`src/app.rs` and `src/i18n.rs`; no schema, no archive, no originals.

- **The block is gone** with both of its callers: the pinned `Panel::bottom("left-stats")` of a wide
  window and the copy after the file card in a narrow one. `stats_block` is deleted; `stat_row`
  stays for the storage pane.
- **The numbers.** Files and size were already in the status bar; "no date" and "no preview" are the
  M25.4 chips now, computed over the whole index; "videos without a cover" and the line "press
  “Index” — ffmpeg will create the covers" moved into the "No preview" hint, as the task asked.
  Nothing is printed twice.
- **The strings leave the table:** `Summary`, `Files:`, `Size:`, `No date:`, `No preview:`, `Videos
  without a cover:` and the ffmpeg line are removed from `PAIRS` — a dead key is a label nobody can
  see.

Measured on the corpus (2232 files, 10 years, 1240×840 headless frame, release):
`panel_years_measurement` now prints the years heading at y 629, the first chip at 654.5, the "…"
at 684.5, the cloud ends at 702.5, the model block starts at 717 and the bottom strip at 812. The
40 px the summary took went to the scrolling part: before M25.5 the model block at 717 would have
sat under the pinned summary at 692. One honest tail: the tag cloud inside the model block is still
about 13 px below the strip at 840 px — M25.6 makes the cloud the first block of the scroll, and
its acceptance asks for exactly that, so the cloud enters the frame there.

Checks: `the_left_panel_fits_the_years_and_the_model_block` (no "Summary"/"Files:"/… in either
language, every year chip above the strip, the model block below the cloud and on screen, "All
years" on the same row after a pick), the updated `english_translates_the_left_panel` (no summary
lines, the two chips instead); `cargo test` 317 passed + 3 guard tests, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Layout (the left panel), § Findable (the panel budget), § Metrics
(the "Summary row" removed), § Screens (the grid, the narrow window); `plan/I18N.md`; this file and
`plan/BACKLOG.md` count M25.5 done and M25.6 next.

## M25.6 — the threshold is pinned, the cloud opens the scroll (2026-11-26)

The owner's sixth task: the threshold must not scroll away from the chips it cuts, and the left
panel must stop being a second settings screen. Done in `src/app.rs` and `src/i18n.rs`; no schema,
no archive, no originals.

- **The pinned block.** `threshold_block` draws "Filter threshold", the 0–100% slider (its
  percentage inside the `custom_formatter`), the "few tags (N files) — the percentile is noisy"
  note and "no tag passes the threshold — lower the slider", in the pinned part of the panel under
  the two "missing" chips and directly above the scroll. The tooltip keeps the percentile
  explanation and adds that the cloud below counts the photos that pass the cut, tag by tag.
- **The cloud opens the scroll.** The `Tags` heading with its reset and the chips come first, then
  the years cloud, then the view context, then the model and text blocks. M22.3's "the years open
  the scroll" is undone on purpose; the slider and its chips never leave each other.
- **No second settings screen.** `model_block` keeps the model state, "Compute tags", the
  confident-tag line and the free-search line; the cloud, the threshold and the "Model settings"
  button left it. The model's path, size limit and choice live only in "Settings → AI model".

Tests: `the_left_panel_pins_the_threshold_above_the_tag_cloud` (at 840 px the threshold and the
"Tags" heading are on screen, the cloud below the slider, no "Model settings" in either language,
no "Summary"), `the_threshold_stays_put_while_the_tag_cloud_scrolls` (24 frames with a wheel notch
over the panel: the threshold keeps its y, the years under the cloud move up), and a `fake_model_dir`
helper so the cloud tests do not depend on the machine's cache; the years tests point the models
folder at a never-downloaded one. `cargo test` 318 passed + 3 guard tests, `cargo clippy
--all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

Measured on the corpus with `panel_years_measurement`: at 1240×840 the pinned threshold is at
y 621 and the "Tags" heading at 685.5, with the years and the model block below the strip (812); at
the owner's 1240×1021 the "Years" heading lands at 941 and the strip at 993. The cloud-first order
costs the years their old place at the top, which is the price of the task's own order; the owner's
window still shows the cloud and the years without a scroll.

**Documents.** `docs/UI.md` § Layout (the left panel), § Findable (the threshold row, the models
row), § Interaction (the cloud and the threshold), § Metrics (the grid description), § States (the
model not downloaded); `plan/I18N.md`; this file and `plan/BACKLOG.md` count M25.6 done and M25.7
next.

## M25.7 — the two archive buttons explain themselves (2026-11-26)

The owner's seventh task: "Start indexing" and "Refresh" must say what they do before a click.
Done in `src/app.rs` and `src/i18n.rs`; no schema, no archive, no originals.

- **`INDEX_HINT`.** The "Start indexing" tooltip walks the folders and reads only new and changed
  files — the path, the size and the modification time decide (`index::can_skip`), computes the
  hash, the date, the dimensions, the sharpness and a 320 px preview, gives a video its cover
  through `ffmpeg`, writes to the cache only (the database and the previews), never to the archive,
  and can be stopped and resumed.
- **`REFRESH_HINT`.** The "Refresh" tooltip re-reads the database into the window (`App::reload`),
  reads no file, indexes nothing and changes nothing; it is for a change made by the CLI or in
  another window.
- **One source.** Both strings are consts used by the button and by the test, so the wording cannot
  drift from the test unnoticed; both go through `t()`.

Test: `the_two_archive_buttons_carry_their_hints` — both hints are in `PAIRS`, read in Russian, and
still name the skip rule ("the path, the size and the modification time"), the untouched archive
("never the archive") and the read-free refresh ("reads no file"). `cargo test` 319 passed + 3
guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `cargo test --test
source_language` 3, `rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Layout (the left panel's catalog block) and the "Folder, Index,
Refresh" row of § Findable; `plan/I18N.md`; this file and `plan/BACKLOG.md` count M25.7 done and
M25.8 next.

## M25.8 — the state word moves into the status bar (2026-11-26)

The owner's eighth task: "Synced" looked like a synchronisation feature. Done in `src/app.rs` and
`src/i18n.rs`; no schema, no archive, no originals.

- **The pill is gone.** `top_bar` no longer reserves or draws the state pill; `SEARCH_MAX_W` grew
  from the mock-up's 576 to 646 px — the English pill's measured ~70 px (its 28 px box plus the
  8 px gap) went to the search field, which stays centred between the mark and the tools.
- **The status bar carries the state.** `App::running_passes` names the seven passes with the same
  strings `pass_rows` prints — `Indexing`, `People`, `Trash`, `Export`, `Integrity` through `t()`,
  `CLIP` and `OCR` as names — and `state_word` returns that name, `{} passes` for two or more, or
  `None`, in which case the bar keeps the last status message exactly as before. The dot and the
  word carry `PASS_HINT`, which lists all seven and says their progress rows and "Stop" are in the
  top strip under the search field.
- **Dead code leaves.** `busy` and `sync_word` are gone with the pill, and `Synced`/`Syncing` left
  `PAIRS`.

Tests: `the_status_bar_names_the_running_pass` (no pill in the first frame; a channel on `rx` →
"Indexing"; a second on `ocr_rx` → "2 passes"; both closed → the last message), and
`the_search_field_took_the_room_of_the_state_pill` (a 646 px well frame in the strip, no state
pill), plus the updated `english_translates_the_window_shell`. `cargo test` 321 passed + 3 guard
tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `cargo test --test source_language`
3, `rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Layout (the top strip), § Metrics (the top strip), § Wording (the top
strip and the new status-bar line); `plan/I18N.md`; this file and `plan/BACKLOG.md` count M25.8 done
and M25.9 next.

## M25.9 — the location row: the coordinates, and the place-name question (2026-11-26)

The owner's ninth task: the card shows what the photo itself knows about its place, and the missing
half is named instead of faked. Done in `src/app.rs`, `src/i18n.rs` and `docs/DECISIONS.md`; no
schema, no archive, no originals, no network.

- **The row.** "Location" keeps the two EXIF coordinates to five decimals and gains a 20 px copy
  target (the hint "copy the coordinates"); a press copies "55.75580, 37.61730" to the clipboard and
  says "coordinates copied" in the status bar. A file without GPS shows no row at all.
- **The hint.** `LOCATION_HINT` says EXIF carries the coordinates only, that a place name needs a
  reverse geocoder, that the network is forbidden by §1 of `AGENTS.md`, and that an offline data set
  is a dependency and an ADR.
- **The open question.** ADR 0049 records the three options for the owner: no place name (today),
  an offline data set (size, licence, coverage — country, region or city), or a lookup by an explicit
  click that sends the coordinates and therefore needs a change of the §1 rule. No map link and no
  network call in this task.

Test: `the_location_row_shows_and_copies_the_coordinates` (a photo with GPS draws "LOCATION", the
five decimals and a copy target that sets the status and issues the copy; the same photo without GPS
draws no row). `cargo test` 322 passed + 3 guard tests, `cargo clippy --all-targets` clean,
`check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**Documents.** `docs/UI.md` § Interaction (the location row) and the file card row of § Screens;
`docs/DECISIONS.md` ADR 0049; `plan/I18N.md`; this file and `plan/BACKLOG.md` count M25.9 done and
M25.10 next.

## M25.10 — the calendar's years and months layers (2026-11-26)

The owner's tenth task: "Year" must show years, "Month" the twelve months of the picked year. Done in
`src/app.rs` and `src/i18n.rs`; no schema, no archive, no originals.

- **The years layer.** The "Year" tab draws one card per year of the archive, newest first, in rows
  of four: the year, its counter, a 16:10 cover of the year's first snapshot fitted whole, a volume
  bar against the fullest year and the size, under the caps line "THE YEARS OF THE ARCHIVE" with the
  number of years · files · size. A click picks a year, a double click opens its months.
- **The months layer.** The twelve month cards that "Year" drew before moved to "Month" — counters,
  covers, volume, sizes, the "12 MONTHS OF <year>" line and the facts of the year — and the month's
  day grid left with them (the week and day layers keep their old code until M25.11). Both layers
  paint their cards through `paint_arch_card`, so they cannot drift apart.
- **Navigation and facts.** The ribbon facts name the layer ("the years of the archive", "2024");
  the arrows step by the layer's own unit — a year inside the archive's range (`year_bounds`), a
  month with files; "Today" sets today's year and month and lands on the months layer.
- **Two repairs.** `i18n::count` gained the `year` noun; and `tag_counts` now computes one threshold
  per label instead of twice per frame — the pinned threshold of M25.6 and the cloud both call it, and
  the calendar measurement caught the double cost (8.82 ms before, 4.63 after).

Tests: `the_years_layer_draws_the_archive_years_and_drills_on_a_double_click` (three years newest
first, a click picks, a double click opens the months), the reworked
`the_month_layer_draws_twelve_months_with_counts_and_sizes`,
`the_picked_month_and_day_are_framed_in_the_accent` (a year card, a month card, a day cell),
`an_empty_year_offers_the_nearest_month_with_files` and `english_translates_the_calendar`. `cargo
test` 323 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

Measured on the corpus (2232 files, 10 years 2016–2026, 1240×840 headless frames, release): the years
layer p50 4.63 ms (216 fps), p95 4.91; the months layer p50 4.69 ms (213 fps), p95 5.17; the oldest
card at y 436, so every card fits the window; the grid scroll re-measured in the same pass p50
5.72 ms (175 fps), p95 6.14 — against M22.5's 5.56 / 5.85 and M22.2b's 6.4–6.7, so the M25 panel
rework costs the frame nothing once the double threshold is gone.

**Documents.** `docs/UI.md` § Interaction (the calendar), § Screens (the calendar), § Metrics (the
calendar cell), § States (an empty year); `docs/ARCHITECTURE.md` § Measurements; `plan/I18N.md`; this
file and `plan/BACKLOG.md` count M25.10 done and M25.11 next.

## M25.11 — the calendar's week and day layers (2026-11-26)

The owner's eleventh task: "Week" must show the weeks of the month, "Day" the seven days of the
week. Done in `src/app.rs` and `src/i18n.rs`; no schema, no archive, no originals.

- **The weeks layer.** `weeks_of_month` returns the Mondays of the 4–6 weeks that touch the picked
  month (the first may start in the previous month); each tile carries `week_range` ("12–18 May",
  "29 Apr – 5 May" across months), the counter, the size, the first snapshot as the cover and a
  volume bar against the fullest week, painted through the same `paint_arch_card` as the year and
  month cards. A click picks the week, a double click opens its days.
- **The days layer.** Seven tiles Monday–Sunday of the picked week — the weekday, the date, the
  counter, up to three thumbnails and the volume bar. A click (or a double click, which ends the
  drill-down) puts that day's photos into the list below; the old `draw_cell` day grid is gone, and
  the day strip is now drawn under every layer.
- **Facts and navigation.** The ribbon facts name the layer ("May 2024", "12–18 May"); the arrows
  step by a week and by a day, and "Today" keeps the layer it is on.

Tests: `the_week_tiles_cover_the_month_exactly_once` (May 2024 five weeks, February 2024 five,
January 2023 six; every day of the month lies in exactly one tile) and
`the_day_layer_shows_the_week_and_opens_a_day_below` (Mon…Sun and 29.04/01.05/05.05, a click on the
1st puts its two photos below), plus the week/day parts of `english_translates_the_calendar`.
`cargo test` 325 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors.

Measured on the corpus (2232 files, 1240×840 headless frames, release): the weeks layer p50 4.44 ms
(225 fps), p95 4.58; the days layer p50 4.61 ms (217 fps), p95 4.74. All four calendar layers now
cost the same 4.4–4.6 ms.

**Documents.** `docs/UI.md` § Interaction (the calendar), § Screens (the calendar), § Metrics (the
calendar cell); `docs/ARCHITECTURE.md` § Measurements; `plan/I18N.md`; this file and
`plan/BACKLOG.md` count M25.11 done and M25.12 next.

## M25.12 — the photos of the period obey the cell view (2026-11-26)

The owner's twelfth task: the "Tile / Rows / List" switch must work on the calendar, on the photos
under the period. Done in `src/app.rs`; no schema, no archive, no originals.

- **The period block.** Under the tiles of every layer stands exactly the files of the picked period,
  drawn by `draw_files` with `cells.cell_style` — the ribbon's cell view: the year's files on the
  years layer, the month's on the months layer, the week's on the weeks layer, the day's on the days
  layer. Above them a heading in the words of a month heading ("2024", "May 2024", "12–18 May",
  "Wed, 01.05.2024") with "· N files, size". Only the day layer keeps the time in the caption
  instead of the repeated date.
- **The fixed strip is gone.** `day_strip` and the day-only `draw_cell` grid left; "No date" stays a
  collapsible group at the bottom of every layer; the calendar keeps its one `ScrollArea`, so the
  photos scroll together with the tiles. An empty period says "No files in this period.".

Tests: `the_calendar_photos_are_exactly_the_periods` (four layers × two cell views: the drawn names
are exactly the period's files, and the list draws "date · size" where the tile caption does not) and
the reworked `the_day_layer_shows_the_photos_of_the_day_in_the_cell_view` (the day in the list with
the time, the same files in the tile view, an empty day says so). `cargo test` 326 passed + 3 guard
tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0
errors.

Measured on the corpus with the photos under the tiles (2232 files, 1240×840, release):
years p50 4.62 ms (216 fps), p95 5.46; months 4.83 (207 fps); weeks 4.66 (214 fps); days 13.66 ms
(73 fps), p95 14.82 — the day layer is the heaviest because its period can hold a whole busy day of
tiles, and it still stays inside the 16.7 ms of a 60 fps frame; the year layer, the acceptance's
case, costs 4.62 ms with all of the year's files below it.

**Documents.** `docs/UI.md` § Interaction (the calendar), § Screens (the calendar photos of the
period), § Metrics, § States (the drill-down); `docs/ARCHITECTURE.md` § Measurements; `plan/I18N.md`;
this file and `plan/BACKLOG.md` count M25.12 done and M25.13 next.

## M25.13 — "Text on the photo" moves where OCR is expected (2026-11-26)

The owner's last task of the M25 review: the left panel stops carrying a feature used once a month.
Done in `src/app.rs`, `src/i18n.rs` and `docs/UI.md`; no schema, no archive, no originals.

- **The panel loses the block.** `ocr_block` is gone from the left panel; the scroll now goes tag
  cloud → years → model block → (the file card in a narrow window).
- **A new pane.** `SettingsTab::Text` ("Text on photos", the eighth tab, the text-fields icon) holds
  the candidate rule, the `tesseract` row (path, version, "without it"), "Check and save the
  programs", the recognized counts and "Recognize documents (N)" / "Resume (N)" / "Stop", with one
  honest line when tesseract does not start. `tesseract` left "External tools", which keeps
  `ffmpeg`/`ffprobe`; both panes share `App::tool_row`.
- **The card button stays**, and the OCR progress row with its Stop is untouched.
- **"By text" points at the pane** when nothing is recognized: `no recognized text yet — run it in
  Settings → Text on photos`.

Tests: the new pane is checked in `english_translates_the_settings_panes` (its labels, the honest
line, `tesseract` no longer in the external tools) and `english_translates_the_left_panel` now lists
"Recognize documents" among the words that must not be in the panel. `cargo test` 326 passed + 3
guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .`
0 errors. The pass is the same `start_ocr(None)` call; it was not run on the owner's archive from
this session (that would write OCR rows to his DB), so the "357 candidates from the pane" is the
unchanged candidate query, and the corpus pass stays the owner's check.

**Documents.** `docs/UI.md` § Layout (the left panel), § Screens (the settings dialog), § States (no
tesseract); `plan/I18N.md`; this file and `plan/BACKLOG.md` close the M25 review.

## M24.4 — the Windows release and the folder picker on a Windows disk (2026-11-26)

The owner asked for a portable Windows build and then reported what a build cannot fix: in the
folder picker he could not reach another disk — it offered only `C:` — and the dialog felt foreign on
Windows. Done in `src/app.rs`, `src/db.rs`, `Cargo.toml` and the documents; nothing touches the
archive or the schema.

- **The build.** `x86_64-pc-windows-msvc`, linked on the Linux machine with `cargo xwin`, `clang-cl`
  and `lld-link` against the xwin CRT/SDK. ONNX Runtime is statically linked into the 58 MB exe;
  `DirectML.dll` and the four VC++ runtime DLLs travel in the folder. `tokenizers` is taken without
  `esaxx_fast`: its C++ object forces `/MT` while ONNX Runtime is `/MD`, and the linker refuses the
  mix — the pure-Rust suffix array is used instead (ADR 0050).
- **The portable folder.** `/home/alx/rdp-share/photocat`: the exe, the DLLs, a Russian
  `ПРОЧТИ-МЕНЯ.txt`, `photocat.cmd` and `photocat-gl.cmd` (the OpenGL fallback). The launcher sets
  `PHOTOCAT_CACHE` to `data\`, a new override in `db::cache_dir`, so the database, previews, faces
  and models stay next to the binary; the GUI also frees its console (`FreeConsole`) once it starts.
- **The picker.** An editable "path to a folder" field with "Go" (Enter works), and on Windows a row
  of drive buttons from `GetLogicalDrives`. The parent of a drive root is `None`, so before this a
  Windows user was locked to the one disk the picker opened on; "not a folder" is said in the dialog
  instead of failing silently. The built-in picker stays — no system dialog and no new dependency.

The first build of the picker had a defect the owner saw at once: the dialog asked for
"available width − 90" and, because a modal sizes itself to its content, it grew by its own padding
on every frame — with the mouse moving, an endless widening. The field and the dialog now have fixed
widths (560 / 440), the list fills its column and the rows wrap; a source guard
(`the_folder_picker_never_uses_the_available_width`) fails if `available_width` comes back into the
picker.

Checks: `cargo test` 329 passed + 3 guard tests (including
`the_folder_picker_lists_folders_and_navigates`, `the_folder_picker_keeps_its_width_while_the_mouse_moves`
and the source guard), `cargo clippy --all-targets` clean, `check.sh` 55
checks, `cargo test --test source_language` 3. The exe under Wine: `--help`, `doctor`, `settings`,
`index` of three images (3/3, 0 errors), `stats`, and the GUI with `WGPU_BACKEND=gl` — D3D12 shader
compilation is a Wine limitation, not a Windows one; the portable launcher wrote `data\photocat.db`.

**Documents.** `docs/DECISIONS.md` ADR 0050; `docs/ARCHITECTURE.md` § Cross-platform; `docs/UI.md`
§ Screens (the folder picker); `plan/I18N.md`; this file and `plan/BACKLOG.md` § M24.4.

## M26.12 (part 1) — the Lumix MJPEG video plays in the built-in player (2026-11-26)

The owner's reason was exact: `photocat.exe play "D:\...\P1050016.MOV" --decode-only` printed
`no decoder for Jpeg: no decoder found for codec mjpeg`. The file is a Panasonic Lumix Motion JPEG
in a QuickTime container with PCM sound (`sowt`); the rff family shipped an MJPEG codec
(`rff-codec-jpeg`) and a PCM one (`rff-codec-pcm`), but neither was a dependency.

- **Dependencies.** `rff-codec-jpeg` and `rff-codec-pcm` (both Apache-2.0) are registered in
  `player::codec_registry`; ADR 0031 has the rows and what we would do without them (the Lumix
  videos go to the external player, the sound is lost).
- **The PTS fallback.** The MJPEG decoder is a still-picture decoder and returns frames without a
  PTS; `video_loop` now takes the packet's PTS when the frame has none — otherwise every frame
  would land at 0 s. Harmless for H.264/HEVC, which carry their own.
- **Checked.** A generated MJPEG/`yuvj420p` MOV with PCM `sowt` (the Lumix layout): Linux 90 frames
  in 2.97 s (30.3 fps), 27.8 fps with sound; the Windows exe under Wine 90 frames in 2.99 s
  (30.1 fps), the last PTS 2.97 s; `the_registry_holds_the_codecs_of_the_archive` holds H.264, HEVC,
  AAC, JPEG and PCM. The Windows release is rebuilt (59 MB) and copied to the shared folder.
- **Still open in M26.12:** the automatic `ffplay`/system-player launch must go — the viewer will
  name the reason and offer the external player only behind an explicit button. The owner's Lumix
  files no longer take that path, because the built-in player opens them now.

**Documents.** `docs/DECISIONS.md` ADR 0031; `docs/ARCHITECTURE.md` § the player's codecs; `AGENTS.md`
§8; `plan/BACKLOG.md` § M26.12.

## M26.12 (part 2) — the built-in player is the only player; an external one is a press (2026-11-26)

The automatic `ffplay`/system-player launch is gone. On `Player::open`'s error `start_video_playback`
starts nothing: it keeps the reason in `App::video_error` and the viewer's frame area becomes a card
on the `card` tone — a warning glyph and "The built-in player cannot open this video" at 14 px, the
file name in mono 11, the reason (the container or codec from the real error) and three worded
actions: "Copy the reason", "Play with ffplay in a separate window" (offered only when
`tools::find_ffplay` finds a sibling of `ffmpeg` or one in `PATH`) and "Open in the system player",
each with the hint "the video leaves the app — a separate window". ▶ in the playback bar retries the
built-in player. A new `Icon::Warning` glyph joins the set (the inventory test holds it).

Tests: `a_refused_video_explains_itself_and_starts_nothing` (a video row with a missing file: the
error is remembered, `video_child` is `None`, the reason is the real one and the card's three words
are drawn) and `ffplay_is_found_next_to_ffmpeg`; the registry test from part 1 still holds. All
of `cargo test` 332 passed + 3 guard tests, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`cargo test --test source_language` 3. The Windows release is rebuilt and copied to the share; under
Wine the error path starts no process and the Lumix MJPEG still plays at 30.1 fps.

**Documents.** `docs/UI.md` § Screens (the refusal card), `docs/ARCHITECTURE.md` § Video playback,
`AGENTS.md` §8, `plan/I18N.md`, `plan/BACKLOG.md` § M26.12.
