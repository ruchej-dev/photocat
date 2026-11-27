# photocat interface

*Русская версия: [UI.ru.md](UI.ru.md).*

The owner is a person who sorts out a family archive, not a designer and not a photographer. The
screen must be dense, dark and predictable: a grid of snapshots, one status line, a filter panel.
Nothing that needs to be learned.

## Rules that are not up for debate

- **Two languages, chosen in "Settings → Interface"** (M21, ADR 0040): English is the main one,
  Russian the second. On the first launch the language comes from the system locale, and from
  English when it cannot be detected. A Russian string in the code goes through
  `i18n::t(lang, "…")`; an untranslated label is shown in Russian, so a screen is translated in one
  step, not half of it. Identifiers in the code are English. Dates are `YYYY-MM-DD HH:MM`, sizes go
  through `db::human_in(lang, …)` (`КиБ/МиБ` or `KiB/MiB`), image dimensions are `W×H` with `×`.
  Counters go through `i18n::count` (Russian 1/2–4/5+, English 1/N), otherwise English shows
  "files 1". Since M21.5 every visible surface of the window goes through `t()` — including the
  full screen, the player controls, the folder dialogs and the storage confirmation; the position
  in the full screen comes from `i18n::position`, the slideshow interval from `i18n::seconds`. CLI
  reports were translated in M21.6. The first status line is built from the settings just read, not
  from the frame before them, so a fresh window opens in its own language (M22.12).
- **Not a single action that deletes or renames anything.** The words "delete", "clear", "move" do
  not appear in the main scenario; at most "mark as spare", "remove the tag" (hide the label in the
  DB, the original does not change) and "export the plan". The exception requested by the owner: "To
  trash" (a DB row only) and "Empty the trash" — a move to the **OS** trash after confirmation. The
  cache is cleaned only in "Settings → Storage" and only after confirmation.
- **The network only by a button.** "Download model" is enabled only after the consent tick; no
  background download, no sending of photos, paths or queries.
- **Long work shows progress and a stop.** Indexing, clustering, inference, OCR: a background
  thread, an `egui::ProgressBar` next to "Stop". After a stop the start button shows the remainder
  ("Resume (N)"), because what is done is skipped by `sha256`.
- **One widget.** The preview grid is drawn by one function (`draw_cell` in `app.rs`); a new screen
  calls it instead of copying it. The exception is faces: `draw_face_cell` draws an aligned face
  crop, not a file; its tile metrics and caption are the same (150×150 + 26).
- **Numbers next to their label**: "214.1 MiB wasted", "238 files", not a column of digits without
  units.
- **An error is visible where it happened**: the status line at the bottom, not a modal window.

## Layout

The normative rules are in `.pi/skills/ui-design/references/verstka.md`. Here are the numbers that
cannot be changed alone, and what is checked by eye. The sections "Layout", "Interaction", "Findable
in a second" are requirements for the interface; what the code does not do yet is in § Debt against
the interface rules at the end of the file.

- **Two themes, one token palette** (M22, ADR 0044). `app::Theme` is the only source of colour:
  `DARK` (the main one) and `LIGHT`; `settings.theme` is `dark | light | system`, `system` follows the
  OS and is re-read every frame. `apply_theme` fills `egui::Visuals` from the tokens and sets the
  shape: step `8×6`, button padding `10×4`, minimum target `32×26`, nesting indent 16, slider 120 px,
  **radius 4 / 6 / 8 px** (controls, items and menus, cards and dialogs — the radii of the rendered
  mock-up screens), shadows off. A screen never writes a colour by hand; the only exception is text
  over a darkened frame, where the background is a photograph. The tokens (canvas, panel, card,
  field, borders, three text tiers, `accent`, `accent_hover`, `on_accent`, `accent_soft`, `danger`)
  are listed in `plan/M22-ui-spec.md` §2.1.
- **A 4 px grid.** Spacing only `2, 4, 6, 8, 12, 16, 24`. Inside a group 4, between groups 8, between
  sections 12, before a heading 8.
- **Three cell views, the mock-up's tile is the main one** (M22, `settings.cell_style`): the
  segment "Tile / Rows / List" in the content panel header, next to the size slider.
  **Tile** (default) — one width for all tiles, a **4:3 frame**, the preview *covering the whole
  frame* (a centring crop: the proportions are kept, the sides that do not fit are cut, the picture
  is never stretched), a 22 px caption band; the columns share the available width. The tile is a
  card: the body on the `card` tone, the well of the preview on `field` (the mock-up's
  `surface-container-highest`), the caption on `panel`; the well shows while a preview is loading
  (M22.14, the fill itself M22.15). **Rows** —
  the row height comes from the slider, the row fills the width, so a tile's width follows its
  frame's aspect and the preview fills the tile with no margins; a row is never lower than 70 % of
  the slider and the last row keeps the target height. **List** — one row whose height comes from the
  same slider (`24 + 0.16 × value`: 40 px at 100, 66 px at 260), a 4:3 thumbnail `row − 8` high, the
  name at 12–14 px and the date · size in mono 10–12 px. The slider never disappears and the header
  keeps its height, so switching the view does not make the right-hand group jump; the slider's
  tooltip names what it scales in the current view. All three share one entry point (`draw_files`)
  and the same click/selection/tooltip rules.
