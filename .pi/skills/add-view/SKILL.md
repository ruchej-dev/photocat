---
name: add-view
description: "Add a new view or filter to photocat end to end: a `View` variant, an item in the left panel, a branch in the central panel, reuse of `draw_cell`, state in `App`, a row in `docs/UI.md` and a check. Use when a tab, viewing mode, type/date/tag filter or a new grouping appears."
license: MIT
compatibility: pi
metadata:
  audience: agents
  workflow: add-view
---

*Русская версия: [SKILL.ru.md](SKILL.ru.md).*

# Add a view to photocat

A view is a way to look at the same index: the grid, duplicates, the timeline. There will be more
views (tags, faces, documents), so adding a view is a repeatable operation, not creativity in
`app.rs`.

## Order

1. **Decision.** Write a row in `docs/UI.md` §Screens: the view name, what is on it, the main
   action. If the view changes the structure on disk or introduces a new filter in the DB — a row in
   `docs/DECISIONS.md`.
2. **Data.** Answer: does the view need new fields/a table? If yes — `db/` first (`ALTER TABLE` in `schema.rs`,
   `open`, a fetch function), then the UI. A view does not run SQL itself.
3. **Enum.** `enum View` in `app.rs` + `selectable_value` in the left panel. The label is in
   English, an infinitive or a noun: "All files", "Duplicates and series", "By dates".
4. **Branch.** In `CentralPanel` add `View::X => { … }`. Tiles are drawn only through `draw_cell` —
   a second copy of the tile is forbidden.
5. **State.** What the view remembers about the display (threshold, selected group, filter,
   selection) is an `App` field with a sensible default. Only display settings that must survive a
   restart go into `settings`: `tile_size`, the grid layout (`grid_layout`) and the sort
   (`grid_sort`); nothing else is written to the settings file.
6. **Heavy work goes to a thread.** Classification/clustering in the view runs in the background,
   the result arrives via `mpsc`, progress is in the top bar (like `start_index`).
7. **Documents.** `docs/UI.md` (the screen, states, metrics), `plan/STATUS.md` (what appeared), and
   for a new DB column — `docs/ARCHITECTURE.md` §Schema.
8. **Check** (see below).

## Insertion template

```rust
// View: one enum variant
#[derive(Clone, Copy, PartialEq)]
enum View { Grid, Dupes, Timeline, /* new */ }

// Left panel
ui.selectable_value(&mut self.view, View::New, "New view");

// Central panel
View::New => {
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            for &i in &visible {
                draw_cell(ui, ctx, &mut cells, &items[i]);
            }
        });
    });
}
```

## Traps we have already hit

- **Borrows.** `self.items` is taken with `std::mem::take`, the textures go into a local `Cells`,
  and after `show` are put back. Otherwise the borrow checker will not let the texture cache change
  inside the closure.
- **Lazy loading.** A preview is loaded only if `ui.is_rect_visible(rect)`; otherwise the first
  frame on 20 thousand photos will eat gigabytes.
- **Counters.** A new view must not break the bottom counters (`files`, `size`, `no date`).
- **Empty state.** Say in words what to do: "Press “Find groups” on the left", not emptiness.

## Check

```bash
cargo clippy --all-targets
bash .pi/skills/enforce-architecture/scripts/check.sh
timeout 6 ./target/release/photocat gui        # only with a warning to the owner
```

View logic that can be checked without a screen (counts, grouping, filter) goes into
`dupes.rs`/`db/` and is checked through the CLI or a separate function — then the view stays thin.

## Done when

- the view is in `docs/UI.md` and in the left panel;
- tiles are drawn by `draw_cell`, there is no copy of its own;
- `check.sh` is green, `clippy` has no warnings in the new code;
- `plan/STATUS.md` records what appeared and what is left.
