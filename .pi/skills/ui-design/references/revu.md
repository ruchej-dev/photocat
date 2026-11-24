# Interface review: a checklist

*Русская версия: [revu.ru.md](revu.ru.md).*

One page to check any screen, panel or mock-up against before it is called done. A point is either
met or not; "generally fine" does not count.

## 1. Twelve checks (the rubric)

| # | Check | How to make sure | Failure |
|---|---|---|---|
| 1 | The main action is one and visible | 5 seconds per screen: what is done here? | the wrong thing is named, or there are two actions |
| 2 | A function is found in a second | the test from `nayti-za-sekundu.md` §7 | >1 click for a frequent action |
| 3 | The state is visible | a pass runs → progress; empty → text; an error → status | an empty screen, a spinner, "done" without numbers |
| 4 | The response is immediate | a click changes the frame in ≤100 ms; heavy work in the background | the window freezes |
| 5 | Nothing jumps | preview loading, progress, a filter change | tiles slide, buttons move |
| 6 | One model | a tile = a file; a click = selection; the same words | "the tile is different here" |
| 7 | Numbers have units | "214.1 MiB wasted", "238 files" | "214.1", "pcs.", "file(s)" |
| 8 | Nothing is destroyed | search for the words "delete/move/clear" | even one word |
| 9 | No modal windows and confirmations | errors in the status; actions reversible | a "Are you sure?" dialog |
| 10 | Hover is only a hint | turn the mouse off: every action is still available | a button only in a tooltip |
| 11 | The keyboard is not mandatory, but works | Tab/Enter/Esc, focus visible | focus is not visible, Tab does not move |
| 12 | The metrics are untouched | 150+26, 260, 320, 1240×840, ≥4 columns | a "better" tile of one's own |

## 2. Ten Nielsen heuristics → our wording

1. **Visibility of state** — `done/total` progress, a status line, "stopped: 120 of 435".
2. **The user's language** — "receipt", "certificate", "spare", not "embedding", "FTS5", "self-join".
3. **Freedom** — "Stop", "Reset", "restore the hidden tags", clear the selection; nothing is locked.
4. **Consistency** — one word per action, one tile on all screens, buttons in one place.
5. **Error prevention** — a plan before the start ("435 candidates"), the reason next to a greyed-out
   button.
6. **Recognition, not recall** — visible labels, a tooltip only adds.
7. **Flexibility** — mouse and keyboard; batch and single runs are separated.
8. **Aesthetics and minimalism** — density, no decoration; everything that carries no information is
   removed.
9. **Help with errors** — what happened + what to do, at the bottom, no stack.
10. **Help and documentation** — hints next to thresholds and models; `doctor` in the CLI for the
    environment.

## 3. UX laws in our numbers

- **Fitts:** the main buttons are at the top edge and large; secondary ones are 20 px high.
- **Hick:** no more than 5–7 items of the same weight in a panel. There are seven views now — the
  limit; an eighth would require rebuilding the navigation into sections.
- **Miller:** thresholds and groups come in 3–7; the 617 face groups are broken up by a list, not a
  "cloud".
- **Jakob:** we behave like Finder/Photos/Lightroom: a grid, a date under the frame, search on top.
- **Proximity:** 4 px inside a group, 8 between groups, 12 between sections.
- **Common region:** a group's heading and content are tied by a frame, not by spacing.
- **Figure and ground (Von Restorff):** the accent — only selection/a confident tag/"keep"; one
  accent per tile.
- **Peak rule:** the end of a pass is an honest summary with numbers, not "Success!".

## 4. What is already violated (an honest list as of 2026-11-24)

Checked against the code and `plan/STATUS.md`, not from memory:

1. **The grid has no keyboard**: `app.rs` does not handle arrows over the grid, the Tab order is the
   default, Ctrl+I/Ctrl+F/F5 are not implemented (Ctrl+A and the ←/→ keys in the full screen do
   work; `docs/UI.md` honestly keeps the rest in the "Nearest" column).
2. **Progress appears instantly** and flickers on short passes; the rule is to show it after 300 ms
   if the operation is still running.
3. **A folder path error** is written into the status, but the field is not highlighted: the
   "error ↔ field" link is text only.
4. **The month heading does not stick** while scrolling: `ScrollArea` in egui does not provide a
   sticky row without manual drawing. Recorded in `docs/UI.md` §Debt, item 8.
5. **The calendar**: the selected day is reset when flipping the month — deliberately, stated in
   `docs/UI.md` §Debt, item 4.

The rule: an item from this list is either fixed or gets an exception row in `docs/UI.md` with the
reason. One cannot silently live with a violation.

## 5. Definition of done for a UI change

1. A five-line brief and one option chosen by the rubric, not by taste.
2. `docs/UI.md` updated: screen/metric/text/state.
3. The layout rules are met: spacing from the scale, font sizes from the list, one accent.
4. Findability: the new function is in the placement map and did not break the left panel budget.
5. Interaction: click/focus/status/error/"Stop" checked.
6. `cargo clippy --all-targets` and `check.sh` are clean.
7. `timeout 6 ./target/release/photocat gui` — the window opened, the grid is in place (with a
   warning to the owner).
8. One line in `plan/STATUS.md`: what appeared and how it was checked.

## Where the rules come from

The rules were rewritten for an egui desktop rather than copied; the sources are for a designer who
wants to dig deeper:

- 10 heuristics and state patterns: [NN/g — Filters vs. Facets](https://www.nngroup.com/articles/filters-vs-facets/),
  [Progressive disclosure](https://www.nngroup.com/articles/progressive-disclosure/),
  [Empty states](https://www.nngroup.com/articles/empty-state-interface-design/),
  [Photos as web content](https://www.nngroup.com/articles/photos-as-web-content/).
- UX laws in numbers: [lawsofux.com](https://lawsofux.com/) (Fitts, Hick, Miller, Jakob, proximity,
  common region, Von Restorff).
- Trust in ML, confidence, error correction: [Google PAIR Guidebook](https://pair.withgoogle.com/guidebook/),
  [Microsoft HAX](https://www.microsoft.com/en-us/haxtoolkit/),
  [Apple HIG: Machine Learning](https://developer.apple.com/design/human-interface-guidelines/machine-learning).
- Dense data and master-detail: `data-management-patterns` from
  [richhemsley3/claude-design-skills](https://github.com/richhemsley3/claude-design-skills),
  [dashboard-design-skill](https://github.com/abdiraimov-otabek/dashboard-design-skill).
- Domain: [Apple Photos — duplicates](https://support.apple.com/guide/photos/find-and-merge-duplicate-photos-pht9e4a2f3b2/mac),
  [Google Photos — face groups](https://support.google.com/photos/answer/6128838),
  [digiKam — Light table](https://docs.digikam.org/en/light_table.html),
  [Immich demo](https://demo.immich.app/).
- Accessibility and the dark theme: [WCAG 2.2 Quick Reference](https://www.w3.org/WAI/WCAG22/quickref/),
  [GNOME HIG](https://developer.gnome.org/hig/), [KDE HIG](https://develop.kde.org/hig/).

Not carried over: landing-page generators, "anti-slop" skills, palettes and Figma integrations —
they are about web pages, not about a dense desktop tool for one person.
