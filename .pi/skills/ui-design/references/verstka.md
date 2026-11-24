# photocat layout

*Русская версия: [verstka.ru.md](verstka.ru.md).*

The normative layout rules. The numbers are taken from `docs/UI.md` and the code in `src/app.rs`, not
invented: changing a metric means changing this file, `docs/UI.md` and the code in one change.

egui 0.37 defaults: body `14 px`, heading `18 px`, `item_spacing = 8×3`, rounding 3–4 px. Do not
override the theme; do not introduce colours of our own except the accent.

## 1. Grid and rhythm

- The base grid is **4 px**. Allowed spacing: `2, 4, 6, 8, 12, 16, 24`. No other numbers may appear
  in `add_space` (today the code uses 2, 4, 6, 8, 12 — that is the scale).
- The common interface step is `item_spacing 8×6` (M16); a button is `10×4`, the minimum target
  `32×26`.
- Between related elements — `4`; between groups — `8`; between sections — `12`; before a section
  heading — `8`.
- **The tile grid lives separately:** a `2 px` step on both axes inside the grid area (M16, like
  PhotoPrism). It is set only in `View::Grid` and does not change `item_spacing` of other screens;
  elsewhere the grid "breathes" with the application.
- **Month groups.** The heading "May 2024" at 14 px + "· N files · size" at 11 px, from the newest to
  the oldest; inside a month, the newest on top; "No date" is the last group.
- Everything in a column is aligned to the left, `8 px` from the panel edge. Numbers and counters —
  to the right edge of their column, not "wherever it landed".
- One region — one `ScrollArea`. Nested scrolling is forbidden; the exception is the face group list
  (`max_height(250)`, `id_salt`), because it lives inside the panel.
- Empty space between related blocks must not exceed `24 px`. If there is more "air", it is lost
  density, not style.

## 2. Sizes that cannot be changed alone

| Element | Value | Where |
|---|---|---|
| Default window | 1240×840 | `app::run` |
| Left panel | 260 px, the width does not stretch | `Panel::left` |
| Right panel "File details" | 280 px; hidden when the window is narrower than 1200 px | `Panel::right` |
| File / face tile | 150×150 + a 26 px caption | `draw_cell`, `draw_face_cell` |
| Preview in the cache | 320 px on the long side | `derive` |
| Tooltip | up to 400 px (file) / 300 px (face) | `draw_cell` |
| Face in the cache | 112×112 | `faces` |
| Calendar cell | `(width − 24)/7` × 96 px | `View::Calendar` |
| Video bar | 16 px, 11 px text | `draw_cell` |

The minimum window is 1000×700: at that size the grid must keep **at least 4 columns**. If the tile
metric grows so much that 4 columns no longer fit — that is a defect, not "it got roomier".

## 3. Typography

- Allowed sizes: **11** (the caption under a tile, service text), **12** (text in the frame, tile
  status), **14** (body, the egui default), **16** (the large "spare" decision label), **18** (a
  section/screen heading). Do not introduce intermediate 13/15/17.
- No more than **four** sizes on one screen.
- Two weights: normal and `RichText::small()`/`weak()` for secondary text. Do not use italics, CAPS or
  letter spacing (except the existing CAPS section labels in the left panel: "LIBRARY", "TOOLS" —
  they are part of the current language; do not add new ones).
- A line in the settings and the card is no longer than **80 characters**; input fields are no wider
  than 420 px, long paths — 520 px.
- Long values: a path wraps, a file name in the status is one line with an ellipsis, the full name is
  in the tooltip. `sha256` — 16 characters and `…`.
- Numbers go only through `db::human()`, dates are `YYYY-MM-DD HH:MM`, dimensions are `W×H`. Custom
  formats are forbidden.

## 4. Colour and contrast

- Background, panels, fields, selection come from `ui.visuals()`. Custom constants are forbidden.
- The only colour of our own is the accent `rgb(90, 170, 255)`. It means exactly three things:
  **selected**, **confident tag**, **keep**. Nothing else is painted with the accent.
