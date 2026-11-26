# M19–M20 — albums in the DB and the export as a physical folder

*Русская версия: [M19-albums.ru.md](M19-albums.ru.md).*

The owner's request (2026-11-23): "Add the ability to create folders. These folders are data
only in the database, not on disk. The rule: all operations only with data, not with files for real,
except the special
tools. Add a function to export the folders: a built folder is exported to disk
with files. But the problem of name collisions has to be solved."

The owner's clarifications (2026-11-23):

- the word is **"Albums"**, not "folders";
- **nesting is mandatory**: an album may contain subalbums;
- **the purpose of an album is to create a physical folder with ordered data**: the structure and the
  order
  built in the app repeat on disk one to one;
- **the export is files, not links**, the folders of subalbums **without numbers** (the order of the
  subalbums
  on disk is alphabetical — that was decided), the file names — `{n}_{orig}`;
- **one shot — one row in an album**: a file does not get into one album twice (that is a DB key, not
  a check in the interface); in different albums — allowed;
- renaming files on disk was **withdrawn** from the request: the originals are untouchable (AGENTS.md
  §1,
  ADR 0005).

Hence two steps: **M19** — albums as data (a tree + an order, not a single file on disk),
**M20** — the export of the tree to disk with real files on an explicit `--apply`.

---

## 1. The word

- "Folders (N)" in the interface is already taken by the archive folders (`settings.roots`), so
  albums are
  a separate word. **In the interface:** "Albums", "Album “Dacha 2024”", "In the album". **In the
  code:**
  `album`, `albums`, `album_files`. **In the CLI:** `photocat albums …`, `photocat export --album N`.
- The old "album = a calendar day" (M13) is cancelled: a day stays a day, an album is an explicit set
  of
  files with an order and nesting.

## 2. What an album is

A tree of nodes that the owner builds by hand in order to get a ready folder on disk:
"Dacha 2024" → "May", "June"; "Lisa" → "1 year", "2 years"; "To print". A node contains **files in a
given order** and **subalbums in a given order**.

- An album knows nothing about a file except `files.id`; a file can be in several albums and in
  several places of one tree (deliberately: one shot is appropriate both in "Dacha 2024" and in
  "To print").
- An album lives in the DB; deleting it — rows only. The file on disk is intact.
- A file in the app trash (`files.trashed_at`) stays an album row, but is hidden in the grid,
  the counter writes "in the trash N".
- A file disappeared from the index (an index reset, the folder was removed) — the album rows for it
  are deleted by the same
  `db` action; there are never dangling references.
- The order is part of the data, not of the presentation: the grid sorting (M17) does not change it,
  and inside
  an album "album order" is shown by default.

## 3. The DB schema (M19)

`SCHEMA_VERSION` 4 → 5, `CREATE TABLE IF NOT EXISTS` only (ADR 0002/0003): the data is not lost,
an old DB opens, a repeated opening writes nothing while the version matches.

```sql
CREATE TABLE IF NOT EXISTS albums(
  id         INTEGER PRIMARY KEY,
  parent_id  INTEGER NOT NULL DEFAULT 0,  -- 0 — the root of the tree (not NULL: NULL is not compared in UNIQUE)
  position   INTEGER NOT NULL DEFAULT 0,  -- the order among siblings
  name       TEXT NOT NULL,               -- as the owner named it, ≤ 80 characters
  name_key   TEXT NOT NULL,               -- name.to_lowercase(): uniqueness case-insensitively
  note       TEXT NOT NULL DEFAULT '',
  created_at TEXT NOT NULL,
  updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS albums_sibling_name ON albums(parent_id, name_key);
CREATE INDEX IF NOT EXISTS albums_tree ON albums(parent_id, position);

CREATE TABLE IF NOT EXISTS album_files(
  album_id INTEGER NOT NULL,
  file_id  INTEGER NOT NULL,
  position INTEGER NOT NULL DEFAULT 0,   -- the order inside the album
  added_at TEXT NOT NULL,
  PRIMARY KEY(album_id, file_id)
);
CREATE INDEX IF NOT EXISTS album_files_file ON album_files(file_id);
```

