---
name: ui-design
description: "Design and review the photocat interface: a panel, the preview grid, a cell, a caption, a loading state, a word in a button, scrolling and selection behaviour. Contains the normative layout, human interaction and “findable in a second” rules. Use before any edit of `src/app.rs`, `docs/UI.md`, when creating a mock-up and when judging “is this comfortable for a person sorting out a family archive”."
license: MIT
compatibility: pi
metadata:
  audience: agents
  workflow: ui-design
  adapted-from: z3l/.pi/skills/ui-design (MIT)
---

*Русская версия: [SKILL.ru.md](SKILL.ru.md).*

# Designing the photocat interface

The owner is sorting out a family archive: 2784 photos, 525 videos, 65 GB, all in a heap. He is not
a designer and not a photographer. His task is to **find, understand, arrange**. The interface must
be dense, quiet and predictable: a grid of snapshots, one status line, a filter panel. This skill is
a procedure and a rubric, not an essay about beauty.

## Sources of truth

| What | Where |
|---|---|
| Project rules, constraints | `AGENTS.md` §1, §4, §6 |
| Metrics, screens, texts, states | `docs/UI.md` |
| Layers and the ban on UI in the core | `docs/ARCHITECTURE.md`, `docs/DECISIONS.md` 0008 |
| Interface code | `src/app.rs` — the only file with `egui` |
| Units and number format | `db::human`, `docs/UI.md` §Texts |

If a document and the code disagree — the code is the evidence, the document is the defect: fix the
document in the same change.

## Normative rules

These are not recommendations but verifiable requirements. Every interface change complies with
them; an exception exists only as a row in `docs/UI.md` with the reason.

| Rule | File | About |
|---|---|---|
| Layout | `references/verstka.md` | the 4 px grid, the spacing scale, font sizes, panels, the tile, colour, layout states |
| Interaction | `references/vzaimodeystvie.md` | click, hover, response, "Stop"/"Resume", errors, reversibility, focus, language |
| Findable in a second | `references/nayti-za-sekundu.md` | the function placement map, the left panel budget, one main action, the findability test |
| Review | `references/revu.md` | 12 checks, 10 heuristics, UX laws in numbers, an honest list of current violations |

Order of application: first `nayti-za-sekundu.md` (where a function lives), then `verstka.md` (how it
looks), then `vzaimodeystvie.md` (how it behaves), and finally `revu.md` (what is wrong). The
reverse order produces beautiful screens with nothing to find.

## Procedure

1. **A five-line brief** before the code: which screen, whose task, the main action (one!), the
   density (panel/grid/tooltip), what we remove. Two main actions on a screen mean the brief is not
   written.
2. **Two or three options** with a cost in code and in the user's habit. One option is not design,
   three are an argument. One line of compromise for each.
3. **A choice by the rubric** (below), not by taste. "Looks cleaner" is not a criterion.
4. **Writing it down** in `docs/UI.md` (metric/screen/text) and in `docs/DECISIONS.md` if the
   decision is irreversible (a new view, a new column, a change to the showcase structure).
5. **Implementation through existing widgets.** The grid is drawn by `draw_cell`; a new screen calls
   it. A second copy of the tile is a defect.
6. **Check**: see §Check.

## Rubric (evaluate point by point, aloud)

1. **One main action.** On the grid — "Index"; in duplicates — "Find groups". The rest is secondary
   and does not compete with it in weight.
2. **Recognition instead of search.** A tile = an image + a date. No puzzle icons and no legends.
3. **A number always has a unit and is always next to its label**: "214.1 MiB wasted", not "214.1".
4. **The state is visible, not guessed.** Indexing runs → `done/total` progress, a status line with
   the path. No preview → text on the tile, not grey emptiness.
5. **Nothing appears only on hover.** A tooltip is a hint, not the only way.
6. **The response is immediate.** A click selects in the same frame; heavy work goes to a thread with
   progress.
7. **Density.** A 150 px cell, a 260 px panel, an 11 px caption: at 1240×840 at least 40 snapshots are
   visible. Any increase in the metrics needs a justification in `docs/UI.md`.
8. **Words.** An infinitive in a button, a fact + a number in the status, no "Success!" and no
   "file(s)".
9. **Nothing is destroyed.** The interface has no words "delete", "move", "clear". At most "mark as
   spare" and "export the plan".
10. **In a second.** The main action of a view is visible without scrolling and hovering and is done
    with one click. If a function needs a menu, a second screen or knowledge of a hotkey — it is
    hidden.
11. **One model.** A tile everywhere means a file, a click everywhere means selection, one action is
    called by one word. A new screen does not invent its own language.
12. **Nothing jumps.** The position of navigation, buttons and fields does not change because of
    preview loading, progress or a filter change. Room for a tile is always reserved.

## Human factors (numbers, not taste)

- **A click target** — the primary button ≥ 32 px, a secondary one ≥ 20 px; we have 150 px with a
  margin — do not reduce it.
- **Response time:** up to 100 ms — instant; up to 1 s — tolerable without an indicator; more —
  progress is mandatory. Decoding an image for a preview on the UI thread = more than 1 s → only
  from the cache.
- **Hick's law:** no more than 5–7 items of the same weight in a panel.
- **Fitts's law:** the main button sits at the top edge, not in the middle of the screen.
- **Contrast:** the main text is no darker than `gray(190)`, secondary — no darker than `gray(150)`.

## Check

Silent by default — a session must not take the owner's screen:

1. `cargo clippy --all-targets` — no errors.
2. `bash .pi/skills/enforce-architecture/scripts/check.sh` — `egui` only in `app.rs`.
3. Screen logic is checked without the GUI: `index` / `dupes` / `organize` on the corpus (see
   `corpus-bench`). If a new view changes only the layout — a compact sample is enough.
4. Pixels (colours, contrast, tooltip, theme) are not covered by a headless check: a run of
   `timeout 6 ./target/release/photocat gui` is needed **with a warning to the owner**, or his own
   review.
5. **Tokens by grep**: font sizes only `11/12/14/16/18`, spacing only `2/4/6/8/12/16/24`, explicit
   greys only `150/190/210/235`, our own colour only `rgb(90,170,255)`. A new number goes into
   `docs/UI.md` first, then into the code.
6. **The findability test** (`references/nayti-za-sekundu.md` §7): the owner's tasks, the time to the
   first correct click. A failure is a defect, not "the user did not figure it out".
7. **A review by `references/revu.md`**: 12 checks, each either "yes" or "no".

A UI change is not ready when it compiles. It is ready when the result has been looked at.

## When to stop and ask the owner

- A new screen or a new view (the number of tabs grows).
- A new metric (cell, panel, preview size) — it is seen every day.
- A change to the showcase structure on disk.
- Removing an existing capability.
- Text that promises what the code does not do (for example, "delete" when there is no deletion).
