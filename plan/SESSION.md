# SESSION — M13: a revision of the owner's interface

*Русская версия: [SESSION.ru.md](SESSION.ru.md).*

> Done: **M19: albums in the DB** (a tree, an order, rows only; ADR 0034), **M20: the album export
> as copies**, **M15.4/M15.5: the tag dictionary**, **M9: the "Integrity" screen**, **M10:
> the check reminder by a due date** (a banner without an automatic start; ADR 0037) and **M11: a
> fast search of similar shots** (a block index instead of a BK-tree; ADR 0038), **M12: the CLIP
> model catalogue** (SigLIP 768, schema 7; ADR 0039). Before them — **M18: multiple selection of
> tiles**, **M17: the grid layout and the sort order**, **M16: the interface after the PhotoPrism
> model** (`plan/M16-interface.md`, ADR 0033). **M15.6 — the multilingual SigLIP 2 model and the
> calibration of the tag weight** is done as well (ADR 0041). **M15.7 — a blind labelled check on two
> folders** is done too (`photocat eval`, 240 labelled shots; ADR 0042), and the open tail of M15.6 is
> closed: the fp16 towers cost no tag quality (ADR 0043), so SigLIP 2 stays opt-in. The queue: the
> owner's re-read of the M15.7 labels.
> The owner cancelled renaming the originals (2026-11-23). The entry point: `plan/STATUS.md`.
> Below is the historical log of M13–M14c, not the current status.

Taken from the remarks after a review (the full list — `plan/BACKLOG.md` § M13).
The state: done, see `plan/STATUS.md`.

## What was done

| Step | File | The essence |
|---|---|---|
| 1 | `src/app.rs` | `Viewer`: a modal full screen, a background decode of 1920 px, ←/→, a slideshow, zoom, the OCR text, a return to the same view |
| 2 | `src/app.rs` | a double click in `draw_cell`, the "Open full screen" button; a portioned decode of textures (8 per frame) and smooth scrolling |
| 3 | `src/app.rs` | the right panel closes with "Hide"/"Details"; the calendar "Year/Month/Week" |
| 4 | `src/app.rs`, `src/db.rs`, `src/faces.rs` | faces: a click leads to the day, "pick" on crops, a batch name, "Photo where everyone is together" |
| 5 | `src/app.rs` | explanations at "keep", "Export the plan", "Compute tags"; the full OCR text |
| 6 | `src/app.rs`, `src/db.rs` | folders: `settings.roots`, the built-in folder picker, indexing of several, removing from the index |
| 7 | `src/db.rs`, `src/recycle.rs`, `Cargo.toml` | the trash: `trashed_at`, the "Trash" view, the OS trash on a confirmation |
| 8 | `src/storage.rs`, `src/app.rs` | the used space, a cleanup of previews/faces, an index reset |
| 9 | `src/embed.rs`, `src/db.rs`, `src/app.rs` | 14 CLIP hints, "Compute tags", own tags `user_tags` |
| 10 | `docs/`, `check.sh`, `AGENTS.md` | the UI rules, the schema, ADR 0025/0026, the exception for `storage.rs` |

## Acceptance criteria

1. A double click opens the full screen; closing returns the view/filter/day — yes.
2. The slideshow flips the current list; a video opens in the system player — yes.
3. The right panel can be closed, the calendar is not cropped — yes.
4. The calendar flips by a year/month/week — yes.
5. A click on a face leads to the day, not to the common grid — yes.
6. Several faces are assigned to one group; "everyone together" searches the intersection of the
   groups — yes.
7. Folders are added with a button and removed only from the index — yes.
8. "To trash" does not touch the disk; emptying the trash — the system one only, with a
   confirmation — yes.
9. The used space and a cleanup of the cache/the index — yes, the archive is not touched.
10. The tile size 100–260 px, scrolling is smooth — yes.

## Checks

- `cargo test` — 79 passed, 3 ignored, including the app trash and the batch assignment of faces.
- `cargo clippy --all-targets` — clean.
- `bash .pi/skills/enforce-architecture/scripts/check.sh` — 38 checks, 0 violations.
- Migration of a copy of the real DB (`/tmp`, 22 MiB, 2230 files): schema 2 → 4, `files` 2230,
  `trashed_at` and `user_tags` in place.
- `timeout 6 photocat gui --db /tmp/photocat-test.db` — code 124, no crashes.

## Limitations

- Above 100% zoom there is no panning: the frame is centred and cropped.
- Own tags are manual; CLIP does not create them, the 14 hints are fixed.
- Video — the system player, inside egui only the cover frame.
- There were no separate albums then: a click on a face leads to a calendar day. Albums — M19
  (`plan/M19-albums.md`).
- Clearing the models is not done: they are downloaded only with consent.

## M13b — after the owner's review

| Problem | Cause | The fix |
|---|---|---|
| The full screen loads for seconds | it waited for the full decode + there was no placeholder preview | the 320 px preview at once, the 1920 px frame in the background, a preload of the selected one, the old stream is dropped |
| A video has no preview | the old DB did not walk videos after M4 | the incremental indexing recomputes videos without `thumb`/`duration_s`; 258/258 |
| Video does not play | there was only the system player | "Watch the video" launches ffplay: video and sound in a separate window |
| The calendar is cropped on the right | 7 columns were wider because of `item_spacing` | a 4 px step, the width from it; the week header too |
| Two tag buttons, photos disappeared | separate passes + a filter by the top tag only | one button; a filter by any weight above the threshold |
| An empty day from "Faces" | the old search/filter hid the shot | `open_from_faces` resets the filters |
| It is unclear how to index a folder | adding did not start a pass | "Add and index", an automatic start, the field on top is filled |
| Indexing is slow | every file was read every time | a skip by path+size+mtime; a repeat 0.024 s, videos 10.2 s |

Measurements: a copy of the DB of 2230 files, `index --jobs 16`: 1971 unchanged, 259 processed (258
videos
+ 1 broken JPEG), 10.2 s; a repeat — 0.024 s. Decoding a 36 MB JPEG — 1.3 s (the preview is visible
at once),
12–14 MB — 0.5 s.

## M13c — no blur while flipping, a normal slideshow

- One background decoder with a queue (the current, the next, the previous, +1), a cache of 4 frames,
  a queue of 8.
- While the next frame is not ready, the previous sharp one is shown; the 320 px thumbnail only in
  the centre
  and is not stretched.
- JPEG on Linux: the TurboJPEG DCT scale (108 MP — 0.52 s instead of 1.33 s).
- The slideshow: an interval of 3/5/10 s, space, in a loop, it waits for a ready frame; ←/→ reset
  the timer.
- Video: "Watch the video" launches ffplay (video and sound in a separate window).
