# M22 — the desktop UI specification: two themes, the mock-up layout, screen by screen

*Русская версия: [M22-ui-spec.ru.md](M22-ui-spec.ru.md).*

Source: [`docs/references/mockups/`](../docs/references/mockups/README.md) — the owner's mock-up set
(Stitch export), committed on 2026-11-24. It holds seven HTML screens, two design-system files and one
screenshot per screen. This document says what of it is adopted, what is refused and in what order the
work goes. It is the source of truth for M22; `docs/UI.md` keeps the normative rules of the window.

## 1. What the mock-up set contains

| File | Screen | Detailed layout |
|---|---|---|
| `functional_precision_desktop/DESIGN.md` | dark palette, type scale, spacing, components | — (design system) |
| `functional_precision_light/DESIGN.md` | light palette, the same components | — (design system) |
| `photocat_1/code.html` | **main screen**: top strip, search, left panel, month grid, inspector, settings dialog | yes |
| `photocat_2/code.html` | **calendar, month layer**: week grid with thumbnails, day list | yes |
| `photocat_12/code.html` | **calendar, year layer**: 12 months with counts, month list below | yes |
| `photocat_3/code.html` | **albums**: card grid, filters, sorting, album selection bar | yes |
| `photocat_5/code.html`, `photocat_6/code.html` | **full-screen viewer**: header, zoom/tools row, series strip, player controls, inspector | yes (two variants) |
| `photocat_4/code.html` | an older shell (top navigation, settings dialog with autostart/GPU/backup) | partial — see §5 |

The mock-up is a **visual** reference. It also shows functions photocat does not have and will not
have (see §5); a missing function is not "a layout detail" and is not implemented silently.

## 2. Adopted as normative

### 2.1 Tokens and two themes

One `Theme` struct in `src/app.rs` is the only source of colour. Two palettes: `DARK` (the main one)
and `LIGHT`. `settings.theme` is `dark | light | system`; `system` follows the OS and is re-read every
frame, so a change of the system theme needs no restart. `apply_theme` fills `egui::Visuals` from the
palette and sets the radius scale; the screens never hard-code a colour except text over a darkened
frame (there the background is a photograph, not the theme).

| Token | Dark | Light | Means |
|---|---|---|---|
| `canvas` | `#0c0e12` | `#f8fafc` | window and grid ground |
| `panel` | `#1a1c20` | `#f1f5f9` | panels, top and bottom strips, caption band |
| `card` | `#1e2024` | `#ffffff` | cards, popups, selected nav row |
| `field` | `#282a2e` | `#e2e8f0` | idle buttons, checkbox wells |
| `border` | `#2b2e35` | `#e2e8f0` | 1 px lines between panels and rows |
| `border_strong` | `#373c4a` | `#cbd5e1` | splitters, active boundaries |
| `text` | `#e2e2e8` | `#0f172a` | names, counters, headings |
| `text_med` | `#a7aab3` | `#475569` | labels, EXIF keys, secondary rows |
| `text_dim` | `#7b808c` | `#64748b` | placeholders, hints, disabled |
| `accent` | `#f59e0b` | `#d97706` | selected, confident tag, keep, progress |
| `accent_hover` | `#d97706` | `#b45309` | hover/pressed of the accent |
| `on_accent` | `#111317` | `#ffffff` | text on the accent fill |
| `accent_soft` | amber 15 % | `#fef3c7` | selection fill, active row, chips |
| `danger` | `#ef4444` | `#dc2626` | "To trash", "Empty the trash" only |