- The accent — no more than one element per tile and no more than one button per screen.
- Explicit greys: `gray(190)` — the caption under a tile, `gray(150)` — secondary, `gray(210)` — the
  word "spare", `gray(235)` — text over a dimmed frame. Do not go darker than `gray(150)` for
  readable text.
- The "spare" dimming is `from_black_alpha(150)`, the video bar — `from_black_alpha(130)`.
- Borders: selection — 2 px inside, rounding 3–4 px. There are no shadows, gradients or "glass".

## 5. The tile (the main element)

The layer order is fixed and the same in every view:

1. the tile background (`extreme_bg_color`, rounding 3);
2. the preview, fitted entirely with its proportions kept (`fit_rect`), the margins filled with the
   background;
3. for a video — a 16 px bar at the bottom and "▶ M:SS" on the left;
4. the decision mark: "spare" (dimming + the word) or "keep" (outline + the word);
5. a 26 px caption: the date on the left, secondary information on the right (for faces, the
   confidence `0.93`);
6. the selection — a 2 px outline around the whole tile.

Rules:

- A tile takes its place **always**, even when there is no preview: "no preview" or "▶ video · M:SS" —
  text in the same rectangle. No layout jumps while loading.
- Buttons on a tile ("keep", "not him") are always visible, not shown on hover, and do not cover the
  date.
- A double click is not needed for any action: one click = selection, a button = action.
- Text in the frame is no smaller than 11 px and not over a light background without a backing.

## 6. Panels and forms

- Panel order: top → bottom (status) → left → right → central. The left and right panels reserve
  their width before the grid is drawn, otherwise the grid "cringes" on the first frame. The right
  panel is drawn only while the grid keeps 4 columns (a window of 1200 px or wider); otherwise the
  card lives in the left panel, not as a separate screen.
- The top bar has at most **two rows** of controls: (1) the folder + "Index" + "Refresh" + on the
  right "Folders", "Details", "Settings", (2) the search with the segmented mode. The third row is
  only the progress of a background pass.
- The content panel (above the grid): the view title at 18 px and the facts at 12 px; on the right
  "Tiles" at 11 px and a 120 px slider; below, chips of active filters at 26 px with "×" and "found
  N of M". The chips row stays in place when the filter is empty — the heading does not jump.
- Sections of panels and the card are **flat**: an 11 px label in the muted colour, the content, a
  line. `Frame::group` is not drawn around blocks (M16): a frame inside a frame competes with the
  photo.
- Left panel: navigation on top and it does not scroll; everything else is in the scroll area. The
  file card is at the bottom, separated by a divider and the heading "File".
- Settings: a dialog 760×620 px over a dimming; on the left a list of sections 200 px wide, on the
  right a single column of the section (a 16 px heading) with a label above the field; at the bottom
  "Changes are saved immediately" and "Close". No more than one screen of scrolling per section.
- A form does not stretch across the whole width of the central panel: input fields 380–420 px, long
  paths 520 px, the rest up to 720 px.

## 7. Layout states

- An empty view: one line with an action ("Enter a folder and press “Index”"), not empty tiles and
  not "Loading…".
- The filter found nothing: "nothing found" + a "Reset" button; the counters stay visible.
- A long pass: `done/total` progress and "Stop" in the top bar; the grid stays available.
- An error: a line at the bottom, the text "what happened + what to do". Not a modal window, not a
  red plaque over the grid.
- A disabled button: the reason is written next to it ("the model is not downloaded", "tesseract
  does not start"), not just a grey colour.

## 8. Layout prohibitions

- Emoji icons instead of words (the only exception is "▶").
- An icon without a caption as the only way to act.
- Text truncated without a tooltip.
- Scrolling inside scrolling.
- "Cards" with shadows and large radii as decoration.
- Centring long text and "beautiful" line breaks in data.
- Two different caption sizes under a tile in one view.
- Empty space instead of a state (a grey placeholder, a skeleton, a full-screen spinner).
