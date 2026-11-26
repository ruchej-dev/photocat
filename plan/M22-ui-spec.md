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
| `accent` | `#f59e0b` | `#b45309` | selected, confident tag, keep, progress |
| `accent_hover` | `#d97706` | `#92400e` | hover/pressed of the accent |
| `on_accent` | `#111317` | `#ffffff` | text on the accent fill |
| `accent_soft` | amber 15 % | `#fef3c7` | selection fill, active row, chips |
| `danger` | `#f45c5c` | `#ba1a1a` | "To trash", "Empty the trash" only |

Where the two `DESIGN.md` files disagree with the HTML screens (the dark front-matter is a warm
Material scheme, the rendered dark screens are cool graphite and the prose says "cool industrial
grays"), the **rendered screens win for dark** and the **light `DESIGN.md` wins for light**. The
reason is written in ADR 0044.

The light amber is the design system's own `amber-focus` `#b45309`, not the drawn screen's `#d97706`:
the latter reads 2.9:1 on the panel and 3.2:1 on white, below AA under the words it colours. The dark
red and the light red were moved a step for the same reason (M22.11; the numbers are in §6).

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

- Top strip **32 px** (one row: the mark, one search field with the mode switch, the query,
  "Search" and "×", then the "Tiles" pill, the state, the details toggle and "Settings" —
  M22.19). The third row exists only for pass progress.
- Bottom strip **28 px**: the last fact on the left, counters and the selection on the right, mono.
- Left panel **260 px** (fixed), right panel **280 px** (fixed, hidden below 1200 px).
- Grid: **three cell views**, `settings.cell_style`, the segment "Tile / Rows / List" in the
  content panel header. **Tile** (default, the mock-up) — one width from the "Tiles" slider
  (100–260 px, 150 by default), the columns share the width, a **4:3 frame** + a **22 px** caption
  band, the preview covering the whole frame (a centring crop, M22.15) on a `field` well, the card
  body on `card` and the caption on `panel` (M22.14). **Rows** — the row height from the slider (never below
  70 % of it), the row fills the width, the tile's width follows its frame's aspect and the preview
  fills the tile (no margins). **List** — a row 44 px with a 58×40 thumbnail, the name, the date and
  the size. A file without dimensions gets 4:3.
- **Grid step 10 px** (the mock-up's `gap-2.5`) between tiles and between rows; month groups are
  separated by 24 px.
- Dialogs: settings 760×620 of content, export and picker as now; every dialog is a card — the `card`
  tone, a 1 px `border`, the 8 px radius and 16 px of air — with one 14 px title and one accent button
  at most (M22.10).

### 2.5 Main screen (the first task)

- The mark (a photo-library glyph and "photocat") in the top strip; the working folder, "Index /
  Refresh" and the folders dialog move into the left panel — the top strip stays one row.
- Left panel: `CATALOG` block → navigation (two groups, numbers on the right, the active row has a
  2 px accent edge and the card tone) → `FILTER` block → view context → model and text blocks →
  pinned summary.
- Content (M22.19, after `photocat_1`): one 44 px ribbon — the view title 18 px and the facts
  12 px mono on the left, the controls (the cell views, "By months / Flat", "Sort") on the right,
  divided by 1 × 16 px lines; the tile size is the pill of the top strip. A chip row with "×" and
  "Reset" is drawn only while a filter is on; the month heading 14 px + facts 11 px mono is pinned
  at the top edge of the grid area while its group is on screen (M22.4). While anything is picked,
  the floating selection bar lies at the bottom edge of the grid.
- Tile: card frame 1 px, preview on the `field` well, caption band 22 px with the file name (mono 10,
  left) and the date (mono 10, `text_dim`, right). A video keeps the `▶ M:SS` strip.
  Selected/keep/picked — accent.
- Inspector (M22.14, the header and the air of `photocat_12` in M22.16): a 44 px header band on
  the `card` tone with a 1 px line under it — the info glyph, "File details" at 12 px strong and
  the mock-up's "×" at the right (the way back is the top strip's details toggle, its hint
  "Details" / "Hide details", M22.19) — then the card
  on 12 px of air; the preview as a card (4:3, the `canvas` well, the 8 px radius, a 1 px `border`,
  all four corners cut, the corner badge "▶ M:SS" or the format); the identity row — the name in
  the numeric family in the accent and the size on the right; the path in mono 11 `text_dim`;
  "Open full screen" and "To trash" as 20 px buttons in one row; a `card_block` "Shooting params"
  with the camera, the two columns "Date and time" / "Resolution", "Location" with a pin and the
  checksum; "Albums" as chips with a folder glyph; "AI tags" as a caps row with `CLIP <weight>%` on
  the right and chips "tag 30% ×"; "My tags" as chips with the own-tag row framed by the mock-up's
  "`#`" and a 22 px bare "+" since M22.18; "Text on the photo" with the engine name on the right.
  A chip has two targets since M22.17: the word filters the grid by the tag (a second click clears the filter, and the accent
  marks the tags the photos are filtered by, not "the confident one") and the "×" removes the tag
  from the photo; the word of the album chip opens the album. The weights and percentiles stay in
  the chip hints; the exposure badges and the place name of the mock-up are not built (the index
  keeps the camera name and no geocoder is allowed offline, §5 q. 5). The mock-up's round action
  buttons over the preview keep their words (docs/UI.md § Layout: an icon is never the only path).
  The album section is always drawn and carries the mock-up's dashed "+ Add" since M22.20: the
  function it promised (adding a photo to an album from the card) was built then, as its own dialog
  with the selection bar as the second entrance.

## 3. Screens: what is drawn by a mock-up and what is not

| Screen | Mock-up | What is adopted | Task |
|---|---|---|---|
| Grid (main) | `photocat_1` | fully: strips, panels, header, tile card, caption band, years, tag chips, scrolling; the top strip, the one-row ribbon and the floating selection bar | M22.2 ✅, M22.2a ✅, M22.2b ✅, M22.3 ✅, M22.4 ✅, M22.19 ✅ |
| Calendar, month | `photocat_2` | month/week grid, day list, mono counters, amber selection | M22.5 ✅ |
| Calendar, year | `photocat_12` | 4×3 months with a count and a size, drill-down; the inspector of the screen is the right panel of M22.16 | M22.5 ✅, M22.16 ✅ |
| Albums | `photocat_3` | the card grid at the root, the control row (filter, order, cards/list, the primary "+"), the counters over the cover, "size · date", the breadcrumb; the **tree stays inside an album** (§5 q. 7, revised in M22.20); the cover is a mosaic of up to four photos, each filling its cell (M22.21, cropped from M22.29); the mock-up's floating album bar is not built — the selection bar carries "Add to an album…" | M22.6 ✅, M22.20 ✅, M22.21 ✅, M22.29 ✅ |
| Full-screen viewer | `photocat_5/6` | header facts, tools row, series strip, player row, inspector inside the viewer | M22.7 ✅ |
| Duplicates | — | no mock-up: same strips, header, `card_block` per group, `draw_cell`, amber "keep", red-free everywhere | M22.9 ✅ |
| Faces | — | same panels, face tile metrics unchanged, caption in mono 10, accent only on a picked face | M22.9 ✅ |
| Trash | — | same, `danger` only on "Empty the trash" | M22.9 ✅ |
| Integrity | — | the content panel's form: path fields in mono, the report in mono, one accent button, no new colour | M22.9 ✅ |
| Settings | `photocat_4` (partial) | the section list (caps heading, icons, the accent edge of the active row, the mono value at the right) and the row rhythm; **the content stays ours**, plus "Interface → theme" | M22.8 ✅ |
| Dialogs (folders, picker, export, storage, album) | `photocat_4` (settings only) | card tone, 1 px border, 8 px radius, mono numbers | M22.10 ✅ |

**Rule for a screen without a mock-up.** A new or unfinished screen is built from the same parts and
in the same order: top strip (nothing), view title + facts, chips row, content, status strip. It uses
`draw_cell` for files, `card_block` for a group of pictures, the same `caps_label`/`num`/`hairline`
helpers, the same palette, the same panel widths and the same word for the same notion. One accent
per view: the main action. Red is allowed only for the OS-trash path. The chips row belongs to the
views that filter (grid, calendar, albums): duplicates, faces and trash apply no filter, and a row
saying "no filters" over a list that cannot be filtered would be a false statement — their own row of
choice (the picked people, the trash counter with its two actions) stands where the chips row stands.

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
   view, and a list view is added — §2.4.** The owner reversed this in M22.15: the tile fills the
   frame (the mock-up's crop), every other preview stays fitted.
5. **Functions the mock-up shows that do not exist and are refused.** Favourites and star ratings,
   "Share", permanent "Delete", an archive quota (`64.8 / 70.0 GB`), RAW/CR2 with 14-bit colour,
   camera/lens EXIF beyond what is indexed (camera name only), histogram and colour profile, GPU/NPU
   acceleration, autostart/tray, automatic update checks, "queue of operations", per-folder grid
   grouping. None of these are in `docs/UI.md` and none is promised by a label. Each one that the
   owner really wants becomes its own task (a DB column, a view, a measurement).
   *Recommendation: refuse all of them for M22; favourites are the only one worth a later task.*
   **Answer: agree, refuse.** The album "+ Add" chip of q. 5 / §2.4 is not on this list: it was a
   missing function, and M22.20 built it.
6. **Sticky month heading.** The mock-up has it; it is `docs/UI.md` debt item 7 and was planned as
   M13e. Folded into M22.4. *Recommendation: do it in M22.4, separately from the main screen.*
7. **Albums: cards vs tree.** The mock-up shows a card grid; our album view is a tree with an order,
   subalbums and "Album order" sorting (M19). The tree carries information the cards cannot (nesting,
   order, trashed counts). *Recommendation: keep the tree, give the open album the card look for its
   subalbum tiles. A card-only album screen needs an owner's decision.* **Answer: the nested tree
   stays.** **M22.20 (the owner's "make the album layout of `photocat_3` and add the way to put
   photos into albums"):** the tree stays inside an album, where the nesting matters; the root of
   the albums is the card grid of the mock-up, because there the tree only repeats the cards.
   Navigation keeps its cost: a card opens an album, the breadcrumb "All albums › …" opens any
   level, one press per level.
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
    **Reversed in M22.19:** the owner asked for the mock-up's bar; it floats at the bottom edge of
    the grid with "N selected · size", "Select all (M)" and "Clear selection" only — no actions of
    its own, so nothing is duplicated, and the chip row is left to the filters.
11. **Icons.** The mock-up is Material Symbols throughout. We have a geometric `paint_icon` for the
    player only and forbid emoji. *Recommendation: keep words + the existing geometric glyphs; an
    icon set is a separate decision (font dependency).* **Answer (2026-11-25): take the mock-up's
    icons — as geometry, next to the word.** Every action button becomes one `WordButton` with a
    14 px glyph before its word; no icon font and no image file (ADR 0048), so §5 q. 2 stands and
    the binary does not grow. Done in M22.8 for every button of the window.

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

1. Top strip 32 px (M22.19): the mark, one search field with the mode switch, the query,
   "Search"/"×", the "Tiles" pill, the state, the details toggle and "Settings";
   "Details", "Settings". The folder field and "Index / Refresh" move to the left panel.
2. Bottom strip 28 px: the last fact on the left, file counters and the selection on the right, mono.
3. Left panel: `CATALOG` block, navigation with a 2 px accent edge on the active row, `FILTER` block,
   view context, pinned summary; separators are 1 px hairlines.
4. Content: canvas background, title 18 px + facts 12 px mono, the group-by/sort controls, chips row.
5. Three cell views in the header ("Tile" by default, "Rows", "List"): the tile is an 8 px card
   with a 1 px frame, a 4:3 frame and a 22 px caption band (name + date, mono 10), the preview
   covering the frame (a centring crop, M22.15); rows fill the tile with the frame's own aspect; the
   list is a 44 px row. Gap 10 px,
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
20 px cell in **three columns of two rows** (M22.19; the old two columns of three left the fifth
year under the pinned summary at 840 px) — mono 11 on the left, the count in mono 10 at the right
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

### M22.8 — settings dialog, and the icons of the mock-up ✅ (this change)

1. The section list in caps 10 px, the active row with the accent edge; fields up to 520 px.
2. The groups of the mock-up that correspond to real functions stay in their panes; the ones that do
   not exist (autostart, tray, GPU, updates, metadata backup) are **not** added.
3. `Interface`: language + theme; the hint about what the theme does not change.

**Result.** The section list now speaks the mock-up's `photocat_4`: a caps 10 px line "SETTINGS
SECTIONS" over seven rows of 28 px — a 14 px icon, the 13 px label, and a mono 10 value at the right
edge that gives the state before a click (the interface language, the model's dimensions, the files
in the index, the enabled tags, the face threshold, `2/3` of the external programs). The active row
sits on the `accent_soft` fill with a **2 px accent edge** on the left; the value is dropped when it
would touch the word (the widest label — the Russian "Индексация и дубли" — keeps its room). The
groups the mock-up shows but photocat does not have (autostart, tray, GPU, backup) are not added; the
Interface pane keeps the language and the theme with the hint that the theme changes neither files
nor the database (M22.1). **The icons are taken from the mock-up for every button of the window**, as
the owner asked: `paint_icon` grew from the five player signs to 62 glyphs of the Material Symbols
family by name, and every action button became one `WordButton` — a 14 px glyph and the word, the
accent for the one main action, the danger border for the OS-trash path, a 20 px compact variant
for a strip switch. 97 `ui.button`/`Button::new` call sites went through it; the four kinds of 20 px
target that cannot hold both (per-tile `keep`/`restore`/`pick`/`not him`, the "×" of a chip, the
album tree's triangle, the "×" of an album row) keep their word alone, and menus keep words (an egui
menu button takes text or a texture; a texture is a second pipeline — ADR 0048). Seven tests:
every glyph paints inside its box, the contact sheet covers the enum, a word button puts the icon
before the word, the settings list is caps + icons + the accent edge, the section list holds both
languages, the content controls keep their own row, and a source scan fails on a button built
without an icon.

The icons also pushed the controls of the content panel into the title: right-aligned, they began at
x 288 on a 1240 px window with the details panel open and ran over "All files · 2232 files". The
heading now has a row of its own (title + facts + the "Tiles" slider on the right) and the controls
sit on a second row of 20 px buttons — the three cell views, "By months / Flat", "Sort" — which wraps
instead of clipping, and the group-by switch uses `add_enabled` rather than `add_enabled_ui`, because
a child UI reports its size only after it is drawn and the wrapped row could not see it. The height is
constant across views, so M22.2a holds.

**Measured** (the real DB, 2232 files, release, 1240×840 frames): the settings dialog p50 6.89–8.01
ms (125–145 fps), p95 ≤ 8.13 ms across the seven sections — the tag dictionary is the heaviest pane;
the section list's caps line sits at y 168. The grid pass of M22.2b repeats over four runs: p50
6.73–6.84 ms (146–149 fps), p95 ≤ 7.18 ms, one noisy run at p95 8.50 — against 6.4–6.7 ms before the
icons, below the noise. `cargo test` 268, `clippy` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics` 0 errors.

**Files.** `src/app.rs` (`Icon`, `paint_icon`, `WordButton`, `settings_nav_item`, `SettingsTab::icon`,
the button sweep), `src/i18n.rs`, `docs/UI.md`, `docs/DECISIONS.md` (ADR 0048), `plan/`.

### M22.9 — screens without a mock-up ✅ (this change)

1. Duplicates, faces, trash, integrity: the header, the chips row, the panel widths, the mono numbers
   and the accent are the same as on the main screen; there is no local colour.
2. `danger` only on "Empty the trash"; `keep`/`spare` keep their words.
3. Each screen gets one headless frame test that checks the labels are unchanged.

**Result.** The four screens the mock-up has no layout for now follow the rule of §3 literally. A
duplicate group is a `card_block` — the `card` tone, a 1 px `border`, an 8 px radius, 8 px inside —
instead of egui's `Frame::group`: the row is "#N" in mono 11, the reason at 13 px strong and the facts
"N files · X spare" in mono 11 at the right edge, the advice at 11 px in `text_dim`, and the tiles lie
on the card tone (the previews' corner cuts used to be painted `faint_bg_color`, which happened to be
the same tone; now `theme(ui).card`, so the two cannot drift). `draw_face_cell` was the last place with
a hard-coded colour: the old blue `rgb(90,170,255)` on a picked face and on the selected file, the
`extreme_bg_color` well, a 3 px radius and grey 190/150 captions in a proportional 11 px font. Now the
tile is a `card` with the 8 px radius, the caption is mono 10 — the date `text_med` on the left, the
detection score `text_dim` on the right, exactly like a file tile — and both frames are the 2 px
accent; the metrics 150×150 + 26 are untouched. The group's name became a 14 px subheading with its
facts in mono 11 and the "Photos with everyone together" heading dropped from 18 px to 14 px, so the
18 px title is the view's alone. In the trash the counter reads in mono 11 `text_dim` and "Empty the
trash" changed from `primary` to `danger`: the accent means "selected" and "keep", while the one path
that changes the disk is the one red word of the window (docs/UI.md § Layout). The integrity screen
grew the content panel's frame (canvas tone, a 12/6 margin), so its title stands where the grid's title
stands; its facts are mono 12, its three path fields take the numeric family, and "Compare the copies"
stays the only accent fill of the form. `highlight_job` — the OCR match in the card and the search
snippet — lost the same old blue (a defect left from M22.5) and paints the theme's accent.

**Checks.** Four headless frame tests, one per screen, each asserting the labels the screen had and the
tokens it now uses: `the_duplicates_screen_borrows_the_parts_of_the_main_one` ("#1" and the facts in
mono 11, the group's card fill across the content panel, no `danger` anywhere, and the whole screen on
the palette), `the_faces_screen_takes_its_caption_from_the_palette` (the date and the score in mono 10
with their tokens, the 14 px name, the 2 px accent frame on a picked face, the palette only),
`the_trash_paints_only_its_emptying_in_danger` ("Empty the trash (1)" in `danger`, "Restore all"
neutral, the counter in mono 11), `the_integrity_screen_takes_its_fields_and_its_one_accent` (the title
next to the grid's, the facts in mono 12, the path in the numeric family, exactly one accent fill). The
palette assertion walks every drawn word with its font and colour (a galley may leave the colour
unset, so the frame's fallback and override are resolved) and every fill and stroke of the frame; text
on a dimmed photograph keeps the documented grey-235/210 exception, and none of the four screens is
red.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, the real DB of 2232 files, 2498 faces, release,
headless frames at 1240×840, 40 frames per screen): duplicates p50 5.61–5.71 ms (175–178 fps), p95 ≤
7.21; faces p50 8.07–8.49 ms (118–124 fps), p95 ≤ 10.63 — the heaviest of the four, because every face
tile decodes its crop within the 8-per-frame budget; trash p50 5.08–5.13 ms (195–197 fps), p95 ≤ 6.96;
integrity p50 4.77 ms (210 fps), p95 ≤ 7.29. `cargo test` 272 passed (268 + 4), `cargo clippy
--all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors.

**Files.** `src/app.rs` (`card_block`, `View::Dupes`, `draw_face_cell`, `View::Faces`, `View::Trash`,
`integrity_panel`, `highlight_job`, the four tests and the measurement), `docs/UI.md`, `docs/ARCHITECTURE.md`, `plan/`.

### M22.10 — dialogs and reports ✅ (this change)

1. Folders, picker, export, storage and album dialogs: card tone, 1 px border, 8 px radius, mono
   numbers, one accent button.
2. Report bodies stay monospace 11 px with a copy button.

**Result.** A modal used to be `Frame::popup` — the frame of a menu: the `menu_corner_radius` of
6 px, 6 px of air and the `menu_margin`, while the spec has a dialog at 8 px. One `dialog_frame`
now gives every dialog of the window the card: the `card` tone, a 1 px `border` line, the 8 px
radius and 16 px of air inside (M22.10). It is used by the folder list, the picker, the three
storage confirmations, the album dialog, the export dialog and the trash confirmation — and by the
settings dialog, whose content keeps its 760×620. The 16 px titles of the dialogs were outside the
type scale (10/11/12/13/14/18): a dialog's title is a 14 px subheading now, the settings pane's
heading too, so the whole window lives on the scale.

A number inside a translated sentence cannot be put into the numeric family, so the dialogs that
had numbers grew a facts row of their own, exactly like the album header (M22.6): the folder list
counts the indexed files of each root ("2 files" in mono 11, counted once when the index or the
folder list changes, not per frame), the trash confirmation reads "1 file · 18.4 MiB" in mono 11
above the sentence "Will be moved to the OS trash.", the album delete shows the names, then
"2 albums · 2 files" in mono 11, then the hint that the rows go and the files stay, and the export
dialog's progress ("export: 3/8") is mono 11. Each of the five action dialogs has exactly one
accent button: "Export N files" in the export dialog was a plain button and is the main action now,
together with "Index all", "Add and index", the cleaning confirmation and the album's confirm. The
trash confirmation is the one dialog whose button touches the disk: it wears `danger` and no accent
(M22.9). The export report was a 13 px proportional paragraph and is a mono 11 body in its own
scroll area with the copy button, as the integrity report already was. Along the way two buttons of
the export dialog ("Close", "Copy the report") were written past `t()` — the last English words in
the Russian window.

**Checks.** Three headless tests. `every_dialog_is_a_card_with_one_accent_button` walks all seven
dialogs after the fade-in (the palette arrives with a multiplied alpha while the modal animates):
the frame is the card tone with a 1 px `border` and the 8 px radius, exactly one accent button in
the five action dialogs, none in the settings dialog (every change is saved at once) and none in the
trash confirmation, which carries the `danger` line instead; no 16 px word is left, and every drawn
word, fill and line is a token of the palette. `the_dialogs_put_their_numbers_in_the_numeric_family`
holds "2 files", "2 albums · 2 files", "1 file · 1 B", "export: 3/8" and the report body in mono 11.
`the_export_dialog_speaks_the_chosen_language` fails on the old code with "Close" in the Russian
window.

**Measured** (`docs/ARCHITECTURE.md` § Measurements, a copy of the real DB of 2232 files, an album
with 500 files so that the export dialog draws a 500-line plan, release, 1240×840 headless frames,
16 frames per dialog after four warm-up frames): folder list p50 0.03 ms, p95 0.04; picker p50
0.08 ms; storage 0.02 ms; album delete 0.02 ms; export 0.18 ms, p95 0.32; trash 0.02 ms; settings
p50 0.06 ms, p95 0.08 — a dialog costs a fraction of the 16.7 ms of a 60 fps frame, and the folder
list cost 0.84 ms per frame before its counters were counted on a reload. `cargo test` 275 passed
(272 + 3), `cargo clippy --all-targets` clean, `check.sh` 55 checks, `rust-analyzer diagnostics .`
0 errors.

**Files.** `src/app.rs` (`dialog_frame`, the six modals, `album_delete_facts`, `root_counts`,
`WordButton::danger` in the trash confirmation, the export report body, the three tests and the
measurement), `src/i18n.rs`, `docs/UI.md`, `docs/ARCHITECTURE.md`, `plan/`.

### M22.11 — light theme pass ✅

1. Every screen is looked at in both themes: contrast of `text_med`/`text_dim` on `panel`/`card` ≥ 4.5
   for body text, ≥ 3.0 for dim hints.
2. The preview area does not tint a photograph: the light canvas is neutral, no warm cast.
3. The dark theme stays the default; the choice survives a restart.

**Done.** The pass found two real failures and no screen that wrote a colour of its own. (1) The light
`accent` `#d97706` read 2.91:1 on the panel and 2.86:1 on the selection fill, yet it colours the active
navigation row, the picked chip, the picked year and the confident tag — all body text. It became the
light design system's own `amber-focus` `#b45309` (4.58:1 on the panel, 5.02:1 on the card, 4.51:1 on
the selection fill); `accent_hover` followed to `#92400e`. The active row of the left panel also changed
from the `field` tone to the `card` tone with a 2 px accent edge — the tone §2.5 asks for, the mark the
settings section list already had, and 5.02:1 under the accent in light against 4.07:1 before. (2) The
danger word read 4.34:1 on the dark card and 4.41:1 on the light panel: the dark red became `#f45c5c`
(5.07:1) and the light one the design system's error `#ba1a1a` (5.90–6.46:1).

**Checked.** `both_themes_pass_the_contrast_ratio` computes the WCAG 2.1 ratio of every pair the
screens actually paint — body ≥ 4.5, dim hints ≥ 3.0, the accent on canvas/panel/card/selection, the
danger word on its three grounds, the text on the idle and hovered accent fill. `the_light_ground_is_
neutral_and_the_tile_frame_is_the_canvas` keeps the light ground cool (`r ≤ g ≤ b`) and asserts the tile
frame is the canvas token in both themes, so the letterbox around a photograph is the neutral ground.
`every_screen_paints_only_the_tokens_of_its_theme` renders the grid, the four calendar layers, the
albums and the full screen in both palettes and fails on any word, fill or line that is not a token;
the four screens without a mock-up and all seven dialogs do the same in their own tests, in both themes.
A fresh install still opens dark (`db::Settings::default`), and the choice survives a restart — the
settings row was already there.

**Measured** (WCAG 2.1; `dark` selection composited, amber 15 % over the card):

| Pair | Dark | Light |
|---|---|---|
| `text` on `canvas` / `panel` / `card` | 14.97 / 13.22 / 12.64 | 17.06 / 16.30 / 17.85 |
| `text_med` on `canvas` / `panel` / `card` | 8.32 / 7.35 / 7.03 | 7.24 / 6.92 / 7.58 |
| `text_dim` on `canvas` / `panel` / `card` | 4.88 / 4.31 / 4.12 | 4.55 / 4.34 / 4.76 |
| `accent` on `panel` / `card` / selection | 7.94 / 7.60 / 4.59 | 4.58 / 5.02 / 4.51 |
| `danger` on `panel` / `card` | 5.31 / 5.07 | 5.90 / 6.46 |
| `on_accent` on `accent` / `accent_hover` | 8.66 / 5.84 | 5.02 / 7.09 |

**Files.** `src/app.rs` (`LIGHT`, `DARK`, `nav_item`, `palette`, `contrast`, `over`, `window_parts_in`,
`assert_palette`, the three tests), `docs/UI.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, `plan/`.

### M22.12 — the right panel keeps its width ✅

The owner's review of the finished block: the card of the inspector left its 280 px. Every caption
lost its first letters on the left ("ата" instead of "Дата"), the preview and "Добавить тег" were cut
on the right, and the first status line of a Russian window spoke English.

1. No row of the card may be wider than the panel: a long field takes the width the word button
   leaves (140 px reserved for the button, at least 60 px for the field). A row that does not fit
   widens the panel, and egui then remembers the wider size, so the defect outlives the file that
   caused it.
2. The load path builds the first status line from the settings it has read, not from the previous
   frame's language.
3. A headless frame of the whole window with a selected file asserts that no painted rectangle
   leaves the window and that the panel title keeps its left edge.

**Result.** The field of an own tag shrinks with the panel; the button ends inside the card. The
first status line follows `settings.lang` ("в индексе 2232 файла, 21.2 ГиБ"). The new
`the_file_card_fits_the_right_panel` fails on the old code with a frame at x 1253.5 in a 1240 px
window — the 13.5 px seen on screen — and passes now. Measured (the real DB, headless frames): the
farthest painted edge is 1240.0 at 1240 and 1691.0 at 1691; nothing is painted outside the window.
`cargo test` 279, `clippy` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors.

**Files.** `src/app.rs` (`file_card`, the load path, `collect_text_rects`, `collect_rect_bounds`,
`window_rects_at`, the test), `docs/UI.md`, `docs/ARCHITECTURE.md`, `plan/`.

### M22.13 — every action button carries an icon ✅

The owner's question after M22.12: "all icons placed?" No: the guard
`every_button_of_the_window_has_an_icon` looked for `ui.button(` and missed a button chained as
`ui\n.button(…)`. Six action buttons were word-only: "Check now" and "Remind me later" (integrity
banner), "Open full screen" and "Show the full text" (file card), "restore hidden tags (N)" and
"Go to <month> <year>" (calendar).

1. Every one is a `WordButton` (`Verified`, `Close`, `FitScreen`, `TextFields`, `Restore`,
   `CalendarToday`).
2. The one menu of the window, "Sort", is a `WordButton` with the `sort` glyph that opens its list
   through `Popup::menu`; a plain `menu_button` is forbidden.
3. A `ComboBox` is a select, not a button — the language, the model, the theme and the album parent
   keep words, as the mock-up's `<select>` does.
4. The guard fails on a chained `.button(`, on any `Button::new(` and on `ui.menu_button(`.

**Result.** The six buttons and the Sort control carry icons; the icon set is 64 glyphs. The guard
fails on the pre-M22.13 code with the six offenders. `cargo test` 279, `clippy` clean, `check.sh` 55
checks, `rust-analyzer diagnostics` 0 errors.

**Files.** `src/app.rs` (`WordButton` call sites, `Icon::Sort`, `paint_icon`, the guard),
`docs/UI.md`, `docs/DECISIONS.md` (ADR 0048 revision), `plan/`.

### M22.14 — the tile card and the file card after `photocat_1` ✅

The owner after M22.13: "the right panel does not match the mock-up; in tile mode you again made the
photographs full size, but they must be fitted into the tile".

1. **A tile is a card again.** The body takes the `card` tone, the well of the preview the `field`
   tone (the mock-up's `surface-container-highest`), the caption the `panel` tone. The preview was
   drawn on the `canvas` tone — the tone of the grid itself — so a fitted snapshot had no visible
   tile around it and read as a photograph at full size with a caption under it. The preview is
   still fitted whole: no crop, no stretch (M22.2), the fitted photo now sits on a visible well.
2. **The file card follows `photocat_1` block by block:** the info glyph before "File details"; the
   preview as a 4:3 card (the `canvas` well, the 8 px radius, a 1 px `border`, all four corners cut,
   the duration badge of a video); the identity row — the name in the numeric family in the accent,
   the size on the right; the path in mono 10; a `card_block` "Shooting params" (the camera after a
   photo glyph, "Date and time" and "Resolution" in two columns, "Location", a hairline, SHA256);
   "Albums" as chips with a count; "AI tags" as a caps row with `CLIP <weight>%` on the right and
   chips "tag 30% ×" (the confident one in the accent and a click that removed it — superseded by
   M22.17: the word filters, the "×" removes; the weight and the percentile in the hint, the "not
   sure" line, the top-3 and "all tags (N)" as before); "My tags"
   as chips with the own-tag field; "Text on the photo" with the engine name on the right. The
   exposure badges and the share/favourite/delete circles of the mock-up stay refused (§5 q. 5).
3. **A block counts its own line.** `card_block` sized its content as `available − 16` and forgot
   the 1 px `border` on both sides, so every block was 2 px wider than the panel: inside the 280 px
   right panel that pushed the panel and the window edge (M22.12's rule). The line is counted now.

**Result.** A letterboxed snapshot stands on a `field` well inside a `card` tile in both themes; the
file card of `photocat_1` is drawn with chips and caps headings; the right panel is 280 px and no
frame leaves the window. `the_light_ground_is_neutral_and_the_tile_well_is_the_field` and
`the_file_card_fits_the_right_panel` cover both; `cargo test` 279, `cargo test --test source_language`
3, `clippy` clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors.

**Files.** `src/app.rs` (`draw_cell`, `round_preview`, `card_block`, `caps_row`, `card_chip`,
`file_card`, `details_panel`, the tests), `src/i18n.rs`, `docs/UI.md`, `plan/`.

### M22.15 — the tile preview fills the tile frame ✅

The owner after M22.14: "in tile mode the photograph is shown in its own proportions and fitted
into the tile square; it must fill the whole tile, as it used to be".

1. **The tile fills its frame.** `cover_rect` scales the preview by the larger of the two ratios and
   centres it, so the picture covers the 4:3 frame edge to edge and the sides that do not fit are
   cut evenly — the proportions are kept, nothing is stretched and no margin is left (`fit_rect`
   would have left the letterbox). The painter clips the picture to the frame, so a portrait loses
   its top and bottom and a panorama its sides. This is the mock-up's own 4:3 crop that §5 q. 4 had
   refused; the owner's answer now is to follow the mock-up.
2. **Only the grid tile fills.** `draw_cell` takes a `ThumbFit` (the tile view passes `Filling`, the
   proportion rows `Whole`) and `paint_thumb` keeps fitting for the calendar, the series strip, the
   album cover and the file card — a preview is still never cropped anywhere the whole shot matters.
   The rows view keeps its fit: its tile already has the photo's proportion.
3. **The tooltip tells the truth:** the cell-view segment now reads "as in the mock-up: a 4:3
   frame, the snapshot fills it, the sides are cut" (the old text promised a fitted whole snapshot).

**Result.** A tile of the grid, the calendar's day layer, the album and the duplicates grids shows a
photograph with no `field` margins; a portrait is cut above and below, a panorama at the sides; the
field well shows only while a preview loads. `a_portrait_covers_the_tile_and_keeps_its_proportions`,
`a_panorama_covers_the_tile_by_height`, `a_square_covers_the_whole_tile`,
`a_zero_size_does_not_break_the_cover` and `the_tile_draws_the_preview_over_the_whole_frame` (a real
160×320 preview drawn into a 200×150 frame: a 200×400 picture, no margin) cover it; `cargo test`
284, `cargo test --test source_language` 3, `clippy` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics` 0 errors.

**Files.** `src/app.rs` (`cover_rect`, `ThumbFit`, `paint_thumb_as`, `paint_thumb_filling`,
`draw_cell`, `draw_tiles`, the tooltip, five tests), `src/i18n.rs`, `src/db.rs` (the `CellStyle`
doc), `docs/UI.md`, `plan/`.

### M22.16 — the right panel after `photocat_12` ✅

The owner after M22.15: "make the layout of the right panel after the mock-up" (`photocat_12`).

1. **The panel's frame.** `Panel::right` used egui's default side-panel frame — an 8 px margin on
   every side — so the 1 px line drawn by `panel_edge` stood 9 px inside the panel, with a strip of
   panel tone between the line and the grid, and the card lived on 264 px instead of 280. The panel
   now carries `Frame::NONE` with the panel tone: the line stands on its left edge and the whole
   280 px belong to the content.
2. **The header band.** The mock-up's `h-11`: 44 px on the `card` tone, a 1 px line under it, the
   info glyph, "File details" at 12 px strong and the close target at the right. The close target
   is the mock-up's "×" — a 22 px `icon_button` 12 px from the edge with the hint "free up room for
   the grid and calendar"; the way back is the top strip's details toggle with "Details"/"Hide
   details" as its hint (M22.19 moved the word into the hint), so the
   rule "an icon is never the only path" holds (docs/UI.md § Layout).
3. **The air.** The card is drawn inside a 12 px margin (`PANEL_PAD`) on every side, as the
   mock-up's `p-3.5`; the preview is 255 px wide at the 280 px panel, and the blocks start and end
   on the same column. In the full-screen inspector the same card keeps the same air.
4. **The details of the mock-up** that the window can carry: the corner badge of the preview shows
   the format of a still ("JPG", "HEIC") next to the video's "▶ M:SS"; the location row gets the
   mock-up's pin; an album chip gets the folder glyph before the name; the own-tag row becomes one
   framed field with the "`#`" of the mock-up and the "Add tag" button inside it, still never wider
   than the card (M22.12); the card's two actions take the 20 px `WordButton::small`/`compact` so
   "Open full screen" and "To trash" stay on one row in both languages.
5. **Refused in this pass** (and why): the mock-up's round icon-only actions over the preview
   (words stay, docs/UI.md § Layout), the album "+ Add" chip (a new flow: adding a file to an album
   from the card), the exposure badges and the RAW/14-bit badge (not indexed, §5 q. 5), the place
   name in the location row (needs a geocoder; the network is for models only), the 320 px panel
   width (the owner's 280 px from M22.12 stays until he asks otherwise) and the mock-up's
   `rounded-full` tag pills (the 4/6/8 radius scale).

**Result.** At 1240×840 a headless frame draws the panel's band from y 31 to y 75 on the full
279 px, the × at x 1206–1228, the preview at 973–1228 (255 px) and every block between those two
columns; no rectangle or word leaves the window. `the_file_card_keeps_the_mock_ups_band_and_air`
locks the band, the × and the 12 px air; `the_file_card_fits_the_right_panel`, `cargo test` 285,
`cargo test --test source_language` 3, `clippy` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics` 0 errors.

**Files.** `src/app.rs` (`details_panel`, `file_card`, `PANEL_HEAD_H`, `PANEL_PAD`, `card_chip`,
`Icon::Location`, the right panel's frame, one test), `docs/UI.md`, `plan/`.

### M22.17 — the word of a chip acts, the "×" removes ✅

The owner after M22.16: the tag cloud of the card highlights "the confident tag" but a click on any
other tag hides it from the photo — a click on a tag must not delete it; removal has the "×" of the
chip; the highlight must mean the tags the photos are filtered by.

1. **Two targets in one 20 px chip.** `card_chip` returns a `ChipClick` (`None`, `Body`, `Remove`)
   and knows where the "×" is: the glyph, the 4 px gap before it and 2 px around it — 16 px of the
   chip's right edge. The "×" turns `danger` under the pointer and carries the removal hint, the word
   carries the action hint; the two zones never overlap, so a click that meant "show the others" can
   not hide a tag.
2. **The word filters.** A click on the word of an AI tag or of an own tag sets `tag_filter` — the
   same filter as the left panel's tag chips — and a second click on the highlighted chip clears it.
   The accent now marks that filter and nothing else; the confident tag keeps its place in the caps
   row ("CLIP 84%") and in the line "tag: <name>".
3. **The album chip opens the album.** Its word goes to the "Albums" view with the album open (the
   same path as a click in the album tree); only the "×" removes the photo from the album.
4. **The left panel keeps its rule** — it already toggled a filter and removed nothing; the new hints
   and the split of the card's chips are noted in `docs/UI.md` § Interaction and § Metrics.

**Result.** `the_word_of_a_card_chip_acts_and_the_cross_removes` (a click at 8 px of the left edge
gives `Body`, at 8 px of the right edge `Remove`), `a_click_on_a_tag_in_the_card_filters_the_photos`
(the filter is set, the tags of the photo are untouched, a second click clears it) and
`the_word_of_an_album_chip_opens_the_album_and_the_cross_removes_the_row`; `cargo test` 288,
`cargo test --test source_language` 3, `clippy` clean, `check.sh` 55 checks, `rust-analyzer
diagnostics` 0 errors.

**Files.** `src/app.rs` (`card_chip`, `ChipClick`, the three chip blocks, three tests), `src/i18n.rs`
(three labels), `docs/UI.md`, `plan/`.

### M22.18 — the own-tag field gets its room back ✅

The owner after M22.17: the "add tag" field is short and the button is far too big — drop the word
and keep only the "+".

1. **The button is a bare "+".** The word button (140 px reserved in the row, a 118 px label) became
   an icon-only 22 px square (`icon_button` with the `add` glyph) inside the framed row; its hint is
   the word `Add tag`. The field now takes `available − 16 − 22 − 8` — about 190 px of the 255 px
   card instead of 81 px in Russian, and the hint "свой тег, например мебель" (164 px) fits whole.
2. **The row still never widens the panel** (M22.12): the reserve is counted (the "`#`", the 8 px of
   the gap, the 22 px of the button) and the field never falls below 40 px.
3. **One line, one height.** egui's text edit is styled as a button: `min_size.y` is the 26 px of
   `interact_size` and the text is aligned `LEFT_TOP`, so inside the framed row the input stuck to
   the upper edge and read as a short, crooked field (the owner's second review of the row). The row
   sets `interact_size.y = 22` and centres the text (`TextEdit::align(LEFT_CENTER)`): the "`#`", the
   tag and the "+" are one 22 px line, the row is 28 px — the mock-up's `h-7` — and the field no
   longer floats. The test compares the vertical centres of all three within 1 px.
4. **A documented exception.** `docs/UI.md` § Layout names the "+" among the targets that keep no
   word: a 22 px square with the hint, the "`#`" and the "My tags" heading around it — the owner's
   request outranks the general rule "an icon is never the only path", as it already did for the
   panel's "×".

**Result.** In Russian the hint of the field is drawn whole and the "+" starts at least 150 px after
the text — `the_own_tag_row_keeps_a_wide_field_and_a_bare_plus` fails on the 140 px reserve
(103 px) and passes now; the word "Добавить тег" is not drawn any more; the "#", the hint and the "+"
share one line (the test fails on the top-aligned text of the theme, 3.5 px off).
`the_file_card_keeps_the_mock_ups_band_and_air` still holds the "+" inside the card's 12 px air;
`cargo test` 289, `cargo test --test source_language` 3, `clippy` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics` 0 errors.

**Files.** `src/app.rs` (the own-tag row, one test), `docs/UI.md`, `plan/`.

### M22.19 — the top strip, the ribbon and the selection bar after `photocat_1` ✅

The owner after M22.18: the top part of the window and several other elements do not match the
mock-up. A screenshot of the window at 1440×875 next to the rendered `photocat_1` showed why: the
strip was a row of eight word buttons without a mark, the content had three rows where the mock-up
has one, the selection lived in the chip row, and the footer repeated the hidden "Details" word.

1. **The top strip is audited against the mock-up, element by element.** The brand gets the
   photo-library glyph in the accent; the subtitle "family archive" is dropped (the mock-up has the
   word only). The eight controls become the mock-up's four groups: one search field in the middle
   (the search glyph, the mode switch "Semantic / By text" with an arrow, the query, "Search" in the
   accent and "×" that clears both the field and the results), and on the right the "Tiles" pill,
   the state pill, the details toggle and "Settings". The search is at most 576 px and centred
   between the mark and the tools: the three sectors are measured from the words, so the centring
   holds in either language.
2. **What left the strip and where it went.** The "Tiles" slider moved from the content heading to
   the mock-up's compact pill (the value is in the hint); "Folders (N)" became the bare "+" of the
   "Catalog" heading that opens the same dialog; "Details / Hide details" became the mock-up's 28 px
   icon toggle that stays lit while the panel is open — the second icon-only target of the window
   after the own-tag "+", with the word in its hint and the panel's "×" as the way back. The facts
   ("N files · M with EXIF") moved into the hint of the state pill; the footer carries the numbers.
3. **The content panel is one ribbon of 44 px.** The title 18 px and the facts 12 px mono stand on
   the left, the controls — the three cell views, "By months / Flat", "Sort" — on the right, in the
   mock-up's order and divided by 1 × 16 px lines. The chip row is drawn only while a filter is on,
   so the common screen has exactly the mock-up's height; the "found N of M" line is gone because
   the title already says "N of M files".
4. **The selection bar floats.** The mock-up's pill at the bottom edge of the grid replaced the
   "N selected · size / Select all / Clear selection" group of the chip row: a dot, the count and
   the volume in mono 11 in the accent, then "Select all (M)" and "Clear selection". It is drawn in
   a child ui at a fixed rect on the panel's own layer, drawn after the scroll area, so it lies over
   the tiles, never moves them, and its buttons win the hit test; an `egui::Area` was tried first
   and refused — its first frame is a sizing pass that paints nothing, so a headless frame and the
   first frame of a window showed no bar.
5. **The footer follows the mock-up's line**: an accent dot, the last fact, the count of files and
   their volume on the left; on the right the selected file as "name · frame · size" (or
   "N selected · volume"). The years moved from two columns of three rows to **three columns of two
   rows**: six cells always take two rows, so the block keeps its height and the fifth year no longer
   falls under the pinned summary at 840 px.
6. **Refused in this pass** (and why): the mock-up's "Filter" menu (date range, rating, geolocation,
   person — functions the window does not have; the chips of the left panel and the search stay the
   way in), the per-month "Select all" link (the footer bar and Ctrl+A cover it), the 224 px left
   panel and the 320 px right panel (the owner's 260/280 of M22.3/M22.12 stand until he asks
   otherwise), the storage bar of the left panel (it needs a volume to measure, not an index fact)
   and the mock-up's third search mode "Name / EXIF" (the path/date field of the left panel is that
   function; one field would have to drive two states at once).

**Result.** At 1440×875 the strip draws the mark at x 12–112, the search frame at 273–860, the
"Tiles" pill at 1030–1150, the state pill, the lit toggle and "Settings" to the right edge; the
ribbon is one row from y 85 to y 129; with two picked files the bar appears at the bottom centre of
the grid (y 751–783) and its "Select all (4)" and "Clear selection" are drawn there instead of the
chip row. `the_content_controls_keep_their_own_row`, `the_size_slider_shows_in_all_three_views` (the
pill is a 1 px frame of 100–140 px in the top strip in all three cell views),
`the_chips_row_shows_the_selection_and_the_buttons` (the tests read the words of the bar),
`the_left_panel_fits_the_years_above_the_summary` and `the_picked_month_and_day_are_framed_in_the_accent`
(only frames below the strip count) cover it. A clean `git checkout` of `src/app.rs` during this
pass wiped the uncommitted M22.16–M22.18 work; it was replayed from the session log of that pass and
verified by the 289 tests of M22.18 passing again before M22.19 was re-applied.

**Files.** `src/app.rs` (`top_bar`, `search_field`, `tile_pill`, `status_bar`, the ribbon, the
"pick-bar", `years_block`, `toggle_icon_button`, `text_width`/`mono_width`, `hairline_vertical`,
four tests), `src/i18n.rs` (five labels; `found` is gone), `docs/UI.md`, `plan/`.

### M22.20 — the album layout of `photocat_3` and the way into an album ✅

The owner: "делай вёрстку альбомов и добавь механизм добавления фотографий в альбомы".

1. **The card grid is the root of the Albums view.** An album card is the mock-up's: a `card`-tone
   frame with 10 px of air, a 16:10 `canvas` cover with the first snapshot fitted whole (a preview
   is never cropped — reversed for the mosaic in M22.29), two dark plates over the cover — "N
   sub-albums" at the top left, "N files" at
   the bottom right, mono 10 on black 60 % — the name at 13 px and "size · date" in mono 10; the
   open album keeps the 2 px accent frame and the accent name, the hovered one the 1 px accent line.
   The card is 100–260 px wide by the "Tiles" slider (`album_card_size`).
2. **The control row of the screen** (under the ribbon): the filter of the album names (150 px), the
   "Sort" menu ("Album order" by default — the order of the export is data —, "By update", "By
   name", "By file count", "By size"), the "Cards / List" switch and the primary "+ New album". The
   list look is a 38–57 px row that follows the slider (`draw_album_row`). `settings.album_sort` and
   `settings.album_list` keep the choice (two settings keys, no schema change).
3. **The tree stays inside an album**, under the breadcrumb "All albums › … › here"; the root is the
   card grid. A press on any crumb opens that level (`AlbumAction::Root`/`Select`).
4. **The way into an album is a function of its own** (`AlbumPick`): one dialog for three entrances —
   the dashed "+ Add" chip of the file card (one photo), "Add to an album… (N)" in the floating
   selection bar (the marked photos) and "Add the picked ones (N)" inside an album (straight into the
   open album). The dialog lists the albums with their paths and counts, filters by name, and has a
   field for a new album: a name makes a root album and fills it in the same press. Rows only.
5. **The date of a card** is the day of the last change of the album or of its contents: `AlbumNode`
   gained `updated_at`, and `add_to_album`, `remove_from_album` and `move_in_album` touch it. No
   schema change: the column has been there since M19.
6. **Refused:** the per-card "⋮" menu of the mock-up (an icon-only target, docs/UI.md § Layout says
   an action is a word; the actions of an album belong to the open album), the mock-up's floating
   album bar ("Open · Export · Delete" duplicates the action row of the open album), the cover
   cropping of the mock-up (M22.15's rule: only the grid tile fills, an album cover fits whole).

**Result.** `cargo test` 301, `cargo test --test source_language` 3, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics` 0 errors. New tests: the root of the albums
is a card grid, the filter and the order of the cards, the breadcrumb back to the root, the card
model at 100 and 260 px, the list row, the control row inside the content panel at 1240 px, the
picker that adds and makes an album (a DB test), the picker frame, the selection bar without and
with albums, the dashed "+ Add" of the card, and the DB order of the cards with `updated_at`.

**Files.** `src/app.rs` (`draw_album_cell`, `draw_album_row`, `draw_album_cards`, `album_card_size`,
`album_row_height`, `album_cards_of`, `album_sort_label`, `album_date`, `cover_badge`,
`dashed_add_chip`, `AlbumPick`, `album_pick_open`/`album_pick_commit`/`album_pick_modal`, the
`View::Albums` branch, the selection bar, the file card), `src/db.rs` (`AlbumSort`, `album_order`,
`AlbumNode::updated_at`, the touched stamps, two settings keys), `src/i18n.rs` (28 labels),
`docs/UI.md`, `plan/`.

### M22.21 — the album card opens from the root and shows a mosaic ✅

The owner: "доработать работу с альбомами. сейчас я не могу провалиться в альбом. Для превью
альбома использовать его фотографии в виде плитки. максимум 4 фото для плитки".

1. **A card at the root opens its album.** `album_apply` demanded an open album before it looked at
   the action, so `AlbumAction::Select` from the root returned at once: the card grid of the top
   level was a dead end, and only the tree and the breadcrumb inside an album worked. Opening and
   "All albums" are handled before the open-album guard now.
2. **The cover is a mosaic of up to four photos** in the album order: `AlbumNode::cover` became
   `covers: Vec<String>` (at most `db::ALBUM_COVERS`), filled by `albums_tree` in one pass, skipping
   a file in the trash or without a preview. `cover_cells` splits the 16:10 frame: one photo whole,
   two columns, three as a left half and a right stack, four as a 2×2 grid, 2 px seams; the card and
   the list row paint it with `paint_cover_mosaic` and keep `paint_thumb`'s fitted whole preview, so
   no snapshot is cropped (M22.15's rule; reversed in M22.29 — a cell is filled and cropped).
3. **Refused:** storing a cover in the DB, a mosaic of more than four photos, cropping a cell (the
   third refused point was reversed in M22.29).

**Result.** `cargo test` 305, `cargo test --test source_language` 3, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. New tests: a click on a root
card opens its album (`a_click_on_an_album_card_opens_it_from_the_root`), the geometry of the mosaic
(`the_album_cover_is_a_mosaic_of_at_most_four_photos`: 0/1/2/3/4/9 photos, no overlapping cells), the
four painted previews (`the_album_cover_paints_four_previews`: four meshes for six photos) and the DB
order (`an_album_carries_up_to_four_previews_in_its_order`: the album order wins, a trashed file and
a file without a preview drop out).

**Files.** `src/db.rs` (`AlbumNode::covers`, `ALBUM_COVERS`, `albums_tree`), `src/app.rs`
(`cover_cells`, `paint_cover_mosaic`, `album_covers`, `draw_album_cell`, `draw_album_row`,
`draw_album_cards`, `album_apply`), `docs/UI.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`,
`plan/`.

### M22.22 — the albums hold one look switch ✅

The owner: "не работают кнопки переключения в список и плитку альбомов". The album switch did not
act on the photos either: it changed only the sub-album cards, so an album without sub-albums looked
dead. Two steps finished it:

1. **M22.22** hid the ribbon's dead "Tile / Rows / List" at the albums root, where no snapshot grid
   is drawn.
2. **M22.23** hid the ribbon's cell views on the whole Albums screen — the owner's screenshot from
   inside the album ("Садик", four files) showed the bottom "Список / Плитки" still doing nothing to
   the four snapshots — and made the album control row's "Cards / List" the only look switch of the
   screen. It drives both the album cards and the album's photos: `App::grid_cell_style` returns
   `List` for cards-on/list and `Tiles` otherwise while the view is Albums, and
   `settings.cell_style` of the other screens is left alone.

**Result.** `cargo test` 307, `cargo test --test source_language` 3, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. Tests:
`the_album_look_switch_changes_the_photos` (inside an album with two photos a click on "Список"
stores the choice and makes the grid a list, "Плитки" brings the tiles back, the global Rows view
stays) and `the_albums_view_holds_one_look_switch` (one "Список" at the root and inside an album, the
ribbon's "Плитка / Ряды" gone).

**Files.** `src/app.rs` (the ribbon, the album control row, `grid_cell_style`, two tests),
`docs/UI.md`, `plan/`.

### M22.24 — the album lists carry folder icons ✅

The owner: "почему у списков альбомов нет иконок? сделай". The tree rows were bare
`selectable_label`s, and an album without a preview showed a "▸" mark in its card or list-row well.
Every tree row now has a 14 px folder glyph (`album_tree_row`), and the whole row is one click
target: the glyph, the name and the facts select the album, and the glyph is not a second, dead
spot. The open row keeps the `accent_soft` fill and the 2 px accent edge, the hovered one the `card`
tone. An empty album draws the real folder icon in its card cover and in the list row instead of the
"▸" text.

**Result.** `cargo test` 308, `cargo test --test source_language` 3, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. The test
`a_click_on_the_folder_glyph_of_a_tree_row_opens_the_album` clicks 12 px left of the name — on the
glyph — and lands in the sub-album.

**Files.** `src/app.rs` (`album_tree_row`, `draw_album_tree`, the empty covers of `draw_album_cell`
and `draw_album_row`, one test), `docs/UI.md`, `plan/`.

### M22.25 — the right panel keeps 280 px with a long file name ✅

The owner: "на странице альбомов правая панель не соответствует своему макету… по ним щёлкать, то
начинают прыгать размеры плиток. Правая панель урезана". The identity row of the file card printed
the whole name in a horizontal `Label`; a long name demanded more than the 255 px card, the
`ScrollArea` widened to 306.5 px, the right panel grew past the window edge, and because the panel's
width is subtracted from the central one, the grid changed its column count and the tiles jumped.
The row now shortens the name to the room the size leaves (`shorten`, the full name in the hint);
the "All files" switch of an album goes through `t()`.

**Result.** On a copy of the real DB at 1691×1021 the panel was 1412–1718.5 (306.5 px) and the tiles
176 → 181 px after selecting a file; now the panel is 1412–1691 (279 px) and the tiles stay 176 px.
`cargo test` 309, `cargo test --test source_language` 3, `cargo clippy --all-targets` clean,
`check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors. The test
`a_long_file_name_does_not_widen_the_right_panel` checks the panel width at the window edge, that no
frame or word leaves the window, and that the tile widths are equal before and after the selection.

**Files.** `src/app.rs` (the identity row, the "All files" switch, one test), `docs/UI.md`, `plan/`.

### M22.26 — the People screen after the owner's look ✅

The owner: the face tiles still carry two worded buttons ("pick" and "not him"), the card of a file
has no business in the right panel of this screen, and the ribbon's "Tile / Rows / List" switch does
nothing there. The tile targets are now round 24 px icons: an empty tick circle at the top left,
filled with the accent and a tick when picked; a head with a slash at the top right, drawn only
inside one group — in "All people" there is nothing to split off. The two words went away, their
hints stayed and are translated. The right panel of the screen is the group list now: the same 44 px
band with the face glyph and "Groups" but no "×", since the list is content and not a toggle; every
row carries the best face of its group as its icon (`face_avatar` reuses the tile texture cache, the
face glyph stands in for a missing crop), the "together" tick and the name with its photo count. The
list left the left panel, which keeps only the threshold, the pass, "Rebuild groups" and the
"together" search; below 1200 px the list falls back into the left panel, as the file card does.
The details toggle of the top strip is not drawn on this screen, and the ribbon keeps no cell-view
switch: the face tiles have one look of their own.

**Result.** The group list is drawn at x ≥ 960 of a 1240 px window, the tile buttons paint no words,
and neither "Tile", "Rows", "List" nor "Details" appears on the screen. `cargo test` 310,
`cargo test --test source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors; the window lives through `timeout 6`. On a copy of the real DB
the screen costs p50 10.27 ms / p95 12.16 ms (dark) and 9.61 / 11.08 ms (light) against 8.07–8.49 ms
before — the 617-row list with a face icon per row, still below 16.7 ms. The new test
`the_faces_screen_keeps_its_list_in_the_right_panel` holds the position of "Groups" and "All people"
and the absence of the dead controls; the palette and caption tests of the screen were updated.

**Files.** `src/app.rs` (`Icon::Pick`/`Icon::NotHim`, `tile_icon_target`, `face_list_rows`,
`face_avatar`, `faces_panel`, the panel wiring and the top strip, three tests), `docs/UI.md`,
`plan/I18N.md`, `plan/`.

### M22.27 — the trash's restore is an icon ✅

The owner: the word "restore" on a trashed tile should be an icon. The tile and the list row now
carry the same round 24 px target as a face tile — a dark scrim, the accent on hover, the restore
glyph (`tile_icon_target`, shared with M22.26) — and the word lives in the hint: "Restore: back into
the usual views — the file was untouched on disk", translated. Nothing else on the screen changed:
"Restore all" keeps its word, and "Empty the trash (N)" keeps the one `danger` border. A click on the
target no longer selects the row or the tile as well.

**Result.** The trash screen and its confirmation paint no word "restore"; the guard test
`every_button_of_the_window_has_an_icon` no longer needs an exception for it. `cargo test` 310,
`cargo test --test source_language` 3, `cargo clippy --all-targets` clean, `check.sh` 55 checks,
`rust-analyzer diagnostics .` 0 errors; the window lives through `timeout 6`.

**Files.** `src/app.rs` (`restore_icon`, `tile_icon_target`, `draw_cell`, `draw_list`, two tests),
`src/i18n.rs`, `docs/UI.md`, `plan/I18N.md`, `plan/`.

### M22.28 — the full screen: a close target, face boxes on demand, a panel of one face's photos ✅

The owner, three words about the viewer: it has no button that closes it, the face boxes are on all
the time, and a face should lead to its other photos. The header now ends with the mock-up's 22 px
"×" (the close target of the screen, its hint "close the full screen — Esc"); "Back to the grid"
stays the worded way back. The boxes are a tool: "Show faces" in the tools row, the face silhouette
and the picked state, off by default, disabled with the hint "no faces found in this file" while the
model found none. Every box is a click target — `db::FaceBox` now carries the face `id` and its
`cluster_id` — and a click opens the right column of 280 px, the same slot the inspector uses: the
44 px band with the face glyph, the name of the group and a "×", the count, and every photo of that
face, oldest first (`App::face_focus_for`), as 64×48 rows with the name, the date and the size; the
row of the frame on screen carries the accent. A row opens its photo in place, a click on the empty
part of the frame or the "×" closes the panel, and paging closes it as well. An ungrouped face lists
only the frame and says so.

**Result.** `cargo test` 311, `cargo test --test source_language` 3, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors; the window lives through
`timeout 6`. `the_full_screen_hides_the_face_boxes_until_asked` checks the off state and the tool;
`a_picked_face_lists_its_photos_in_the_right_panel` checks the chronological group, the panel words
and the ungrouped case.

**Files.** `src/app.rs` (`Viewer`, `FaceFocus`, `viewer_modal`, `viewer_face_panel`,
`face_focus_for`, `draw_viewer_image`, `paint_face_chips`, two tests), `src/db.rs` (`FaceBox`),
`src/i18n.rs`, `docs/UI.md`, `plan/I18N.md`, `plan/`.

### M22.29 — an album cover has no margins ✅

The owner: the mosaic of photo previews on an album card must not be shown at the photo's own size
with margins around it — the previews should be squeezed into the square of their cell. The mosaic
kept `paint_thumb`'s fitted whole preview (M22.21, M22.15's rule), so every cell showed the photo's
ratio on the `canvas`: a 16:10 cover of 4:3 photos left a band on all four cells at once.

1. **A cell is filled and cropped.** `paint_cover_mosaic` paints through
   `paint_thumb_as(..., ThumbFit::Filling)`: the preview covers its cell edge to edge and the sides
   that do not fit are cut evenly — the centring crop of the grid tile (`cover_rect`). No stretching,
   no margins; the `canvas` shows only through the 2 px seams.
2. **The cover keeps its 4 px radius.** The crop would square the corners of the well, so the mosaic
   ends with `round_preview(ui, rect, RADIUS_CTRL, bg, true)`, `bg` being the tone under the well —
   `card` on an album card, `canvas` in the list row of the albums. The card and the row share the
   same call, so the mosaic looks the same in both looks.
3. **Refused:** a square cover, a separate "cover crop" setting, more than four cells; the fitted
   whole preview stays everywhere else (the calendar, the series strip, the file card).

**Result.** `cargo test` 311, `cargo test --test source_language` 3, `cargo clippy --all-targets`
clean, `check.sh` 55 checks, `rust-analyzer diagnostics .` 0 errors; the window lives through
`timeout 6`. `the_album_cover_paints_four_previews` now also checks that each of the four painted
rectangles covers its cell, so a return of the margins fails the test.

**Files.** `src/app.rs` (`paint_cover_mosaic`, `ThumbFit`, `draw_album_cell`, `draw_album_row`, one
test), `docs/UI.md`, `plan/`.

## 7. Not doing in M22

- A third theme, an accent picker, a "compact mode".
- Bundled fonts until the owner's decision (q. 2).
- Cropped 4:3 previews outside the grid tile (the album cover filled and cropped in M22.29 is the
  second exception), a floating selection bar, an icon font (icons are geometry, M22.8, ADR 0048).
  The tile crop itself is M22.15.
- Any function from §5 q. 5; any network call; any file operation.
