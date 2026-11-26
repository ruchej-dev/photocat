# photocat architecture

*Русская версия: [ARCHITECTURE.ru.md](ARCHITECTURE.ru.md).*

## Layers

```
L3  main.rs ── app.rs            CLI and GUI
L2  index.rs   dupes.rs   organize.rs   verify.rs   embed.rs   eval.rs   ocr.rs   faces.rs
    recycle.rs   player.rs
L1  db.rs   storage.rs
L0  i18n.rs   derive.rs   tools.rs
```

| Layer | Knows about | Must not know about |
|---|---|---|
| L0 `i18n` | `std` only; `Lang`, the "English source → Russian" table, `t(lang, …)`, `fill(lang, template, values)`, `count(lang, n, noun)` | DB, UI, decoding |
| L0 `derive` | files, `image`, `image_hasher`, `kamadak-exif`, `sha2`, `tools` | DB, UI, parallelism |
| L0 `tools` | `std::process`, `PATH`, `anyhow` | DB, UI, decoding |
| L1 `db` | `rusqlite`, `anyhow`, `tools` (the settings type), `i18n` (the language type) | decoding, egui |
| L2 `index` / `dupes` / `organize` / `verify` | L0/L1 + `rayon`, `walkdir`, `tools` | egui |
| L2 `embed` | L0/L1 + `ort`, `tokenizers`, `serde_json`, `tools` | egui, SQL strings |
| L2 `eval` | L0/L1 + `image`; the labelled sample, CSV and contact sheets | egui, onnx, the network |
| L2 `ocr` | L0/L1 + `rayon`, `tools`; the `tesseract` process | egui, SQL strings |
| L2 `faces` | L0/L1 + `ort`, `rayon`, `image`; ONNX SCRFD and ArcFace | egui, SQL strings |
| L1 `storage` | `db` for the DB paths, `walkdir`; removes only `<cache>/photocat/` | archive, egui |
| L2 `recycle` | `db`, the `trash` crate; moves to the OS trash after confirmation | previews, egui |
| L2 `player` | `rff-format-*`/`rff-codec-*` (demux, H.264/HEVC/MJPEG/PCM/AAC decode), `cpal` (sound), `rayon` (frame conversion) | DB, egui, `tools` (the external ffplay is `app`'s business) |
| L3 `app` | everything below + `eframe`/`egui` | direct SQL strings, decoding on the UI thread |

Language (M21) is a layer like everything else: `i18n` is allowed only to those who talk to the
owner — L1 `db`, L0 `tools` and the L2 modules that print reports (`index`, `dupes`, `organize`,
`verify`, `embed`, `ocr`, `faces`, `recycle`), plus L3 `app`/`main`. The language is passed
explicitly: as a `lang` argument or as a `lang` field in a report struct; there is no global
language variable.

The boundary is checked by `.pi/skills/enforce-architecture/scripts/check.sh` (a grep over
`use crate::`); it also forbids `fs::remove_*` and `fs::rename` calls in `src/`. The language rule is
checked by `tests/source_language.rs`: no Cyrillic identifier anywhere, no Russian comment or
production string in a converted module, and since M23.5 no Russian message inside `#[cfg(test)]` —
the guard parses the assertion, `panic!`, `bail!`, `anyhow!`, `.expect("…")` and `#[ignore = "…"]`
positions with a small tokenizer and leaves the arguments a test compares alone, because those are
the Russian labels the window must show. There is one
exception — `src/storage.rs`: it removes only inside `<cache>/photocat/` after checking the
canonical path; it does not touch the archive or the models.

## Indexing flow

```
walkdir → Vec<PathBuf> → rayon(par_iter, jobs threads)
   per file: metadata → sha256 → (image: decode → dHash + preview 320) → exif → date
        → db::FileRow ──mpsc──► writer thread: one transaction for the whole run
```

- One decode per file: dimensions, dHash, sharpness and the preview come from a single
  `DynamicImage` (`derive::analyze_image`).
- **A repeated pass does not read unchanged files.** The key is path + size + mtime. If the row is
  in the DB, sha256 is not empty, the kind matches and the preview is in place, the file is
  skipped. A video without a cover or a duration is processed again: that is an old index. On the
  corpus of 2230 files an old DB is caught up by 259 files (258 videos + 1 broken JPEG) in 10.2 s;
  a repeat takes 0.024 s. `Stats.skipped` and the line "unchanged N" show this in the GUI and the
  CLI. Clearing the cache ("Settings → Storage → Clear the previews") also nulls `files.thumb`
  (`storage::clear_thumbs` → `forget_thumbs`), so the rule reads "no preview" and the next pass
  rebuilds it instead of skipping the file forever (M25.4).
- `Progress` is atomics + a `Mutex<String>`; the GUI and the console read it without blocking the
  workers.
- An error in one file does not sink the run: the `errors` counter grows and the file is skipped.
  If the file was indexed but without a preview or metadata (broken JPEG, no `ffmpeg`), it stays in
  the DB while the error counter still grows — otherwise the blind spot would be invisible.

## DB schema

```sql
files(id, path UNIQUE, size, mtime, sha256, kind, width, height,
      taken_at, taken_src, lat, lon, camera, dhash, sharpness, duration_s,
      thumb, decision, trashed_at, indexed_at)
tags(file_id, tag, score, source)      -- PK(file_id,tag,source); source='clip:<model_id>': one weight per dictionary tag
tag_defs(model, tag_id, label, prompts, enabled, position, min_pct)
                                       -- the model's tag dictionary: label and prompts (M15.4)
tag_hidden(file_id, tag, hidden_at)    -- the owner removed a tag from a photo
user_tags(file_id, tag, created_at)    -- the owner's own tags, by hand
embeddings(file_id, model, sha256, dim, vec, PRIMARY KEY(file_id, model))
settings(key, value)                   -- model, path, limit, thresholds (dupes, tags, faces),
                                       -- threads, OCR languages, program paths, roots, roots_off,
                                       -- tile_size, verify_*: the check reminder (M10)
ocr(file_id, sha256, lang, text, chars, created_at)   -- text from a photo; PRIMARY KEY(file_id)
ocr_fts(text)                          -- FTS5, content='ocr', content_rowid='file_id'
faces(id, file_id, sha256, x, y, w, h, score, quality, thumb,
      embedding, cluster_id, manual, created_at)
face_clusters(id, label, created_at)
face_scans(file_id, sha256, faces, created_at)       -- the photo has already been scanned
albums(id, parent_id, position, name, name_key, note, created_at, updated_at)
album_files(album_id, file_id, position, added_at, PRIMARY KEY(album_id, file_id))
```

- `kind`: `image` | `video` | `other`.
- `taken_src`: `exif` | `filename` | `mtime` — the source of the date, so that an exact date is
  never confused with a guess.
- `dhash`: `u64` stored as `i64` (SQLite INTEGER). For videos it comes from the cover frame;
  `NULL` only when no frame could be extracted.
- `sharpness`: the variance of the Laplacian over the grey frame or preview (≤512 px), `NULL` for
  old rows before re-indexing and for files without a frame. It is a comparative measure: between
  files of one corpus, not absolute.
- `duration_s`: video duration in seconds from `ffprobe`; `NULL` for photos and for videos where
  `ffprobe` failed. The resolution is stored with the frame rotation applied.
- `decision`: `keep` | `drop` | `NULL` — the owner's decision for a duplicate group. It survives a
  restart and re-indexing and is reset only when the file's `sha256` changes.
- `trashed_at`: the time the file was put into the app trash; ordinary views read
  `WHERE trashed_at IS NULL`. This is not a physical delete: the file on disk is intact and
  `Restore` clears the field. `Empty the trash` moves the file to the OS trash and only then deletes
  the row (`recycle.rs`).
- `settings.roots` / `settings.roots_off`: the archive folders and the ones switched off (M26.1),
  both one newline-separated string; no schema change. Switching a folder off is a **view filter,
  not a deletion**: `App::reload` drops the rows of a switched-off root before they reach `items`,
  so the grid, the calendar, the albums, the duplicates, the faces, the trash, the semantic search
  and the counters see one visible set. The rows, the previews and the files on disk stay, and
  switching the folder back on needs no pass. Indexing and the face, tag and text passes skip a
  switched-off root; `stats` names the switched-off folders with their own counts.
- `user_tags`: the owner's own tags. They are not mixed with CLIP: the filter checks both sources
  and the card shows them in a separate block. A change of `sha256` does not wipe own tags — that is
  a human decision.
- `thumb`: a path relative to `~/.cache/photocat/thumbs/`, the key is `sha256[..24]` (identical
  files share one preview). The preview is built **with the EXIF orientation applied** (`load_oriented`),
  so `width`/`height` in the row are the dimensions the owner sees, not the raw pixel order. The cached
  file is rewritten when its own dimensions do not match the expected ones — that is the only way an
  old sideways preview is cured without re-hashing the whole archive; the file is overwritten in the
  cache, never deleted (see ADR 0045).
- `faces.embedding`: the ArcFace vector, 512 f32 (2 KiB), of an aligned face; `thumb` is the 112 px
  crop in the cache, key `<sha256[..24]>-<face number>`. `manual=1` means the owner split the face
  off with the crossed-head button of the tile; such groups are not rebuilt. Replacing `files.sha256` via a trigger
  wipes the faces and the pass mark.
- `faces.cluster_id`: the group; `face_clusters.label` is the name given by hand. A non-empty label
  also pins the group during a rebuild.
- `ocr.text`: text from a photo produced by `tesseract`. One row per file, `sha256` is the content
  at the moment of recognition; the trigger `invalidate_ocr_after_sha_change` wipes the text if the
  file was replaced. `ocr_fts` is FTS5 (`tokenize='unicode61'`), kept in sync by triggers on `ocr`.
- `albums` / `album_files`: the album tree and the order of files in it (M19). `parent_id = 0` is the
  root (not `NULL`: `NULL` is not compared in `UNIQUE`). A name is unique **among siblings**
  case-insensitively (`parent_id, name_key`), so "2023/Dacha" and "2024/Dacha" are different
  albums. `position` is dense 0..n−1 for files and for subalbums; `db` renumbers it after an
  insert, a move and a delete. This is data, not a view: the grid sort does not change the order.
  An album stores only `files.id`; one snapshot can be in several albums but only once in one
  (`PRIMARY KEY(album_id, file_id)`). No cover is stored: the previews of the first non-trashed files
  by `position` serve as the card cover — a mosaic of up to four, `db::ALBUM_COVERS`
  (`albums_tree().covers`, M22.21). `updated_at` (unix seconds) is touched by
  every change of the row **and** of its contents (`add_to_album`, `remove_from_album`,
  `move_in_album`): it is the date an album card shows and the key of the "By update" order
  (M22.20). The card orders (`db::AlbumSort`, `db::album_order`) are a look — `position` stays the
  order of the export.
- `PRAGMA foreign_keys` is not enabled, so there are no cascades: `delete_album` recursively clears
  the subtree (`album_files` → `albums`) in one transaction, and deleting `files` rows (index
  reset, "remove the folder") clears `album_files` and compacts `position`. The files on disk do not
  change: an album is rows.
- `embeddings.vec`: f32 little-endian, `dim` numbers from the model (512 — 2 KiB, 768 — 3 KiB per
  photo); `dim` is a property of `ModelEntry`, not a constant; `sha256` is the file content at the
  moment of the computation. If `files.sha256` changes, the trigger
  `invalidate_clip_after_sha_change` deletes the vectors and CLIP tags of that file; other tags are
  untouched.
- `tags.score`: the cosine "photo ↔ prompt". One weight is stored per enabled dictionary tag; only
  the first tag is considered confident, if it is ≥ 0.22 (`settings.tag_show`) and leads the second
  by 0.01 (`settings.tag_margin`, `embed::confident_tag`). This is about the card, OCR and `{tag}` in
  the showcase.
- `tag_defs`: the tag dictionary **per model** (`model` = `embeddings.model`), the row key is
  `(model, tag_id)`. `label` is the Russian caption and also the key in `tags.tag`; it is unique
  within the model (`UNIQUE(model, label)`), otherwise two `tags` rows would share one key.
  `prompts` is one template per line; a label may have 2–3 common templates, and its weight is the
  **best** of them (not the average: a description may match only one wording, the average blurs
  it). `enabled=0` means the tag is neither computed nor shown in the filter. `position` is the
  order in the filter and the editor, `min_pct` is the minimum display percentile on top of the
  slider. The default (`embed::DEFAULT_TAGS`, 14 common labels, 2–3 templates per label) is seeded
  only if the model has no rows; a rerun does not overwrite the owner's edits. The set version
  (`settings.tag_defs_version`) is bumped together with the default, and `settings.tag_defs_sig` is
  the fingerprint of the seeded dictionary: a new default replaces the old one only when the
  fingerprint matches (nobody edited it), otherwise the dictionary stays; an import is marked with
  version 0 and is not subject to auto-update. Photo vectors do not depend on the dictionary, so
  editing a label or prompts is `embed --retag`: one pass of the text tower over the stored
  `embeddings` (2229 files in 1.0–1.2 s), without CLIP over the photos.
- The **filter** threshold by tag is a separate setting, `settings.tag_show_pct`, a percentile 0–100
  (ADR 0032). It is computed per tag: 100% is the strongest snapshot of that tag, 0% is all of them,
  70% is the top 30%. At 100% a selected tag always has at least one snapshot, and a lower maximum
  of another tag does not zero it. The distributions (`embed::tag_distributions`) are built when the
  tags are loaded, not per frame. Below 100 files the percentile is noisy, so the raw `tag_show` is
  used. In the card a confident tag is highlighted; when there is no gap it says "tag: not sure",
  shows the top-3 with weights and percentiles, and puts the rest of the weights under "all tags
  (N)", without inventing an answer. The model input is an ensemble of two frames: a centre crop
  224×224 and the whole frame with margins (letterbox, the margins are the average colour of the
  frame); the vectors are averaged and normalised again. The preview is decoded once for both
  frames, and one vector stays in the DB. `embed --refresh` resets the vectors of the active model:
  without it the sha256 key treats the old computation as current. The crop moves "document" out of
  the leaders but does not guarantee the right top-1 — the inflated base of common prompts remains
  (see § Measurements, `8.jpg`); the percentile threshold and the explicit "not sure" are
  M15.2–M15.3, the common prompts are M15.5 (`plan/M15-tags.md`).

A schema change happens only through `CREATE TABLE IF NOT EXISTS` / `ALTER TABLE` in `db::init`, and
the data is preserved. The schema has a `PRAGMA user_version` (`db::SCHEMA_VERSION`): while the
version matches, `open` performs no write at all. That matters not for beauty — otherwise every
`open` (opening a file, `stats`, a search) would try to take a write lock and wait for a long run:
before this fix, reads during OCR hung for 5 s and failed with "database is locked".

## Video

`derive::analyze_video`: `ffprobe -of json` gives the duration, width and height; a rotation from
`side_data_list`/`tags.rotate` of 90/270 swaps width and height. The cover frame comes from the
external `ffmpeg` (ADR 0010): `-ss 10%` before `-i` (a fast seek), one frame, `scale=320:320` with
`force_original_aspect_ratio=decrease`; if the seek yields no frame, the command is repeated from
zero. `dhash` and `sharpness` are computed from the saved frame; the preview lies in the same cache
under `sha256[..24]` as the photos. An `ffmpeg`/`ffprobe` error is soft: the row lands in the DB
without a preview and the `errors` counter grows. The console window is suppressed on Windows
(`CREATE_NO_WINDOW`), on Unix it is a plain `Command`.

**Video playback.** Since M14 a video plays in the built-in player (`src/player.rs`, ADR 0030): the
frame is drawn in the app window, the sound goes through `cpal`. External programs are no longer
needed for viewing. A file the player cannot handle (a codec out of scope, a broken container) is
refused honestly: nothing is started, the viewer shows the reason and offers `ffplay` / the system
player behind explicit buttons (M26.12) — the app never opens a second window by itself.

Three threads, each responsible for one thing:

```
photocat-video    its own demuxer + frame decode → queue (3 frames ahead)
photocat-audio    its own demuxer + audio decode → ring (~2 s) → cpal
photocat-player   clock and presentation: frame → RGBA (1280 px) → Shared.frame
```

- **Why three and not one.** The first version decoded frames and sound in one thread: while a
  frame was being decoded the audio ring drained, the sound choked, and the clock, counted from
  played samples, slowed down — and the video "barely moved" together with the sound. Now the audio
  is fed by its own thread and does not depend on the video decode.
- **The clock.** The master is the sound, but only while it actually plays: the ring holds at least
  20 ms of data. If the audio is starved (or there is no device), the wall clock takes over and the
  video continues. So a "sagging" sound no longer stops the picture.
- **Lag.** Without sound frames are not dropped: the clock follows the decoder, the video plays
  slower but smoothly. With sound a lag of up to 0.5 s is allowed; if the decoder falls more than
  1.2 s behind, presentation asks the video thread to jump to the current position (at most once in
  1.5 s). Two such catch-ups in 4 s mean the decoder cannot keep up: the sound is turned off and
  the video plays as well as it can — this is visible in the status.
- **Frame size.** The frame is downscaled to 1280 px in width (`Options::max_width`): the screen
  does not show 2520 pixels in a row, and every extra thousand costs megabytes per frame. The
  YUV→RGBA conversion is 8.8 fixed-point integer arithmetic by the matrix from the container,
  honouring the full/limited range and the rotation (`rotation` from MP4); rows are computed in
  parallel (rayon).
- **Seek.** By the sample index (`SampleIndex::sync_at_or_before`): the decoders restart from zero,
  the cursors go to a keyframe, the queue and the ring are cleared. Measured: a jump to 10 s gives
  the first frame in 0.11 s.
- **Stop.** `Player::stop` does not wait for the threads (`join` hung the window: the owner could
  not close the program). The threads see the flag and exit on their own; verified — exiting after
  five frames takes 0.42 s.
- **Memory.** A queue of 3 frames (YUV), an audio ring of ~2 s, one frame for presentation. Lag is
  dropped, not accumulated.
- **Codecs.** H.264 (the whole corpus), HEVC Main 8/10 bit, MJPEG in a `.MOV` (the Panasonic
  Lumix camera; `rff-codec-jpeg`, the packet's PTS becomes the frame's when the still-picture
  decoder has none), PCM sound (`rff-codec-pcm`) and AAC-LC. Other frame formats and codecs
  (VP9, Opus) are rejected by name; the viewer names the codec and waits for an explicit press.
- **Build.** The decoder in a plain debug profile gives 2.2 fps instead of 41.9 — `cargo run`
  turned video into a slideshow. In `Cargo.toml` the dev profile is tuned: dependencies
  `opt-level = 3`, our own code `opt-level = 1`; after that `cargo run` shows 1080p at 29.2 fps.
  Release is still the fastest and remains what we measure with.
- **Checking without a screen:** `photocat play FILE [--frames N] [--seek S] [--no-audio]
  [--decode-only] [--max-width W] [--dump frame.ppm]`.

**Full-screen viewing.** A snapshot and a video have different paths, and the code says so
explicitly. A video has no "full frame" (nobody decodes it), so the 320 px cover frame is shown with
a big ▶ button in the centre; the previous snapshot frame stays on screen only for
`kind == "image"` — before, it stayed on top of the video and paging looked like "it shows nothing".
The player frame is drawn only while `video_id` matches the open file. Paging (←/→, the panel icon
buttons, the slideshow) first calls `viewer_step`: it stops the player and only then changes
`Viewer::idx`; the slideshow does not wait for the next video's decode, because there is nothing to
wait for. Space on a video is play/pause (also by clicking the frame), on a snapshot it is the
slideshow. When a video has finished, `poll_video` sets `video_ended`, and the viewer opens the next
file itself; on the last file the cover frame stays so that viewing does not start over.

**Own tags and faces together.** `user_tags` are added by hand in the card and take part in the
filter. In "Faces" groups can be ticked and "Photos where everyone is together" can be requested: a
photo enters the selection only if it has at least one face from each ticked group. The ticked faces
can be assigned to one group in a batch (`db::group_faces`, `manual=1`).

## Album export (M20)

`organize::plan_album` turns the album tree into a plan: `--out/<album>/<subalbum>/…`, file names by
template (`{n}_{orig}` by default, `{n}` is the position in its own album, 2 digits up to 99, 3 up
to 999, then by the number of digits). Subalbum directories have **no numbers**: the file system
does not know `position`, on disk they go alphabetically, and the order stays in the app and is
visible through the file numbers. Planning only reads the DB and creates nothing.

`organize::apply_export` is the only place where the app writes into a user directory outside
`organize --apply`: copies via `std::fs::copy`, not links and not hardlinks (the inode differs);
`export` has no `--mode`. Files in the app trash do not enter the plan. Nothing in the target is
overwritten or deleted: a name taken by other content gets `-2`; a file with the same `sha256` is
skipped (the run is idempotent); the same snapshot under an old number is "target order is stale"
(reordering does not rename what has already been exported). The name sanitiser is the shared
`organize::tidy`/`component`; directory names are additionally cleaned by `tidy_sep` (a forbidden
character becomes `_`), an empty name becomes `album-<id>`, and Windows device names get `_`.

Existing target files are compared by size, then by `sha256` (this is reading, not writing). The
cache `<cache>/photocat/exports/<album>-<directory hash>.txt` stores the `sha256`, size, mtime and
relative path of the previous export: if the size and mtime match, the file is not read again. The
cache lives in the app cache, not in `--out`: the target stays a clean directory without service
files. CLI codes: 0 — plan/success, 1 — there were copy errors, 2 — arguments, a missing `--out` or
an `--out` inside `settings.roots`.

## Integrity: comparison and manifest (M8, GUI — M9)

`verify::verify`, `verify::manifest` and `verify::check_manifest` are shared by the CLI and the
"Integrity" screen: the GUI rewrites neither the traversal, nor the comparison, nor the report text.
The report is assembled by `verify_report`, `check_report` and `manifest_report`; the CLI prints
them with `print!`, the screen shows the same text in a scroll area and copies it to the clipboard
with "Copy the report".

All three passes only read. The only write is the manifest file by an explicit field and button; an
existing file is not overwritten without a second press ("Overwrite the manifest"). A cancelled or
failed pass does not write an incomplete manifest: otherwise a later check would declare unread
files "extra". Progress is `db::Progress` (the files that need a sha256, not all of them), "Stop" is
the same `cancel` flag as in indexing: the started file is finished, `cancelled`/`unchecked` land in
the report and are not counted as a match. There is one background thread: a new pass is not
started while the previous one runs.

## Reminder to check (M10)

The check deadline is data, not a timer: `settings.verify_root`, `verify_manifest`,
`verify_interval_days` (30 by default, 0 — off), `verify_last_at` (RFC3339 UTC), `verify_last_ok`,
`verify_last_report`. One function `db::verify_due(&settings, now)` is used by the banner, the
"Integrity" screen and the tests; `now` is passed explicitly, so "exactly due", "overdue", "too
early" and "the clock went back" are tested without waiting. A date in the future does not panic and
does not make it "due": the deadline simply has not come. An empty or broken date means "due".

On start the GUI shows the banner "Time to check the archive" with "Check" and "Remind later";
"Remind later" lives only in memory until a restart. There is no automatic pass: "Check" opens the
"Integrity" screen and starts the same background check against the manifest as the button on that
screen (`verify::check_manifest`), with progress and "Stop". The interval is edited on the
"Integrity" screen and written to `settings` on change.

A finished manifest or check writes the date, "ok" and the full report to `settings` and to
`<cache>/photocat/verify-last.txt`; nothing goes into the archive directory. Writing a manifest also
sets the reference point: the archive has just been read in full. A cancel or an error (a broken
manifest, a missing file) does not touch the previous successful report and date — otherwise the
reminder would lose the last good state. The CLI path `verify --manifest` for cron/systemd is
unchanged; the app sets no system timers.

## Duplicates: the block index (M11)

`dupes::find_groups` first merges exact copies by `sha256`, then looks for snapshot pairs by dHash.
Up to 20 000 hashes (`INDEX_MIN`) the old pairwise pass runs: on 2230 files it takes 0.017–0.025 s.
Above the threshold an index of eight buckets by dHash bytes is built: if the distance is ≤ 7, at
least one byte matches entirely, so the candidates are the union of the eight buckets, and only they
are checked with the full Hamming distance over `u64`. For a radius of 8–64 the pairwise pass
remains (the index gives no guarantee, and such thresholds glue almost everything together).
`find_groups_with(..., index_from)` is the only path selection; a test compares both on the same
random sets.

A bare BK-tree was rejected for this task after measuring: at radius 6 on 64-bit hashes almost all
points lie at a distance of ~32, and the triangle inequality does not cut off the bulk — 20k in
2.58 s, 50k in 17.2 s (ADR 0038). An empty dHash does not enter the search; union-find, the frame
advice and `plan_csv` are unchanged, and there are no new dependencies.

## Model catalogue (M12)

A model is a data record, not a constant: `ModelEntry` holds the `id`, the directory, the name, the
licence, `dim`, the input frame side, the token maximum, the normalisation, the number of frames and
the ONNX output names. `DIM` is gone from the code: the dimension comes from the model and is
checked against the ONNX metadata at load time and against the actual output on the first run; a
mismatch is an error with both numbers, not a silent comparison of vectors of different lengths.

The catalogue holds four models:

| id | architecture | vector | text | frames | size | licence |
|---|---|---|---|---|---|---|
| `clip-vit-b32-multilingual` | CLIP ViT-B/32 + DistilBERT+Dense | 512 | Russian/English | 2 (crop+frame) | 217.5 MiB | MIT + Apache-2.0 |
| `clip-vit-b32-en` | CLIP ViT-B/32 | 512 | English | 2 (crop+frame) | 148.6 MiB | MIT |
| `siglip-base-224-en` | SigLIP base patch16-224 | 768 | English | 1 (whole frame squared) | 203.5 MiB | Apache-2.0 |
| `siglip2-base-224-multi` | SigLIP 2 base patch16-224 | 768 | 100+ languages (Gemma tokenizer) | 1 (whole frame squared) | 393.3 MiB | Apache-2.0 |

`TextKind` describes how to read the text tower: `Projected` — hidden states + dense 768→dim,
`Direct` — CLIP `text_embeds`, `Pooler` — SigLIP `pooler_output`; the pooler reads the **last**
token, so the padding rules are part of the model record. SigLIP 1 is padded on the right with
`</s>`; SigLIP 2's tokenizer pads to 64 on the right with `<pad>` itself, so `pad_left` strips that
tail and pads on the left — the EOS stays last. Without it every text collapses into one vector
(cosine between "a dog" and "a cat" 0.976) and the image tags stop differing at all. SigLIP has
its own mean/std 0.5 normalisation and a bicubic resize; the frame is neither cropped nor padded but
stretched to 224×224, as in its processor. Vectors and tags of different models live side by side:
`embeddings` keyed by `(file_id, model)`, `tags` by `(file_id, tag, source)` (schema 7). Search, tags
and clusters read only the active model; switching does not wipe the other vectors.

**The tag weight is calibrated, not compared raw.** The cosine scale is a property of the model:
CLIP's median best tag is 0.263, SigLIP 2's is 0.086, SigLIP 1's is 0.059 — so one `tag_show` of
0.22 would mean "every shot is confident" for CLIP and "no shot ever is" for SigLIP (that was the
M12 tail). `ModelEntry::score_calib` holds an affine map `cos * scale + shift` into the CLIP scale,
and `Model::tag_scores` stores the mapped value in `tags`. The coefficients are chosen so that both
medians (the best tag and its lead over the second) land where CLIP's are: SigLIP 2 `(1.42, 0.14)`,
SigLIP 1 `(0.55, 0.23)`, CLIP `(1.0, 0.0)` — identity. Values are in **cosine**, so the percentiles
(`tag_show_pct`) are unaffected by the map, and `embed --retag` recomputes the rows from the stored
vectors in 2–4 s. `score_version` is part of `tags.source` (`clip:<id>@v2`), so rows scored under an
older calibration are never mixed into the new distribution; ADR 0041.

**SigLIP 2 is opt-in, not the default.** It does not fit the 250 MiB limit (393.3 MiB, of which
270 MiB is the 256k-token Gemma text tower), it is 2.7× slower than CLIP on the same CPU, and on
the owner's archive it is **worse** than CLIP on the document category (see § Measurements).

**The 8-bit files are not the reason.** The open tail of M15.6 was whether the int8 quantisation,
not the model, costs the document recall; the fp16 towers (vision 186.0 MiB + text 564.9 MiB) were
measured on the same 2231 files and made it **no better**: the confident share stays at 55.2–55.8 %,
the fp16 text tower catches fewer documents (64.8 % against 69.0 %), and the fully fp16 model only
trades recall for precision (49.0 % caught, 3.6 % false against 69.0 % / 10.0 %). Full fp16 would
cost 748.9 MiB and a slower load for nothing, so it is not a catalogue entry; the decision and the
numbers are in ADR 0043 and `plan/STATUS.md` § M15.6 tail.

## Labelled check of the tags (M15.7)

Tags are hints, so the only honest way to tell whether a change helped is a sample labelled by a
human and scored by code. `src/eval.rs` (L2: `db`, `embed`, the `image` crate) does the mechanical
half:

```bash
photocat eval --db D --folder DIR --out OUT [--sample 120]   # the sample, labels.csv, sheets
photocat eval --db D --labels OUT/labels.csv                 # the numbers
```

The default output is `<cache>/photocat/eval/`: `labels.csv` carries archive paths, so it stays out
of the repository, like the cluster report of M3.

**The sample is reproducible and blind.** Files of the folder that have a preview and tags are
sorted by `sha256` (content, not name or date) and every k-th is taken, so the same folder always
gives the same sample and the choice does not depend on what the model likes. `labels.csv` has only
`n,path,label`; the model's top-3 go to a separate `guesses.csv`, so a label can be filled in
without seeing the prediction. Contact sheets (`sheet-NN.png`, 16 tiles of 320 px with the number
burned in) make labelling possible without opening 120 files one by one; the tiles come from the
previews in the cache, the archive is only read. Re-running the export keeps labels already filled.

**Labels are tag ids, not free text:** one of the dictionary ids (`furniture`, `children`, …),
`other` — no dictionary label fits (an adult portrait, a household detail, a document form), or
`skip` — the shot cannot be judged. Two ids joined by `|` mean both are equally acceptable
(`children|city`); that is honest for a boundary shot and is counted as one more column in the
report. Labels are never tuned to one folder and never derived from the predictions.

**The metrics** (all over the shots with a dictionary label, `N`):

- **top-1 / top-3** — the label appears in the first one / in the first three weights of the file.
- **coverage** — the share of the sample a dictionary label fits at all; it bounds everything else.
- **confident tag accuracy** — of the shots where the model was confident (weight ≥ `tag_show`,
  lead ≥ `tag_margin` — the same rule the card uses for "not sure"), the share where the first tag
  is right.
- **confidently wrong** — the `other` shots that still got a confident tag. This is the number that
  hurts the owner: the card highlights a wrong label instead of saying "not sure".
- **a tag at the percentile filter** — how many labelled shots of a tag the filter keeps at 70 % and
  100 %; the 100 % threshold keeps the single strongest shot of the whole index by construction, so
  it is a sanity check, not a quality measure.

**Limits.** The labels are one reviewer's reading of a 320 px preview; a coarse dictionary cannot
name every shot (a third of the sample is `other`); 120 shots give about ±10 percentage points at
95 % confidence. The numbers are comparable between models and between runs of the same protocol,
not with a different labelled set.

## Measurements

| Date | Corpus | Operation | Result |
|---|---|---|---|
| 2026-11-22 | 2230 files, 21.2 GiB (`Фотографии с телефона`) | `index --jobs 16`, release | 64.9 s, 0 errors, DB 1.2 MB, previews 29 MB |
| 2026-11-22 | a copy of the corpus DB, 2230 files | `index --jobs 16`, old index | 1971 unchanged, 259 processed (258 videos + 1 broken JPEG), 10.2 s; video covers 258/258 |
| 2026-11-22 | the same DB | repeated `index --jobs 16` | 0.021 s, 2229 unchanged, 1 broken JPEG read again |
| 2026-11-22 | JPEG 108 MP, 36 MB (12032×9024) | old path: full decode + thumbnail 1920 | 1.33 s |
| 2026-11-22 | the same JPEG | new path: TurboJPEG DCT 1/4 → 3008 px | 0.54 s; the 320 px preview is shown immediately |
| 2026-11-22 | 2230 files, 21.2 GiB | `index --jobs 16`, release, with sharpness | 60.2 s, 0 errors, DB 1.3 MB — sharpness is invisible in the cost (machine load noise is larger) |
| 2026-11-22 | 1972 photos | `dupes --max-dist 6 --csv --plan` | 103 groups, 238 files, 214.1 MiB; `sharpness` filled for all 238 rows |
| 2026-11-22 | 122 files (hardlinks to the corpus) | `index --jobs 16`, release | 1.5 s (~80 photos/s) |
| 2026-11-22 | 1972 photos | `dupes --max-dist 6` | 0.03 s, 103 groups, 238 files, 214.1 MiB |
| 2026-11-23 | a copy of the corpus DB, 2230 files | `dupes --max-dist 6`, release | 0.017–0.025 s, the same 103 groups, 238 files, 214.1 MiB |
| 2026-11-24 | a copy of the corpus DB, 2230 files | `dupes --max-dist 6`, release, English report (M21.6) | 0.023 s, the same 103 groups, 238 files, 214.1 MiB — translating strings did not touch the logic |
| 2026-11-24 | the same DB | `stats` in both languages (M21.6) | 2230 files, 21.2 GiB, 258 videos, 3:19:46; the numbers match, only labels and units change (`ГиБ`/`GiB`) |
| 2026-11-23 | synthetic 20k hashes, pairs differing by 1 bit | `find_groups` release test, `max_dist 6` | 0.082 s, 10 000 groups, test peak RSS 27.1 MB |
| 2026-11-23 | synthetic 50k hashes, pairs differing by 1 bit | `find_groups` release test, `max_dist 6` | 0.446 s, 25 000 groups, test peak RSS 48.2 MB |
| 2026-11-23 | synthetic 20k / 50k hashes | the rejected BK-tree, release test | 2.58 s / 17.2 s (a non-working variant, ADR 0038) |
| 2026-11-22 | 1971 photos with previews (corpus 2230) | `embed --jobs 8 --batch 8`, release | 66.9 s (34 ms/photo), 0 errors, vectors 3.85 MiB, 13 797 tags, model 217.5 MiB |
| 2026-11-22 | the same corpus | repeated `embed` | 0.025 s: 1971 skipped by sha256 |
| 2026-11-22 | 20 120 vectors (1971 real + synthetic) | `search "кошка" --repeat 4` | 48–51 ms per query, including reading 40 MiB from SQLite |
| 2026-11-22 | 1971 photos, 30 clusters | `clusters --k 30` | 1.6 s, a report of 30 clusters × 12 examples |
| 2026-11-22 | 122 files | `index --jobs 8`, **debug** | 48 s — a debug build is unusable for images |
| 2026-11-22 | 2230 files, 21.2 GiB, incl. 258 videos (17.4 GiB) | `index --jobs 16`, release, with ffmpeg | 84.7 s (+20 s over the base), 1 error (the known broken JPEG), DB 1.3 MB, previews 33 MB (+4 MB for covers) |
| 2026-11-22 | 258 videos | `ffprobe` + a 320 px cover frame | 258/258 with a preview, dHash and duration; 3:19:46 in total; 0 ffmpeg failures |
| 2026-11-22 | 3 videos (original + transcode + an unrelated one) | `dupes --max-dist 6` | the transcode glued to the original: 1 group, Δ2, 1.4 MiB of "extra" |
| 2026-11-22 | corpus 2230 files, photos + 258 videos | `embed --jobs 8 --batch 8` | 2229/2229 in 77.3 s (34.7 ms/file), 0 errors, vectors 4.35 MiB, 15 603 tags, DB 12 MB |
| 2026-11-22 | the same DB, 2229 vectors | `search "торт"` (the model already loaded) | 11 ms, videos and photos with cake first |
| 2026-11-22 | 2229 files, k=30 | `clusters --k 30` | 1.0 s |
| 2026-11-22 | network, 155 MB | `models download --id clip-vit-b32-en --yes` | 18.8 s, 148.6 MiB, the sha256 of all 3 files matched |
| 2026-11-22 | limit 100 MiB | `models download` | both models rejected by size, an error with the numbers |
| 2026-11-22 | limit 160 MiB, a non-existent model requested | `models download --id nonexistent --yes` | `clip-vit-b32-en` (148.6 MiB) picked and verified |
| 2026-11-22 | 1972 corpus snapshots | `ocr --dry-run` | 435 candidates: no EXIF camera or a confident "document"/"screenshot" tag; 1 ms |
| 2026-11-22 | 435 candidates (tesseract 5.5.4, rus+eng) | `ocr --jobs 8`, release | 106.2 s (0.24 s/file), 434/435 (1 known broken JPEG), 66 444 characters in 146 files |
| 2026-11-22 | 435 candidates | `ocr --jobs 4`, release | 169.8 s — 1.6× slower, the same text (66 444 characters) |
| 2026-11-22 | 434 recognized texts | repeated `ocr`, release | 0.0 s, 434 skipped by sha256 |
| 2026-11-22 | 434 texts, 66 444 characters | `search --text` ("сбербанк", "счет-фактура", "2023") | 2–3 ms per query |
| 2026-11-22 | DB of 2230 files | before / after OCR | 12.4 → 12.7 MB (+276 KiB: text and FTS5) |
| 2026-11-22 | 10 documents | quality by eye | a Sberbank receipt, an invoice and a book page read in full; a price list with glare — partially |
| 2026-11-22 | 1972 corpus snapshots | `faces --jobs 4` (SCRFD+ArcFace, threshold 0.45) | 630.2 s, 1972/1972 scanned (1 error — a broken JPEG), 2498 faces in 856 snapshots, 617 groups, DB 11.1 → 22 MB, face crops 11 MB |
| 2026-11-22 | 2498 faces | `faces --recluster --threshold 0.45` | 1.2 s, 617 groups; the largest 362, 252 and 225 faces |
| 2026-11-22 | 2498 faces | threshold 0.40 / 0.50 / 0.60 | 534 / 710 / 1006 groups — the threshold changes the glue; labels and manual splits stay pinned |
| 2026-11-22 | network 182.4 MiB | `faces download --yes` | 31.3 s, the sha256 of both files matched |
| 2026-11-22 | 2230 files, 21.2 GiB | `organize --out /tmp/show-day` (dry run) | 0.017 s, 2230 files with a date, 0 without, 573 day directories |
| 2026-11-22 | the same corpus | `organize --by month` (dry run) | 0.019 s, 76 month directories |
| 2026-11-22 | the same corpus | `organize --by event` (dry run) | 0.021 s, 576 directories: 573 days + 3 places, 7 snapshots with GPS |
| 2026-11-22 | the same corpus, template `{date}_{time}_{camera}_{tag}_{orig}` | `organize --name …` (dry run) | 0.037 s; tags (13 797 rows) are read only under `{tag}`; an empty camera/tag leaves no `__` |
| 2026-11-22 | the same corpus | `organize --apply` (hardlink) | 2230 links in 0.065 s, 0 errors, 583 directories; the inode matches the original, link count 2, 0 bytes used |
| 2026-11-22 | DB copy, 135 snapshots marked extra by the duplicate plan | `organize --exclude-dup` (dry run) | 0.015 s: 2095 included, 135 excluded |
| 2026-11-22 | corpus 2230 files, 21.2 GiB, two reflink copies | `verify --jobs 8`, release | 19.9 s; 2230/2230 matched, sha256 read 2 × 21.2 GiB, 0 errors |
| 2026-11-22 | the same corpus with 1 changed, 1 appended, 1 deleted and 1 extra file | `verify --jobs 8`, release | 16.1 s; 2227 matched, size 1, content 1, only in A 1, only in B 1; code 1 |
| 2026-11-22 | corpus 2230 files, 21.2 GiB | `manifest --out corpus.sha256 --jobs 8`, release | 10.1 s; 2230 lines, file 193 KiB, 0 errors |
| 2026-11-22 | the same manifest, 2230 lines | `verify --manifest corpus.sha256 --jobs 8` | 11.4 s; 2230/2230 matched; `sha256sum -c` reads the manifest without errors |
| 2026-11-22 | the corpus manifest against a copy with 1 changed, 1 appended, 1 deleted and 1 extra file | `verify --manifest` | 8.9 s; 2227 matched, 2 changed, 1 missing from the directory, 1 missing from the manifest; code 1 |
| 2026-11-23 | 2230 files, 21.2 GiB | `manifest --out /tmp/...sha256`, release | 8.9 s; 2230 lines, 196 789 B, 0 errors |
| 2026-11-23 | the same corpus and manifest | `verify --manifest`, release | 10.0 s; 2230/2230 matched; code 0 |
| 2026-11-23 | a copy of the real DB (2230 files), deadline overdue by a day | `gui --db /tmp/...` under `timeout 6` | the banner "Time to check the archive", no auto-start; code 124, no crashes |
| 2026-11-23 | network, SigLIP base 224 | `models download --id siglip-base-224-en --yes` | 36.7 s, 3 files, 203.5 MiB, the sha256 of all matched |
| 2026-11-23 | corpus 2227 files (2 in the trash) | `embed --refresh --jobs 8 --batch 8`, release, SigLIP | 418.2 s (0.188 s/file, one 224 frame), 2227/2227, 0 errors; vectors 6.8 MiB, DB 25.4 → 40.1 MB together with the CLIP vectors |
| 2026-11-23 | 2227 SigLIP vectors | `embed --retag` | 2.4 s, 14 tags per model, the tags of both models in the DB (31 206 + 31 178) |
| 2026-11-23 | 2227 SigLIP vectors | `search` (the model loaded), 3 queries | 35–40 ms per query; the relevant top frames; "red/blue" on control squares — the right order |
| 2026-11-23 | 2227 SigLIP vectors | `clusters --k 30` | 1.5 s, a report of 30 clusters × 12 examples |
| 2026-11-23 | 640×480 Baseline, 300 frames | `play --decode-only` | 331.5 fps, RGBA conversion 0.5 ms/frame |
| 2026-11-23 | 1280×720 Baseline, 153 frames | `play --decode-only` | 71.6 fps, conversion 1.0 ms/frame |
| 2026-11-23 | 1920×1080 Baseline, 300 frames | `play --decode-only` | 49.9 fps, conversion 2.6 ms/frame |
| 2026-11-23 | 2520×1080 High, 300 frames | `play --decode-only` | 27.7 fps, conversion 2.9 ms/frame |
| 2026-11-23 | HEVC Main 1920×1080, 140 frames | `play --decode-only` | 25.3 fps, conversion 2.9 ms/frame |
| 2026-11-23 | 1920×1080, 30 fps, 900 frames | `play` (built-in player, with sound) | 29.8 fps — real time, the sound plays, position 29.95 s in 30.2 s |
| 2026-11-23 | 1280×720, 9 fps source, 150 frames | `play` (built-in player) | 9.2 fps — exactly real time |
| 2026-11-23 | 2520×1080 High, 30 fps, 200 frames | `play` (built-in player, with sound) | 22.9 fps: decode 27.7 fps, the lag is capped at 0.5 s, the sound stays |
| 2026-11-23 | the same file before the fix (single decode thread) | `play` | 6.7–11 fps, the sound and the picture "barely moved" |
| 2026-11-23 | 1920×1080, seek to 10 s | `play --seek 10 --frames 60` | first frame 0.11 s, position 11.96 s, 60 frames in 2.23 s |
| 2026-11-23 | exit after 5 frames | `play --frames 5` | 0.42 s — `stop` no longer waits for the threads |
| 2026-11-23 | 1920×1080, 100 frames | `play --decode-only`, **plain debug** | 2.2 fps — `cargo run` without the profile fix gives a slideshow |
| 2026-11-23 | the same file, 300 frames | `play --decode-only`, dev profile (`opt-level` 1 / 3) | 39.0 fps; presentation with sound — 29.2 fps |
| 2026-11-23 | a 2520×1080 frame against `ffmpeg` | `play --dump` + PPM | orientation and colour matched: mean \|Δ\| per channel 0.63, max 3 |
| 2026-11-23 | `8.jpg`, CLIP ViT-B/32 multilingual, 224 px | centre crop vs the whole frame (letterbox), one frame | centre: document 0.267 / furniture 0.254; whole frame: furniture 0.254 / document 0.252. Preliminary diagnostics; the rerun below refined the conclusion — the crop is necessary but not sufficient |
| 2026-11-23 | `8.jpg` 433×800, CLIP ViT-B/32 multilingual | centre / whole frame / ensemble, full frame | centre: screenshot 0.278 / document 0.275 / interior 0.271 / furniture 0.257; whole: interior 0.274 / furniture 0.262 / screenshot 0.261 / document 0.257; ensemble: interior 0.278 / screenshot 0.275 / document 0.272 / furniture 0.265. The crop pushes "document" down, but "furniture" does not become first — M15.3 and M15.5 are needed |
| 2026-11-23 | corpus 2229 files, two frames per snapshot | `embed --refresh --jobs 8 --batch 8` | 176.6 s (79 ms/file, 2.3× over 77.3 s), 0 errors; "furniture" ≥0.22: 717→1029; top-1 "document" 266→248; confident tag 1284→1285; for 1263 of the 1284 old confident ones the top-1 is the same (98.4%); vectors 4.35 MiB, the DB did not grow |
| 2026-11-23 | a copy of the real DB, 2230 files, 25.4 MB | opening `albums list` (schema migration 4 → 5) | data intact: 2230 files; DB 25 403 392 → 25 411 584 B (+8 KiB for two tables) |
| 2026-11-23 | the same DB, a 3-level tree: "Дача 2024 → Май → Неделя 1", 3 more nodes | `albums add --id 2` with 500 paths | 25 ms, 500 added, positions 0..499; DB 25 411 584 → 25 440 256 B (+28 KiB) |
| 2026-11-23 | the same 500 rows | repeated `albums add` | 19 ms, 0 added, "already had 500"; the order and the DB did not change |
| 2026-11-23 | a copy of the real DB, a 3-level tree, 500 files (2.4 GiB), `/tmp` | `export --album 1 --out /tmp/m20-500a` (dry run) | 14 ms; 4 directories, 500 files, 2.4 GiB, 0 collisions, 0 long paths |
| 2026-11-23 | the same 500 files, an empty target | `export --apply` | 2045 ms, 500 copies, 2.4 GiB, 0 errors; the inode differs, sha256 matched; nesting `Дача_2024/Май/Неделя_1` |
| 2026-11-23 | the same target after the first export | repeated `export --apply` | 20 ms: 0 exported, 500 skipped, 500 files in the target; the cache in `exports/` — 500 lines |
| 2026-11-23 | a copy of the real DB, 2230 files (21.2 GiB) in one album | `export --album 5 --out ~/pctest-m20-all` (dry run) | 21 ms; 1 directory, 2230 files, 21.2 GiB |
| 2026-11-23 | the same 2230 files, an empty target | `export --apply` | 111 ms, 2230 copies, 21.2 GiB, 0 errors; the FS gave `copy_file_range` (reflink/extent copy), on a FS without it the time will be closer to writing the volume |
| 2026-11-23 | the same target | repeated `export --apply` | 45 ms: 0 exported, 2230 skipped, 2230 files in the target; 5 checked copies — sha256 matched, the inode is different |
| 2026-11-23 | a copy of the real DB, 2229 vectors, CLIP ViT-B/32 multilingual | `tags --add "Шкаф" --prompt …` + `embed --retag` | 1.0–1.2 s to recompute the weights of all 2229 files (the model already on disk), the photos were not read; 15 tags in the DB, "Шкаф" 0.187–0.292, top-1 for 218 snapshots |
| 2026-11-23 | the same 15 tags, "Шкаф" | percentile 70% (`tag_show_pct`) | threshold 0.2454, 669 snapshots pass — the same as the other tags at the same percentile |
| 2026-11-23 | the phone corpus, 2229 files; 145 with recognized text, 1655 photos with a camera and no text | prompts v1 (1 per label) → v2 (2–3 common templates, the best weight), `embed --retag` on DB copies | confident 1285 (57.6%) → 1242 (55.7%); top-1 the same for 1855 (83.2%); "document" in top-3 804 → 534, "furniture" 392 → 407, "interior" 849 → 905; a false "document/screenshot" on photos with a camera 270/1655 (16.3%) → 225/1655 (13.6%), on snapshots with text 123/145 (84.8%) — no losses |
| 2026-11-23 | "день рожденья", 110 files (a second independent directory), CLIP ViT-B/32 multilingual | the same: v1 → v2 | confident 66 (60.0%) → 69 (62.7%); top-1 the same for 95 (86.4%); "document" in top-3 38 → 21, "screenshot" 39 → 38; `8.jpg` of the phone corpus: document 0.274 / furniture 0.273 — "not sure" (gap 0.001) |
| 2026-11-24 | the phone corpus (2229 files) + "день рожденья" (111), one `/tmp` DB, 2341 rows | `index --jobs 16`, release | 68.8 s + 4.1 s, 1 error (the known broken JPEG), 23.3 GiB |
| 2026-11-24 | the same DB, 374 OCR candidates | `ocr --jobs 8` | 373 recognized, 142 with text, 68 218 characters, 83.1 s, 1 error |
| 2026-11-24 | the same DB, 2340 `(file, model)` pairs | `embed --jobs 8`, CLIP ViT-B/32 multilingual | 147 s (0.063 s/file, two frames), 32760 tags, vectors 4.6 MiB |
| 2026-11-24 | the same DB | `embed --jobs 8`, SigLIP 2 base 224 multilingual | 398.8 s (0.170 s/file, one frame), 0 errors, vectors 6.9 MiB — 2.7× CLIP |
| 2026-11-24 | the same DB | `embed --jobs 8`, SigLIP base 224 English | 404.5 s (0.173 s/file), 0 errors, vectors 6.9 MiB |
| 2026-11-24 | the same DB, 4 models' vectors and tags | DB size | 62.8 MB (vectors 18.4 MiB, tags 131 040 rows) |
| 2026-11-24 | the same DB, stored vectors | `embed --retag` | 3.5 s (SigLIP 2) and 2.1 s (SigLIP 1) for 2340 files — the photos are not read |
| 2026-11-24 | the corpus + "день рожденья", 2340 files, `tag_show` 0.22 / `tag_margin` 0.01 | median best tag / median lead / confident / documents with text caught / false "document" on photos without text | CLIP: 0.263 / 0.0118 / 55.6% / 85.9% / 3.3%; SigLIP 2 calibrated: 0.263 / 0.0118 / 55.3% / 69.0% / 9.7%; SigLIP 1 calibrated: 0.262 / 0.0119 / 55.6% / 53.5% / 1.0% |
| 2026-11-24 | the same files, both directories separately | the same metrics split by folder | the picture repeats in each folder: CLIP 55.4%/61.3% confident, SigLIP 2 54.8%/64.0% — the calibration does not depend on one folder |
| 2026-11-24 | 142 shots with recognized text, all three models | agreement of the confident label | CLIP and SigLIP 2 are both confident on 815 files and agree on 629 (77.2%) |
| 2026-11-24 | weights of SigLIP 2 (`onnx-community/siglip2-base-patch16-224-ONNX`) | download over HTTPS with sha256 | 412 354 647 B = 393.3 MiB in 83 s (vision 90.2 MiB, text 270.3 MiB, tokenizer 32.8 MiB) |
| 2026-11-24 | 120 shots of `Фотографии с телефона` (of 1969 eligible), labelled blind by the reviewer | `eval --labels`, CLIP ViT-B/32 multilingual, two frames | dictionary fits 68.3 % of the shots; top-1 70/82 = 85.4 %, top-3 80/82 = 97.6 %; confident tag right in 55/69 = 79.7 %; confidently wrong on “other” 14 of 38 (with a second acceptable label on 14 shots) |
| 2026-11-24 | 120 shots of `фото с Wats app от Лизы` (of 289 eligible), the same protocol | `eval --labels` | dictionary fits 71.7 %; top-1 71/86 = 82.6 %, top-3 84/86 = 97.7 %; confident tag right in 49/62 = 79.0 %; confidently wrong on “other” 13 of 34 (a second label allowed on 22 shots) |
| 2026-11-24 | 13 labelled “furniture” shots of the phone corpus (of 120) | percentile filter of the tag, whole index 2229 files | 70 %: found 13/13, threshold 0.235, the filter shows 676 of 2229 shots; 100 %: found 0/13, threshold 0.306 — by construction it keeps the single strongest shot of the tag |
| 2026-11-24 | phone corpus 2231 files | `embed --refresh --jobs 8 --batch 8`, release, CLIP multilingual | 126.1 s (56.5 ms/file, two frames), 0 errors, DB 30 MB together with OCR and faces |
| 2026-11-24 | `фото с Wats app от Лизы`, 289 files, 49.1 MiB | `index --jobs 16` + `embed --refresh` | indexing 1.8 s, embedding 16.7 s (57.8 ms/file), DB 2.3 MB, previews in the cache |
| 2026-11-24 | phone corpus, 2231 files with tags, 145 with recognized text, `/tmp` bench DB (M15.6 tail) | `embed --refresh`, SigLIP 2 int8 vision + int8 text, release | 510.0 s; 55.2 % confident tags, 69.0 % (100/145) of the documents with text caught, false “document” on images without text 10.0 % (183/1828); model 393.3 MiB |
| 2026-11-24 | the same DB and the same image vectors | temporary int8 vision + fp16 text, `embed --retag` | 8.3 s including the model load; 55.8 % confident, 64.8 % (94/145) documents caught, false “document” 12.7 % (232/1828); model 661.6 MiB |
| 2026-11-24 | the same files, image vectors recomputed | temporary fp16 vision + fp16 text, `embed --refresh` | 528.7 s; 55.4 % confident, 49.0 % (71/145) documents caught, false “document” 3.6 % (65/1828); model 748.9 MiB |
| 2026-11-24 | each of the three variants, the same DB | process-level `search "документ"` | 2.8–4.6 s int8, 4.2–5.1 s int8+fp16 text, 3.9–4.0 s fp16 — dominated by the sha256 of the weight files and the ONNX session load; the query itself is milliseconds |
| 2026-11-24 | a copy of the real DB, 2232 files, 1240×840 headless frames (M22.2b) | 160 frames of continuous scrolling over the grid: 80 wheel notches and a 80-frame trackpad gesture with the coast (release, `PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored grid_scroll_measurement`) | p50 6.4–6.7 ms (149–157 fps), p95 6.6–6.9 ms (145–152 fps), p99 6.7–7.4 ms over six runs — layout, culling and preview decoding stay far below the 16.7 ms of 60 fps; a wheel notch is 280 px (a third of the window), a short trackpad flick coasts 274 px in 81 frames |
| 2026-11-25 | the real DB, 2232 files, 1240×840 headless frames (M22.4) | one wheel notch over the grouped grid, then 24 frames (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored sticky_heading_measurement`) | the group heading "октябрь 2026" is drawn at y 132 on the first screen and at y 132 after the notch — pinned at the top edge of the grid area (the owner's settings keep "Flat", so the bench switches to "By months" itself) |
| 2026-11-25 | the same DB, the code with the pinned heading | the 160-frame scrolling measurement of 2026-11-24, six runs (`… --ignored grid_scroll_measurement`) | p50 6.9–7.0 ms (142–148 fps), p95 8.9–9.6 ms, p99 11.2–13.7 ms; the same DB on the code before the change (a stash of `src/app.rs`) gave p50 6.77 ms, p95 9.75 ms, p99 12.24 ms — one rect and one text row per frame stay below the noise |
| 2026-11-25 | the real DB, 2232 files, 1240×840 headless frames (M22.5) | 16 frames in the calendar's year layer, then 16 in the month layer (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored calendar_measurement`) | year layer p50 5.56 ms (180 fps), p95 5.65 ms, p99 5.65 ms; month layer p50 5.85–5.90 ms (169–171 fps), p95 6.26–7.39 ms — twelve covers, 31 day cells and the preview budget stay inside a 60 fps frame; the whole year layer fits a 1240×840 window (December's card at y 442) |
| 2026-11-25 | a copy of the real DB, 2232 files, 1240×840 headless frames (M22.6) | an album with eight sub-album cards over real previews, 16 frames of the Albums view (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored albums_measurement`) | p50 6.94–7.51 ms (133–144 fps), p95 7.18–7.99 ms over four runs — eight covers and the snapshot grid stay inside a 60 fps frame; the cards end above the grid and three snapshot tiles are drawn under them, so the card area never eats the album's files |
| 2026-11-25 | the real DB, 2232 files, 1240×840 headless frames (M22.7) | the full screen on the day with most snapshots (2019-12-17: 16 in the series strip), one face on the frame, 16 frames with the inspector off and 16 with it on (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored viewer_measurement`) | over four runs without the inspector p50 6.91–8.65 ms (116–145 fps), p95 7.73–10.45 ms; with the inspector p50 6.89–7.89 ms (127–145 fps), p95 9.47–11.46 ms — the header, the tools row, the frame, the face chips, sixteen 64×48 strip thumbnails and (with the inspector) the whole file card stay inside a 60 fps frame; the first version computed the series on every frame and cost p50 9.66 ms, so the series is computed on a file change now |
| 2026-11-25 | the real DB, 2232 files, 1240×840 headless frames (M22.8) | the settings dialog on its seven sections, 16 frames each after three warm-up frames (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored settings_measurement`) | p50 6.89–8.01 ms (125–145 fps), p95 ≤ 8.13 ms — the tag dictionary is the heaviest pane; the section list's caps line stands at y 168 and all seven rows fit the 620 px dialog. The grid pass of M22.2b repeats after the icons over four runs: p50 6.73–6.84 ms (146–149 fps), p95 ≤ 7.18 ms over 160 frames against the 6.4–6.7 ms of the code before them (one run on a busy machine: p95 8.50 ms) |
| 2026-11-25 | the real DB, 2232 files, 52 duplicate groups, 2498 faces, 1240×840 headless frames (M22.9) | 40 frames of each screen without a mock-up (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored screens_without_a_mock_up_measurement`) | duplicates p50 5.61–5.71 ms (175–178 fps), p95 ≤ 7.21 ms; faces p50 8.07–8.49 ms (118–124 fps), p95 ≤ 10.63 ms — the heaviest of the four, every face tile decodes its crop within the 8-per-frame budget; trash p50 5.08–5.13 ms (195–197 fps), p95 ≤ 6.96 ms; integrity p50 4.77 ms (210 fps), p95 ≤ 7.29 ms — the four screens stay far below the 16.7 ms of a 60 fps frame |
| 2026-11-25 | a copy of the real DB, 2232 files, an album with 500 files, 1240×840 headless frames (M22.10) | each dialog of the window, 16 frames after four warm-up frames (`PHOTOCAT_BENCH_DB=<a copy of the DB> cargo test --release -- --ignored dialogs_measurement`) | folder list p50 0.03 ms, p95 0.04 ms (the counted roots are cached: they cost 0.84 ms per frame before); picker p50 0.08 ms; storage 0.02 ms; album delete 0.02 ms; export with a 500-line plan p50 0.18 ms, p95 0.32 ms; trash 0.02 ms; settings p50 0.06 ms, p95 0.08 ms — a dialog costs a fraction of the 16.7 ms of a 60 fps frame, and the plan text is laid out once and comes from the galley cache afterwards |
| 2026-11-25 | both palettes of `app::Theme` (M22.11) | `cargo test both_themes_pass_the_contrast_ratio` — the WCAG 2.1 ratio of every pair the screens paint; `every_screen_paints_only_the_tokens_of_its_theme` renders the grid, the four calendar layers, the albums and the full screen in both palettes | the light accent `#d97706` read 2.91:1 on the panel and 2.86:1 on the selection fill, now `#b45309` 4.58:1 and 4.51:1 (5.02:1 on the card, white on it 5.02:1); the dark danger word `#ef4444` was 4.34:1 on the card, now `#f45c5c` 5.07:1, the light one `#dc2626` 4.41:1 on the panel, now `#ba1a1a` 5.90:1; `text_med` 6.15–8.32:1, `text_dim` 3.63–4.88:1 on every ground, `accent_soft` composited for dark (amber 15 % over the card) 4.59:1 under the accent; the active navigation row moved from the `field` tone to the `card` one (accent 4.07:1 → 5.02:1 in light); on the real DB of 2232 files the four screens without a mock-up cost the same in the light palette as in the dark one — p50 duplicates 5.93 vs 5.72 ms, faces 8.46 vs 8.41, trash 5.25 vs 5.23, integrity 4.88 vs 4.90 (40 frames each, `--ignored screens_without_a_mock_up_measurement`), and no word, fill or line fell off either palette on real previews, tags and faces; `cargo test` 278 passed, `clippy` clean, `check.sh` 55 checks |
| 2026-11-25 | the real DB, 2232 files, headless frames at 1240×840 and 1691×1021 (M22.12) | the whole window with a selected file; every painted rectangle (`cargo test --release the_file_card_fits_the_right_panel`, plus a temporary helper for the two sizes) | the farthest right edge is 1240.0 in a 1240 px window and 1691.0 in a 1691 px one — nothing is painted outside the window; on the code before the fix the panel's frame reached 1253.5 at 1240 (13.5 px outside), because the own-tag row (a fixed 150 px field plus the word button) was wider than the 263 px card and egui widened the 280 px panel to fit it |

| 2026-11-24 | `src/` — 17 modules, 205 Russian message strings in ten of them (M23.5) | `cargo test --test source_language` after the conversion | 0 Russian messages left in test code; both probes fail the new check — an `assert!(…, "…")` message and an `expect("…")` text — while the compared data (`assert_eq!(t(Lang::Ru, "Settings"), "Настройки")`) passes; `cargo test` 236 + 3 passed, `clippy` clean |
| 2026-11-24 | 2230 files, 21.2 GiB (`Фотографии с телефона`), a `/tmp` bench DB | `index --jobs 16`, release, after the English conversion (M23.6) | 67.7 s, 1 error (the known broken JPEG), DB 1.4 MB, 2229 previews 28.1 MiB (24.4 photos + 3.8 video covers) |
| 2026-11-24 | the same corpus and DB | repeated `index --jobs 16` | 0.029 s, 2229 unchanged, the broken JPEG read again |
| 2026-11-24 | the same DB, 2230 files | `stats` under `lang=en` and `lang=ru` | the same numbers in both: 2230 files, 21.2 GiB, 258 videos, 3:19:46 in total, 1 without a dHash, 0 without a date; only the labels and the units differ (`КиБ`/`KiB`) |
| 2026-11-24 | the same DB | `dupes --max-dist 6` | 0.023–0.026 s, 101 groups, 234 files, 210.3 MiB — 2 groups fewer than the 103 of 2026-11-22, and the cause is the dHash (M22.2, ADR 0045), not the translation: an index of the same corpus by the code from before M22.2 (`51d6d78`) gives exactly 103 / 238 / 214.1 MiB, while the two lost pairs are portrait shots of one field taken 4 s apart, whose oriented hash differs by 14 and 7 bits against 5 and 3 on the sideways frame |
| 2026-11-24 | the same DB | `organize --by day/month/event` (dry run) | 0.021–0.023 s; 2230 files with a date, 0 without; 573 / 76 / 576 folders |
| 2026-11-24 | the same DB, 2229 files | `embed --jobs 8 --batch 8`, CLIP ViT-B/32 multilingual, two frames | 160.6 s (72 ms/file), 2229/2229, 0 errors, vectors 4.4 MiB (f32, 2048 B each), 31 206 tags (14 per file); the machine carried an unrelated build (load 9–14), the same operation on a quiet machine took 126.1 s on 2026-11-24 |
| 2026-11-24 | the same DB | repeated `embed` | 0.022 s, 2229 skipped by sha256 |
| 2026-11-24 | the same DB, 2229 vectors | `search` "cake" / "sea" / "document", the model already loaded | 10–11 ms per query, the expected frames first |
| 2026-11-24 | the same DB | `clusters --k 30` | 0.9 s, a report of 30 clusters × 12 examples |
| 2026-11-24 | the same DB, 1972 photos | `ocr --dry-run` | 357 candidates: 203 photos without a camera plus 154 whose confident tag is `document`/`screenshot`; the same DB under the pre-M23.6 code reported 203, because the dictionary was looked up by the empty `settings.model_id` and the tag branch of the heuristic was silently dead |
| 2026-11-24 | 357 candidates (tesseract 5.5.4, rus+eng) | `ocr --jobs 8`, release | 70.3 s (0.20 s/file), 356/357 (1 known broken JPEG), 66 584 characters in 138 files, 1 error |
| 2026-11-24 | the same DB, 356 recognized texts | repeated `ocr` and `search --text` ("сбербанк", "счет-фактура", "2023") | 0.09 s, 356 skipped by sha256; 1 ms per text query, the expected files first |
| 2026-11-24 | the same DB with CLIP and OCR | DB size | 16.0 MB (vectors 4.4 MiB, 31 206 tags, OCR 176 KiB + FTS5): +3.3 MB over the 12.7 MB of 2026-11-22, and the cause is `idx_tag_source` (M12) plus b-tree slack, not the text |
| 2026-11-24 | a copy of the bench DB | `gui` under `timeout 6`, `dupes`, `ocr --dry-run`, `models list` under `lang=en` and `lang=ru` | the window opens on the new DB and lives to the timeout (code 124, no output); every report prints the same numbers and the same order in both languages, only the labels and the units differ |
| 2026-11-24 | network, `clip-vit-b32-en`, a temp DB and models folder | `models download --id clip-vit-b32-en --yes` with the in-process client (`ureq` + `rustls`, ADR 0047) | 22.7 s for 148.6 MiB in 3 files, the sha256 of all three matched; the same download through `curl` on 2026-11-22 took 18.8 s, so the difference is the network; the release binary grew 85.1 → 87.6 MB |
| 2026-11-24 | a clean install: `XDG_CACHE_HOME=$(mktemp -d)`, no DB, no models, no settings (M24.1/M24.2 smoke) | `gui` under `timeout 6`, then `stats`, `models list`, `doctor`, `ocr --dry-run`, `settings` on the same cache | the window opens and lives to the timeout (code 124, no output) and writes `<cache>/photocat/photocat.db` (176 KiB with the seeded 14-tag dictionary) plus `thumbs/`; `lang=ru` is detected from the locale and stored; `stats` says 0 files, `models list` says "not downloaded" for all four and "over the limit" for SigLIP 2, `ocr --dry-run` finds 0 of 0 candidates, `doctor` lists the programs, no pass starts by itself. The smoke found a defect: the GUI read the default English and stored no language, while the CLI detected the locale — fixed by `app::first_lang` |
| 2026-11-26 | the real DB, 2232 files, 1240×840 headless frames (M22.15) | the 160-frame scrolling measurement of 2026-11-24 again, six runs (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored grid_scroll_measurement`) | p50 6.55–6.64 ms (151–153 fps), p95 6.72–7.19 ms, p99 6.90–12.48 ms — the per-picture clip rect and the larger source area of the cover crop cost nothing measurable against the 6.4–6.7 ms of the fitted tile |
| 2026-11-26 | the real DB, 2232 files, 1240×840 headless frames (M22.16) | the whole window with a selected file in both languages; every painted rectangle and word (`cargo test the_file_card_keeps_the_mock_ups_band_and_air`, plus a temporary dump of the panel's geometry) | the right panel now owns its 280 px (the default side-panel frame used to leave 8 px on each side, so the 1 px line stood 9 px inside and the card had 263 px): the header band is 279×44 px at y 31–75, the "×" 22×22 px at x 1206–1228, the file card 255 px wide at x 973–1228 — 12 px of air on both sides — and every word and rectangle of the panel stays inside the window in English and in Russian; the two card actions no longer wrap in Russian (a 20 px row) |
| 2026-11-26 | the real DB, 2232 files, 1240×840 headless frames (M22.21) | the Albums screen with eight sub-album cards, every card carrying a mosaic of four real previews (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored albums_measurement --nocapture`) | p50 7.38 ms (135 fps), p95 9.87 ms, p99 9.87 ms over 16 frames; the one-preview card of the same run before M22.21 was p50 7.41 ms / p95 12.40 ms, so three extra previews per card cost nothing measurable; the first sub-album card sits at y 686 and the snapshot grid keeps its place under it |
| 2026-11-26 | the real DB, 2232 files, 2498 faces in 617 groups, 1240×840 headless frames (M22.26) | 40 frames of each screen without a mock-up, the group list in the right panel with the best face of every group as its row icon (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored screens_without_a_mock_up_measurement`) | faces p50 10.27 ms (97 fps), p95 12.16 ms in the dark palette and p50 9.61 ms, p95 11.08 ms in the light one — up from the 8.07–8.49 ms of M22.9: that is the price of the 617-row list with a face icon on every row, still inside the 16.7 ms of a 60 fps frame; duplicates, trash and integrity stay at 5.1–6.1 ms, and no word, fill or line fell off either palette on the real previews and crops |
| 2026-11-26 | the real DB, 2232 files, 10 years (2016–2026), 1240×840 headless frames (M25.10) | 16 frames of the calendar's years layer and 16 of the months layer after four warm-up frames (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored calendar_measurement --nocapture`) | years layer p50 4.63 ms (216 fps), p95 4.91 ms; months layer p50 4.69 ms (213 fps), p95 5.17 ms — faster than M22.5's 5.56 / 5.85 ms because the pinned threshold had computed every tag's threshold twice per frame and now computes them once (`tag_counts`); the 10 year cards of the new layer put the oldest at y 436 and every card fits a 1240×840 window. The same pass measured the grid scroll again: p50 5.72 ms (175 fps), p95 6.14 ms — against M22.2b's 6.4–6.7 ms, so the panel rework of M25.1–M25.6 costs the grid nothing |
| 2026-11-26 | the real DB, 2232 files, 1240×840 headless frames (M25.11) | 16 frames of the calendar's weeks layer (the weeks touching May) and 16 of the days layer (Monday–Sunday) after four warm-up frames (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored calendar_measurement --nocapture`) | weeks layer p50 4.44 ms (225 fps), p95 4.58 ms; days layer p50 4.61 ms (217 fps), p95 4.74 ms — the four layers now cost the same 4.4–4.6 ms, well inside the 16.7 ms of a 60 fps frame |
| 2026-11-26 | the real DB, 2232 files, 1240×840 headless frames (M25.12) | the four calendar layers with the period's photos under the tiles, 16 frames each after four warm-up frames (`PHOTOCAT_BENCH_DB=<db> cargo test --release -- --ignored calendar_measurement --nocapture`) | years p50 4.62 ms (216 fps), p95 5.46; months p50 4.83 ms (207 fps), p95 5.16; weeks p50 4.66 ms (214 fps), p95 5.06; days p50 13.66 ms (73 fps), p95 14.82 — the day layer is the heaviest because its period can hold a whole busy day of tiles, and it still stays inside the 16.7 ms of a 60 fps frame; the year layer, the acceptance's case, costs 4.62 ms with all of the year's files below it |
| 2026-11-26 | the real DB copied to `/tmp` (`roots` = `Фотографии с телефона` + `Pictures/wallpapers`), 2234 files, 21.2 GiB | `settings.roots_off = /home/alx/Pictures/wallpapers`, then `stats` under `lang=ru` and `index` on the switched-off root | the switched-off folder is named with its own count — 4 files, 4.8 MiB — the shown set is 2230 files, 21.2 GiB, the total stays 2234; `index` writes no row and says the folder is switched off, exit code 0; the rows, the tags and the previews of those 4 files are in the DB untouched and the folder comes back by the same switch with no pass |

Rule: numbers without a corpus and a date do not enter the table.

## Long passes: cancel and resume

Indexing, CLIP and OCR run in a background thread and hold one transaction per run. The DB can be
read meanwhile (`WAL` + `SCHEMA_VERSION`), and the pass can be interrupted with "Stop":
`Progress::cancel()` sets the flag, the workers check it between files (OCR — also inside a batch of
documents), the started file is finished, the transaction is committed, and `Stats.cancelled` says
"stopped". Resuming is the same pass again: unchanged `sha256` values are skipped, so "Resume (N)"
finishes the remainder. The `tesseract` process runs with `OMP_THREAD_LIMIT=1`, so that N workers do
not take all cores and the window does not stutter.

## External programs

The app is one Rust binary; everything that needs a C stack runs as a process (ADR 0019). The path
to each program is a setting (ADR 0020): an empty field means a `PATH` lookup, a non-empty one means
starting that file. The lookup, the launch and the Windows console suppression live in
`src/tools.rs`: `derive` asks for `Tool::Ffmpeg`/`Tool::Tesseract`, not for a name as a string. The
weight downloader needs no program: it is an HTTPS client inside the binary (ADR 0047).

| Program | Module it works for | Setting | Without it |
|---|---|---|---|
| `ffmpeg` | `derive::analyze_video` | `tool_ffmpeg` | a video in the index without a preview and dHash, an error in the counter |
| `ffprobe` | `derive::video_probe` | `tool_ffprobe` | a video without duration and resolution |
| `tesseract` | `derive::ocr_image` | `tool_tesseract` | OCR is off (the button is greyed out, `ocr` suggests a path) |

**Checking a path.** On save the program is started with its version flag (`-version` for
ffmpeg/ffprobe, `--version` for tesseract). The GUI writes the outcome or the error to the
status line and does not save a non-working path; `settings --tesseract PATH` in the CLI exits with
code 2 on an error and writes nothing to the DB. There is nothing to check for an empty path — the
program is looked up in `PATH` at work time.

**`doctor`** prints for each program the version, where the path comes from (settings or `PATH`),
the file it found, why it is needed and what breaks without it; then the tesseract languages against
`settings.ocr_lang` (`rus+eng` by default), the CLIP model and the DB size. A missing program is a
state, not a crash: only its own kind of work is disabled.

## Cross-platform

The target is Linux (ADR 0014), but the code has no dead ends: the cache is found through
`PHOTOCAT_CACHE` (a portable copy names it itself, ADR 0050) or
`XDG_CACHE_HOME` / `LOCALAPPDATA` / `HOME`, symlinks use `#[cfg(unix)]` and `#[cfg(windows)]`, and
`ffmpeg`/`ffprobe`/`tesseract` come from the settings or `PATH` (`tools::Tools`) and are started
without a console window on Windows. A known limitation: non-UTF-8 names (allowed on Linux) are lost
when the path is written to SQLite via `to_string_lossy`; video and OCR need the respective programs
in `PATH` (without tesseract only OCR is lost — everything else works, the weight download included,
because it is the binary's own HTTPS client, ADR 0047).
What exactly is present on the machine is printed by `photocat doctor`; packages are installed with
the OS package manager (Arch: `tesseract` + `tesseract-data-rus` + `tesseract-data-eng`,
Windows/macOS — their own ffmpeg and tesseract builds).
