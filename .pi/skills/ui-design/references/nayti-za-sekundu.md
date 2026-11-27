# Findable in a second

*Русская версия: [nayti-za-sekundu.ru.md](nayti-za-sekundu.ru.md).*

The project rule: **any needed function is found within one second and one click** from the current
view. If a function requires opening a menu, remembering a key combination or reading a hint, it is
hidden.

## 1. What "in a second" means

A function counts as found when all of the following are true:

1. **visible** — it is present on the screen without scrolling and without hovering;
2. **named** — it is labelled with a word, not an icon;
3. **one click** — from the current view to the action takes no more than one press;
4. **in place** — where it is looked for out of habit: global things on top, navigation at the top
   left, the view action as the first group under the navigation;
5. **not competing** — there is no second button of the same weight with a different meaning nearby.

The time to the first correct click in a test is **no more than 1 second** for frequent functions and
**no more than 10 seconds** for rare ones (settings, model, programs). More — redo it.

## 2. Placement map (the only allowed one)

| What | Where | Distance |
|---|---|---|
| The working catalog | left panel, the catalog pill (M26.7) | 0 clicks (always visible) |
| The catalog's scope, another catalog, the pass | the pill's menu: a row is the scope, "Connect a catalog…" and "Re-index" | 1 click |
| A catalog's own settings (its box, "Index this catalog", "Remove from the index") | the gear of its row in that menu, then the folder list | 2 clicks |
| Progress and "Stop" | top bar, next to the start | 1 click |
| Search (semantic / by text) | top bar, row 2 | 0 clicks, 1 — "Find" |
| Seven views | left panel, right under the heading | 1 click |
| Filter by path/date, "All / Photos / Videos" | left panel, the "Filter" section | 0–1 click |
| Active filters and their reset | above the grid, chips with "×" | 1 click |
| Tile size | above the grid, to the right of the view title | 0 |
| The main action of the view (find groups / find faces) | the screen's own block: the top of the People page (M26.14), the left panel for the duplicates | 1 click |
| The view threshold | in the same block, above the action | 0 clicks |
| View totals (how many groups, faces, texts) | next to the action | 0 clicks |
| The card of the selected file | right panel | 0 clicks after selection |
| Models, limits, tag thresholds, program paths | "Settings" in the top bar, sections in the dialog | 1–2 clicks — acceptable, this is rare |
| Plan export, copying | the "Duplicates" view, under the summary | 1 click |

The one-place rule: the same function is not duplicated in two places. If it is really necessary, it
is duplicated **only** through a hotkey, not with a second button.

## 3. The left panel budget

The left panel is 260 px wide and 840 px high. It fits:

- the catalog block (a caps heading and a 40 px pill), 7 views, 2 filter rows — **without scrolling**;
- the view context — with scrolling;
- the counters and the file card — below, with scrolling.

The rule: **a new function is not added to the left panel if it already scrolls when nothing is
selected.** The options: move it into the view screen, into "Settings", or replace an existing one.
The left panel does not turn into a dump.

Navigation, a path search and the view switcher never scroll: they are the map of the application.

## 4. One screen — one main action

- "All files" → "Re-index" in the catalog menu (global) and "Find" (search); everything else is
  secondary.
- "Duplicates and series" → "Find groups".
- "Calendar" → picking a day.
- "Faces" → "Find faces (N)"; after the pass — "Save the name" for the selected group.
- "Albums" → "+ New album" / "Add selected"; inside an album — "Export to disk…".
- "Integrity" → "Compare the copies".
- "Settings" (a dialog) → "Download model" (if not downloaded), otherwise nothing: the screen is
  reference material.

A second action of the same weight on a screen is forbidden: the user must not choose between two
"main" ones.

## 5. Words that lead

- A button starts with a verb and names its scale: "Find groups", "Find faces (1972)", "Recognize
  documents (435)", "Recognize text", "Save the name".
- The same action — the same word in all views. Not "Show", "Open", "Go to" for the same thing.
- Next to an action — a number explaining the cost: how many candidates, how many snapshots without
  faces, how much is "spare". A person decides whether to press before pressing.
- A disabled button explains the reason in text next to it, not only with a grey colour.

## 6. The keyboard as acceleration, not as an entrance

- Frequent actions are always available with the mouse; keys are an addition.
- Ctrl+I — indexing, Ctrl+F — focus the search, F5 — refresh, Enter — the main action, Esc —
  reset/cancel, arrows — across the grid, Home/End — the edges.
- No action exists **only** on the keyboard.
- Delete is not assigned: there is no deletion.

## 7. The findability check (do it before and after implementation)

1. **A list of the owner's tasks** — 10 phrases in his words: "find duplicates", "show May 2024",
   "where are the receipts", "label Lisa", "how much will be freed", "what camera is this", "find
   the cake", "why is there no preview", "resume recognition", "check the copy".
2. **A paper run**: on a mock-up/screenshot, show where a person will click first. If the path is
   longer than one click for a frequent action — move it.
3. **A stopwatch** on a live window: the time to the first correct click. The norm is from §1.
4. **Five seconds per screen**: look for 5 seconds and name what can be done here. If the named thing
   is not the main action — the hierarchy is broken.
5. **A blind search**: give the name of a function and see whether a person finds it without a hint
   (this is the "intuitive" check).

The result of the check is one line in `plan/STATUS.md`: which tasks failed and what was moved.

## 8. Findability anti-patterns

- A hamburger, "More", a drop-down menu for main actions.
- An icon without a caption in the main panel.
- A setting without which the main function does not work, hidden two screens away.
- Two search lines with an unobvious difference (semantic ↔ text) without an explicit switch.
- An action only on a right click or only on hover.
- A "Stop" button that has to be looked for during a pass.
- Progress without a percentage and without numbers.
- A new tab for every new view: more than seven navigation items is already searching, the eighth
  requires sections ("Library", "Tools"). A view setting is not a tab.
- A panel that appears and disappears with the file selection: the grid must not change width.
- An empty screen without a hint of what to do next.