- `parent_id = 0` — the root. `PRAGMA foreign_keys` is not enabled in `db::open`, so `ON DELETE
  CASCADE` does not work: deleting a subtree and cleaning `album_files` are done explicitly, in one
  transaction (`delete_album` recursively; deleting `files` rows on an index reset and on "remove the
  folder" cleans `album_files`). Tests for all the paths are mandatory.
- The name is unique **among siblings** case-insensitively (`parent_id`, `name_key`), not globally:
  "2023/Dacha" and "2024/Dacha" are different albums.
- `position` is dense 0..n−1 among siblings; `db` renumbers after an insert/a move/a delete,
  so that the order does not "spread" with holes.
- No cover is stored: the first non-deleted file by `position`, otherwise by date. Otherwise — an
  empty
  folder icon.
- The depth is not limited artificially, but a cycle is forbidden: `move_album` refuses if the new
  parent is the album itself or its descendant (a check by walking up).

## 4. The API and the CLI

`src/db.rs` (L1), without interface logic:

| Function | What it does |
|---|---|
| `create_album(conn, parent_id, name, note) -> i64` | the name, `name_key`, `position` to the end |
| `rename_album(conn, id, name)` | a check of the siblings + `updated_at` |
| `move_album(conn, id, new_parent, position)` | a refusal on a cycle, a renumbering of the siblings |
| `delete_album(conn, id)` | the subtree: `album_files` → `albums`, rows only |
| `add_to_album(conn, id, &[file_id]) -> (added, already there)` | to the end, idempotently |
| `remove_from_album(conn, id, &[file_id])` | a renumbering |
| `move_in_album(conn, id, file_id, delta)` | "up/down" in the album order |
| `albums_tree(conn) -> Vec<AlbumNode>` | id, parent, position, the name, note, files, size, a cover, the depth |
| `album_items(conn, id) -> Vec<FileRow>` | by `position` |
| `albums_of_file(conn, file_id) -> Vec<String>` | for the inspector |

The CLI (`src/main.rs`), headless — we check with it:

```
photocat albums list [--tree]
photocat albums new --name "Dacha 2024" [--parent ID] [--note "..."]
photocat albums rename --id N --name "..."
photocat albums move --id N --parent ID [--position K]
photocat albums add --id N PATH...          # the paths are looked up in the index, not on disk
photocat albums remove --id N PATH...
photocat albums delete --id N [--yes]       # the subtree, rows only
photocat albums show --id N [--tree]
```

No command opens a file by a path or writes to disk: `PATH` is a key in `files.path`, not
a file operation.

## 5. The interface (M19, the cards of M22.20)

- The left panel, the "LIBRARY" group: the item **"Albums (N)"** with the number of albums.
- The "Albums" view: since M22.20 it opens on the **cards of the top level albums** — a 16:10 cover
  with the first snapshot, "N sub-albums" and "N files" over it, the name and "size · date" under it
  — with the control row of the screen (the filter of the names, the order, "Cards / List" and
  "+ New album"). The album order of the export is the default order of the cards; "By update",
  "By name", "By file count" and "By size" are looks (`AlbumSort`), and `AlbumNode::updated_at` is
  the stamp they sort by, touched by `add_to_album`, `remove_from_album` and `move_in_album`.
- Inside an album: the breadcrumb "All albums › … › here", then the tree with indents and
  collapsing triangles; a node — a cover, the name, "N files · M subalbums · size". For the open
  one — "+ Subalbum", "Rename", "Move to…" (picking a parent from the tree; its own descendants are
  unavailable in the list), "Up / Down" (the order among siblings), "Delete the album".
- Inside an album: a row of cards of the subalbums (the same order), below — a grid of files
  (`draw_cell`), the chip "Album “Name”" with "×". The actions: **"Add the picked ones"** (M18),
  **"Remove from the album"**, **"Up / Down"** for a file (the order), **"Export to disk…"** (M20).
- **The way into an album (M22.20)** is a dialog of its own, `AlbumPick`, with three entrances: the
  dashed "+ Add" chip of the file card (one photo), "Add to an album… (N)" in the floating selection
  bar (the marked photos) and "Add the picked ones (N)" inside an album (straight into the open
  album). The dialog lists the albums with their paths and file counts, filters by name and has a
  field for a new album: a name makes a root album and fills it in the same press. The photos go to
  the end of the album, a repeat does not duplicate rows, and only `album_files` is written: not a
  single file on disk is opened.
- The sorting inside an album by default is "Album order" (a new option in the M17 list, available
  only inside an album); "By date" and "By name" stay as a temporary look.
- Deleting: the confirmation shows the subtree ("delete “Dacha 2024”, “May”, “June” — 3 albums,
  412 rows; the files on disk will remain").
- Empty: "The album has no files yet — pick the shots in the grid and press “Add picked”".
- The inspector: the row "albums — Dacha 2024 / May, To print" with "×".

## 6. M20 — exporting an album as a physical folder

**The export is a copy of the files**, not links: the purpose of an album is a ready folder that can
be
handed over, uploaded or opened without the archive. There is no `--mode` in `export`; links stay
with `organize`
(the showcase), they are different tools with different purposes.

```
photocat export --album N --out DIR [--name TEMPLATE] [--apply]
```

- `--out DIR` — a folder **inside** which the root folder of album N is created; `--out` itself
  must exist and must not be an archive folder.
- The structure repeats the album tree: `--out/<album>/<subalbum>/…`, in each folder —
  its own files. The file order is visible in a file manager, because the name starts with the
  position
  number (`{n}`), not with the date.
- `--name` by default `{n}_{orig}`; `{date} {time} {camera} {tag} {orig} {ext}`
  and `{n}`, `{hash8}` are supported. `{n}` — the position of the file in its album, the width — by
  the number of files (2 digits up to
  99, 3 further), so that "01_", "02_", … are without confusion.
- **The folders of subalbums without numbers** (decided by the owner): on disk they go alphabetically,
  because
  the file system does not know `position`. The order of the subalbums stays the order of the tree in
  the
  app, and the order of the "data" is provided by the numbers in the file names.
- An empty album — an empty folder: the structure matters more than "do not create an empty one".
- The code in `src/organize.rs` (L2): `plan_album(...)` + `apply_copy(plan)`. We do not start a new
  module,
  so that an L2→L2 dependency does not appear; `organize` can already copy and clean names.
  The existing `apply` (hard/copy/symlink for the showcase) does not change.

### 6.1. Name collisions — the analysis and the rule

The nesting and the position number remove most collisions **by construction**; five cases remain.

| Kind | Example | The rule |
|---|---|---|
| Equal file names in one album | `IMG_0001.jpg` twice | the position prefix makes the names different: `01_IMG_0001.jpg`, `02_IMG_0001.jpg` |
| The name is already taken in the target by other contents (an edit by hand, a past export) | `01_IMG_0001.jpg` exists, but with another `sha256` | do not overwrite: `01_IMG_0001-2.jpg`; the plan shows what exactly was renamed |
| The name is already taken by the same contents | a repeated export of the same album | **skip** ("already exported"), and do not create `-2`: the run is idempotent |
| The subalbum names matched after an FS cleanup | "Dacha/2024" and "Dacha:2024" → `Dacha_2024` | `-2` for the second folder, the plan says why |
| Case/normalization | `A.JPG` and `a.jpg` on Windows/macOS | a case-insensitive comparison on Windows/macOS (`cfg`); NFD/NFC on macOS — a known limitation, cured by `{hash8}` |

The cleanup of names before the comparison: the symbols not allowed by the FS (`: ? * " < > |` and `/`
in an album name),
the Windows device names (`CON`, `PRN`, `AUX`, `NUL`, `COM1..9`, `LPT1..9`), a dot/space at the end,
the length of a component ≤ 120 characters, the full path ≤ 240 (a margin up to 260 in Windows), an
empty name →
`{hash8}`. There is one sanitizer — `organize::component`/`tidy`; there is no second copy.

A repeat of one shot in a folder does not happen by construction: in an album a shot is one row
(`PRIMARY KEY(album_id, file_id)`), and the same shot in two different albums gives two different
folders. This was decided by the owner: "one".

The check "the same contents" — by the `sha256` from the DB for the source and by the fact for the
target (reading the targets
goes in the background, the progress is visible, the "Stop" button; that is reading, not writing).
The cache "what was already exported"
(`<cache>/photocat/exports/<album>-<folder hash>.txt`) speeds a repeat up, but does not cancel the
check.

**An honest limitation of the order.** The files are not renamed in the target (AGENTS.md §1), so
after
reordering an album a repeated export does not move an already exported file to a new position —
it stays under the old number, and the new one is skipped as "already exported". The right way is to
delete
the old export folder by hand and export again; the app writes about it in the plan and in the report
("the order in the target is stale: N files"). The app will not delete or rename anything in `--out`.

### 6.2. The plan and the report

The plan is a tree: the folders, in each — the files "the old name → the new name"; the counters
"copy N
files · X GiB", "skipped N (already exported)", "renamed because of a collision N", "no file on
disk N", "paths longer than 240 characters N". Without `--apply` nothing is created, not even the
`--out` folder.

The report after `--apply`: files created, skipped, errors, bytes written; in the GUI — a row in the
status and
"Copy the report". A copy of the report — in the cache, not in the archive.

### 6.3. Acceptance criteria for M20

1. Without `--apply` — only the plan, nothing on disk; `--out` does not exist, `--apply` without
   `--out`,
   `--out` inside any `settings.roots` — a refusal with code 2 and an explanation.
2. The structure on disk repeats the tree: `--out/<album>/<subalbum>/…`, the files only in their own
   folders, an empty album gives an empty folder; the folders of subalbums without numbers — on disk
   they
   go alphabetically, the tree order stays in the app.
3. The files are real copies (the inode and the size differ from the original, the contents are the
   same by `sha256`),
   no links; the original is not opened for writing.
4. The order: the names start with `{n}` by `position`; up to 99 files — two digits; "Dacha/May" and
   "Dacha/June" are sorted in a file manager in the same order as in the album.
5. The collisions — by the table of §6.1 (five rows), including the matching of the subalbum names
   after the cleanup and
   the idempotency by `sha256`; nothing is overwritten or deleted. One shot twice in
   one folder is impossible: in an album it is one row.
6. The report: copied / skipped / errors / bytes written / "the order in the target is stale N"; an
   error
   of one file (not on disk, no permissions) does not break the pass.
7. GUI: "Export to disk…" — the folder field, the name template, the checkbox of the subalbum
   numbers; first
   a plan tree, then "Export N files"; the progress and "Stop" in the top bar.
8. Tests: the cleanup of names and rarely used symbols, the five kinds of collisions, idempotency, a
   copy
   (not a link), an empty album, a depth of 3, a refusal of an `--out` in the root, a cancel in the
   middle (the created
   files are listed), a missing source.
9. A measurement on the corpus: a tree of 3 levels, 500 files — the time and the volume of the copy;
   a repeat — the time and 0
   new files; 2230 files — the time and the volume; `docs/ARCHITECTURE.md` §Measurements.
10. ADR 0035 "The album export: the album structure, file copies, nothing is overwritten",
    `docs/UI.md`, `plan/M19-albums.md`, `plan/STATUS.md`.

### 6.4. Not doing in M20

- Do not export as links, do not add `--mode`: for links there is `organize` (the showcase).
- Do not delete or rename in `--out`, do not "sync" the target.
- Do not write service files into `--out` (a manifest, `.photocat/`, JSON) — the folder stays clean.
- Do not export automatically by a schedule, at start or when an album changes.
- Do not index `--out` silently.

## 7. The order of work

1. M18 (a multiple selection) — otherwise there is nothing to press "Add picked" on.
2. M19: the schema (the tree, `position`) → `db` → the CLI `--tree` → migration tests on a copy of
   the real DB →
   the "Albums" view with nesting and order → "Album order" in the M17 sorting → `docs`.
3. M20: `plan_album` (the tree, the collisions, `{n}`) → the CLI `--apply` → the GUI plan tree →
   measurements →
   ADR 0035.

## 8. Open questions to the owner

All the questions on M19–M20 are closed by the owner (2026-11-23):

1. ~~The word~~ **"Albums"**.
2. ~~Links or copies~~ **File copies**.
3. ~~Flat or by dates~~ **The structure repeats the album tree**.
4. ~~Numbers on the folders of subalbums~~ **Without numbers**; the order of the subalbums on disk is
   alphabetical.
5. ~~The name template~~ **`{n}_{orig}`**; the width of the number — by the number of files (2 digits
   up to 99, 3 further).
6. ~~One shot twice in one album~~ **Once** (the key `PRIMARY KEY(album_id, file_id)`),
   in different albums — allowed.

Only what the code gives, not a decision, stays open: the numbers of the M20 measurements (the time
and the volume of the copy on the
corpus).

## 9. Done: M19 (2026-11-23)

Schema 5, `db.rs` (CRUD of the tree and the order, `albums_tree`, `album_items`, `albums_of_file`),
`main.rs` (`albums …`), `app.rs` (the "Albums" view, the tree, the subalbums, the chip, "Album
order",
the file card), `storage.rs` (an index reset cleans `album_files`), ADR 0034.
Not a single file operation: `fs::remove_*`/`fs::rename` are still absent from `src/`.

- **The data.** `parent_id=0` — the root, the name is unique among siblings (`parent_id`,
  `name_key`),
  `position` is dense 0..n−1, `PRIMARY KEY(album_id,file_id)` — one shot once in an album.
  `delete_album` recursively cleans the subtree; deleting `files` rows cleans `album_files` and
  compacts the order. A cycle is forbidden in `move_album`.
- **The CLI.** `list [--tree] | new | rename | move | add | remove | delete [--yes] | show
  [--tree]`;
  the paths are the `files.path` key, the files are not opened. The codes: 0 — done, 1 — a data
  error,
  2 — the arguments/a refusal without `--yes`.
- **The GUI.** The tree in the content panel, "+ New album / + Subalbum / Rename /
  Move to… / Up / Down / Delete the album", the tiles of the subalbums and the grid in the album
  order.
  "In the album / All files" — a temporary look for "Add picked" (M18): a change of the view still
  drops the selection, so the shots have to be picked inside the view. "Export to disk…" works since
  M20 (§10).
- **The order.** `GridSort::Album` is available only inside an album; outside it — date ↓. One
  function
  `App::ordered` gives the order to the grid, the full screen and the selection; a change of the
  album drops the selection
  with a reason.

**Measurements (a copy of the real DB, 2230 files, 21.2 GiB):** the 4 → 5 migration — the files
intact, the DB
25 403 392 → 25 411 584 B (+8 KiB). A tree of 3 levels (6 nodes), `albums add` of 500 files —
25 ms, the positions 0..499, the DB +28 KiB; a repeat — 19 ms, added 0, "500 were already there".
`cargo test` — 135 passed; `clippy` and `enforce-architecture` clean; the GUI on a copy of the DB —
no crashes.

Left for M20: `plan_album` and `apply_copy` in `organize.rs`, copies (not links), the structure of
the
tree with alphabetical subfolders, `{n}_{orig}` and the five kinds of collisions; the report and the
GUI plan.

## 10. Done: M20 (2026-11-23)

`organize::plan_album` and `apply_export`: the structure `--out/<album>/<subalbum>/…` without numbers
on the
folders, the names `{n}_{orig}`, real copies (another inode, the same `sha256`), `-2` on a taken
name, a skip by `sha256` and "the order in the target is stale" for what was already exported under
an old number.
The export cache — in `<cache>/photocat/exports/`, not in the target. The CLI `photocat export` (the
codes 0/1/2) and
the GUI dialog "Export to disk…" with a plan tree, background progress and "Stop". The criteria of
§6.3
are closed; the numbers — in `docs/ARCHITECTURE.md` §Measurements, the decision — ADR 0035. Left
deliberately:
a reordering does not rename what was already exported (the files in the target are untouchable).
