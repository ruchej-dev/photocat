# M16 — the interface after the PhotoPrism model

*Русская версия: [M16-interface.ru.md](M16-interface.ru.md).*

The task: rework the GUI shell taking PhotoPrism as the model, without touching the data, the
archive and the logic of the passes. The document is the analysis of the current interface, the
options, the choice and the boundaries of the work. The normative rules stay in `docs/UI.md` and
`.pi/skills/ui-design/`; here — why they change.

## 1. What there is now (an analysis, 2026-11-23)

The shell: the top bar in two rows (the folder + "Index" + "Refresh" + "Folders" +
"Details" + "Settings"; the search mode + the field + "Find" + "Reset"), a status row at the bottom,
the left panel 260 px (the brand, 5 views, a filter by path/date, "images only", the tile slider,
the view context, the model block, the OCR block, a pinned "Summary"), the centre (the view title
14 px + the counter 12 px, the `draw_cell` grid 150 px, a step of 8 px), the right panel 280 px
("File details").

The style: the egui 0.37 default (font 14, `item_spacing` 8×3, rounding 3–4), flat panels, every
block of the left panel and the cards wrapped in `Frame::group` — a frame in a frame. One accent
`rgb(90,170,255)`, four allowed greys. There are no icons, all the items are words.

The elements and their price by the `ui-design` rubric:

| Element | Now | What is wrong |
|---|---|---|
| Navigation | 5 words without groups, without numbers | there is no hierarchy "library / tools", the price of an item ("103 groups") is not visible before a click; "Trash · N" — the only counter |
| The top bar | the folder and the search in two rows, the actions on the right | the folder field competes with the search by weight; "Details" — a panel switch, not an action |
| The view title | 14 px + 12 px in one row | it does not differ from the body; there is no room for controlling the view (tiles, filters) |
| The grid | a flat list of 2230 tiles, the order from the old to the new | there is no "moment" support: a person looks for "May 2024" and sees an endless feed; the date is only under every tile |
| A tile | the `extreme_bg` background, a preview, the date 11 px | there is no response to hovering (a cursor/an outline), the step of 8 px eats 15% of the grid width |
| Filters | path/date and "images only" in the panel, the tag and the threshold — in the model block | three places for one notion "filter"; the active filter is not visible above the grid; the reset — a separate button in another place |
| Panel sections | each in a `Frame::group` frame | "card soup": the frames compete with the photos for attention, the density falls |
| Details | a 4:3 preview, two buttons, the metadata as a stack of "label above value" | 6 rows = ~130 px of vertical space; the file name is not the main thing anywhere; the "Open full screen" button is not highlighted |
| Progress | up to five rows under the top bar | with two passes the grid moves down; every row has a "Stop" — that is right |
| Status | 11 px on the left, the selected file on the right | with an empty status there is a "—" — an empty place instead of a fact |

## 2. What we take from PhotoPrism (and what we do not)

We take the **structure**, not the look:

1. **Side navigation in groups** — "Library" (All files, Calendar, Faces) and "Tools"
   (Duplicates and series, Trash), every item with a number on the right, the brand on top, the
   settings — in the top
   bar (in PhotoPrism they are at the bottom of the list; our rule `nayti-za-sekundu.md` puts them
   into the
   top bar, we keep that).
2. **Flat sections** instead of frames: a label 11 px in a muted colour, the contents, a thin line.
3. **The search as the main element of the top bar**, the mode "Semantic / By text" — a segmented
   switch before the field.
4. **The content panel**: the view name 18 px, the facts next to it, on the right — the tile size;
   below, a row
   of the active filters as chips with "×" and the total "found N of M".
5. **Grouping by months**: the header "May 2024 · 132 files · 5.4 GiB", the shots from the newest
   to the
   old, a tile step of 2 px (in PhotoPrism — 1 px; 2 px stays within the 4 px scale).
6. **The inspector**: the file name as a heading, the metadata as "label — value" rows, the actions
   above
   the metadata, below the tags and the text.

We do not take: a light theme, icons without labels, "tile-cards" with shadows and big radii,
a multiple selection with checkboxes (that is a separate task: it changes the state model, the card
and the duplicates), a map of
places, albums, "Favourites" — they are not in the data model, there is nothing to promise.

## 3. The options and their price