Where the two `DESIGN.md` files disagree with the HTML screens (the dark front-matter is a warm
Material scheme, the rendered dark screens are cool graphite and the prose says "cool industrial
grays"), the **rendered screens win for dark** and the **light `DESIGN.md` wins for light**. The
reason is written in ADR 0044.

### 2.2 Shape, lines and depth

- The radii of the rendered mock-up screens (Tailwind `rounded` / `rounded-md` / `rounded-lg`):
  **4 px** for buttons, chips, badges and progress tracks; **6 px** for navigation rows and menus;
  **8 px** for tile cards, fields and dialogs. No fourth radius, no shadows, no gradients, no blur.
- Depth is a tonal step plus a 1 px line (`hairline`). Panel boundaries are lines, not gaps.
- The focus ring is a 1 px accent border; the selection is a 2 px accent inset line plus a square
  flag — visible without hovering.

### 2.3 Typography

- Scale: **10 / 11 / 12 / 13 / 14 / 18**. 10 px is allowed only for caps section labels and
  mono-numeric values; 18 px is the view title; 13 px is the dense body.
- Two families: the UI family (proportional) and the numeric family (monospace). Every number with a
  unit, every counter, date, size, ratio and duration is monospace (egui's built-in monospace: no font
  file is bundled, see §5 question 2).
- Section labels are `label-caps`: 10 px, uppercase, `text_dim`.

### 2.4 Layout metrics

- Top strip **32 px** (one row: brand, search mode, query, Find/Reset, state, Settings). The third
  row exists only for pass progress.
- Bottom strip **28 px**: the last fact on the left, counters and the selection on the right, mono.
- Left panel **260 px** (fixed), right panel **280 px** (fixed, hidden below 1200 px).
- Grid: **three cell views**, `settings.cell_style`, the segment "Tile / Rows / List" in the
  content panel header. **Tile** (default, the mock-up) — one width from the "Tiles" slider
  (100–260 px, 150 by default), the columns share the width, a **4:3 frame** + a **22 px** caption
  band, the preview fitted inside whole (no crop, no stretch). **Rows** — the row height from the
  slider (never below 70 % of it), the row fills the width, the tile's width follows its frame's
  aspect and the preview fills the tile (no margins). **List** — a row 44 px with a 58×40 thumbnail,
  the name, the date and the size. A file without dimensions gets 4:3.
- **Grid step 10 px** (the mock-up's `gap-2.5`) between tiles and between rows; month groups are
  separated by 24 px.
- Dialogs: settings 760×620, export and picker as now.

### 2.5 Main screen (the first task)

- Brand "photocat" + "family archive" in the top strip; the working folder and "Index / Refresh /
  Folders" move into the left panel — the top strip stays one row.
- Left panel: `CATALOG` block → navigation (two groups, numbers on the right, the active row has a
  2 px accent edge and the card tone) → `FILTER` block → view context → model and text blocks →
  pinned summary.
- Content: view title 18 px + facts 12 px mono; on the right "Tiles" slider, "By months / Flat",
  "Sort"; a chip row that never disappears; the month heading 14 px + facts 11 px mono, pinned at
  the top edge of the grid area while its group is on screen (M22.4).
- Tile: card frame 1 px, preview, caption band 22 px with the file name (mono 10, left) and the date
  (mono 10, `text_dim`, right). A video keeps the `▶ M:SS` strip. Selected/keep/picked — accent.
- Inspector: title `FILE DETAILS`, preview, name, path, `label — value` rows, albums, AI tags, my
  tags, OCR text — same block order as now, in the new tokens.

## 3. Screens: what is drawn by a mock-up and what is not

| Screen | Mock-up | What is adopted | Task |
|---|---|---|---|
| Grid (main) | `photocat_1` | fully: strips, panels, header, tile card, caption band, years, tag chips, scrolling | M22.2 ✅, M22.2a ✅, M22.2b ✅, M22.3 ✅, M22.4 ✅ |
| Calendar, month | `photocat_2` | month/week grid, day list, mono counters, amber selection | M22.5 ✅ |
| Calendar, year | `photocat_12` | 4×3 months with a count and a size, drill-down | M22.5 ✅ |
| Albums | `photocat_3` | album card look, counters, selection bar; the **tree stays** (see §5 q. 7) | M22.6 ✅ |
| Full-screen viewer | `photocat_5/6` | header facts, tools row, series strip, player row, inspector inside the viewer | M22.7 ✅ |
| Duplicates | — | no mock-up: same strips, header, chips row, `draw_cell`, amber "keep", red-free everywhere | M22.9 |
| Faces | — | same panels, face tile metrics unchanged, accent only on a picked face | M22.9 |
| Trash | — | same, `danger` only on "Empty the trash" | M22.9 |
| Integrity | — | form fields and report in mono, one accent button, no new colour | M22.9 |
| Settings | `photocat_4` (partial) | the section list and the row rhythm; **the content stays ours**, plus "Interface → theme" | M22.8 |
| Dialogs (folders, picker, export, storage, album) | `photocat_4` (settings only) | card tone, 1 px border, 8 px radius, mono numbers | M22.10 |

**Rule for a screen without a mock-up.** A new or unfinished screen is built from the same parts and
in the same order: top strip (nothing), view title + facts, chips row, content, status strip. It uses
`draw_cell` for files, the same `caps_label`/`num`/`hairline` helpers, the same palette, the same
panel widths and the same word for the same notion. One accent per view: the main action. Red is
allowed only for the OS-trash path.

## 4. What changes in the behaviour documents

- `docs/UI.md` § Layout: the theme paragraph, the type scale, the accent, the tile caption, the
  strips; § Metrics: the caption band, the top and bottom strips; § "What the interface will not
  have": "theme settings" is removed, "a third theme / accent picker" stays forbidden.
- `docs/DECISIONS.md` ADR 0044: two themes and the token palette, the radius scale, amber accent, mono
  numbers, fonts not bundled.
- `plan/STATUS.md`: M22.1/M22.2 done, the rest queued; `plan/BACKLOG.md`: the M22 block.
- `plan/I18N.md`: the new labels (theme names, "Interface theme", panel captions) are covered.

## 5. Disputed questions for the owner (with a recommendation and the answer)

1. **Amber instead of blue.** The mock-up's only signal is amber `#f59e0b`; the code and `docs/UI.md`
   have used blue `rgb(90,170,255)` since M16. Adopted amber in this change (it is the mock-up's
   core), and a second colour `danger` for the OS-trash path. *Recommendation: keep amber; if blue
   feels more "ours", say so and it is one line in `Theme`.* **Answer: amber.**
2. **Fonts.** The mock-up uses Inter and JetBrains Mono. Bundling them means two `.ttf` files
   (OFL licence, ~1.5 MB, a line in `NOTICE`) and `FontDefinitions` setup; not bundling means the
   egui default proportional family and its built-in monospace. *Recommendation: do not bundle until
   the owner has looked at the built-in fonts; the difference is visible only at 10–11 px.*
   **Answer: no external dependency; the built-in fonts, the binary must not grow.**
3. **Radii.** The design-system prose says strictly 0 px, but every rendered screen uses
   `rounded`/`rounded-md`/`rounded-lg`. **Owner's answer: the mock-up has rounding — restore it.**
   Adopted 4 / 6 / 8 px by role (§2.2), a fourth radius is forbidden.
4. **Previews: square fit vs 4:3 crop.** The mock-up crops previews to 4:3. Cropping hides the edges
   of a snapshot and contradicts "the preview fits entirely". *Recommendation: keep the fit; the grid
   looks denser with the mock-up's gap, not by cropping.* **Answer: keep the fit; the tile view is
   the mock-up's (a 4:3 frame with the snapshot fitted whole), the proportion rows stay as a second
   view, and a list view is added — §2.4.**
5. **Functions the mock-up shows that do not exist and are refused.** Favourites and star ratings,
   "Share", permanent "Delete", an archive quota (`64.8 / 70.0 GB`), RAW/CR2 with 14-bit colour,
   camera/lens EXIF beyond what is indexed (camera name only), histogram and colour profile, GPU/NPU
   acceleration, autostart/tray, automatic update checks, "queue of operations", per-folder grid
   grouping. None of these are in `docs/UI.md` and none is promised by a label. Each one that the
   owner really wants becomes its own task (a DB column, a view, a measurement).
   *Recommendation: refuse all of them for M22; favourites are the only one worth a later task.*
   **Answer: agree, refuse.**
6. **Sticky month heading.** The mock-up has it; it is `docs/UI.md` debt item 7 and was planned as
   M13e. Folded into M22.4. *Recommendation: do it in M22.4, separately from the main screen.*
7. **Albums: cards vs tree.** The mock-up shows a card grid; our album view is a tree with an order,
   subalbums and "Album order" sorting (M19). The tree carries information the cards cannot (nesting,
   order, trashed counts). *Recommendation: keep the tree, give the open album the card look for its
   subalbum tiles. A card-only album screen needs an owner's decision.* **Answer: the nested tree
   stays.**
8. **Favourites.** `photocat_1` has "Favorites 128" in the navigation and a star in the inspector.
   There is no `files.favorite` column. *Recommendation: a separate task after M22: one column, a
   nav item and a filter chip; no new screen.* **Answer: yes, a later task.**
9. **Left panel budget.** The mock-up adds a folder list, a year list, a storage meter and tag chips
   with counts. Today the panel already scrolls on a 840 px window. The year list and tag chips are
   cheap and useful; the folder list duplicates the "Folders" dialog; the quota is refused (q. 5).
   *Recommendation: add years and tag chips (M22.3), drop the folder tree and the quota.*
10. **Status strip vs floating selection bar.** The mock-up floats "Selected: 1 · 18.4 MB" above the
    grid with five icon buttons. We keep the selection in the chips row and the status strip
    (documented, one place for actions). *Recommendation: keep ours; a floating bar covers
    photographs and duplicates the chips row.* **Answer: keep ours for now, look again later.**
11. **Icons.** The mock-up is Material Symbols throughout. We have a geometric `paint_icon` for the
    player only and forbid emoji. *Recommendation: keep words + the existing geometric glyphs; an
    icon set is a separate decision (font dependency).*

## 6. Tasks

An item is done only when it is measured or looked at (`docs/UI.md` § Check) and the documents are
in the same commit.

### M22.1 — theme foundation ✅ (this change)

**Goal.** One palette source, two themes, a choice in the settings.
1. `db::ThemeMode {Dark, Light, System}` in `settings.theme` (key/value, `SCHEMA_VERSION` unchanged).
2. `app::Theme` with the tokens of §2.1, `DARK`/`LIGHT`, `apply_theme`, `theme()`/`accent()`/`dim()`.
3. Radius 4/6/8 px, no shadows, the type scale and the mono/caps helpers in `apply_theme`.
4. "Settings → Interface" has "Interface theme" (three options), saved at once; `system` follows the OS.
5. Tests: the palette switches, the radius is zero, the settings roundtrip.

**Files.** `src/db.rs`, `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `docs/DECISIONS.md`, `plan/`.

### M22.2 — the main screen ✅ (this change)

1. Top strip 32 px: brand + subtitle, search mode, query, Find/Reset, state, "Folders (N)",
   "Details", "Settings". The folder field and "Index / Refresh" move to the left panel.
2. Bottom strip 28 px: the last fact on the left, file counters and the selection on the right, mono.
3. Left panel: `CATALOG` block, navigation with a 2 px accent edge on the active row, `FILTER` block,
   view context, pinned summary; separators are 1 px hairlines.
4. Content: canvas background, title 18 px + facts 12 px mono, the group-by/sort controls, chips row.
5. Three cell views in the header ("Tile" by default, "Rows", "List"): the tile is an 8 px card
   with a 1 px frame, a 4:3 frame and a 22 px caption band (name + date, mono 10), the preview fitted
   inside whole; rows fill the tile with the frame's own aspect; the list is a 44 px row. Gap 10 px,
   the `▶ M:SS` strip, accent selection with a square check flag.
6. Previews follow the EXIF orientation (`load_oriented`), the DB stores the displayed dimensions,
   and a stale preview is overwritten when its dimensions do not match (ADR 0045, `SCHEMA_VERSION` 8).
7. Month heading 14 px + 11 px mono facts.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/STATUS.md`.

### M22.2a — the list view scales with the size slider ✅ (this change)

The owner's remark after the main screen: the size slider must **not disappear** in the list view.
Hiding it made the panel header jump (the row layout changed on every switch) and the list has nothing
to scale — yet a list of 1976 rows wants a larger thumbnail and a larger name just as much as a grid.

1. The "Tiles" slider is always visible in the content panel header, in all three cell views; the
   header keeps its height and the right-hand group does not shift when the view changes.
2. In the list view the slider scales the row: row height `24 + 0.16 × value` px (≈40 px at 100,
   ≈66 px at 260), the thumbnail `row − 8` high in 4:3, the name 12–14 px and the mono meta 10–12 px
   proportionally; the row count per screen changes, the columns do not.
3. The selected value is the same `settings.tile_size` as for the tile and rows views (one size, three
   interpretations), so switching views keeps the owner's scale.
4. Checked at 100, 150, 200 and 260: no clipped text inside a row, the header does not jump, the
   tooltip of the slider says what it scales in the current view.
5. `docs/UI.md` § Metrics and the list row follow; a headless frame checks the row height for two
   slider values.

**Result.** `draw_list` takes its metrics from `list_row_height` / `list_name_px` / `list_meta_px`
(one clamp 100–260 px, the same `settings.tile_size` as the other two views). At 100 a row is 40 px
with a 42.7×32 thumbnail, the name at 12 px and the mono meta at 10 px; at 260 — 66 px, 76.8×57.6,
14 px and 12 px. The two text lines are centred as one block (`name_top = centre − name − 1`, the
meta 2 px below), the kind/duration sits on the name's top line and the keep/restore button is
centred, so nothing is clipped at either end of the range. The header is one `<div>`-like block with
a constant height: the slider, its label and the three view buttons are drawn in every view, and only
the tooltip changes (`tile_hint` has one sentence per view). Two tests: `список_рисует_строки_выше_с_большим_ползунком`
runs `draw_list` in a headless frame with two rows — 90 px at 100 and 141 px at 260 (the 6 px frame
cursor is constant, the difference is exactly 2 × 25.6); `ползунок_размера_виден_во_всех_трёх_видах`
asserts the label "Плитки 200" is drawn in the tile, rows and list views.

**Files.** `src/app.rs` (`draw_list`, the header), `docs/UI.md`, `plan/STATUS.md`.

### M22.2b — scrolling with a trackpad and the wheel ✅ (this change)

The owner's remark: on a trackpad the grid is heavy to scroll, and the wheel is slow. Today the scroll
animation is `1400 pts/s`, `0.08–0.24 s`, and the wheel delta is applied as-is; a precision trackpad
sends many small deltas that the animation damps into a crawl, and a notch of the wheel moves a few
dozen pixels in a 2000-row grid.

1. A wheel notch moves a third of a screen (a constant in `apply_theme`/`run`, not a per-screen value);
   a trackpad two-finger swipe moves proportionally to the gesture and keeps kinetic smoothing after
   the fingers stop.
2. The smoothing profile is tuned once for both: acceleration and decay params in one place, so a short
   swipe glides ~200–300 px and a long one reaches the end without repeated swipes. No jump on
   direction change.
3. Same behaviour in every scrollable region (grid, left panel, card, dialogs) — one `ScrollArea`
   configuration, no per-view overrides.
4. Measured on the corpus: a full-height wheel pass over the 2230-file grid does not stall below
   60 fps, and a trackpad swipe produces one continuous motion without stutter (owner's look, plus a
   `repaint_after` check that idle frames are not requested while not scrolling).
5. `docs/UI.md` § Interaction gets the numbers; the debt entry about progress blinking (item 2) is
   unaffected.

**Result.** One profile, in two places: `apply_theme` sets `line_scroll_speed` = 280 px (a third of
the 840 px default window), `scroll_animation` 2400 pts/s (0.10–0.30 s) and `kinetic.decay_time` =
0.35 s; `Scroll` (`src/app.rs`) does the trackpad half. winit reports `MouseWheelSource::Unknown` on
Linux and Windows, so egui gives a precision trackpad no inertia at all: the profile estimates the
gesture velocity from point deltas (60 Hz bound, smoothed by 50 ms), and once the fingers are gone for
80 ms it keeps adding `v · dt` to `smooth_scroll_delta` with exponential decay until the remaining
path is under 6 px. A new touch, the opposite direction or a wheel notch drops the coast in the same
frame. No `ScrollArea` sets its own multiplier or scroll source (a source-scan test forbids it), so
the grid, the left panel, the card and the dialogs move the same way.

**Measured** (a copy of the real DB, 2232 files, release): a trackpad flick of 1200 px/s coasts
274 px in 81 frames (≈1.35 s); a wheel notch is 280 px; 160 frames of continuous scrolling over the
grid — p50 6.4–6.7 ms (149–157 fps), p95 6.6–6.9 ms (145–152 fps), p99 6.7–7.4 ms over six runs;
idle frames request nothing.

**Files.** `src/app.rs` (`apply_theme`, `Scroll`, `App::ui`), `docs/UI.md`, `plan/STATUS.md`.

### M22.3 — left panel: years and tag chips ✅ (this change)

1. A `YEARS` block: the five newest years with counts, a click sets the path/date filter, "all years"
   clears it; mono counts; a year with zero files is not drawn.
2. Tag chips: 20 px chips, mono 11, `text_med`, the count after a middle dot, the selected chip in
   accent; the percent threshold slider next to them; the row does not jump when a tag is picked.
3. Panel budget: brand, catalog block, 7 nav items, filter block and years fit into 840 px without
   scrolling when nothing is selected. Verified with a headless frame at 840 px.
4. "Summary" stays pinned at the bottom.

**Result.** The years open the scrolling part of the panel, right under the pinned filter block: a
20 px cell in **two columns of three rows** — mono 11 on the left, the count in mono 10 at the right
edge, the picked year on the accent fill and in the accent text — and "All years" as the first cell
only while a year is picked, so the panel does not spend a row on a filter that is already empty and
six cells are still three rows: the block never changes height, not even on a click. A single column
of five rows costs 130 px and pushed the model block out of the panel on the default 840 px window,
which is why the list is two columns wide. A click writes the year into the date filter (`query`, the
same field the filter block shows), which also drops a semantic search and the text snippets: a year
is a date, not words; a second click on the picked year clears it. The block is drawn on the grid and
the calendar only — the duplicates, faces, trash and integrity screens have a context of their own
and the date filter does not drive them. Counts come from `taken_at`; five rows at most, a year with
zero files does not exist. The rows of the pinned "Summary" went from 26 to 16 px (`stat_row` has no
button, and `horizontal` otherwise assumes an interactive row): four rows cost 48 px less, which is
where the years live. Measured with a headless frame at 1240×840: the scrolling part starts at 560 px,
the years end at 680 px and the pinned "Summary" starts at 692 px — every year is seen without a
scroll and stands 12 px above the summary, and the same frame proves the summary does not move when a
year is picked (`the_left_panel_fits_the_years_above_the_summary`).
The tag menu became chips: the word with the count of snapshots after a middle dot, 4 px apart,
wrapping, idle `field` with `text_med`, picked — accent fill, accent 1 px border and accent text, the
width following the text and never the state (`tag_chip`). The row therefore keeps every position
when a tag is picked (`the_tag_chips_do_not_jump_when_a_tag_is_picked` compares the words and their
places of two frames); the percent slider sits right under it and the model block warns "no tag
passes the threshold" as before.

**Files.** `src/app.rs`, `src/i18n.rs`, `docs/UI.md`, `plan/STATUS.md`.

### M22.4 — month headings pinned while scrolling (folds in M13e) ✅ (this change)

1. The current month heading stays at the top of the grid area while its group is on screen; the
   next group's heading pushes it out.
2. No nested `ScrollArea`; the heading is drawn over the grid area with the canvas background.
3. A headless test: after scrolling by one group's height, the heading text is still drawn.
4. `docs/UI.md` debt item 7 is closed.

**Result.** Each frame the grid records where every group heading landed (`MonthHead`: the top, the
height, the key, the counts); after the one `ScrollArea` returns its viewport, `draw_sticky_month`
picks the last heading above the top edge, paints a canvas-toned rect over the tiles and draws the
same row (`month_labels` + `month_row`, shared with the group heading, so the pinned row and the
natural one cannot drift apart) in a child UI that does not advance the grid's cursor. The row slides
up out of the viewport as the next heading approaches: `y = min(top, next_top − height)`, and it is
not drawn at all before the group's own heading has gone above the edge. "Flat" and a semantic search
have no group headings, so nothing is pinned there.

Two tests, both headless. `the_month_heading_stays_pinned_while_its_group_is_on_screen` runs the whole
window at 1240×840 over two months of 25 snapshots with the wheel turned over the grid (egui smooths a
notch over ~10 frames, so the clock has to run): after one notch the caption of the top tile has moved
yet exactly one "May 2024" is drawn, at the same y as before the scroll, and after four more notches
"April 2024" stands there and "May 2024" is gone. `the_next_month_heading_pushes_the_pinned_one_out`
draws the row alone with synthetic headings: with the next heading far below it sits on the top edge,
with a 6 px overlap it moves up by exactly 6 px.

**Measured** (the real DB, 2232 files, release): the group heading "октябрь 2026" is drawn at y 132 on
the first screen and at y 132 after a wheel notch — pinned at the top edge of the grid area (the
owner's settings keep "Flat", so the bench switches to "By months" itself); the 160-frame scrolling
measurement of M22.2b repeats it: p50 6.9–7.0 ms, p95 8.9–9.6 ms, p99 11.2–13.7 ms against p50
6.77 ms on the code before the change — one rect and one text row per frame are below the noise.

**Files.** `src/app.rs` (`month_labels`, `month_row`, `MonthHead`, `draw_sticky_month`, the grid loop
and the benchmark), `docs/UI.md`, `plan/`.

### M22.5 — calendar (month, year) ✅ (this change)

1. Year: 4×3 month cells, each with a count and a size (mono), a selected month in accent, a double
   click drills down.
2. Month: the week grid Mon–Sun, up to three thumbnails per day, the day list under it with time,
   name and size; the selected day has a 1 px accent frame; the counters are mono.
3. Week and day layers: the same tokens; the day layer uses `draw_cell`.
4. Empty month: "No files this month" + "Go to YYYY-MM"; arrows skip empty months.

**Result.** One helper pair now owns the calendar's look: `cal_cell` paints a cell (the card tone and a
1 px line when it holds files, the `panel` tone when it is empty, the `accent_soft` fill and a 1 px
accent frame when it is picked) and returns the palette for the number, the counter and the bar inside
it; `cal_bar` draws the volume share of the busiest month or day. The old per-cell `faint_bg_color`,
`from_gray(190/150)` and the blue `rgb(90,170,255)` 2 px strokes are gone from the calendar. **Year**:
a caps line "12 MONTHS OF 2024" with the facts of the year in mono 10 on the right, then 4×3 cards of
92 px and a 6 px gap (`CAL_GAP`): the mono number and the month's name, the counter at the right edge,
a 16:10 cover of the month's first snapshot fitted whole, the volume bar and the size. **Month**: the
caps weekday row, then cells of `CAL_DAY_H` = 96 px — the day number in mono 11, the counter as a badge
(accent on `accent_soft`), up to three thumbnails and the bar against the busiest day of the month.
Empty days keep the panel tone, so the month reads as a grid and the gaps of the archive are seen. A
click selects a day or a month, a double click goes one layer down; the day list under the upper
layers is the shared `draw_list` forced to the list view with `Cells::time_only`, so a row shows
"18:27 · 1.6 MiB" instead of repeating the same date on every line (`caption_date`), and the heading
is "Wed, 01.05.2024" at 14 px with the facts in mono (`day_heading`, `day_title`). On the "Day" layer
the owner's cell view is kept (`draw_cell`), the caption showing the time. An empty month leads to the
nearest month with files (`nearest_file_month` was dead code since M21.4b and is used now); the arrows
still skip empty months. The title facts name the layer (a year, a month, a week or a day).

Four tests, all headless: `the_year_layer_draws_twelve_months_with_counts_and_sizes` (the caps line,
the year's facts, all month names, ten empty zeros and the sizes),
`the_picked_month_and_day_are_framed_in_the_accent` (exactly one accent-stroked 1 px rect in the
content area with a month picked and one with a day picked, none with nothing picked — a stroke scan
of the frame), `the_day_strip_shows_the_time_name_and_size` ("09:10 · 2.0 KiB", the names, the day
heading, no repeated date) and `an_empty_month_offers_the_nearest_month_with_files` (backwards first,
then forwards).

**Measured** (the real DB, 2232 files, release, headless frames at 1240×840): year layer p50 5.56 ms
(180 fps), p95 5.65; month layer p50 5.85 ms (171 fps), p95 6.26 — well inside the 16.7 ms of 60 fps.
The whole year layer fits the window (December's card at y 442); a month grid of five weeks is 510 px,
so its day list sits below the fold and is reached by the same scroll as the grid.

**Files.** `src/app.rs` (`cal_cell`, `cal_bar`, `day_title`, `day_heading`, `day_strip`, `caption_date`,
`Cells::time_only`, the four layers), `src/i18n.rs`, `docs/UI.md`, `docs/ARCHITECTURE.md`, `plan/`.

### M22.6 — albums ✅ (this change)

1. The tree keeps its metrics; the open album gets the card header (name 14 px, "N files · M
   subalbums · size" mono 11) and the same action row.
2. Subalbum tiles get the card look of the mock-up (canvas cover area, name, counts), one tile model.
3. The selection bar is the chips row; `Album order` keeps its place.
4. No new screen: "Album cards instead of a tree" is an owner's decision (q. 7).

**Result.** The view heading is "Albums" with the count of albums; the open album's header
(`album_header`) carries the name at 14 px and "N files · M sub-albums · size" in mono 11 plus " · N in
the trash", above the unchanged action row, and "+ New album" became the view's accent button. A
sub-album card (`draw_album_cell`, `album_card_size`) is the width of the "Tiles" slider, a 16:10
`canvas` cover with the sub-album's first snapshot fitted whole (a folder mark when it has none), the
name at 13 px clipped to the card, "N files" and the size in mono 10, the card tone with a 1 px border
and the accent line under the cursor — 105 px at 100, 205 px at 260, one model for every sub-album. The
cards take one row, two at most, the rest scrolls, and the snapshot grid keeps 220 px: measured before
the cap, eight cards at 150 px pushed the grid out of a 840 px window. The album chip stays in the chips
row (no floating bar) and "Album order" stays in the Sort menu. Measured on a copy of the real DB (2232
files, release): the Albums view p50 6.94–7.51 ms (133–144 fps), p95 7.18–7.99 ms. Four headless tests;
`cargo test` 254, `clippy` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors.

### M22.7 — full-screen viewer ✅ (this change)

1. Header: name 14 px mono details (size, `W×H`), position in accent, "Back to the grid".
2. The tools row (zoom, 1:1, fit, rotate, inspector toggle) with words and hints; no icon-only path.
3. The series strip under the frame: neighbouring snapshots with the date and the position.
4. The face chips over the frame (a name and a percentage) — only when the faces are computed.
5. The player row keeps its geometric icons (`paint_icon`) and its hints.

**Result.** The viewer is the mock-up's `photocat_5/6` inside the same modal: the header ("Back to the grid", the name at 14 px, the date and the facts in mono 11, "N of M" in the
accent at the right) over the tools row, where every tool is a word with a hint ("Zoom out" · the
percentage in mono · "Zoom in" · "Fit" · "1:1" · "Rotate" · "Text on the photo" · "In the system
viewer" · "Inspector"): no icon-only path. "1:1" (`one_to_one`) is the scale that shows one image
pixel per screen pixel, so the zoom ceiling went from 400 % to `MAX_ZOOM` 800 %; "Rotate" (`paint_frame`, `rotated_size`) turns the *view* by 90° through a
rotated mesh — the file on disk, the DB and the preview are untouched, and the next file opens
unturned. Under the frame the series strip (`viewer_series`, `draw_series_strip`) draws the neighbours
of the same day: the caps label, the day in mono 11 and "· N files", then a window of up to sixteen
64×48 plates with the preview fitted whole, the position inside the day in mono 10 and a 2 px accent
frame on the current one; a click jumps there (`viewer_goto`). A window, not a scroll area: the strip
is computed around the current file, so the one-scroll-per-region rule holds. The face chips
(`paint_face_chips`, `db::load_face_boxes`) mark every face the model found with a 1.5 px accent box
and a dark plate above it ("Lisa (92%)"), read once per file and turned together with the view. The
"Inspector" toggle — or the I key — opens the same `file_card` in a 280 px right column, so the full
screen has a second caller and no second card. The playback row keeps its geometric icons, seek and
volume. Measured on the real DB (2232 files, release, headless frames at 1240×840) with sixteen
neighbours in the strip and one face on the frame: p50 6.91–8.65 ms (116–145 fps), p95
7.73–10.45 ms; with the inspector on p50 6.89–7.89 ms (127–145 fps), p95 9.47–11.46 ms over four
runs. Computing the series on every frame cost p50 9.66 ms, so it is computed on a file change now. Seven
headless tests (the series of the day and its absence without a date, the turn's geometry, the turn
not following the next file, the strip in a frame, the face chip, the inspector's card, the English
labels); `cargo test` 261, `clippy` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors.

**Files.** `src/app.rs`, `src/db.rs`, `src/i18n.rs`, `docs/UI.md`, `docs/ARCHITECTURE.md`, `plan/`.

### M22.8 — settings dialog

1. The section list in caps 10 px, the active row with the accent edge; fields up to 520 px.
2. The groups of the mock-up that correspond to real functions stay in their panes; the ones that do
   not exist (autostart, tray, GPU, updates, metadata backup) are **not** added.
3. `Interface`: language + theme; the hint about what the theme does not change.

### M22.9 — screens without a mock-up

1. Duplicates, faces, trash, integrity: the header, the chips row, the panel widths, the mono numbers
   and the accent are the same as on the main screen; there is no local colour.
2. `danger` only on "Empty the trash"; `keep`/`spare` keep their words.
3. Each screen gets one headless frame test that checks the labels are unchanged.

### M22.10 — dialogs and reports

1. Folders, picker, export, storage and album dialogs: card tone, 1 px border, 8 px radius, mono
   numbers, one accent button.
2. Report bodies stay monospace 11 px with a copy button.

### M22.11 — light theme pass

1. Every screen is looked at in both themes: contrast of `text_med`/`text_dim` on `panel`/`card` ≥ 4.5
   for body text, ≥ 3.0 for dim hints.
2. The preview area does not tint a photograph: the light canvas is neutral, no warm cast.
3. The dark theme stays the default; the choice survives a restart.

## 7. Not doing in M22

- A third theme, an accent picker, a "compact mode".
- Bundled fonts until the owner's decision (q. 2).
- Cropped 4:3 previews, a floating selection bar, an icon font.
- Any function from §5 q. 5; any network call; any file operation.
