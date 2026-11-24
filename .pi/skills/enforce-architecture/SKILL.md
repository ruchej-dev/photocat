---
name: enforce-architecture
description: "Check photocat's layer boundaries: the core does not know about egui, a module depends only on lower layers, and no code deletes or renames files. Use when moving code, adding a dependency, before a commit and at the end of any task that touches `src/` or `Cargo.toml`."
license: MIT
compatibility: pi
metadata:
  audience: agents
  workflow: architecture
---

*Русская версия: [SKILL.ru.md](SKILL.ru.md).*

# Keeping photocat's architecture

The layer map is not a wish: a module depends only on lower layers, the GUI contains no logic, and
archive files are untouchable. This skill is the procedure for checking the boundaries before a task
is called done.

## When to apply

- Adding, moving or deleting code across a module boundary.
- A new dependency in `Cargo.toml`.
- Reviewing a change or closing a task that touched `src/`.

## Sources of truth

| Document | What it contains |
|---|---|
| `AGENTS.md` §1, §3 | The prohibitions and the module map |
| `docs/ARCHITECTURE.md` | Layers, the data flow, the DB schema |
| `docs/DECISIONS.md` 0008, 0005 | Why the boundary exists and why files must not be deleted |
| `.pi/skills/enforce-architecture/scripts/check.sh` | The machine check |

When these four disagree — fix them all in one change. A violation is not "fixed" by weakening the
rule: either the code, or a proposal to the owner.

## The automatic check

```bash
bash .pi/skills/enforce-architecture/scripts/check.sh          # fast, grep only
bash .pi/skills/enforce-architecture/scripts/check.sh --build  # + cargo check
```

What is checked:

1. `use crate::<module>` does not leave the layer map (`derive` ↓, `db` ↓,
   `index`/`dupes`/`organize`/`verify`, `app`/`main` on top).
2. `egui`/`eframe` appears only in `src/app.rs`.
3. There is no `fs::remove_file`, `fs::remove_dir`, `fs::remove_dir_all`, `fs::rename` in `src/` —
   archive originals are untouchable (ADR 0005).
4. The network is only for model weights: one HTTP client (`ureq`, ADR 0047), used only in
   `src/embed.rs`, GET requests only — no second client and no data leaving the machine (ADR 0015).
5. Every dependency is mentioned in `docs/DECISIONS.md` (a warning, not an error).
6. The code speaks English (AGENTS.md §6): no Cyrillic identifier in `src/`, and
   `cargo test --test source_language` passes — the converted modules hold no Russian comment or
   string, and the `PENDING` list has no stale entry.

## A manual checklist by module

### `derive` (L0)
- Allowed: `image`, `image_hasher`, `kamadak-exif`, `sha2`, `chrono`, `anyhow`.
- Forbidden: `rusqlite`, `egui`, `rayon`, `walkdir` — reading one file is not their job.
- Check: functions take a `&Path` and return values; they do not write to the DB.

### `db` (L1)
- Allowed: `rusqlite`, `anyhow`, `std`.
- Forbidden: image decoding, `egui`, directory traversal.
- Check: the schema changes only through `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE`, no data is
  lost; `human()` is the only size formatter.

### `index` / `dupes` / `organize` / `verify` (L2)
- Allowed: L0, L1, `rayon` and `walkdir` in `index` and `verify`.
- Forbidden: `egui`, `eframe`, reading GUI settings.
- Check: `organize` without `--apply` creates nothing; `dupes` does not write to the DB; `verify`
  and `manifest` only read the archive, `manifest` writes the single file given by an explicit
  `--out`, and only in full (an incomplete snapshot is not created).

### `player` (L2)
- Allowed: `rff-format-*`, `rff-codec-*`, `cpal`, `rayon`, `std`.
- Forbidden: `egui`/`eframe`, `rusqlite`, `crate::db`, starting external programs (`tools`).
- Check: the player only reads a file, writes nothing into the archive; UI strings live in `app.rs`;
  `play` in `main.rs` is the only CLI entry and it has no graphics.

### `app` (L3)
- Allowed: everything below + `eframe`/`egui`.
- Forbidden: SQL strings, decoding full-size images on the UI thread, logic that cannot be checked
  without a screen.
- Check: logic is moved into `dupes::`/`db::`; `app.rs` keeps fetching and drawing.

## Signs that the boundary is already broken

- `conn.execute(` or a `SELECT …` string appeared in `app.rs`.
- `image::open` appeared in `dupes.rs`.
- `use crate::derive` appeared in `db.rs`.
- `organize` learned to delete "the excess" instead of reporting.
- `verify` or `manifest` write into the archive folder without an explicit `--out` or delete files.
- A new dependency without a row in `docs/DECISIONS.md`.

## Bottom line

A task is not closed if `check.sh` is red. The report of the check is one line in `plan/STATUS.md`
("boundaries: ok", or exactly what is left).