**A. Cosmetics.** Keep everything, change the paddings, the title 18 px, drop the frames. The price:
~120 lines
in `app.rs`, a habit does not change. It does not solve the main thing: the grid has no support by
dates, the filters are
smeared. The rubric: item 7 (density) — yes, item 10 (in a second) — no.

**B. The PhotoPrism shell (chosen).** Side navigation in groups with counters, flat sections,
a content panel with filter chips and the tile size, the grid grouped by months from the newest,
tiles with a 2 px step and a response to hovering, an inspector with the file name and "label —
value" rows, one style (a step 8×6, a button 26 px, a radius 4). The price: ~450 lines in `app.rs`
and edits to
`docs/UI.md`; a habit: month headers and the order "newest on top" — as in a phone and in
PhotoPrism; everything else is in place. A compromise: the tiles stand denser, the date under a tile
stays
smaller than the month header — recognition does not suffer.

**C. A full copy of PhotoPrism.** A light theme, icons, albums, favourites, a multiple selection, a
map.
The price: weeks, new data in the DB, a violation of "an icon without a label is forbidden" and "there
are no own colours".
Rejected: it is someone else's archive model, not ours.

## 4. The decision (by the rubric)

| Rubric item | A | B | C |
|---|---|---|---|
| 1. One main action | yes | yes | no (the albums compete with the search) |
| 2. Recognition instead of a search | partly | yes (month + date) | yes |
| 3. A number next to a label | yes | yes | yes |
| 4. The state is visible | yes | yes (filter chips) | yes |
| 5. Nothing only by hovering | yes | yes | no (icons) |
| 7. Density | no | yes (a 2 px step) | no (big cards) |
| 10. In a second | no | yes (the filter and the reset above the grid) | partly |
| 12. Nothing jumps | yes | yes (the title and the chips are always in place) | no |

**B** is chosen. The record: `docs/DECISIONS.md` ADR 0033, the rules — `docs/UI.md`.

## 5. The boundaries of the work

- Only `src/app.rs` changes (the shell, the grid, the panels, the style) and the documents.
- They do not change: the DB schema, indexing, duplicates, CLIP, OCR, faces, the player, the trash,
  the network.
- The originals are not touched; not a single new action with files on disk.
- The left panel stays 260 px, the right one 280 px, the minimum window 1000×700, 4 columns at
  1240 px.
- One accent, the fonts 11/12/14/16/18, the steps 2/4/6/8/12/16/24, the greys 150/190/210/235.

## 6. Acceptance criteria

1. The left panel: two groups with headings, the items with numbers on the right; "All files" and
   "Trash"
   show real counters; in a 840 px window the navigation and the filter are visible without
   scrolling.
2. The top bar: two control rows; the search with a segmented mode; "Index" —
   the only accent button; the progress does not shift the grid by more than its own row.
3. The grid: months in descending order, the header "May 2024 · N files · size", inside — the newest
   on top;
   a tile step of 2 px; hovering gives an outline and a cursor; 4 columns at 1240 px.
4. Above the grid: chips of the active filters with "×", the total "found N of M", the "Tiles"
   slider.
5. The inspector: the file name 14 px, the path 11 px, the metadata as "label — value" rows, the
   actions
   before the metadata; nothing appears on hover.
6. The sections of the left panel and the cards without `Frame::group` frames; one style for the
   whole GUI.
7. `cargo clippy --all-targets` clean, `enforce-architecture` without violations, `cargo test` green,
   `timeout 6 photocat gui --db <a copy of the real DB>` does not crash (with a warning to the
   owner).
8. The documents are updated by the same change: `docs/UI.md`,
   `.pi/skills/ui-design/references/verstka.md`,
   `docs/DECISIONS.md` 0033, `plan/STATUS.md`, `plan/BACKLOG.md`.

## 7. What stayed outside the boundary (honestly)

- The multiple selection of tiles and batch actions over the selection — the next task, not this
  one.
- Pinning the month header while scrolling: the egui `ScrollArea` does not give a sticky row without
  manual drawing; the header lives in the flow.
- The map, albums, favourites, tag clouds — they are not in the data model; we do not promise them.
- The keyboard (Ctrl+I/Ctrl+F/F5, the arrows over the grid) — still in §Debt of `docs/UI.md`.