- **The grid step is 10 px** (M22, the mock-up's `gap-2.5`), both between tiles in a row and between
  rows; the gap does not change `item_spacing` of other screens.
- **A tile's aspect never stretches a snapshot** (M22): the frame is 4:3 in the tile view and equal to
  the frame's own ratio in the rows view; a file without dimensions (broken, no preview) gets 4:3.
  The rows layout is computed from `files.width`/`files.height`, so it does not wait for textures.
- **Previews follow the EXIF orientation** (M22): the thumbnail is built with the rotation applied, so
  a phone portrait is shown upright in the grid, in the card and in the full screen; `width`/`height`
  in the DB are the displayed dimensions. A stale preview is rewritten when its dimensions do not
  match (ADR 0045).
- **Sections are flat:** an 11 px label in the muted colour, the content, a thin line. `Frame::group`
  is not used around panel blocks and the card — a frame inside a frame competes with the photo.
- **Screens without a mock-up borrow the parts** (M22.9): duplicates, faces, trash and integrity are
  built in the order of the main screen (title + facts, then the content), on the same canvas, with
  the same `draw_cell`/`draw_face_cell`, mono numbers and one accent. A duplicate group is a
  `card_block` — the `card` tone, a 1 px `border`, an 8 px radius and 8 px inside — and its tiles lie
  on that tone; a face tile keeps its 150×150 + 26 metrics and takes the caption of a file tile (mono
  10, `text_med`/`text_dim`); the faces screen puts its group list into the right panel, where the
  card would stand (M22.26); `danger` appears only on "Empty the trash"; the integrity form is the
  content panel (canvas, 12/6 margin) with the path fields in the numeric family. The chips row
  stays with the views that filter (grid, calendar, albums): these four apply no filter, so their own
  row of choice stands where the chips row stands and the header rhythm does not change.
- **The grid is grouped by month** (M16): the heading "May 2024" at 14 px + "· 132 files · 5.4 GiB"
  at 11 px, from the newest to the oldest; inside a month the snapshots also go newest first. "No
  date" is the last group.
- **The month heading is pinned** (M22.4): while a group runs under the top edge of the grid area,
  its heading stands at that edge, drawn over the tiles on the canvas tone so nothing shows through;
  the next group's heading pushes it up and out instead of taking its place at once. The grid keeps
  one `ScrollArea` — the pinned row is painted over the area, not put into a second one. In "Flat"
  there are no headings and nothing is pinned.
- **The owner chooses the layout and the order** (M17): in the view header, next to "Tiles N", two
  buttons "By months / Flat" and a "Sort" menu ("Date: newest first" by default, "Date: oldest
  first", "Name", "Size: largest first"). "Flat" is one continuous ribbon without month headings.
  "No date" is the last group in both directions. While a semantic search runs, sorting and grouping
  are disabled: the results are a flat list by relevance, "the search sets the order".
- **Fonts are only `10, 11, 12, 13, 14, 18`** (M22). 10 px is allowed only for caps section labels
  and mono-numeric values; 18 px is the view title; 13 px is the dense body. No more than four sizes
  on a screen. Two weights: normal and `strong/small`. No italics; CAPS only for section labels.
- **Numbers are monospace** (M22): every number with a unit, every counter, date, size, ratio and
  duration goes through the numeric family, so a column does not jitter while indexing. Section
  labels are `label-caps`: 10 px, uppercase, `text_dim`.
- **Colour** comes from the `Theme` tokens; `accent` (amber) means only "selected", "confident tag",
  "keep", progress. No more than one accent per tile. `danger` (red) is allowed only on the path to
  the OS trash.
- **Readable text** is the `text_med`/`text_dim` tier of the theme: the caption under a tile
  `text_dim`, the word "spare" `text`, text on a dimmed frame `gray(235)` (a photograph, not a
  theme). Contrast is checked in both themes by `app::tests::both_themes_pass_the_contrast_ratio`
  (M22.11): body text and the accent words ≥ 4.5:1 on `canvas`/`panel`/`card` and on the selection
  fill, dim hints ≥ 3.0:1, the danger word ≥ 4.5:1. A token that fails is changed, not the rule —
  that is why the light accent is the design system's `amber-focus` `#b45309`, not the drawn
  screen's `#d97706` (2.91:1 on the panel). The accent on the `field` tone is not a pair the
  screens paint (an idle chip and a disabled row carry `text_med`/`text_dim`), so it is not claimed.
- **The window is looked at in both themes** (M22.11): `app::tests::every_screen_paints_only_the_
  tokens_of_its_theme` renders the grid, the four calendar layers, the albums and the full screen in
  dark and in light and fails on a word, fill or line that is not a token of the palette in use; the
  four screens without a mock-up and all seven dialogs do the same in their own tests.
- **The ground is neutral** (M22.11): the light `canvas` is the design system's cool `canvas-base`
  `#f8fafc`, never its warm Material `background` `#fff8f5`; every ground token keeps `r ≤ g ≤ b`,
  and a tile's frame is painted in `canvas`, so the letterbox around a photograph does not cast on
  the photograph. `accent_soft` is the only warm tone and it means "selected", never ground.
- **One scroll per region.** No nested `ScrollArea`; the face group list is the scroll of the right
  panel, and only in a narrow window does it become a `max_height 250` block of the left panel
  (M22.26).
- **A place for a tile is always reserved**: no preview — the text goes in the same rectangle. There
  are no layout jumps while loading.
- **A tile:** a card with an 8 px radius and a 1 px frame → the preview, covering the frame edge to
  edge in the tile view (a centring crop, the sides that do not fit are cut) and fitted whole in the
  rows view (its frame already has the photo's ratio) → a 16 px video bar → the
  decision mark → a 22 px caption band (`panel` tone, a mono 10 file name on the left, the date in
  `text_dim` on the right) → a 2 px accent selection outline and a square check flag. The list view
  keeps the same words and accent: a row with a 58×40 thumbnail, the name and the date · size.
- **The top strip is 32 px and one row** (M22.19, after `photocat_1`): the mark (a 20 px
  photo-library glyph in the accent and "photocat" at 14 px), one search field in the middle (the
  search glyph, the "Semantic / By text" mode switch with its arrow, the query, "Search" in the
  accent and "×"), and the tools on the right — the "Tiles" pill (the grid glyph and a compact
  100–260 px slider, no number: the value is in the hint), the details toggle (a 28 px icon that
  stays lit while the right panel is open) and "Settings". The state pill is gone (M25.8): the
  running pass is named in the status bar, and its former ~70 px went to the search field, which is
  now 646 px at most and centred between the mark and the tools. The third row is the pass progress
  only. **The one row is exactly `TOP_H` (32 logical px) at every scale, and no child may take the
  strip's height** (M26.4): the search well is 28 px and the "Tiles" pill 26 px, both shorter than
  the row and centred in it, so a pass row or the reminder adds height under the row and the strip
  returns to 32 px when the pass ends. Before M26.4 the well and the pill filled the available
  height, and every pass row the owner's indexing added was kept for good — on Windows the strip
  reached twice the design. The folder field
  and "Index / Refresh" live in the left panel: working with the archive is not search. Both
  carry their tooltip (M25.7): "Start indexing" walks the folders, reads only the new and changed
  files (path + size + mtime), computes the hash, the date, the dimensions, the sharpness and a
  320 px preview, gives a video its cover through ffmpeg and writes to the cache only — never to the
  archive — and can be stopped and resumed; "Refresh" re-reads the database into the window, reads
  no file and changes nothing, for a change made by the CLI or in another window.
- **The bottom strip is 28 px** (M22.19): a dot and the last fact on the left, then the count of
  files with their volume in the numeric family; on the right the selected file — its name, the
  frame ("6000×4000") and the size, or "N selected · volume".
- **The content ribbon is one 44 px row** (M22.19, the mock-up's `h-11`): the view title at 18 px
  and the facts next to it at 12 px on the left; on the right the controls of the view as 20 px
  buttons — the three cell views, "By months / Flat" and the "Sort" menu — separated by 1 × 16 px
  lines. The cell views appear only where a snapshot grid is drawn; on the "Albums" screen they
  are hidden, and the "Cards / List" switch of the album control row is its one look switch: it
  changes the album cards and the album's photos together (M22.23). The "People" screen hides them
  too: its face tiles have one look of their own (M22.26). The row of active filter
  chips with "×" and "Reset" is drawn **only while a filter is on**,
  so the grid does not pay for an empty row; the counts are already in the title.
- **Three columns:** on the left the app map and the filters, in the centre the grid, on the right
  the details of the selected file. The right panel is 280 px and appears while the grid keeps 4
  columns (window ≥ 1200 px); below that the card moves into the left panel, so that the grid does
  not shrink to 2 columns. On the "People" screen the right panel holds the group list instead of
  the card; below 1200 px that list falls back to the left panel (M22.26).
- **The left panel:** navigation, the folder block and the filter block are pinned on top and do
  not scroll; everything else scrolls between them — the years, the view context, the model and
  text blocks, and the file card (in a narrow window). The years open that scrolling part, so they
  are the first thing under the pinned filter and need no scroll of their own; they are a filter of
  the grid and of the calendar only — the duplicates, faces, trash and integrity screens have a
  context of their own. The years are a wrapped cloud of 18 px chips (M25.2): the five newest and
  a "…" for the older ones, "All years" while a pick is on. There is no "Summary" block (M25.5):
  the files and the size live in the status bar, and the two "missing" numbers are the counts of
  the filter chips. The "Catalog" heading carries a bare "+" that opens the folders dialog (the
  worded path is the dialog itself).
- **The right panel (M22.16, after `photocat_12`):** a 44 px header band on the `card` tone with a
  1 px line under it — the info glyph and "File details" at 12 px strong on the left, the mock-up's
  "×" at the right (a 22 px target, the hint "free up room for the grid and calendar"; the worded
  way back is "Details" in the top bar) — then the file card on 12 px of air, scrolled as one
  column. Nothing is selected by itself: while there is no file, the panel says "Pick a snapshot in
  the grid". The "People" screen keeps the same band, with the face glyph and "Groups", but no "×":
  its list is the screen's content, not a toggle, and the details toggle of the top strip is not
  drawn there (M22.26).
- **A row never widens a panel** (M22.12): the width of a panel is the rule, the row is the content.
  A long field takes what the word button leaves (the own-tag field is `available − 140 − 16 px`
  inside its frame, never below 40 px), a long name is shortened or wrapped, and no row may push the
  card past the 280 px.
  egui widens a panel to fit an overflowing row and remembers the wider size, so the defect outlives
  the file that caused it: the card is shifted right and clipped on both sides, and the canvas of the
  central panel paints over its left margin. `app::tests::the_file_card_fits_the_right_panel` draws
  the whole window at 1240×840 with a file selected and fails on any painted rectangle outside the
  window or on a title that left the panel's left edge.
- **Forms:** input fields 380–420 px, long paths 520 px, a line no longer than 80 characters.
- **A dialog is a card** (M22.10): the `card` tone, a 1 px `border` line, the 8 px radius and 16 px
  of air inside, its title at 14 px (a dialog had 16 px, which is outside the type scale) and at most
  one accent button — the main action of the dialog. The one exception is the trash confirmation,
  whose button touches the disk: it wears the `danger` line and no accent. A number of a dialog is
  never inside a translated sentence but in a facts row of its own in mono 11 ("1 file · 18.4 MiB",
  "2 albums · 2 files", "export: 3/8"), exactly like the album header; the folder list counts the
  indexed files of each root. **A report** — the export and the integrity one — is a mono 11 body in
  its own scroll area with a copy button, never a proportional paragraph.
- **Forbidden:** shadows, gradients, radii beyond the 4/6/8 scale, emoji icons (except "▶"), an
  icon without a label as the only path, skeletons and full-screen spinners.
- **Icons (M22.8, M22.13).** The set is the mock-up's Material Symbols by name, drawn as geometry
  by `app::paint_icon` — no icon font, no image file, the binary does not grow (ADR 0048). Every
  action button is one `WordButton`: the icon 14 px (12 px in a compact row), a 1.5 px stroke, the
  colour of its word, 6 px before the word. The word always stays: an icon is never the only path.
  Navigation rows (`All files`, `Albums`, `Calendar`, `People`, `Duplicates & series`, `Integrity`,
  `Trash`), the section list of the dialog and the "Sort" control (a `WordButton` over
  `Popup::menu`) carry the same icons. A `ComboBox` is a select, not a button: the language, the
  model, the theme and the album parent keep words, as the mock-up's `<select>` does. The colour
  comes from the theme, so both themes and the disabled state need no second drawing.
- **Where a button keeps its word alone.** A few kinds of target are too small or too special for
  an icon, and each is checked by `every_button_of_the_window_has_an_icon` in `src/app.rs`:
  the per-tile action placed into a fixed 20 px rect (`keep`), the
  "×" of a filter chip (it is part of the chip), the collapse triangle of the album tree, the
  "×" that removes a row from an album, the "×" that closes the right panel (M22.16: a 22 px
  target in the mock-up's header with a hint), the round 24 px icon targets — the two of a face
  tile, the empty tick circle and the head with a slash (M22.26: the owner replaced the words
  "pick" and "not him"; "not him" is drawn only inside a group, where there is something to
  split off), and the restore of a trash tile (M22.27: the owner asked for an icon; the word
  "Restore" lives in the hint, so the action stays findable), the
  details toggle of the top strip (M22.19: the
  mock-up's 28 px icon that stays lit while the panel is open; its hint is "Details" / "Hide
  details" and the panel's "×" is the second way), and the "+" that adds an own tag (M22.18: a
  22 px square inside the framed field row, the owner's request — the word made the field short;
  the hint "Add tag", the "`#`" and the block's heading "My tags" say what it does). A plain `menu_button` is forbidden: the one menu of the
  window ("Sort") is a `WordButton` that opens its list through `Popup::menu`, so a menu carries an
  icon like every other action.
- **The player.** ⏮ ▶ ⏸ ⏭ and the sound sign are the first members of the same set (`Icon`), drawn
  as geometry and not as emoji; every button has a hint, ▶ in the centre of a video frame is
  labelled with words, and paging and play are duplicated by ←/→ and space. This is the owner's
  request: "icon buttons, like players have".

## Interaction

The full rules are in `.pi/skills/ui-design/references/vzaimodeystvie.md`. In short:

- **One click = select or act.** A double click opens the full-screen view (added at the owner's
  request); a single click still only selects. There is no dragging and no right click in the main
  scenario. Hovering gives a hint only.
- **Selecting several tiles (M18):** Ctrl/Cmd+click adds and removes a file, Shift+click takes a
  range in the same order as the grid (`App::ordered`), Ctrl+A takes everything found, a click on an
  empty place or Esc clears. A selected tile is visible without hovering: an accent outline and a
  tick. Above the grid: "N selected · size", "Select all (M)" and "Clear the selection". Changing
  the view, filter, query, search or order clears the selection with the reason in the status
  ("selection cleared: the path/date filter changed"): an action must not hit invisible files. The
  selection lives in memory only and is empty after a restart.
- **Albums (M19, the card grid of M22.20):** the "Albums" view opens on the cards of the top level
  albums — a mosaic of up to four of the album's first photos (M22.21), each filling its cell
  (M22.29), the counters on it, the name
  and "size · date" under it; a click opens the album. Inside an album the tree with indents and collapse
  triangles stays
  (a node shows "N files · M subalbums · size", a non-empty trash "N in the trash"), above the
  album's own header, its sub-album cards and the grid of snapshots in album order; the breadcrumb
  "All albums › … › here" is the way up, one press per level. The control row filters the albums by
  a part of the name, orders them ("Album order" by default, "By update", "By name", "By file
  count", "By size" — a look only: the album order stays the order of the export), switches the
  cards to a list and carries the one main action "+ New album". Inside an album: "Add the picked
  ones", "Remove from the album", "File up / down", "Export to disk…"; the "In the album / All
  files" switch is a temporary look that lets you collect snapshots and add them. "Album order" is
  also a sort option for the snapshots, available only inside an album; outside it the sort returns
  to date ↓. Deleting an album asks for confirmation and shows the subtree; the files on disk do
  not change.
- **Adding photos to an album (M22.20):** one dialog for every path into an album — the dashed
  "+ Add" chip of the file card (one photo), "Add to an album… (N)" in the floating selection bar
  (the marked photos) and "Add the picked ones (N)" inside an album (the open album directly). The
  dialog lists the albums with their paths and file counts, filters by name, and offers a field for
  a new album: a name there makes a root album and fills it in the same press. The photos go to the
  end of the album; a repeat does not duplicate rows. Rows only: not a single file on disk changes.
- **The chips of the file card (M22.17, M25.1):** the word and the "×" on a chip are two targets. A
  click on the word of a tag chip adds that tag to the tag filter ("add this tag to the filter"),
  a second click on the highlighted chip removes it ("remove this tag from the filter"); the accent
  marks exactly the tags the photos are filtered by, not "the confident one". The filter
  **accumulates**: a new tag narrows the grid instead of replacing the old one, and a file passes
  only when it carries every picked tag above that tag's own threshold. The "×" (which turns red
  under the pointer) removes the tag from this photo, the own tag from the photo, or the photo from
  the album; a click on the word of an album chip opens that album in the "Albums" view. The left
  panel's tag chips keep the same rule and sit under the caps heading "Tags"; while at least one
  tag is picked a bare 20 px "×" stands at the right of that heading ("reset the tag filter") and
  clears the whole set — there is no permanent "all tags" chip.
- **Album export (M20):** "Export to disk…" opens a dialog: the destination folder (it must already
  exist), the name template (`{n}_{orig}` by default), "Show the plan" and "Export N files". First
  the plan tree with folders and names is visible; copying runs in the background, progress and
  "Stop" are in the top bar. The destination gets **copies** of the files in the structure of the
  album; subfolders have no numbers, the order is visible through the number in the file name.
  Nothing is overwritten or deleted: a taken name gets `-2`, an already exported file (the same
  `sha256`) is skipped. A folder inside the archive cannot be chosen — that is a refusal with an
  explanation, not a write next to the originals.
- **Response:** selection and filters within 100 ms; duplicate recomputation, face group rebuild and
  semantic search within 1 s with the result in the status; longer — background, `done/total`
  progress, "Stop". An action is never performed silently.
- **Scrolling (M22.2b):** one profile for every scrollable region — the grid, the left panel, the
  card, dialogs; no view overrides it. One wheel notch moves a third of the window height (280 px
  for the 840 px default window); a two-finger trackpad swipe follows the fingers one to one and,
  once they lift, coasts with an exponential decay of 0.35 s: a short flick glides 270–290 px, a
  long one keeps going until it dies out (≈1.3 s). A new touch, the opposite direction or a wheel
  notch stops the coast in the same frame, so a direction change never jumps; in idle nothing
  requests a frame. `Scroll` in `src/app.rs` holds these numbers and `apply_theme` the rest.
- **Reversibility:** hidden tags come back, a duplicate decision is replayed, a group name is
  cleared, a file comes back from the trash. The only confirmations are "Empty the trash" (the OS
  trash) and cleaning the cache/index: there the state on disk changes or the index is lost.
- **Errors** go to the status line: what happened + what to do. No modal windows, no stacks.
- **A disabled button** explains the reason in text next to it ("the model is not downloaded").
- **A long pass:** the plan before the start ("435 candidates"), "Stop" always available, after a
  stop "Resume (N)", a rerun skips what is done by `sha256`.
- **Focus is visible** with a 2 px outline; Tab goes in visual order; Enter is the main action, Esc
  is reset. Delete is not assigned: there is no deletion.
- **The view state is kept**: the last selected file, the filter, the threshold, the face group, the
  calendar month, the search. The multi-selection (M18) is not written to the settings: it lives
  within a session until the view/filter/order changes, and is empty after a restart. The grid
  layout and sort (`settings.grid_layout`, `settings.grid_sort`) survive a restart; an unknown value
  from an old database is read as the default. Switching the search mode resets the results and says
  so in the status.
- **Vocabulary:** file, snapshot, video, tile, group, face, tag, model, pass, index. One notion — one
  word in every view.

### Filters

- **The path/date field** is literal, not smart: the typed text is a case-insensitive substring of
  the file path or of the ISO date, so "2024-05" finds May 2024 and "vacation" finds a folder or a
  file whose path holds that word. The hint stays "2024-05, vacation"; the tooltip says the same in
  words and names the field of the content search. A year click no longer writes into this field
  (M25.2): the year cloud is a filter of its own, and the chips row draws the word and the year as
  two chips with two "×".
- **The content search is another field:** "Semantic / By text" in the top bar searches the meaning
  (CLIP) or the recognized text (OCR), never the path.
- **The location row** shows what the file itself knows: the EXIF coordinates to five decimals and
  a copy target. No place name, no map link and no network call (M25.9, ADR 0049): EXIF carries the
  coordinates only, a name needs a reverse geocoder, and the hint says so.
- **The two "missing" chips** ("No date · N", "No preview · N") are toggles over the whole index,
  not over the current filter, and they narrow the grid together. "No date" is a file without any
  date; "No preview" is a file whose preview could not be built or is gone from the cache — a
  HEIC/AVIF the decoder cannot read, a broken file, a video without a cover, or previews cleared in
  "Settings → Storage". The hint names the four causes and says that indexing the folder again
  rebuilds them; at zero the chip is dimmed and does nothing.

## Findable in a second

The rule: a needed function is visible without scrolling and without hovering and is done with
**one click**; the time to the first correct click is up to 1 s for a frequent action, up to 10 s for
a rare one.

| What | Where | Clicks |
|---|---|---|
| Folder, "Index", "Refresh" | left panel, the catalog block, each with a tooltip that says what the pass reads and writes (M25.7) | 0 |
| Progress and "Stop" | top bar, next to the start | 1 |
| Search (semantic / by text) | top bar, row 2 | 1 |
| Six views | left panel, two groups with numbers | 1 |
| Copy comparison and manifest | "Integrity" → fields and buttons on the screen | 1 |
| Filter by path/date, "All / Photos / Videos" | left panel, the "Filter" section (the field's tooltip says what it compares) | 0–1 |
| One year of the archive | left panel, the "Years" cloud: the five newest as chips and a "…" for the older years; the picked one in the accent, "All years" is the way back | 1 |
| Active filters and reset | above the grid, chips with "×" | 1 |
| Filter by a tag seen on a photo | the tag chips of the file card: a click on the word adds the tag to the filter, a second click removes it (the filter accumulates; the "×" removes the tag from the photo) | 1 |
| Selecting several files | Ctrl/Shift+click on tiles, "Select all (M)" above the grid | 1 |
| Albums: cards, tree, subalbums, order | left panel "Albums (N)" → the cards of the top level in the content panel, the tree inside an album; "+ New album", "+ Sub-album" | 1–2 |
| Putting a photo into an album | the dashed "+ Add" of the file card, or "Add to an album… (N)" in the selection bar | 1–2 |
| Tile size (the "Tiles" pill) | in the top strip, right of the search field (M22.19) | 1 |
| Cell view (tile / rows / list) | there too, next to the size slider | 1 |
| Layout and sort | there too: "By months / Flat" and the "Sort" menu | 1 |
| The main action of the view | left panel, the first group after navigation | 1 |
| Totals and counters of the view | in the view title and in the navigation numbers | 0 |
| File card | right panel | 0 |
| Models, limits, programs, the tag dictionary | "Settings" in the top bar, sections in the dialog | 1–2 (rare) |
| The tag filter threshold | left panel, pinned under the filter block, above the tag cloud (M25.6) | 0 |

- **One main action per screen**: "All files" → "Index"/"Find"; "Duplicates" → "Find groups";
  "Faces" → "Find faces (N)"; "Integrity" → "Compare the copies"; in the "Settings" dialog →
  "Download model" (if not downloaded).
- **Left panel budget:** the folder block, two navigation groups (7 views), the filter rows
  (path/date, "All / Photos / Videos", the two "missing" chips) and the tag threshold are pinned;
  the tag cloud opens the scrolling part and the years follow it — M25.6 undoes the M22.3 order on
  purpose, so the slider never leaves the chips it cuts. At 840 px the threshold stands at 621 px
  and the "Tags" heading at 685.5 px without a scroll (checked by
  `app::tests::the_left_panel_pins_the_threshold_above_the_tag_cloud`; on the corpus the years
  heading lands at 941 px in the owner's 1021 px window, the bottom strip at 812 / 993 px). A new
  function is not added to the panel if it already scrolls when nothing is selected: move it into
  the view screen, into "Settings", or replace an existing one.
- **A number next to an action** explains the cost before the press: "435 candidates", "still
  without faces: 0 of 1972", "135 marked spare, 214.1 MiB will be freed".
- **The keyboard is an addition**, not an entrance: no action is available only on the keyboard and
  none only on hover.
- **Check:** the owner's tasks and a stopwatch
  (`.pi/skills/ui-design/references/nayti-za-sekundu.md` §7). A failed task is a findability defect.

## Metrics (change only together with docs/UI.md)

| What | Value | Where |
|---|---|---|
| Cell view "Tile" (default) | one tile width from the "Tiles" slider (100–260 px, 150 by default), the columns share the available width; a 4:3 frame + a 22 px caption band; the preview covers the frame (a centring crop, no stretch — M22.15) on a `field` well, the body on `card`, the caption on `panel`, a 1 px `border` and the 8 px card radius; a video gets a "▶ M:SS" bar at the bottom of the frame | `app.rs::draw_tiles`, `draw_cell`, `cover_rect` |
| Cell view "Rows" | the row height comes from the slider and never falls below 70 % of it; the row fills the available width, so a tile's width follows its frame's aspect; the preview fills the tile (no crop, no margins) | `app.rs::draw_flow`, `row_fit` |
| Cell view "List" | the row height `24 + 0.16 × slider` (40 px at 100, 66 px at 260), a 4:3 thumbnail `row − 8` high, the name at 12–14 px, the date · size in mono 10–12, the kind/duration on the right in mono; the two text lines are centred as one block and never leave the row; gap 4 px | `app.rs::draw_list`, `list_row_height` |
| Grid step | 10 px between tiles inside a row and between rows (the mock-up's `gap-2.5`); other screens live on the common `item_spacing` 8×6 | `app.rs::draw_tiles`, `draw_flow` |
| Month heading | "May 2024" at 14 px on the left, "· 132 files · 5.4 GiB" at 11 px next to it, 4 px before the tiles and 16 px after the group; the heading of the group under the top edge of the grid area is pinned there on the canvas tone and pushed out by the next group's heading (M22.4) | `app.rs::month_header`, `draw_sticky_month` |
| Content panel | one 44 px ribbon (M22.19): the view title 18 px with the facts 12 px mono on the left; on the right the controls as 20 px buttons — the three cell views, "By months / Flat" and the "Sort" menu (11 px label) — divided by 1 × 16 px lines; the chips row with "×" and "Reset" appears only while a filter is on | `ui()` → `draw_filter_chips` |
| Navigation item | 28 px high, a 14 px icon 8 px from the left edge, the 13 px label 6 px after it, the 11 px number on the right; the selected one — the `field` tone and accent text (M22.3, the mock-up's row without a bar) | `app.rs::nav_item` |
| Action button | 26 px high, 10 px padding, 4 px radius: a 14 px icon, 6 px, the word of 13 px; the one main action of a screen — the accent fill and the text on the accent; the OS-trash path — a 1 px `danger` border, no fill; a 20 px compact variant for a strip switch, for a dense row and for the two actions of the file card (M22.16, so they stay on one line in both languages); the icon takes the colour of the word, the disabled state takes `text_dim` | `app.rs::WordButton`, `word_button` |
| Icon | 14 px (12 px in a compact/small button), a 1.5 px stroke, the box = the glyph: the set is the mock-up's Material Symbols by name, drawn by `paint_icon` — the player signs (⏮ ▶ ⏸ ⏭, sound) were the first of it | `app.rs::Icon`, `paint_icon` |
| Details row | an 11 px label on the left in a 72 px column, a 12 px value with wrapping | `app.rs::meta_row` |
| Face tile | 150×150 px + a 26 px caption (the metrics do not change, M22.9); a 112×112 crop aligned by 5 points; two round 24 px icon targets — the empty tick circle at the top left, filled with the accent and a tick when picked, and the head with a slash at the top right, drawn only inside one group (M22.26); under the frame the date in mono 10 `text_med` on the left and the detection score in mono 10 `text_dim` on the right; a picked face and the selected file carry a 2 px accent frame; the tile is a `card` with an 8 px radius | `app.rs::draw_face_cell`, `tile_icon_target` |
| Duplicates group | one `card_block` per group: the `card` tone, a 1 px `border`, an 8 px radius, 8 px inside; the row "#N" in mono 11 `text_dim`, the reason at 13 px strong, the facts "N files · X spare" in mono 11 at the right edge; the advice at 11 px `text_dim`; the tiles lie on the card tone; no red on this screen | `View::Dupes`, `app.rs::card_block` |
| Trash | the counter "N files · size" in mono 11 `text_dim`; "Restore all" in the neutral tone; "Empty the trash (N)" with the one `danger` border in the window and no fill; the tiles marked "in trash" with a round 24 px restore target, its word in the hint (M22.27) | `View::Trash` |
| Face crop in the cache | 112×112, JPEG, key `<sha256[..24]>-<number>` | `faces.rs` → `<cache>/faces/` |
| Calendar cell | years (M25.10): cards of 92 px with a 6 px gap in rows of four — the year at 12 px, its counter in mono 10, a 16:10 cover of the year's first snapshot fitted whole, the volume bar against the fullest year and its size; above them the caps line "THE YEARS OF THE ARCHIVE" and the number of years · files · size in mono 10. months: 4×3 month cards of the same height — the mono number and the month's name, the counter, the cover, the volume bar and the size, above the caps line "12 MONTHS OF 2024" and the facts of the year. weeks (M25.11): 4–6 week tiles of the picked month, three to a row, with the date range ("12–18 May", "29 Apr – 5 May" for a week that starts in the previous month), the counter, the cover and the bar. days (M25.11): the seven days Monday–Sunday of the picked week, each with the weekday and the date, the counter, up to three thumbnails and the bar. A cell holds files → `card` tone and a 1 px line, is empty → the `panel` tone, is picked → the `accent_soft` fill and a 1 px accent frame. A double click on a year/month drills down a layer | `View::Calendar`, `app.rs::cal_cell`, `paint_arch_card`, `cal_bar` |
| Calendar photos of the period | under the tiles of every layer (M25.12): a hairline, the heading at 14 px ("May 2024", "12–18 May", "2024", "Wed, 01.05.2024") with "· N files, size" at 11 px in mono, then exactly the files of the picked period drawn by `draw_files` in the ribbon's cell view — the year, the month, the week or the day. Only the day layer keeps the time in the caption instead of the repeated date (`caption_date`); every other layer shows the date | `App::ordered`, `draw_files`, `day_heading` |
| Refused video (full screen) | when the built-in player cannot open a file, the frame area becomes a card on the `card` tone with all four corners cut and a 1 px `border`: a warning glyph and "The built-in player cannot open this video" at 14 px, the file name in mono 11, the reason (the container or codec, in mono 11, wrapped) and three worded actions — "Copy the reason", "Play with ffplay in a separate window" (only when `ffplay` is found) and "Open in the system player", each with the hint "the video leaves the app — a separate window"; nothing is started by itself, and ▶ in the playback bar retries the built-in player (M26.12) | the codec is out of scope or the file is broken; the reason and a way out without hunting |
| Full-screen view | a modal window over the whole window; the header line ("Back to the grid", the name 14 px, the date and the facts in mono 11, "N of M" in the accent and the mock-up's "×" at the right — the close target of the screen) and the tools row ("Zoom out" · the percentage in mono · "Zoom in" · "Fit" · "1:1" · "Rotate" · "Text on the photo" · "Show faces" · "In the system viewer" · "Inspector") — every tool has a word and a hint, there is no icon-only path; the 320 px thumbnail is not stretched; a 1920 px frame from the preload queue; 100% = fit, 1:1 = one image pixel per screen pixel (zoom 50–800 %); above 100% the frame is dragged with the left button (the cursor is a hand, the hint "drag to move the frame" sits in the tools row) and never leaves the area; "Rotate" turns the view by 90° (the file on disk is unchanged) and a new file starts at the centre and unturned; the playback row: icon buttons ⏮ ▶/⏸ ⏭ of 32 px, ←/→, "Slideshow" with a 3/5/10 s interval, "Text on the photo" | `App::viewer_modal`, `draw_viewer_image`, `paint_frame` |
| Series strip | under the frame: the caps label "SERIES", the day in mono 11 and "· N files", then up to 16 neighbours of that day — a 64×48 plate on the card tone with the preview fitted whole, the position inside the day in mono 10 on a dark plate, "▶" for a video; the current one carries a 2 px accent frame and an accent number; a click opens it; a file without a date has no strip | `draw_series_strip`, `App::viewer_series` |
| Face chips in the full screen | a tool, off by default: "Show faces" in the tools row (the silhouette glyph) draws a 1.5 px accent box for every face the model found on this file and, above it, a dark plate with the name of the group (or "face") and the detection score (`Lisa (92%)`); the boxes follow a turn of the view; the button is disabled with the hint "no faces found in this file" when the model found none; nothing is drawn while the faces are not computed | `paint_face_chips`, `db::load_face_boxes` |
| Face panel in the full screen | the right column of 280 px, opened by a click on a face box and closed by a click on the empty part of the frame or by its "×": the same 44 px band with the face glyph and the name of the group, the count ("N photos") and every photo of that face, oldest first — a 56 px row with 4 px of air, a 64×48 thumbnail, the name at 12 px, the date and the size in mono 10; the frame on screen carries a 1.5 px accent outline; a click on a row opens that photo in place; an ungrouped face says "This face is not in a group yet." and lists only the frame on screen | `App::viewer_face_panel`, `App::face_focus_for` |
| Inspector in the full screen | the right column of 280 px, off by default, toggled by "Inspector" or I: the same file card as the right panel, scrolled, for the frame on screen — one card for the whole interface; the face panel takes this column while a face is picked and the card comes back when it closes | `App::file_card`, `App::viewer_modal` |
| Video in the full screen | a 320 px cover frame + a big ▶ button in the centre (a 76×76 plate, rounding 4, the label "video · press ▶ to play"); the built-in player plays it: ▶/⏸, seek, sound, "Stop the video"; the end of the file opens the next one | `App::viewer_modal`, `src/player.rs` |
| Preview in the cache | 320 px on the long side, JPEG q≈80 | `index.rs` → `derive::analyze_image` |
| Tooltip preview | up to 400 px on the long side, proportions kept | `draw_cell` |
| Left panel | 260 px, width not draggable | `Panel::left(...).exact_size` |
| Navigation row | 28 px, a 14 px icon and the word at 13 px, the count in mono 11 at the right edge, radius 6; the active row sits on the `card` tone with a 2 px accent edge on the left and its word and number in the accent — the same mark as the settings section list (M22.11); hovering shows the card tone as well | `app.rs::nav_item` |
| Year chip | 18 px, mono 11, the width from the text, 4 px apart, wrapping — the geometry of a tag chip, so the cloud stays dense; idle — `field` fill and `text_med`, picked — `accent_soft`, an accent border and accent text; no number inside (the count is the hint "2019 · 1 234 files"); the five newest years and, when older years exist, a "…" whose hint says "older than 2019 · N files · 2004–2019"; "All years" appears only while a year or the "…" is picked and clears the pick (M25.2) | `app.rs::year_chip`, `cloud_chip`, `App::years_block` |
| Tag chip | 20 px high, mono 11, the word and the count of snapshots after a middle dot ("furniture · 669"), 4 px apart, wrapping; idle — `field` fill and `text_med`, picked — accent fill, accent 1 px border and accent text; the width follows the text and never the state, so a click does not move the row | `app.rs::tag_chip` |
| Filter toggle chip | 20 px high, mono 11, the word and the count after a middle dot ("No preview · 2"), 4 px apart, wrapping; idle — `field` fill and `text_med`, picked — `accent_soft`, an accent border and accent text; at zero files it is dimmed in `text_dim` and does not react to a click (M25.4); the count is over the whole index, never over the current filter | `app.rs::toggle_chip`, `App::has_no_preview` |
| Card chip | 20 px high, mono 11, 7 px of padding on each side; the word, an optional 12 px glyph before it (the album's folder) and the "×" after a 4 px gap; two targets in one box — the word (the tag filter, the album) and the "×" (removal, `danger` under the pointer); idle — `field` fill and `text_med`, the tag the grid is filtered by — `accent_soft`, an accent border and accent text | `app.rs::card_chip`, `ChipClick` |
| Right panel | 280 px, its frame with no margin of its own so the 1 px line stands on its left edge; a 44 px header band on the `card` tone with the info glyph, "File details" at 12 px strong and the 22 px "×" 12 px from the right edge, a 1 px line under it; closed by that "×" (hint) and by the details toggle of the top strip (M22.19; its hint is "Details" / "Hide details"); hidden while the window is narrower than 1200 px — then the card is in the left panel | `Panel::right`, `App::details_panel` |
| Preview in the card | the panel width minus 24 px of air, 4:3, no higher than 210 px, the frame fitted entirely, the `canvas` well, an 8 px radius, a 1 px `border`, all four corners cut; a dark corner badge — "▶ M:SS" for a video, the format ("JPG", "HEIC") for a still; below the name mono 12 in the accent and the path mono 11 `text_dim`; "Open full screen" and "To trash" as 20 px buttons in one row in either language | `App::file_card` |
| Own-tag row | one line in a framed `field` row (a 4 px radius, 8 px/2 px inside, M22.16, 28 px high with its 1 px line): the "`#`" in mono 12, the frame-less field takes `available − 16 − 22 − 8 px` (never below 40), the "Add tag" button is an icon-only 22 px square with the `add` glyph and the hint, and both are 22 px high with the text centred on the line (M22.18 — the theme's 26 px field with its text at the top made the input stick to the upper edge); the field is ~190 px of the 255 px card and the row never widens the panel (M22.12) | `App::file_card` |
| "Settings" dialog | 760×620 px of content inside the dialog frame, a 200 px section list on the left, fields up to 520 px; above the list the caps 10 px line "SETTINGS SECTIONS"; a section row 28 px with a 14 px icon, the 13 px label and a mono 10 value at the right edge that is dropped when it would touch the word; the active row on the `accent_soft` fill with a 2 px accent edge on the left; the pane title 14 px, a 1 px rule under it | `App::settings_modal`, `app.rs::settings_nav_item`, `SettingsTab::icon` |
| Dialog | one `dialog_frame` for all seven: the `card` tone, a 1 px `border`, the 8 px radius and 16 px of air inside; the title 14 px, the body 13 px, hints 11–12 px; at most one accent button — the dialog's main action — and no accent at all on the settings dialog, where every change is saved at once; the trash confirmation is the one dialog whose button moves files, so it carries the `danger` border and no fill; a number stands in a facts row of its own in mono 11, never inside a translated sentence; a report body is mono 11 in its own scroll area with a copy button (M22.10) | `app.rs::dialog_frame`, the `*_modal` functions |
| Caption under a cell | a 22 px band on the `panel` tone: the file name (mono 10, left) and the date `YYYY-MM-DD HH:MM` or "no date" (mono 10, `text_dim`, right); the name stays on one line and is cut with an ellipsis at the edge of the date — a long name never wraps over the preview and never runs over the date | `draw_cell`, `caption_name` |
| Top strip | exactly `TOP_H` = 32 logical px at any `pixels_per_point`, plus its 1 px bottom line drawn on the last row, one row, `panel` tone (M22.19, after `photocat_1`; the height is a constant of the layout and no child may take it — M26.4); the search well is 28 px high (a 20 px row with 3 px of air and a 1 px line) and at most 646 px wide (the mock-up's 576 plus the ~70 px of the state pill that left in M25.8), centred between the mark and the tools; the "Tiles" pill is 26 px high — 14 px of glyph, a 6 px gap and an 84 px slider; the details toggle (28 px) and "Settings" (26 px) on the right; a pass row or the reminder adds height under the one row and leaves it at 32 px afterwards | `App::top_bar`, `App::search_field`, `App::tile_pill` |
| Bottom strip | 28 px, `panel` tone, 1 px top line, mono counters: a dot and the last fact, then — while an external program does not start — a 16 px amber warning target 2 px after the state word (M26.2, the glyph `Warning`, the hint "some features are limited — click for the details"), then the count of files and their volume; on the right the selected file (name · frame · size) | `App::status_bar`, `icon_button_tone` |
| Selection | a 2 px accent outline on the selected tile and a square accent check flag; while anything is picked, a floating bar at the bottom edge of the grid (the mock-up's pill): a dot, "N selected · size" in mono 11 in the accent, then "Add to an album… (N)" while albums exist (M22.20), "Select all (M)" and "Clear selection" (M22.19); it lies over the grid on its own layer and never moves the tiles | `draw_cell`, `App::ui` (the "pick-bar") |
| Album tree | a row 26 px, a 14 px indent per level, a 22 px triangle, a 14 px folder glyph and the name at 14 px; "N files · M subalbums · size" at 11 px next to it; the whole row is one click target (the glyph included); the open album keeps the `accent_soft` fill and the 2 px accent edge; the tree area no higher than 190 px | `draw_album_tree`, `album_tree_row` |
| Album header | above the actions of an open album: the name 14 px strong, the facts next to it in mono 11 — "N files · M sub-albums · size" and " · N in the trash" when it is not empty; the view heading keeps "Albums" with the count of albums, so the name is printed once | `app.rs::album_header` |
| Album card | the width from the "Tiles" slider (100–260 px) and 10 px of air on every side (M22.20, after `photocat_3`): a 16:10 `canvas` cover with a mosaic of up to four of the album's first photos, every photo filling its cell edge to edge (M22.21; cropped on the sides that do not fit since M22.29, so a cell has no margins: one fills it, two split it into columns, three give the first a half and the others the right stack, four a 2×2 grid, 2 px seams; the corners of the cover are cut to its 4 px radius; a folder glyph when it has none) and two dark plates over it — "N sub-albums" at the top left, "N files" at the bottom right, both mono 10 on black 60 %; the name 13 px, then "size · date" in mono 10; the `card` tone with a 1 px `border`, the accent frame and the accent name on the open album, the accent line under the cursor; 10 px apart; inside an album the cards take one row, two at most, the rest scrolls, and the grid keeps 220 px | `app.rs::draw_album_cell`, `draw_album_cards`, `album_card_size`, `cover_badge`, `cover_cells`, `paint_cover_mosaic` |
| Album list row | the "List" look of the albums: full width, 38–57 px high following the "Tiles" slider (`26 + 0.12 × value`), a 16:10 mosaic cover 26–48 px high in the `canvas` well (M22.21, cells filled and cropped, M22.29), the name 12–14 px and "N files · M sub-albums · size · date" in mono 10–12; the accent edge and the `accent_soft` fill on the open album | `app.rs::draw_album_row`, `album_row_height`, `paint_cover_mosaic` |
| Album control row | under the ribbon of the "Albums" view, right to left: the filter field 150 px ("Filter the albums"), the "Sort" menu, the "Cards / List" switch at the 20 px rhythm and the primary "+ New album"; every control fits the content panel at 1240 px. The ribbon's cell views are hidden on this screen (M22.23), so this switch is the one look switch: it changes the album cards to rows and the album's photos to tiles or a list, and leaves the global cell view of the other screens alone | `App::ui` (the `View::Albums` branch), `album_cards_of`, `album_sort_label`, `App::grid_cell_style` |
| Album breadcrumb | above an open album: "All albums" and every ancestor as a 20 px word button with a folder glyph, a "›" between them in `text_dim`; a press opens that level, the root is the card grid | `App::ui` (the `View::Albums` branch) |
| "Integrity" screen | the `card`-free form of the content panel (canvas tone, 12/6 margin); the "Folder A/B" and "Manifest file" fields 460 px wide in the numeric family with labels on the left; the "Show all paths" tick; the verdict at 14 px in the accent / red / grey; the report monospaced at 11 px in a scroll area; the reminder row "Remind me to check every [30] days" (0 — off), the date and result of the last pass, the `verify-last.txt` path and "Copy the last report" at 11 px | `App::integrity_panel` |
| Reminder banner | in the top bar, below the search rows: "Time to check the archive" at 13 px in the accent, a fact about the manifest and the deadline at 11 px in grey, the "Check" and "Remind later" buttons; it starts nothing itself | `App::verify_banner` |

Colour comes only from the `Theme` tokens (M22, ADR 0044); the screens write a colour by hand only
for text over a darkened frame, where the background is a photograph.

## Screens

| Screen | What is on it | Main action |
|---|---|---|
| Grid (`View::Grid`) | the top strip: the mark, one search field (mode switch, query, "Search", "×"), the "Tiles" pill, the state, the details toggle, "Settings"; the left panel, top down: the "Catalog" block (a "+" in its heading, the folder field, "Index", "Refresh"), two navigation sections with numbers, the "Filter" section (path/date, "All / Photos / Videos", the two toggle chips "No date · N" / "No preview · N" under them), then the pinned "Filter threshold" row — the 0–100% percentile slider with its notes, above the scrolling part; in the scroll: the tag cloud under the caps "Tags" with the count of snapshots after a middle dot ("furniture · 669"), the picked chips in the accent (the filter accumulates, M25.1), the "×" at the right of the heading that resets the set (M25.6 puts the cloud first, the threshold cuts it), then the "Years" cloud (the five newest years as chips, the "…" for the older ones, M25.2), then the model block (the text block moved to the settings, M25.13); above the grid — one 44 px ribbon: the view title at 18 px with the facts at 12 px in the numeric family on the left, the three cell views, "By months / Flat" and the "Sort" menu on the right, and the filter chips with "×" on a row of their own only while a filter is on; the model block keeps the state and "Compute tags" only (M25.6): the model itself is chosen in Settings; "By months" — the grid in groups ("May 2024 · 132 files · 5.4 GiB") from newest to oldest, "Flat" — one ribbon without headings; three cell views ("Tile" by default, "Rows", "List") in the header; step 10 px, an 8 px rounded tile card with a 22 px caption band, a video comes with a cover frame and a "▶ M:SS" strip; while anything is picked — the floating bar at the bottom edge of the grid with "N selected · size", "Select all (M)" and "Clear selection"; on the right — the details of the selected file; the bottom strip: a dot, the last fact, the counters, and the selected file on the right | find and open |
| Duplicates (`View::Dupes`) | a threshold slider, "Find groups", the summary "in groups N, marked spare M, X will be freed"; every group is a card of the `card` tone with "#N", the reason at 13 px, the facts in mono 11 and the advice "keep <name> — …", the pictures inside; every tile has a "keep" button, the others are marked "spare" | decide what is spare and export the plan |
| Calendar (`View::Calendar`) | the tabs "Year/Month/Week/Day", "← … →" (the years step through the archive's years, the months only through months with files, the week and the day one by one), "Today"; the years layer — one card per year of the archive, newest first, with counters, covers, volume and sizes (M25.10); the months layer — 4×3 month cards of the picked year with counters, covers and sizes above the line "12 months of 2024" and the facts of the year; the weeks layer — 4–6 week tiles of the month (M25.11); the days layer — the seven days of the week (M25.11); a double click on a year/month drills down a layer, a single click selects (an accent frame); under every layer — the selected day as a list with the time, the name and the size (a double click on a day ends the drill-down and opens its photos there), and "No date"; an empty year says "No files in this year." and the button "Go to <the nearest month with files>" | find a day and look at its snapshots |
| Faces (`View::Faces`) | the merge threshold, "Find faces (N)" / "Resume (N)", "Rebuild groups" and the "all together" switch in the left panel; the right panel with the 44 px band (the face glyph and "Groups") and the list of groups — "All people · N", then every group with the best face of the group as its icon, a "together" tick, its name and photo count; the open group's name at 14 px with its facts in mono 11; the face crops of the group with the date and the score in mono 10, the round tick circle and the head with a slash on every face tile inside a group, "Name the picked people", the name field and "Save the name", the "Photos where everyone is together (N)" title at 14 px; the ribbon has no cell-view switch and no details toggle (M22.26) | name a person, split off strangers and find photos where people are together |
| Albums (`View::Albums`) | the control row: the filter of the album names, the "Sort" menu ("Album order", "By update", "By name", "By file count", "By size"), the "Cards / List" switch and the primary "+ New album"; the ribbon's cell views are hidden on this screen, so this switch is the one look switch — it changes the album cards and the album's photos together (M22.23); at the root — the cards of the top level albums; inside an album — the breadcrumb "All albums › …", the tree with indents and collapsing, the album's own header (the name 14 px, the facts in mono 11), "+ Sub-album", "Rename", "Move to…", "Up / Down", "Delete the album", the sub-albums as cards (their mosaic cover, name, counters and date), and a grid of snapshots in album order, "In the album / All files", "Add the picked ones (N)", "Remove from the album (N)", "File up / down", "Export to disk…"; the chip "Album "Name" ×" in the chips row (the floating bar is for the selection only, M22.19) | build a tree and an order, add snapshots, export copies to disk |
| Add to an album (dialog, `App::album_pick_modal`) | the title, the facts of the selection ("N photos") in mono 11, the hint "The photos go to the end of the album…", the filter of the albums, the tree as selectable rows with the file counts, the field "or a new album:", the primary "N photos" and "Cancel" — one accent button, the dialog frame of M22.10 | put one photo or a selection into an album, or make an album out of them |
| Album export (dialog, `App::export_modal`) | "Folder" (must exist), "File name" (`{n}_{orig}`), a hint about the substitutions, "Show the plan", "Export N files", "Close"; below — a summary and the tree "album / subalbum / 001_name.jpg"; after the pass — a report line and "Copy the report" | get a real folder with copies in album order |
| Limited features (dialog, `App::limited_modal`) | the title row — the amber `Warning` glyph and "Some features are limited" at 14 px strong — and one line saying that only the work of the missing programs is off; one block per program: its name in mono 12 strong, the state line of "Settings" in mono 10 (the version or the error, the file found, why it is needed), "without it: …" at 11 px; a rule, then "How to install on <system>:" at 12 px strong and the commands in mono 11 on the `field` tone, one line each, with the language-pack note at 11 px in `text_dim`; the download pages, when the system has no package manager here, as plain words under the line "named here only, nothing is opened"; then "photocat installs and downloads nothing itself and opens no browser" and the path to "Settings" at 11 px; two buttons — the accent "Copy the commands" and the neutral "Close"; 560–640 px wide, no field, nothing is installed (M26.2) | an external program is missing: understand what is lost and get the command without a terminal |
| Trash (`View::Trash`) | the counter and size in mono 11, "Restore all" in the neutral tone, "Empty the trash (N)" in the one `danger` border with confirmation, tiles marked "in trash" and a round 24 px restore target on the tile and in the list row, the word in its hint (M22.27) | return a file or deliberately move it to the OS trash |
| Integrity (`View::Integrity`) | the "Folder A (archive)", "Folder B (copy)", "Manifest file" fields; the "Show all paths" tick; the "Compare the copies", "Write the manifest" (with an existing file — "Overwrite the manifest" and "Cancel"), "Check against the manifest" buttons; the row "Remind me to check every [N] days" (0 — off), the date and result of the last pass, the report path `verify-last.txt` and "Copy the last report"; the verdict in words, a report in the same format as the CLI, and "Copy the report" | compare a backup or check the archive against the manifest without a terminal |
| "Folders" (dialog) | a tick box on every path (ticked — shown; cleared — off: the path dimmed and the word "off", with the rows, the previews and the files on disk kept), the count of indexed files, "Remove from the index", "Add a folder…", the built-in folder picker, "Index all" | add several folders without typing paths by hand, and hide one of them from every view |
| Folder picker (dialog) | the current folder in an editable "path to a folder" field with "Go" (Enter works too), the drive buttons on Windows (`C:\`, `D:\`, … from `GetLogicalDrives`) so another disk is one click away, "Up" (absent at a drive root), "Add and index" / "Add without indexing" / "Cancel", and the folders of the current folder (files are not listed), "No subfolders." when there are none; a path that is not a folder says so in the dialog | add a folder from any disk without a system dialog |
| Full screen (modal) | the header: "Back to the grid", the name at 14 px, the date and the facts (`18.4 MiB · 6000×4000`) in mono 11, "N of M" in the accent and the "×" that closes the screen; the tools row: "Zoom out", the percentage in mono, "Zoom in", "Fit", "1:1", "Rotate", "Text on the photo", "Show faces", "In the system viewer", "Inspector"; the frame with the face chips over it while "Show faces" is on; below it the playback row ⏮ ▶/⏸ ⏭ (on a snapshot ▶ starts the slideshow), "Slideshow", a 3/5/10 s interval; under the frame the series strip of the day; with "Inspector" on — the file card on the right; with a face picked — the panel of its photos there | look at a snapshot in full, look at a detail at a zoom above 100 %, walk the series of the day and come back to the same place |
| Video in the full screen (modal, the same screen) | a cover frame with a big ▶ button in the centre; after it starts the frame plays in the window, ▶/⏸, "0:12 / 0:22" with seek, the sound sign and a slider, the "no device" label, "Stop the video"; when finished — the next file opens itself | watch a video with sound without leaving the app |
| Settings (dialog, `App::settings_modal`) | on the left, under the caps "SETTINGS SECTIONS", the sections with their icons and their state: "Interface" (EN) / "Интерфейс" (RU) (the language — English or Русский, the choice is saved immediately; in English an untranslated label stays Russian — `plan/I18N.md`), "AI model", "Indexing and duplicates", "Tags", "Faces", "Storage", "Text on photos", "External programs"; inside — the model from the catalogue with the number of features, the licence and a hint (CLIP 512, SigLIP 768), the state "Downloaded · verified", the model folder, the size limit, the consent and "Download model", "Verify the model", "Compute tags", the tag and duplicate thresholds (a 0–100% filter percentile per tag and a separate confident-tag threshold 0.22 + margin; next to it — how many files pass), the tag dictionary editor (a row per tag: enabled, label, minimum percentile, "Remove from the dictionary", 2–3 prompts per line — the tag weight is the best of them; below — the "new label" and "prompt" fields, "Add to the dictionary", "Recompute tags", "Export JSON", "Import JSON", "Restore the defaults", the JSON file path), the thread count, the face model with consent and "Download the face model", the face merge threshold, the used space ("Compute the used space"), cleaning previews/faces/index, the DB/cache/model paths, the `ffmpeg`/`ffprobe` path fields with the version and a "without it" hint, "Find the programs" — one press looks for every program in `PATH` and in the usual places of the system (`%ProgramFiles%`, the WinGet links, the scoop and chocolatey shims; `/usr/bin`, `/usr/local/bin`, `/opt/*/bin`; Homebrew), fills only an empty field and checks it, and the report under the buttons says one line per program, "found ffmpeg 6.1.2 at /usr/bin/ffmpeg" or "not found: tesseract", with the command of the running system and the promise of a second press when something is missing (M26.3) — and "Check and save the programs"; in "Text on photos" — the candidate rule, the `tesseract` path with its version and "without it", the same "Find the programs" with its report and "Check and save the programs", the recognized counts ("with text: N of M photos", "N candidates, M left") and "Recognize documents (N)" / "Resume (N)" / "Stop", with one honest line when tesseract does not start (M25.13); at the bottom "Changes are saved immediately" and "Close" | download/switch the model, set thresholds, the tag dictionary, faces, disk space and external programs |
| File card (right panel, `App::file_card`) | the panel's 44 px header band — the info glyph, "File details" at 12 px strong and the mock-up's "×" at the right — then the card on 12 px of air (M22.16); a 4:3 preview card — the `canvas` well, the 8 px radius, a 1 px `border`, the snapshot fitted whole with all four corners cut, the corner badge ("▶ M:SS" or the format); the identity row — the name in the numeric family in the accent on the left, cut to the room the size leaves (the full name in the hint, M22.25), the size on the right; the path in mono 11 `text_dim`; "Open full screen" and "To trash"; the "Shooting params" `card_block` — a caps heading, the camera after a photo glyph, "Date and time" and "Resolution" in two columns, "Location" with a pin, the coordinates to five decimals and a 20 px copy target (the hint names the four digits of why a place name is absent: EXIF keeps the coordinates only, a name needs a reverse geocoder, the network is forbidden and an offline set is a dependency — ADR 0049), a hairline and SHA256; "Albums" as chips with a folder glyph and a count in the caps row (a click on the word opens the album, the "×" removes the photo from it) and the dashed "+ Add" chip that opens the "add to an album" dialog (M22.20); "AI tags" — the caps row with "CLIP <weight>%" on the right, the line "tag: <name>" or "tag: not sure — the top weights are close", chips "tag 30% ×" (a click on the word adds the tag to the filter, a second click removes it, the filter accumulates and the accent marks it, the "×" removes the tag from the photo, the weight and the percentile are in the hint), the top-3 when unsure and the rest under "all tags (N)", "restore hidden tags (N)"; "My tags" as chips (the same split: the word filters, the "×" removes) and the framed own-tag row ("`#`", the field, the "+"); "Text on the photo" with the "tesseract" engine, "Recognize text" / "Recognize again" and the text block with the match highlighted | understand what this is and fix the tags |

## States

- Empty DB → one line in the grid: "Type a folder and press "Index"", no empty tiles.
- An active filter → a chips row appears under the ribbon: "path/date: … ×", "year: … ×" or
  "older than … ×", "tag: … ×" (one per tag), "photos only ×", "no date ×", "no preview ×",
  "by relevance: … ×" and "Reset"; "×" removes only its own filter, the grid does not shift. No
  filter → no row (M22.19).
- Several tiles selected → each gets an accent outline and a tick, and the floating bar appears at
  the bottom edge of the grid: "N selected · size", "Add to an album… (N)" while albums exist,
  "Select all (M)", "Clear selection" (M22.19, M22.20); the card shows the file clicked last and
  "N in the selection"; the footer shows "N selected · volume". Changing the
  view/filter/search/order clears the selection and writes the reason.
- "Flat" on → one continuous grid without month headings; the same order as in the full screen and
  in the selection: one function `App::ordered`, key `(date, id)`.
- A semantic search is running → the results are a flat list by relevance; "By months / Flat" and the
  "Sort" menu are disabled with the hint "the search sets the order", the chip "by relevance: N
  files" with "×" clears the search and returns the chosen layout and sort.
- The grid is grouped by month → a month without files is not drawn; "no date" is the last group with
  the same heading and count.
- An album is open → the view heading stays "Albums" with the count of albums, the breadcrumb
  "All albums › … › here" stands above, and the album's own header carries the name at 14 px with
  "N files · M sub-albums · size" and "N in the trash" in mono 11; the chip "Album "Name" ×" sits
  in the chips row; the grid shows only its snapshots in its order. An empty album says "There are
  no files in the album yet — switch to "All files"…". "All files" inside an album is the same
  archive, but the selection and the "Add the picked ones" button target the open album. A
  sub-album card opens it with one click; a many-sub-album album scrolls its cards instead of
  pushing the grid off the window. The tree stands inside an album, under the breadcrumb.
- No album selected → the view is the card grid of the top level albums with the control row and a
  hint below when there are no albums at all; the tree appears once an album is open. The delete
  dialog shows the subtree and the number of rows: "delete … — 3 albums, 412 rows; the files on
  disk will stay".
- The album filter hides everything at the current level → "no album with the name "…"" and the
  "Reset the filter" button; the same filter also narrows the sub-album cards and the tree is not
  touched.
- "Add to an album" → the dialog lists the albums with their paths, the drawn files are counted in
  mono 11, the field of a new album is empty; after the press the status says "N added to "path",
  M were there already" and the counts of the cards change in the same frame. No album and no new
  name → "pick an album or type a name for a new one", nothing is written.
- Album export → the dialog has the "Folder" and "File name" fields, the "Show the plan" and "Export
  N files" buttons, below a summary "4 folders · 500 files · 2.4 GiB; in the trash …" and the tree
  "Дача_2024/ Май/ 001_….jpg". Copying → the top bar shows "Export: 173/500" and "Stop"; finished →
  the report line "exported 500 files · 2.4 GiB; skipped already exported 0 (target order stale 0);
  renamed because of a collision 0; errors 0" and "Copy the report". The folder does not exist, is
  inside the archive, or the template has an unknown substitution → a red line with the reason, no
  plan is built, nothing on disk changes.
- Indexing is running → a progress bar in the top bar, the status "indexing <path>…", the buttons are
  active but a second start is ignored.
- A folder is switched off (M26.1) → the folder list shows its path dimmed with the word "off" and a
  cleared tick box; its files leave every view (the grid, the calendar, the albums, the duplicates,
  the faces, the trash, the search and the counters) while the rows, the previews and the files on
  disk stay; the status bar adds "· N files in switched-off folders". Indexing skips the root, and
  switching it back on brings the files back without reading a disk; the rows are never deleted by
  this switch — that is "Remove from the index".
- No preview (a broken file, no `ffmpeg`) → a tile with the text "▶ video · M:SS" (if the length is
  known) or "no preview", without placeholder icons. A video with a cover frame — the frame and a
  "▶ M:SS" bar.
- No files in the filter → "nothing found", the counters stay visible.
- A group decision is made → the group caption changes from "advice" to "decision", the counter
  "marked spare M" is recomputed in the same frame, the spare tiles are dimmed with the word "spare".
- Sharpness not computed (an old DB without re-indexing) → the duplicates panel shows "sharpness not
  computed for N snapshots — index the folder", the advice is computed from the resolution and size.
- The model is not downloaded → the "AI model" block shows "the model is not downloaded" and keeps
  "Compute tags" only: the model's path, size limit and choice live in "Settings → AI model"
  (M25.6); the "Semantic" mode is disabled, next to "Find" it says "the model is not downloaded"; the "By text" mode works if the text has already been recognized; the rest of the
  interface works.
- The model is switched → the old vectors and tags stay in the DB, but search, tags, the filter and
  clusters read only the new one; switching back shows its tags without recomputation. The
  catalogue shows "768 features · 203.5 MiB · Apache-2.0". The confident-tag threshold keeps its
  meaning across models: the weight of a tag is brought to the CLIP scale by the model's own
  calibration (ADR 0041), so 0.22 and the 0.01 lead mean the same thing on CLIP, SigLIP and SigLIP 2.
  SigLIP models answer English prompts better than Russian ones; Russian semantic search is the
  strong point of the multilingual CLIP, which stays the default.
- A model is bigger than the size limit → the row says "over the limit" and "Download model" refuses
  with both numbers (SigLIP 2 needs 393.3 MiB: the limit has to be raised to 400 MiB); a model that
  is already on disk is shown as "present" regardless of the limit, because it works.
- The search mode is switched → the results of the previous query are reset, the status names the
  mode: "semantic search by the content of the snapshots" / "search by the text recognized on the
  snapshots".
- An empty folder (`items` is empty) → the grid says "Type a folder and press "Index"", no empty
  tiles; the filter found nothing → "nothing found" and a "Reset the filter" button, the counters
  stay visible.
- Nothing is selected → the right panel says "Pick a snapshot in the grid — the date, camera, tags
  and text will appear here"; the grid does not change width when the card appears.
- The window is narrower than 1200 px → the right panel is not drawn and the card moves into the
  left panel under the view blocks (in the same scroll): the grid keeps at least 4 columns.
- "Settings" is open → a dialog over a dimmed background; it is closed by "Close", Esc and a click
  outside. Changes are applied and saved immediately, there is no "Cancel" button — there is nothing
  to cancel, the originals do not change.
- An external program does not start (M26.2) → the status bar carries an amber warning mark right
  after the state word (the hint "some features are limited — click for the details"); a click
  opens the dialog: one row per program with its state line and what is lost, "How to install on
  <system>:" with the command of the running system in mono, the language-pack note, the download
  pages as words, the honest line that the app installs and downloads nothing and opens no browser,
  where the path goes in "Settings", the accent "Copy the commands" and "Close". Nothing is started
  and nothing is opened: the command is copied to the clipboard and the status says so. The mark and
  the dialog leave in the same frame the program starts — after a saved or a corrected path. With
  every program found there is no mark.
- `tesseract` is not found → "Settings → Text on photos" says "tesseract does not start — set its
  path above" and the pass button is disabled; the per-photo "Recognize text" in the card is
  disabled too (its hint says why), and the rest works. "By text" in the top bar, with nothing
  recognized yet, points at that pane. The warning mark of the status bar is the second, earlier
  entrance to the same fact (M26.2).
- The face model is not downloaded → the "Faces" block shows "the face model is not downloaded",
  the "Find faces" button is disabled, the "Face settings" button leads to the consent and the
  download; the rest of the interface works.
- A face scan is running → the progress "Faces: 120/1972" in the top bar and a "Stop" button; the
  window does not freeze, after a stop the button shows "Resume (N)", a rerun skips snapshots by
  `sha256`.
- No faces → "No faces yet: press "Find faces" on the left (the face model is needed)".
- A group without a name → the caption "no name · N photos"; after typing a name the "Save the name"
  button renames the group and pins it during a rebuild.
- A false merge → the crop carries a head with a slash at the top right (only inside a group — in
  "All people" there is nothing to split off); the face moves to a new group, the old group keeps
  the remaining faces. The target works without modal windows, the status at the bottom explains the
  result.
- The threshold changed → "Rebuild groups" applies it to unpinned faces; groups with a name and manual
  splits do not change. The counter "faces: N on M snapshots, groups: K" is updated.
- A program path does not start → the path is not saved, the status says "the programs were not
  saved: …" with the reason; "Settings" keeps the previous working path, OCR/video keep working as
  before.
- A program path is saved → the field shows the version and the found file: "ffmpeg version 9.0.2 ·
  /usr/bin/ffmpeg (set in the settings)".
- Recognition is running → the progress "OCR: 120/435" and a "Stop" button in the top bar, the window
  does not freeze, selecting files responds immediately (reading the DB is not blocked by the run's
  transaction); when it finishes the counters "with text: N of M snapshots" and "N left" are updated.
- "Stop" is pressed → the current file is finished, the status "stopped: 120 of 435 recognized…", the
  batch button shows "Resume (315)"; a rerun starts from what is left. The button in the card
  recognizes **only the selected** snapshot; the batch pass is a separate "Recognize documents (N)"
  button, where N is the number of candidates visible before the press.
- A snapshot is selected but there is no text → the card has a "Recognize text" button; after the
  pass either "text recognized: N characters" or "no text found on this snapshot". The button
  becomes "Recognize again" once text exists.
- Text search is on (a tick) but there is no text yet → "Find" finds 0 files and the status honestly
  says "0 files"; the "Recognize text" button is next to it.
- A text search is running → the status "search by text "справка": 3 files in 2 ms"; the match in the
  card is highlighted with the accent, the excerpt is limited to 12 words.
- A model download is running → the consent tick, the button disabled, the progress "files: 2/4", and
  in the status — that no user data is sent.
- The selected model is unavailable → the downloader tries the mirror and compatible variants within
  the limit; the status keeps the result: "model <id>: N files, X MiB (did not fit: …)".
- A model does not fit the limit → the catalogue rows say "over the limit", the download is refused
  with the numbers.
- A tag is removed from a photo → the tag line disappears from the card and the filter in the same
  frame; "restore hidden tags (N)" brings all of them back; the original on disk does not change.
- A calendar year is empty (for example after a filter) → "No files in this year." and a
  "Go to April 2024" button naming the nearest month with files (backwards first, then forwards);
  when paging with the arrows empty months are skipped, an arrow into emptiness is inactive.
- A double click on a year/month drills the calendar down a layer: year → month → week → day, and a
  double click on a day ends the drill-down and opens its photos below. A single click only selects a
  cell — the picked one carries a 1 px accent frame — and the files of the picked period stand below
  the tiles in the ribbon's cell view (M25.12); the "Year/Month/Week/Day" tabs go back up, so going
  up is not hidden in a gesture.
- No files on the "Day" layer → "No files on this day" and the ←/→ arrows page through neighbouring
  days.
- CLIP tags are being computed → the progress "CLIP: 480/1971" in the top bar, the window does not
  freeze; when it finishes the counter "with a confident tag: N" is updated.
- A semantic search is running → "loading the CLIP model…" in the search row; repeated queries
  without a pause, the status shows "N files in M ms".
- A semantic search found nothing → an empty grid, "nothing found" and "Reset the filter"; "Reset" in
  the top bar clears the query itself.
- A tag is not sure (the first weight is below 0.22 or the gap to the second is under 0.01) → the
  card shows "tag: not sure — the first weights are close", no tag is highlighted, the top-3 are
  visible with weight and percentile (for example "document 0.27 · 98%"), the other 11 are under
  "all tags (14)". The file still enters the filter by any tag above the percentile threshold: "not
  sure" is about the caption, not about the search. Inventing a tag is forbidden.
- A confident tag (the first ≥ 0.22 and the gap ≥ 0.01) → the line "tag: furniture", the accent on
  that line; the other weights are visible without the accent.
- The card shows exactly what `photocat eval` scores: the first weight as top-1, the first three as
  top-3, and the same 0.22 / 0.01 rule decides "not sure" and "confident". A change of the card's tag
  logic must be checked against the labelled sample (`docs/ARCHITECTURE.md` § "Labelled check of the
  tags"), not by eye on one photo.
- Every tag chip in the left panel shows the snapshot count at its percentile threshold
  ("furniture · 669"): zero is visible before the press, not after. The number is the promise
  "this click shows N photos", so it counts the photos that carry this tag alone, never the
  intersection of the picked tags. A tag is picked and there are zero snapshots →
  "no snapshots with the tag "furniture" above the 70% threshold (0.23) — lower the threshold slider
  in the left panel" and "Reset the filter"; with several tags picked the grid holds only the photos
  that carry all of them, and the message says so.
- The filter threshold is a percentile of **its own** tag, not of the whole index: 100% is its
  strongest snapshot, 0% is all, 70% (the default) is the top 30%. Therefore at 100% any selected tag
  has at least one snapshot, and a lower maximum of another tag zeroes nothing. "Furniture" at 70%
  has the threshold 0.229, "children" — 0.248: every tag has its own scale.
- On a folder of fewer than 100 files the percentile is noisy → the label "few tags (N files) — the
  percentile is noisy, the raw 0.22 threshold is used", the filter works by the raw threshold.
- Tag confidence (the highlight in the card, OCR, `{tag}` in the showcase) is a separate threshold
  0.22 and a margin of 0.01: the filter slider does not move it. An old `tag_show` above 0.22 is
  migrated when the database is read: confidence 0.22, filter 70%; the archive, the DB and the tags
  do not change.
- The dictionary is changed (a label, prompts, enabling, adding, an import, a reset) → the line "the
  dictionary has changed — press "Recompute tags"" in the accent, the button gets a dot "Recompute
  tags •". The recomputation runs in the background with progress and reads no photos; the status
  says "tags (model): recomputed N/N in M s". After it the filter and the tag counts are updated in
  the same frame.
- A tag is disabled in the dictionary → it disappears from the filter and is not computed; its old
  weights stay in the DB until "Recompute tags" is pressed, but the filter no longer offers that tag.
  A removed tag disappears from `tags` only after the recomputation — the status says so.
- The dictionary is not seeded (the model has not computed tags yet) → "Settings → Tags" shows "the
  dictionary is not seeded yet — it will appear after the first "Compute tags"". "Add to the
  dictionary" works before that too (it does not need the 14 standard tags to be seeded first — only
  your own tag is added).
- A dictionary error (an empty label, a label already taken, a JSON file that cannot be read or
  parsed) → the status in words with the reason; the dictionary in the DB does not change.
- "Restore the defaults" replaces the model's dictionary with the 14 common tags; the owner's edits
  are lost, so the button is labelled "14 common tags from the code; your edits will be replaced".
- "Export JSON" writes `<cache>/photocat/tags.json` if no path is set; the status shows the path and
  the number of tags. "Import JSON" replaces the model's dictionary entirely and asks for a
  recomputation. No network is needed.
- "Integrity": a comparison is running → the top bar shows "Integrity: 120/2230" and "Stop"; the
  window responds. After "Stop" the verdict is "the comparison was stopped: not everything was
  checked", not "matches": what was not checked is not passed off as checked.
- The manifest file already exists → the first press of "Write the manifest" does not write but asks
  for confirmation ("the manifest file already exists: …"), the button becomes "Overwrite the
  manifest", "Cancel" is next to it; editing the "Manifest file" field resets the confirmation and
  the old result.
- The manifest is written → the verdict "manifest written: N files (size) → path"; the path appears
  in the report only when the file has actually been written. A read error or a cancel — "not
  written" with the number of errors, an incomplete manifest never lands on disk.
- The comparison found differences → the verdict "differences found: only A N, only B M, size K,
  content J, errors E"; below the report by section and the list of paths (the first 20 without the
  "Show all paths" tick). "Copy the report" puts into the clipboard the same text that `photocat
  verify` prints.
- The same path in A and B, or a non-existent folder → a red line with the reason, the pass does not
  start, nothing on disk changes.
- "Check against the manifest" without a manifest file → "manifest file not found: …"; the check
  starts only by the button.
- The check deadline has come (a manifest is set, the interval is not 0, at least the interval has
  passed since the last pass) → the top bar shows the banner "Time to check the archive": the
  manifest name, the date of the last pass and how many days overdue, the "Check" and "Remind later"
  buttons. The banner starts nothing by itself; "Check" opens "Integrity" and starts the same
  background check as the button there.
- "Remind later" → the banner is hidden until the next start, the status "the reminder is postponed
  until the next start"; the deadline in the settings does not change.
- Interval 0 days → the reminder is off, the banner is not shown; the value is saved immediately.
- The last pass is in the future (the clock was set back) → the banner stays silent: the deadline has
  not come. A broken date — the opposite, "due": better to remind than to stay silent.
- The manifest is written or a check has finished → the settings hold the date, "ok" and the full
  report, the file `<cache>/photocat/verify-last.txt`; the screen shows the line "last pass:
  YYYY-MM-DD HH:MM · matched / there were differences" and a "Copy the last report" button. Writing
  a manifest also sets the reference date: the archive has just been read in full.
- A check failed or was stopped → a red line with the reason; the previous successful report, the
  date and the reminder line do not change. Nothing is written into the archive folder.

## Keyboard and pointer

| Action | Now | Nearest |
|---|---|---|
| Select a file | a click on a tile | arrows in the grid, Home/End, Enter — open |
| Scroll | the wheel over the grid area | PageUp/PageDown |
| Start indexing | a button | Ctrl+I |
| Refresh from the DB | the "Refresh" button | F5 |
| Focus the search | a click on the field | Ctrl+F |
| Reset search/selection | the "Reset" button; Esc clears the selection; Ctrl+A selects what was found | arrows in the grid, Home/End |
| The main action | a button | Enter in the field |
| Full screen | a double click on a tile or a button in the card | Enter — open the selected one |
| Calendar layers | a double click on a month in "Year", on a day in "Month" and in "Week" | — |
| Paging in the full screen | ←/→, the icon buttons ⏮ ⏭, "Slideshow", a 3/5/10 s interval, a click on a tile of the series strip | on a snapshot space starts/stops the slideshow |
| Inspector in the full screen | the "Inspector" button, the I key | — |
| Face boxes in the full screen | the "Show faces" word in the tools row | — |
| Face panel in the full screen | a click on a face box; a click on the empty part of the frame or its "×" closes it | — |
| Turning the view in the full screen | "Rotate", 90° per click; the next file opens unturned | — |
| Video in the full screen | the big ▶ button in the centre of the frame, the icons ⏮ ▶/⏸ ⏭, seek, sound, "Stop the video" | on a video space pauses and resumes, a click on the frame does the same |
| Moving the zoomed frame | drag with the left button (the cursor becomes a hand); ←/→ inside it pages the files | — |
| Deletion | "To trash" in the card (DB only) | do not assign Delete: files are not deleted |

Rule: no action is available **only** on hover and none **only** from the keyboard; the focus is
visible with a 2 px accent outline.

## Wording

The code keeps the Russian string as the translation key (`i18n::PAIRS`); the English labels below
are what the owner sees in English mode. See `plan/I18N.md`.

- Buttons: a verb in the infinitive — "Index", "Refresh", "Settings", "Find groups", "Export the
  plan", "Copy the list", "Compute tags", "Find", "Reset", "Reset the filter", "Close", "Download
  model", "Verify the model", "Apply the path", "Find the programs", "Check and save the
  programs", "Download the face
  model", "Verify the face model", "Find faces (1972)", "Resume (N)", "Rebuild groups",
  "Save the name", "Recognize text", "Recognize again", "Recognize documents (435)", "Resume (315)",
  "Stop", "Today", "Add a folder…", "Remove from the index", "Index all", "Open full
  screen", "Back to the grid", "Zoom out", "Zoom in", "Fit", "1:1", "Rotate", "Inspector",
  "Slideshow", "Stop the slideshow", "3 s", "5 s", "10 s", "Text on the photo", "Show the
  full text", "Stop the video", "Close", "To trash", "Restore all", "Empty the trash
  (N)", "Move to the OS trash", "Name the picked people", "Unpick", "Photos where everyone is
  together (N)", "Compute the used space", "Clean the previews…", "Clean the face crops…", "Reset
  the index…", "Compute tags", "Add tag", "Add to the dictionary", "Remove from the dictionary",
  "Recompute tags", "Export JSON", "Import JSON", "Restore the defaults", "Add and index", "Add
  without indexing".
- Model catalogue: "217.5 MiB · in place", "148.6 MiB · not downloaded", "over the limit"; the limit
  is "250 MiB" with a unit.
- Search mode: "Semantic" / "By text" (a switch inside the search field, the active one in the
  accent, with a down arrow); the field's hint names the mode ("cake, sea, furniture, document" or
  "receipt, invoice, 2024"); "Search" submits, "×" clears the field and the results.
- Top strip: "photocat", the details toggle with "Details" / "Hide details" as its hint,
  "Settings"; no state pill (M25.8).
- Status bar: a dot and the running pass ("Indexing", "CLIP", "OCR", "People", "Trash", "Export",
  "Integrity"; "N passes" for two or more) with the hint that names all seven and points at their
  progress rows in the top strip; when nothing runs the last status message stays.
- Content ribbon: the view title and its facts, the cell views "Tile / Rows / List", "By months /
  Flat", "Sort"; the filter chips — "path/date: … ×", "tag: … ×", "by relevance: … ×" (a chip has
  only one "×" and the tooltip "remove the filter") and "Reset".
- Month heading: "May 2024" and "· 132 files · 5.4 GiB"; without a date — "No date"; a month with
  zero files is not drawn at all.
- Navigation: "LIBRARY" (All files, Calendar, Faces) and "TOOLS" (Duplicates and series, Trash); the
  number on the right is how many files, groups or faces there are, zero is a number too.
- Card: "File details" at 12 px in the panel's header band, the file name at 12 px, the path at
  11 px, "Shooting params", "Date and time", "Resolution", "Location", "SHA256", "Albums" with its
  dashed "+ Add", "AI tags", "My tags", "Text on the photo", "Selected: <file> · <date>" in the
  status line.
- Albums: "+ New album", "+ Sub-album", "Rename", "Move to…", "Up", "Down", "Delete the album",
  "Cards" / "List", "Filter the albums", "Album order", "By update", "By name", "By file count",
  "By size", "All albums", "In the album" / "All files", "Add the picked ones (N)", "Remove from
  the album (N)", "File up", "File down", "Export to disk…", "Add to an album… (N)", "Add to an
  album", "or a new album:", "Cancel"; a card reads "N sub-albums" and "N files" over the cover and
  "<size> · <dd.mm.yyyy>" under it.
- Date source: "(EXIF)", "(file name)", "(file time)" — the DB holds `exif`/`filename`/`mtime`.
- Semantic: "Text on the photo:", "tag: children (0.24)", "tag: not sure", "with a confident tag:
  1122", "with text: 146 of 1972 snapshots", "candidates for the pass: 435, N left", "text from the
  photo:".
- A duplicate decision is only "keep" and "spare"; the words "delete", "clear", "move" are not used.
- Faces: "faces: 2498 on 856 snapshots, groups: 617", "still without faces: 0 of 1972 snapshots", "the
  face model buffalo_l · 182.4 MiB", "the face was split into a new group; give it a name".
- Statuses: a fact + a number + a unit — "indexed 2230/2230 files (21.2 GiB), 0 errors, in 64.9 s".
- Do not write "file(s)" or "pcs": either a concrete number with the right word, or "files: 238".
- No exclamation marks and no "Success!".
- About the network: "I allow downloading the model weights (photos, paths and queries are not
  sent)".
- About the trash: "untouched on disk", "to the OS trash", "there is no permanent deletion in the
  app".
- About tags: "My tags" (by hand) and "AI tags" (CLIP) are different blocks, not to be mixed.
- Tag counters: "furniture · 669", "mine: dacha · 12", "threshold 70% (0.23 — its own for this
  tag)", "above the threshold: 2129 of 2229 files with tags". Zero is a number too: "screenshot ·
  0".
- About people together: "Photos where everyone is together" — only snapshots that have a face from
  each ticked group, not "any of them".

## What the interface will not have

- A third theme, accent colours, a "compact mode" (the one choice is dark / light / follow the
  system, M22); an icon font (the player's geometric signs and words stay).
- Modal confirmations for non-destructive actions.
- Progress bars that move "approximately": only real `done/total`.
- Emoji icons instead of words, except "▶" for video. The geometric player signs (⏮ ⏸ ⏭, sound) are
  not emoji but `paint_icon` shapes with hints; this exception is recorded in § Layout.
- Automatic model downloads and telemetry.

## Debt against the interface rules

An honest list as of 2026-11-25: the rules above are in force, but the code does not follow them
everywhere yet. An item is either fixed or gets an exception line here with a reason.

1. There is no keyboard navigation in the grid: the arrows work only in the full screen; the Tab
   order is the default one, Ctrl+I/Ctrl+F/F5 are in the "Nearest" column.
2. Progress is shown immediately and blinks on short passes; by the rule — only if the pass runs
   longer than 300 ms.
3. A wrong folder path goes to the status, but the field is not highlighted: the link "error ↔ field"
   is text only.
4. The selected calendar day is reset when paging the month — left there on purpose: the month and
   the day change by one action, otherwise selecting a day in another month would be meaningless.
5. My tags are manual: CLIP does not create them. A free semantic search ("furniture") remains the
   way to find a thing that is not among the 14 hints.
6. The end of a video on the last file does not page round: the view stays on the cover frame, so
   that watching does not start over from the first snapshot. On the other files the end of a video
   opens the next one — that is "change the file automatically".
7. Duplicate decisions are made one tile at a time: the multi-selection (M18) works in the grid,
   in albums and in the trash, but marking N snapshots of a group as "spare" at once is not possible
   — batch actions over duplicates are not promised, that is a separate task.
