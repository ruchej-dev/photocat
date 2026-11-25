# Mock-ups of the window (the owner's set)

*Русская версия: [README.ru.md](README.ru.md).*

The owner's design set, exported from Google Stitch on 2026-11-24 and kept here so that the source of
the layout is in the repository rather than on one machine. Seven HTML screens, one screenshot per
screen (four of them), and two design-system files.

**These files are a reference, not a build step.** photocat is one Rust binary with an egui window
(AGENTS §1): nothing here is compiled, loaded, bundled or shipped, and no webview ever appears in the
application. They are read by a human — in a browser, from disk — and the adopted part is written down
in `plan/M22-ui-spec.md` (what was taken, what was refused and why) and in `docs/UI.md` (the normative
layout and interaction rules).

The HTML relies on CDNs (`cdn.tailwindcss.com`, Google Fonts, hosted placeholder images), so a browser
with no network shows the structure but not the final look. The `screen.png` files need no network.

| Folder | What it shows | Taken into the app |
|---|---|---|
| `functional_precision_desktop/` | dark palette, type scale, spacing, components | `docs/DECISIONS.md` ADR 0044, the `DARK` palette |
| `functional_precision_light/` | the light palette, the same components | ADR 0044, the `LIGHT` palette |
| `photocat_1/` | main screen: top strip, search, left panel, month grid, inspector, settings dialog | M22.1/M22.2, the main screen of to-day |
| `photocat_2/` | calendar, month layer: week grid with thumbnails, day list | M21.4b, fully since M22.5 |
| `photocat_12/` | calendar, year layer: 12 months with counts, a month list below | M22.5 |
| `photocat_3/` | albums: card grid, filters, sorting, the selection bar | the albums screen of to-day: the card look inside the tree (M22.6); the tree stays, the floating bar is refused |
| `photocat_5/`, `photocat_6/` | full-screen viewer: header, zoom/tools row, series strip, player controls, inspector (two variants) | M13/M21.5, the viewer of to-day, partly |
| `photocat_4/` | an older shell: top navigation, a settings dialog with autostart, GPU and backup | refused — see `plan/M22-ui-spec.md` §5 |

**The icons.** The mock-ups are Material Symbols throughout; photocat adopts the *choices* — which
sign stands for which action — and draws every glyph as geometry in `src/app.rs` (`app::Icon`,
`app::paint_icon`). No font and no image file is taken from this set, so the binary does not grow
(ADR 0048, M22.8).

**Do not "fix" the mock-ups.** They are the owner's design, kept as exported; the difference between
them and the window is recorded in `plan/M22-ui-spec.md`, and the mock-up wins only where the spec
says it does. A function that a mock-up shows but photocat refuses (autostart, GPU settings, backups,
ratings, sharing, cloud sync) is refused in `plan/M22-ui-spec.md` §5 and must not appear in the app.

**Origin and licence.** The set was generated for the owner with Google Stitch from his own brief and
is part of his project; the placeholder photographs inside are stock images served by Google, not the
family archive. Nothing in these files carries archive paths, file names, GPS coordinates or any other
data of the owner's archive.
