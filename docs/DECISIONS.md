# Decisions (short ADRs)

*Русская версия: [DECISIONS.ru.md](DECISIONS.ru.md).*

Format: what we decided, why, what it forbids. Append here, not to the chat. A decision that is
irreversible (DB schema, showcase file format, ML model) is not taken without a record here.

---

## 0001. GUI on egui/eframe, not GTK/Qt

The owner is on Arch + Wayland (niri); GTK drags in system dependencies and a "foreign" look, Qt is a
heavy runtime. egui is pure Rust, GPU rendering, one binary, a fast edit cycle.
**Forbids:** webview, Tauri, Electron, JS.

### Why not gpui and the other Rust frameworks

- **gpui** (Zed) — a framework for Zed's tasks: the API changes without stability promises, there is
  little documentation outside the editor, and input/IME widgets, drag&drop and HiDPI edges would
  have to be finished by us, while the dependency would have to be pinned to a git revision. The gain
  is animation, which the interface does not use under `ui-design` (shadows, gradients, skeletons are
  forbidden). The price is rewriting `app.rs` (5.6k lines) and the risk of breakage on every update.
- **iced** — the only serious competitor: pure Rust, retained mode, a more stable API. The minus is
  weaker display of thousands of textures and text input; for our thin drawing over `db::` there is
  no gain.
- **slint** — its own DSL and licensing questions for the desktop; a DSL contradicts "one language,
  no build steps".
- **fltk-rs** — C++ underneath, weak HiDPI and look. **makepad / dioxus-blitz** — raw.

What egui does not give and we accept: text input and IME are weaker than native, there is no system
menu or tray, and immediate mode recomputes the layout every frame. That is accepted: the corpus is
2230 files, the heavy work (decoding, hashes, inference) is in the background anyway per AGENTS.md
§"Expensive work goes to the background", and the grid loads a texture only when
`ui.is_rect_visible`.

**When to revisit:** scrolling stops holding the frame on tens of thousands of files (that is cured
by virtualisation first, not by a framework change); a native integration is needed that egui does
not have (tray, system dialogs, several windows) — that is solved by a targeted crate, not by a stack
change; or egui finally stalls.

Related: ADR 0008 (GUI without logic), ADR 0024 (three columns), `docs/UI.md`.

## 0002. State in SQLite in the cache, not in the archive folder

The index is derivative: it can be recreated in 65 s. Not a single service file appears in the photo
folder (except the showcase on an explicit command).
**Forbids:** writing `.json`/`.xmp` next to originals without a separate decision.

## 0003. dHash (gradient, 8×8) instead of pHash and CLIP at the first stage

64 bits, the comparison is `count_ones` on XOR: 2.3 M comparisons in 0.03 s, no dependencies and no
models. For "series/forwarded copy" that is enough; semantic tags are a separate stage (M3).
**Forbids:** treating dHash as the only ground for deleting a file — the threshold produces false
positives.

## 0004. Union-find clustering by the Hamming threshold

Transitive closure: if A resembles B and B resembles C, all three are in one group. Simple,
predictable, does not need the number of clusters up front.
**Price:** a chain of "slightly similar" can glue different things; cured by the M2 interface (a
human makes the decision).

## 0005. We delete and rename nothing

The showcase is built with hardlinks (`ln`, 0 bytes) or copies. The original stays where it lay, cloud
syncs and backups do not break.
**Forbids:** `fs::remove_*` and `fs::rename` in `src/` — checked by the boundary script.

**Checked by the owner (2026-11-23).** The owner asked for renaming files on disk (one, the selected
ones, by a mask), then looked at the consequences and **withdrew the task**: a mass rename breaks
`sha256sum` manifests, external backups and the showcase, and after a failure it can only be undone
by hand. The rule stays without exceptions; there is no `fs::rename` in `src/`, and `check.sh` gets
no new exceptions. Renaming originals lives in the owner's file manager, not in photocat.

## 0006. Date: EXIF → file name → mtime, with the source recorded

EXIF is present for 92% of the corpus; for the rest the name `IMG_20240517_144243` is more reliable
than mtime (copying changes mtime). The source is stored in `taken_src`, so the interface can say
"the date comes from the name". Converting `mtime` to a string and parsing dates goes through
`chrono`; the project has no date arithmetic of its own.
**Forbids:** silently substituting a guess for the date without a note.

## 0007. A 320 px preview, keyed by sha256

The same files (including exact copies) share one preview; 2230 files → 29 MB, a page of the grid
does not load originals.
**Forbids:** loading full-size photos into the GUI grid.

## 0008. Modules by layer, `egui` only in `app.rs`

The core (derive/db/index/dupes/organize) is tested and measured without graphics; the GUI cannot be
run in a headless check, so it must not contain logic.
**Check:** `.pi/skills/enforce-architecture/scripts/check.sh`.

## 0009. Russian UI, English identifiers

The owner is Russian-speaking; the names in the code are English, so as not to argue with the
ecosystem. Dates and sizes are formatted in one place (`db::human`).

> Superseded for the interface language by 0040 (M21): the window is bilingual, English first. The
> identifiers stay English, and `db::human` is joined by `db::human_in(lang, n)` for the GUI.

## 0010. Video through an `ffmpeg` process, not a Rust decoder

`ffmpeg` is already installed and pulls in all codecs; dragging video decoders into the project means
hundreds of dependencies.
**Consequence:** M4 appears only when `ffmpeg` is in `PATH`; without it — a file without a preview and
an error in the counter, not a failed indexing.

## 0011. ML only locally, models as files in the cache

A family archive (documents, children) does not go to the cloud under any conditions. The model is
downloaded once into `~/.cache/photocat/models/` with a sha256 check.
**Forbids:** any HTTP calls to model APIs from the application code.

## 0012. A duplicate decision lives in `files.decision`, not in a file next to the original

The owner goes through 103 groups by eye; there is nowhere to put that decision back if it is kept
only in the GUI memory. A separate file next to the photo is forbidden by ADR 0002, and renaming and
deletion by ADR 0005. So `keep`/`drop` is a DB column: it survives a restart, does not change the
archive and is exported by the CSV plan. Advice ("sharper than the rest") is not written to the DB —
it is derived from `sharpness` and recomputed.

**Price:** the decision is tied to the path and the `sha256`; when a file is replaced under the same
path it is reset (otherwise a different snapshot could end up "spare").
**Forbids:** treating `decision` as permission to delete: the plan is a report, not a command.

## 0013. CLIP: ONNX through `ort`, two towers, weights as files in the cache

The visual part is **CLIP ViT-B/32** (`Xenova/clip-vit-base-patch32`, int8 ONNX, 85 MiB), the text part
is **multilingual DistilBERT + Dense 768→512**
(`sentence-transformers/clip-ViT-B-32-multilingual-v1`, int8 ONNX, 130 MiB + 1.6 MiB projection). An
English-only CLIP does not understand "торт" and "документ"; a multilingual text encoder maps a
Russian query into the same 512-dimensional space as the visual tower. Inference is CPU through `ort`
(ONNX Runtime 1.31 API), tokenisation through `tokenizers`; `serde_json` reads the safetensors header
of the projection. Vectors are f32 little-endian (2 KiB/photo): 1971 photos give 3.85 MiB, f16 would
save half but would need another dependency and lose precision on a small corpus.

**Network.** The app downloads only model weights — by the owner's explicit consent (ADR 0015),
nothing else. The binary is the only path to the weights (`models download`, the Settings button) and
there is no shell script for it (M24.1: a cross-platform application is one binary); the download is
made in-process by `ureq`/`rustls` (ADR 0047); the sha256 is pinned in `src/embed.rs` and checked on
download. On the first build `ort-sys` downloads ONNX Runtime itself
(cache `~/.cache/ort.pyke.io`); that is a build step, not runtime. The model needs AVX2 (int8
quantisation), otherwise the fp32 variant is required.
**Forbids:** auto-download without consent, sending previews, paths and query texts outside, storing
weights in the repository.

**Tags.** All 14 weights are stored; only the first is considered confident, if it is ≥ 0.22 and
leads the second by 0.01. That is the confidence threshold (`settings.tag_show`), which OCR and the
showcase also use. The tag filter is set separately — by a 0–100 percentile per tag (M15.2,
ADR 0032): 100% is the strongest snapshot of that tag, 0% is all, 70% is the top 30%. At 100% any tag
keeps at least one snapshot, and a lower maximum of another tag does not zero it. On a folder of
fewer than 100 files the percentile is noisy, so the raw confidence threshold applies. False tags are
cheaper to let through than to show ("tag: not sure" is an honest answer).
**Forbids:** automatically deleting or renaming a file by a CLIP tag.

## 0014. Cross-platform: Linux is the target, the code has no platform dead ends

The target environment is Linux (Arch + Wayland), but the stack (`egui/eframe`, `rusqlite` with
bundled SQLite, `image`, `rayon`, `ort`) is portable, and no core function may require an
OS-specific API without a fallback. Cache: `XDG_CACHE_HOME` → `LOCALAPPDATA` (Windows) →
`HOME/.cache` → a temporary directory; symbolic links — `std::os::unix::fs::symlink` on Unix and
`symlink_file` on Windows (which needs developer mode; an error is counted, not fatal to the plan).
External processes: `ffmpeg`/`ffprobe` are looked up in `PATH` (`Command` adds `.exe` on Windows
itself), the console window is suppressed via `CREATE_NO_WINDOW`.

**Boundary:** no shell script ships with the application: preparing a model is the binary's job, over
HTTPS from inside the process (ADR 0047). Non-UTF-8 file names
(possible on Linux) are stored in the DB via
`to_string_lossy` — a known limitation, not an M4 goal.
**Forbids:** `unsafe` workarounds, `#[cfg]` branches without a second implementation, a hardcoded
home path in the UI.

## 0015. Model downloads by the app: consent, limit, sha256

The owner needs a choice of model and download path without a manual script. The app goes to the
network **only** for model files from the built-in catalogue and **only** after an explicit
"Download" press with a consent tick. The only request that goes out is an HTTPS GET to the weight
address; neither photos, nor paths, nor queries, nor tags are sent. The response is written to disk
and the sha256 is compared with the one pinned in the catalogue; on a mismatch the file is not
considered a model.
**Limit.** The settings hold a maximum model size; the catalogue is filtered by it and a download over
the limit is refused. If the selected model is unavailable (404/network/check), the downloader tries
the mirrors, then compatible catalogue variants within the limit, and reports what it chose.
**Forbids:** a background download without a human action, any POST/PUT, sending user data, models
without a sha256.

## 0016. ~~The weight downloader is the `curl` process~~ — superseded by ADR 0047

The downloader was a `curl` process, like `ffmpeg` for video (ADR 0010): present on Linux/macOS and
in Windows 10+, it knows TLS, redirects and retries, and no TLS code entered the project. The owner
reversed this on 2026-11-24 — a cross-platform application is one binary — so the weights are
downloaded in-process with `ureq`/`rustls`. The module boundary this ADR introduced is kept: the
downloader is the only network code, it lives in `src/embed.rs` and does nothing but `GET` a
catalogue file. See ADR 0047.
**Forbids:** a second way to download and network calls outside the model catalogue.

## 0017. Hidden tags in the DB, not in a file and not in the model

A CLIP tag is a guess. The owner sees mistakes ("документ" on a child's drawing) and must fix the
classification without touching the original. The decision "this photo has no such tag" lives in
`tag_hidden(file_id, tag)`: it survives re-indexing and tag recomputation, does not depend on the
model, and hides the tag from the card, the filter and the counters. When the file content is
replaced (`sha256` changed) a trigger wipes the decision — otherwise it would apply to a different
snapshot.
**Forbids:** editing EXIF/XMP next to a photo, deleting tags from the shared table, building
"learning" on user edits without a separate decision.

## 0018. OCR is an external `tesseract`, the text lives only in the DB

Documents and screenshots have to be searched by words, not by eye. OCR in Rust dependencies
(leptess, tesseract-rs) drags in a C/C++ build of leptonica and tesseract, and an ONNX recognition
model means more weight downloads and an own CTC decoder implementation. Like `ffmpeg` for video
(ADR 0010), OCR stays an external process: `tesseract stdin stdout -l rus+eng`. The language weights
live in system packages and are not downloaded by the app.
**Consequence.** Without `tesseract` in `PATH` OCR is unavailable (the button is disabled, `ocr` says
so), the rest works. No network is needed for OCR at all. We prepare the frame ourselves: we apply
the EXIF rotation and limit the long side to 2000 px — otherwise `tesseract` reads a phone snapshot
sideways, and a PNG through stdin leaves no temporary files next to the archive.
**Text only in the DB** (`ocr` + FTS5 `ocr_fts`): neither a file nor metadata appears next to the
original. The FTS index is kept in sync by triggers, replacing the content (`sha256`) wipes the text.
**Forbids:** writing `*.txt` next to a photo, changing EXIF/XMP, sending the image to a cloud OCR,
downloading anything for OCR without consent per ADR 0015.

## 0019. External programs: PATH, soft degradation and `doctor`

The app does not pull ffmpeg and tesseract into itself: a C/C++ build and dozens of megabytes in the
distribution (ADR 0010, 0018). Weights stopped being an external program: the downloader is the
binary's own HTTPS client (ADR 0047). For the programs that remain — one contract:

| Program | Why | Without it |
|---|---|---|
| `ffmpeg` | a video cover frame, dHash, previews | a video enters the index without a preview and dHash, the error is counted in `errors`; photos do not suffer |
| `ffprobe` | video duration and resolution | a video without a duration `▶ M:SS`, the rest is indexed |
| `tesseract` | text from documents and screenshots | OCR is off: the button is disabled, `ocr` says what to install; search, duplicates and CLIP work |

**Rules.**
1. A program is started through `std::process::Command` (`.exe` is added by itself on Windows) by an
   explicit path from the settings, or, if the field is empty, by the name from `PATH` (ADR 0020).
   Arguments are an array, without `sh -c`: user input never reaches a command line.
2. A missing program is not a crash but a state: `tool_version`/`tesseract_langs` return `None`, that
   kind of work is disabled, the rest works. An error in one file is a counter, not a stop of the run.
3. The console window on Windows is suppressed (`CREATE_NO_WINDOW`); data is passed to the program
   through stdin/stdout (OCR), and there are no temporary files next to the archive.
4. Where to look is shown by `photocat doctor`: the versions of ffmpeg/ffprobe/tesseract, the
   tesseract languages against `ocr_lang`, the presence of the model and the DB size. One screen
   instead of guesses.
5. The launching of all three programs lives in `src/tools.rs` and they are called by
   `src/derive.rs`; the downloader needs no program at all (ADR 0047). The boundary check guards
   this.
**Forbids:** bundling binaries in the repository, calling `sh`/`cmd`, the network for OCR, downloading
OS packages from the app. The fallback contract is `PATH`; explicit paths are ADR 0020.

## 0020. The path to an external program is a setting, not only `PATH`

On Arch the tesseract packages are called `tesseract-data-eng`/`-rus`, while on Windows and macOS
ffmpeg and tesseract are often not in `PATH` at all. Forcing the owner to fix the environment or
rebuild the app because of one path is wrong. Hence `settings.tool_ffmpeg`, `tool_ffprobe`,
`tool_tesseract`: empty — the name is looked up in `PATH` (the former behaviour of ADR
0019), non-empty — the given file is started. `curl` had such a key too until ADR 0047.

**Rules.**
1. A path is checked before saving by starting the program with its version flag; an error is shown
   in the GUI status line or in a CLI message, not in the console/log. A non-working path is not
   saved.
2. `photocat doctor` is the only place where it is visible where each program comes from and what
   breaks without it; `settings` prints the same as a list.
3. Paths are stored in the `settings` table next to the index (ADR 0002), survive a restart and a
   model change; not a single file appears next to the archive.
4. Paths do not change the process `PATH` and do not substitute system programs: they only choose
   what to launch.
**Forbids:** bundling binaries, downloading OS packages from the app, silently saving a non-working
path, looking for a program in the current directory instead of `PATH`.

## 0021. Faces — local SCRFD + ArcFace, groups and names in the DB

"Show me all photos with Lisa" is the owner's next question after duplicates, dates, meaning and
text. Face detection and embedding are two ONNX models from `immich-app/buffalo_l`: SCRFD (16.9 MiB)
and ArcFace (174.4 MiB). Both run through `ort` on the CPU, the weights are downloaded only with
consent and checked by sha256 (ADR 0015). The pipeline: EXIF rotation → 640×640 detection → NMS →
alignment by 5 points into 112×112 → ArcFace 512 f32 → normalisation.

**Why so.** Separate detector and recogniser give boxes and points for alignment; ArcFace compares
aligned faces, not "roughly the same area of the photo". The cosine over normalised vectors gives a
simple threshold and union-find — like duplicates (ADR 0004), without training on the archive.

**Data.** Faces, vectors and crops are derivative: `faces`, `face_clusters`, `face_scans`, files in
`~/.cache/photocat/faces/`. A group's name and a split ("not him") live in the DB. A group with a name
or a manual face is pinned and survives a rebuild by threshold; empty unnamed ones are deleted on the
next rebuild. Replacing `files.sha256` wipes the faces and the pass mark, but does not touch the
originals.

**Threshold.** `settings.face_threshold`, 0.45 by default. Lower — more false merges, higher — one
person splits into groups; the owner sees the result as tiles and splits with a button. Deleting
automatically or "learning" from the edits is forbidden.

**Forbids:** sending faces, crops, embeddings, paths or queries outside; writing faces and labels next
to the original; deleting/renaming photos; treating a false merge as a sentence without a way to split.

## 0022. The `organize` showcase: choice of depth and name template, originals do not change

Arranging by date stopped being one rigid scheme. The owner chooses the depth (`--by day` by default,
`--by month`, `--by event`) and, if he wants, builds the name from substitutions (`{date}`, `{time}`,
`{camera}`, `{tag}`, `{orig}`, `{ext}`) or a short `--rename`. Showcase files are only new: hardlink
(by default), a copy or a symlink in a separate folder; not a single original is renamed or deleted
(ADR 0005).

**Why so.** An open question of the owner from STATUS — "by days or by months, original names or from
the date". There is no single answer for everyone: browsing the whole archive is more convenient by
months, while finding one snapshot is easier by a name with the date. Hence a parameter instead of a
third default scheme. `--exclude-dup` links the showcase with the duplicate decision (ADR 0012): a
"spare" file does not enter the layout but stays on disk.

**Event.** A snapshot without GPS stays in the day folder; a snapshot with coordinates gets a
`<DD.DDDN_EEE.EEEE>` subfolder — a cell of ~1 km. There is deliberately no reverse geocoding
("street, city"): that is the network, and the network is allowed in the project only for model
weights (ADR 0011, 0015). A name from coordinates is more honest than an invented "vacation" and lets
you find the place by searching in a file manager.

**Name.** Spaces become `_`, forbidden characters are dropped, repeated separators and trailing
`_`/`-` are trimmed, the length is capped at 120 characters; an empty substitution leaves no rubbish.
Collisions in a folder get `-2`, `-3`. An unknown substitution and an unknown `--by` are a refusal
with code 2 and a list of the allowed ones: a typo in a template must not silently spoil the showcase.

**Forbids:** renaming or moving originals; writing the showcase without `--apply`; going to the
network for place names; silently substituting an unknown placeholder with an empty string.

## 0023. Copy comparison and manifest — read only, size before sha256

Checking that the archive on the second disk matches the first used to be possible only with `cmp`
over folders or by eye. `photocat verify <a> <b>` compares two trees by relative paths: size first,
and only on a matching size is the sha256 computed. There is no point reading a file of a different
size — the difference is already known. The manifest (`manifest --out file.sha256`) records the state
of the archive for the future in the GNU `sha256sum` format.

**Why without a DB.** The comparison has to work even if the archive is not indexed or the index is
stale; the sha256 in the DB is a snapshot of a past indexing, not the current content of the disk. So
`verify` and `manifest` do not open the DB and do not depend on the model, tags or decisions.

**Manifest format.** `<sha256>  <relative path>`, paths with `/` on any OS; names with a backslash, a
line feed and a carriage return are escaped as in `sha256sum`. Such a file can be checked both by the
standard utility (`sha256sum -c`) and by `photocat verify --manifest` itself, and transferring it
between Linux and Windows is not hindered by the separator. An incomplete manifest is not written: if
a file could not be read or the pass was cancelled, a later check would declare the skipped files
"extra". In manifest parsing only relative paths without `..` are allowed: a planted file must not
lead the reading outside the folder being checked.

**Codes and cancellation.** 0 — the copies/manifest match, 1 — differences, read errors or a cancel,
2 — an argument error. Progress goes to stderr, `--jobs N` sets the threads; in the CLI an
interruption is `Ctrl-C` (the command writes nothing), `Progress::cancel` is ready for a future GUI
button.

**Forbids:** deleting, renaming or changing archive files; writing anything except the manifest by an
explicit `--out`; sending hashes, paths or the file list to the network; writing an incomplete
manifest; counting files of different sizes as matching without reading.

## 0024. Three columns: the grid in the centre, details on the right, settings in a dialog

Before, the whole interface was two-column: navigation on the left, the view actions and the card of
the selected file mixed in, the grid in the centre. At 840 px the card slid down and was cut off, and
"Settings" took up a whole view: to change one threshold the owner left the grid and came back.

Now there are four regions: the top bar (folder, indexing, search), on the left the app map (views,
filter, view context, model, text, summary), in the centre the grid, on the right the details of the
selected file with a preview. "Settings" is a dialog over the screen with sections on the left (model,
indexing, tags, faces, storage, programs) and a "Close" button.

**Why a right panel and not a card below on the left.** The snapshot and its metadata are read
together; on the right they take no space from navigation and do not jump under the cursor when a
tile is selected. The grid and the card do not overlap, and the selection stays in place.

**Why "Settings" is a dialog and not a view.** A setting is a rare action (a threshold, a path, a
model); keeping it as a separate view means losing space in the grid every time. The dialog opens with
one click from the top bar and closes with Esc or a click outside; changes are applied and saved
immediately, there is no "Cancel" button, because there is nothing to cancel — the originals do not
change.

**Narrow window.** The grid must keep 4 columns at 1000×700 (see `docs/UI.md` §Metrics). 260 + 280 px
of panels leave too little in the centre, so the right panel is drawn only at a window of 1200 px and
above, and below that the card moves into the left panel and its "Summary" joins the common scroll.
One function `App::file_card` draws the card in both places — there is no second instance.

**Why "Settings" is a word and not a gear.** An icon without a label as the only path is forbidden
(`.pi/skills/ui-design/references/nayti-za-sekundu.md` §8); the button is labelled with a word.

**Forbids:** hiding the main action of a view in a dialog; duplicating settings in two places; drawing
the card with a second set of widgets; flooding more than one button per screen with the accent;
showing progress in the controls row while shifting the buttons.

## 0025. The trash and cache cleaning: an exception to "we delete nothing"

The owner asked to "delete" in a way that does not accidentally lose the family archive. Hence two
different mechanisms, and neither deletes permanently:

- **The app trash.** `files.trashed_at` — a DB row only: the file on disk does not change, it is
  hidden from ordinary views, "Restore" clears the field. There is no physical deletion.
- **Emptying the trash.** Only after confirmation, with the count and the size. Every file goes to the
  **OS** trash through the `trash` crate (`src/recycle.rs`); the app does not call `remove_file` and
  does not overwrite the file. The index row is deleted only after a successful move.
- **The cache.** "Settings → Storage" removes only `<cache>/photocat/thumbs` and `faces`, as well as
  index rows. `src/storage.rs` canonicalises the path and refuses anything outside the cache. This
  code does not touch the archive or the models.

**Why the `trash` dependency.** An own implementation of the system trash on Linux/macOS/Windows is
three different protocols and a risk of "deleting for good" on an error. `trash` does exactly one
thing: it moves a file to the OS trash; permanent deletion stays with the owner and the system file
manager. The crate uses no network.

**Why not "delete immediately".** A family archive is unrecoverable, and the decision "this frame is
spare" is often wrong. The two-step "to trash → empty the trash" gives a pause in which a file can be
returned.

**Forbids:** deleting a file from the archive without confirmation; bypassing the OS trash; deleting
anything from the cache without checking the path; touching the models when cleaning previews; writing
"deleted" when a file is only marked.

## 0026. The full-screen view: the frame in the background, return to the same view

> Refined by 0027: the frame is downscaled to 1920 px and a 320 px preview is shown immediately;
> video is now played by the external ffplay (0029).

A double click on a tile opens a modal window over the interface. It does not change the `View`, the
filter, the selection or the calendar month: closing returns exactly where we came from. Paging goes
through the list of the current view (a calendar day, a face group, search results), not through the
whole DB.

The frame is decoded in a background thread by `derive::load_oriented`, downscaled to 1920 px and only
then loaded as a texture on the UI thread: a full-size JPEG would otherwise freeze the window. The
grid preview (320 px) is not good enough for the full screen — it is blurry.

Video is opened by the external `ffplay` (see 0029); the manual frame stream and the separate sound
were removed. OCR text is shown next to the frame in full, without a 400-character cut.

**Forbids:** decoding a full-size frame on the UI thread; changing the view on close; paging through
someone else's list (the whole DB instead of the selected group).

## 0027. Re-indexing skips unchanged files; video plays as a stream

After the first pass the owner pressed "Index" again and waited minutes: the app re-read the sha256,
decoded JPEGs and ran ffmpeg over every file, although nothing had changed.

**Incrementally.** The key is path + size + mtime. If the DB already has a row with the same key, a
non-empty sha256, the right kind and a preview, the file is not read. A video without a cover or a
duration is the exception: that is an old index and has to be filled in. On a corpus of 2230 files an
old DB is caught up by 259 files in 10.2 s; a repeated pass — 0.024 s. Skipped files are counted
separately (`Stats.skipped`) and visible in the status, so that "0 indexed" does not scare. Risk: a
file changed but the size and mtime matched (rare); then the content is not recomputed — that is the
price of speed, and it is recorded here.

**Why not always recompute sha.** sha256 is half the cost of a pass over 65 GB; re-reading all files
for unchanged data contradicts the meaning of an incremental index.

**Video in the window.** `ffmpeg` returns rawvideo RGBA 720×480 at 15 fps, the channel is limited to
two frames, the UI shows the latest. Sound was added by a separate decision — ADR 0028. Later replaced
by ffplay (ADR 0029): the manual frame stream and the sound were removed.

**Forbids:** re-reading unchanged files "just in case"; counting a skip as an error; starting video on
the UI thread; keeping an unbounded frame buffer; promising sound in the window.

## 0028. Video sound: ffmpeg gives PCM, rodio plays it

> Refined by 0029: this path was removed. Video and sound are now played by ffplay; rodio and the
> second ffmpeg are no longer needed.

The owner rightly noted: if we are already launching `ffmpeg`, it is silly to show video without
sound. `tools::audio_stream` starts a second `ffmpeg -vn -ac 2 -ar 44100 -f s16le -` process; the
stream reads chunks of 0.5 s, converts i16 to f32 and puts them into a `rodio::Player` created on the
default device (`DeviceSinkBuilder::open_default_sink`). The queue is limited to three chunks: the
decoder does not run ahead and does not eat memory. Stopping is an atomic flag in `VideoState`;
`Player::stop` and `child.kill` end the sound.

**Why rodio and not symphonia+cpal by hand.** `rodio` is a thin wrapper over `cpal` and can play a
`SamplesBuffer` without a file and without decoders; we do not need the mp3/flac features. We take only
`playback`, so as not to drag in codecs. The Linux dependency is system ALSA, on Windows/macOS there
are standard backends.

**Honest limitation.** Video and sound are two independent processes, both start from zero. There is
no seeking, and with a long clip a slight drift is possible; for exact viewing there remains "With
sound in the system" with a full player. This is better than silence and needs no video codec in egui.

**Why TurboJPEG for photos.** A 108 MP JPEG (12032×9024, 36 MB) decodes fully in ~0.8 s, plus a resize
and RGBA — 1.3 s. imv is fast because it uses libturbojpeg with DCT scaling: decoding straight to 1/4
(3008 px) takes ~0.52 s. On Linux `derive::fast_view` does the same; on other OSes the `image` path
remains, so as not to drag in a native build. `turbojpeg` links through pkg-config on Linux only and
does not affect the other platforms.

**Forbids:** keeping an unbounded audio queue; starting sound on the UI thread; promising a seek that
does not exist; sending sound or paths to the network.

## 0029. Video — ffplay, without a manual frame stream

The owner demanded using `ffplay` instead of assembling video from frames and mixing in sound
separately. That is more correct: ffplay is part of ffmpeg, decodes video and sound itself, knows all
codecs, seeking, pause and synchronisation. The app needs neither `rodio`, nor a second ffmpeg process,
nor a PCM queue, nor manual synchronisation.

`tools::ffplay` looks for `ffplay` next to the configured `ffmpeg`, otherwise in `PATH`, and runs
`ffplay -autoexit -loglevel error -window_title photocat <file>`. The process is kept in `App`;
"Stop the video" and closing the full screen kill it. If ffplay did not start — the fallback path
`tools::open_path`.

**Honest limitation.** This is a separate window, not a modal window inside egui. There is no
built-in video player in egui: it would require a video decoder, an audio output and synchronisation —
that is, effectively a second ffplay. A separate window with a full ffplay is better than a mute
slideshow.

**Forbids:** assembling video from frames and sound by hand; dragging in rodio/cpal; promising a
built-in player; leaving an ffplay process behind after the view is closed.

## 0030. A built-in player in pure Rust; ADR 0029 is cancelled

The owner demanded a built-in player in the app window, cross-platform and without external
dependencies. This cancels the ADR 0029 item "forbids promising a built-in player": it was true for a
build out of processes, but not for pure Rust.

The decoders and the demultiplexer are the `rff` family (`rff-format-mp4`, `rff-format-mkv`,
`rff-codec-h264` over `rusty_h264`, `rff-codec-hevc` over `rusty_h265`, `rff-codec-aac`), the sound is
output by `cpal` (ALSA/PipeWire, WASAPI, CoreAudio — an operating system API, not a package to
install). Neither `ffmpeg`, nor `ffplay`, nor `mpv`, nor GStreamer take part in playback, and there is
no C in the dependency tree.

Why not "codecs from the system": the `libav*` ABI is unstable between majors (60/61/62/63 are ffmpeg
6.1/7/8/9), on Windows and macOS there are no system `libav*` at all, and Media Foundation and
VideoToolbox give only a decoder without a demuxer. Three platform branches mean three different
behaviours, that is, a refusal of "the same everywhere". The same codec in the binary is the only way
to get identity.

**Verified on the corpus (2026-11-23).** H.264 High 2520×1080 — 667 frames, the md5 of the Y/U/V
planes matched the ffmpeg decode (bit-exact), 29–35 fps; 1280×720 — 90 fps; HEVC Main 1080×2520 —
47 fps, also bit-exact; AAC 48 kHz — 1 068 032 samples, exactly 22.250667 s as in ffmpeg; the MP4
rotation (`rotation`) is read and applied to the frame.

**Honest limitation.** The decode is 4–5 times slower than a multithreaded ffmpeg: 4K H.264 will go at
~14 fps, that is, it will not go. For such files there remains "Open in the external player" (`ffplay`
is needed by the indexer anyway). The crates are pre-1.0; H.264 Baseline/High, HEVC Main 8 bit and
AAC-LC are verified. Frame formats other than 4:2:0 8/10 bit are rejected by name and yield to the
external player.

**Forbids:** pulling `ffmpeg`/`ffplay`/`mpv`/GStreamer into the player as a mandatory dependency;
promising 4K; keeping more than one or two frames ahead in memory; decoding on the UI thread; deleting
or renaming files (playback only reads).

## 0031. Player dependencies: `rff-*`, `rusty_h264`, `rusty_h265`, `cpal`

| Crate | Licence | Why | Without it |
|---|---|---|---|
| `rff-core`, `rff-format`, `rff-format-mp4`, `rff-format-mkv` | Apache-2.0 | MP4/MOV and Matroska demux, packets with PTS, a sample index for seek | an own MP4/MKV parser — months of work |
| `rff-codec` | Apache-2.0 | the shared `Decoder` trait and the codec registry: the player knows nothing about codecs | a separate decode loop per codec |
| `rff-codec-h264` → `rusty_h264` | Apache-2.0 / BSD-2-Clause | H.264 (the whole corpus: 258/258 files) | video does not play |
| `rff-codec-hevc` → `rusty_h265` | Apache-2.0 | HEVC (the iPhone default) | HEVC goes to the external player |
| `rff-codec-aac` | Apache-2.0 | AAC-LC | video without sound |
| `cpal` | Apache-2.0 | PCM output: ALSA/PipeWire, WASAPI, CoreAudio | we have no audio output of our own; ADR 0028 already tried and removed it |

The check on real data: see 0030. All licences are permissive, there is no copyleft in the tree.
`cargo tree` contains no C crates with a `cc` build: `rusty_h264`/`rusty_h265` are pure Rust with
`#![forbid(unsafe_code)]` in the core.

**Forbids:** adding a second audio output or a second decoder "just in case"; enabling the `ffmpeg`
fallback by default instead of an explicit button.

## 0032. Tags: a common dictionary and calibration by folder, not tuning the prompts

One archive cannot be turned into a reference: prompts tuned to "Фотографии с телефона" will work
badly for another owner. At the same time a pattern is visible on the current corpus: in 337 of the
717 snapshots with "furniture", document/screenshot is higher, and furniture is almost always second.
The cause is not only the prompts — the central 224×224 crop cuts off the context of vertical frames
(see `docs/ARCHITECTURE.md` § Measurements, `8.jpg`).

**Decision.** The default set of tags and prompts is common, without words from a particular archive.
The model input is an ensemble of frames (centre + the whole frame with margins). The filter threshold
is a percentile per tag (100% = the strongest snapshot of that tag), not a raw cosine and not a global
maximum. Labels and prompts are stored in the DB per model (`tag_defs`), edited by the owner and
recomputed from the already stored image vectors, without a repeated CLIP over the photos. Confidence
comes from the gap between the first tag and the second, otherwise the card says "not sure". A larger
model (SigLIP 2 multilingual, CLIP ViT-L/14) is added only after M12 (dynamic dimension) and with a
CPU measurement.

**Consequences.** A schema migration is needed, `TAG_LABELS`/`PROMPTS_*` must be replaced by a loaded
dictionary, `ocr.rs`/`organize.rs` need edits, the tags need a recomputation and a check on two
independent folders. On a small folder (< 100 files) the percentiles are noisy — a fallback raw
threshold and a warning.

**Implementation check (M15.3).** The card does not pass the first weight off as an answer: with a gap
below 0.01 it writes "tag: not sure", shows the top-3 with weight and percentile and highlights
nothing; with a confident tag the line is "tag: <name>" with the accent. On `8.jpg` (document 0.271 /
interior 0.270, gap 0.001) — "not sure", not "document". Tests without a window through
`egui::Context::run_ui`.

**Implementation check (M15.2).** The filter threshold is a percentile per tag: at 100% "furniture" has
the threshold 0.298 and 1 snapshot, "document" — 0.327 and 1, "children" — 0.280 and 1; at 0% — all
2229 files; at 70% every tag has 669 snapshots (thresholds 0.229–0.248), at least one tag passes for
2129 of 2229. The old `tag_show` migrates: confidence returns to 0.22, the filter to 70%; a folder of
fewer than 100 files falls back to the raw threshold with a warning in the interface.

**Implementation check (M15.1).** The frame ensemble is done: centre crop + the whole frame, averaging
of the vectors, `embed --refresh`. Measurement on `8.jpg` (full frame): centre — screenshot 0.278 /
document 0.275 / interior 0.271 / furniture 0.257; whole — interior 0.274 / furniture 0.262 /
screenshot 0.261 / document 0.257. The crop really does remove "document" from the leaders, but does
not make "furniture" first: "interior" comes forward. Hence the crop is a necessary but not a
sufficient condition; the promise "furniture first" from a crop alone would be tuning. What remains is
the percentile threshold (M15.2), the explicit "not sure" (M15.3) and the common prompts (M15.5).

**Forbids:** hardcoding prompts or thresholds for one folder; auto-tuning to the content of the
archive; sending prompts, photos and paths to the network; promising an object detector instead of
hints. The implementation is M15, plan `plan/M15-tags.md`.

## 0033. A PhotoPrism-shaped shell: month groups, filter chips, flat sections

The interface can do everything needed (indexing, duplicates, faces, OCR, trash, viewing), but is
arranged as a set of panels: three columns, the view heading on one line with the body, a flat ribbon
of 2230 tiles, filters in three places, every block in a frame. The owner looks at this every day; the
example he knows is PhotoPrism.

**Decision.** We take the structure of PhotoPrism, not its look:

- side navigation in two groups ("Library", "Tools"), with a number on the right of an item;
- flat sections instead of `Frame::group` in the panels and the card;
- a content panel: the view title at 18 px, the facts next to it, "Tiles" on the right, below the
  chips of active filters with "×" and "found N of M";
- the grid grouped by month from newest to oldest, a tile step of 2 px, hover gives an outline;
- an inspector: a preview, the file name at 14 px, the path at 11 px, the actions, then
  "label — value" rows;
- one style for the whole GUI: step 8×6, button 26 px, radius 4 px, the palette from `visuals()` and
  one accent.

**Why not a copy.** The light theme, icons without labels, favourites, multi-select and the map need
new data, new dependencies and violate §Layout (`docs/UI.md`): an icon cannot be the only path, there
are no colours of our own, the originals do not change. Albums (M19, `plan/M19-albums.md`) are not a
transfer of someone else's model: they are our own data in the DB, without file copies and without
PhotoPrism's photoshopped "albums"; they are made separately. The structure is transferred, the
foreign archive model is not.

**Consequences.** Only `src/app.rs` and the documents change: the DB schema, indexing, duplicates,
CLIP, OCR, faces, the player, the trash and the network are untouched. The "newest first" order and
the month headings change a habit, but not the data: `filtered()` still returns row indices, the sort
lives in the drawing. The metrics are in `docs/UI.md` §Metrics, the rules — in
`.pi/skills/ui-design/references/verstka.md`.

**Check.** `cargo clippy --all-targets`, `cargo test`, `enforce-architecture`, a short GUI run on a
copy of the real DB (2230 files, 25 MB) with a warning to the owner; 4 columns at 1240 px and at
1000 px with the right panel closed. The reasoning and the options — `plan/M16-interface.md`.

**Forbids:** a second set of styles or colours; an icon instead of a word in navigation; "view
settings" appearing in a dialog where their place is the content panel; deleting, renaming or moving
archive files for the sake of the layout.

## 0034. Albums in the DB: a tree and an order, separate from folders on disk

The owner has been collecting the family archive for years and wants ready-made sets out of it:
"Дача 2024 → Май, Июнь", "Лиза → 1 год", "На печать". Folders on disk are not good for this: an
archive folder is the place where files lie, while an album is a human decision about which snapshots
should stand next to each other and in which order. If albums became folders, any rename or move would
affect the originals, and §1 AGENTS.md forbids that. Besides, one snapshot is appropriate both in
"Дача 2024" and in "На печать" — a file system cannot do that without copies.

**Decision.** Albums are data in SQLite only (schema 5):

- `albums(id, parent_id, position, name, name_key, note, created_at, updated_at)`, `parent_id = 0` is
  the root; a name is unique **among siblings** case-insensitively (`parent_id, name_key`), not
  globally;
- `album_files(album_id, file_id, position, added_at)`, `PRIMARY KEY(album_id, file_id)`: one snapshot
  is one row in an album, but it may be in different albums as many times as needed;
- `position` is dense 0..n−1 for files and for subalbums; `db` renumbers it after an insert, a move
  and a delete, so "up/down" is data, not a view;
- all operations (`create`, `rename`, `move`, `add`, `remove`, `move_in_album`, `delete`) are
  `INSERT/UPDATE/DELETE` in `db.rs`; `delete_album` recursively clears the subtree. There are no
  cascades: `PRAGMA foreign_keys` is not enabled in `open`, so deleting `files` rows explicitly clears
  `album_files` and compacts the order;
- a cycle is forbidden: `move_album` refuses if the new parent is the album itself or its descendant;
- the CLI `photocat albums …` works without graphics; `PATH` is the key `files.path`, the files are
  not opened; codes 0 — done, 1 — a data error, 2 — wrong arguments or a refusal without `--yes`.

**Consequences.** An "Albums" view appears (tree, subalbums, a grid in album order) and the sort option
"Album order", available only inside an album. An album stores no cover: the preview of the first
non-trashed file by `position` serves as one. A file in the app trash stays an album row but is hidden
in the grid (the counter "N in the trash"); a file that disappeared from the index loses the album
row — there are no "dangling" references. Export to disk is a separate task M20
(`plan/M19-albums.md` §6): file copies, the structure repeats the tree, `--out` is not overwritten.

**Forbids:** creating, renaming or moving archive files for an album; making an album a folder on
disk; storing albums in a settings file instead of the DB; allowing identical names among siblings;
adding one snapshot twice to one album; deleting files when an album is deleted; calling an album a
"folder" in the interface — "Folders (N)" is already about archive folders.

**Check (M19).** Migration of a copy of the real DB 4 → 5: 2230 files intact, DB +8 KiB. A 3-level
tree, `albums add` of 500 files — 25 ms, a repeat — 19 ms and "0 added, already had 500"; DB +28 KiB,
order 0..499. `cargo test` — tests of the tree, the order, uniqueness among siblings, the cycle,
recursive deletion, idempotence, the trash and the migration. `cargo clippy --all-targets`,
`enforce-architecture` — clean; the GUI on a copy of the real DB — no crashes.

## 0035. Album export: the album structure, file copies, nothing overwritten

The owner builds albums in the app (ADR 0034), but an "album" is useless until a real folder can be
made out of it: to send for printing, to upload to the cloud or to open without photocat. Links
(hardlink/symlink) do not work for that — the recipient has to take away a copy. The order is not
stored in the file system, so it has to be encoded in the file name.

**Decision.** `photocat export --album N --out DIR [--name TEMPLATE] [--apply]`:

- the structure repeats the album tree one to one: `--out/<album>/<subalbum>/…`; every folder gets its
  own files. Subalbum folders are **without numbers** — on disk they go alphabetically, the tree order
  stays in the app. An empty album gives an empty folder;
- the files are **copies**, not links: `std::fs::copy`, the inode differs, the content matches by
  `sha256`. `export` has no `--mode` option; links remain with `organize` (the showcase);
- the default name is `{n}_{orig}`: `{n}` is the position in its own album (2 digits up to 99, 3 up to
  999), available are `{date} {time} {camera} {tag} {orig} {ext} {hash8}`. The name sanitiser is shared
  with the showcase;
- **nothing is overwritten or deleted**: a name taken by other content gets `-2`; a file with the same
  `sha256` is skipped (an idempotent repeat); the same snapshot under an old number — "N stale in the
  target". Reordering does not rename what is already exported — the file system is not obliged to,
  and §1 AGENTS.md forbids touching files; the remedy is a manual re-export;
- `--out` must exist, be a folder and not lie inside `settings.roots`; without `--apply` nothing is
  created, not even the folder. No service files are written into `--out`: the cache "what has been
  exported" (`sha256`, size, mtime, relative path) lives in `<cache>/photocat/exports/`;
- existing files are compared by size, then by `sha256` (reading, not writing); the cache speeds up a
  repeat if the size and mtime of the target file have not changed;
- CLI codes: 0 — plan/success, 1 — there were copy errors, 2 — arguments, a missing `--out`, an
  `--out` inside the archive. GUI: "Export to disk…" → fields, plan tree, "Export N files", progress
  and "Stop" in the top bar, a report and "Copy the report".

**Consequences.** `organize.rs` gets `plan_album`/`apply_export` and the only allowed exception:
copying into a user folder by an explicit `--apply`. `db::album_export_data` returns the subtree and
the files in album order. `organize` now uses `derive::sha256_file` (L0), so the layer map in
`check.sh` allows `derive` for `organize`; there is still no deletion or renaming in `src/`.

**Forbids:** exporting with links and adding `--mode` to `export`; deleting, renaming or
"synchronising" anything in `--out`; writing manifests and `.photocat/` into `--out`; exporting
automatically on a schedule, at start or when an album changes; indexing `--out` without asking;
touching the archive originals.

**Check (M20).** A 3-level tree, 500 files (2.4 GiB): dry run 14 ms, `--apply` 2045 ms, a repeat 20 ms
and "500 skipped". 2230 files (21.2 GiB): dry run 21 ms, `--apply` 111 ms (the FS gave
`copy_file_range`), a repeat 45 ms and "2230 skipped"; 5 checked copies — `sha256` matched, the inode
differs. `cargo test` — 16 new export tests (tree, position number, name cleaning, collisions,
idempotence, a copy is not a link, an empty album, a stale order, a missing file, cancellation, refusal
of `--out` inside the root); `clippy`, `check.sh` — clean; the GUI on a copy of the real DB — no
crashes.

## 0036. The tag dictionary in the DB: the default is seeded once, recomputation from the stored vectors

Prompts and labels used to be constants in the code (`TAG_LABELS`, `PROMPTS_RU/EN`): every new tag
meant a Rust edit and a rebuild, and the archive owner is not a programmer. At the same time ADR 0032
already required a dictionary per model and a recomputation without a repeated CLIP over photos: image
vectors do not depend on the prompts.

**Decision.** The table `tag_defs(model, tag_id, label, prompts, enabled, position, min_pct)`
(schema 6). `label` is the Russian caption and at the same time the `tags.tag` key, unique within the
model. `prompts` is one template per line; several templates give one tag, and its weight is the
**best** of the templates, not the average: a description may match only one wording, and the average
blurs such a guess. The default is `embed::DEFAULT_TAGS`, 14 common labels with 2–3 templates each,
without words from a particular archive; prompts are chosen by the model language (`Projected` →
Russian, `Direct` → English). Seeding happens only if the model has no rows at all. The default
version (`settings.tag_defs_version`) is bumped together with the set, and `settings.tag_defs_sig` is
the fingerprint of the seeded dictionary: a new default replaces the old one only if the fingerprint
matches (nobody edited it); the owner's edits and an import (version 0) are not touched by the
auto-update. `ensure_tag_defs` is the only entrance; `Model::set_tag_defs` computes the vectors of the
enabled prompts. A dictionary edit is recomputed by `embed --retag`: the text tower once, then
`replace_tags` over every stored `embeddings` (2229 files in 1.0–1.2 s), photos are not decoded. A
disabled tag is neither computed nor put into the filter; a removed one disappears from `tags` on
recomputation. `min_pct` raises the percentile threshold of a single tag, the common slider stays the
main one. Exchange is via JSON (`{id,label,prompts,enabled,position,min_percentile}`), without paths
and photos; an import replaces the model's dictionary entirely. CLI: `photocat tags [--list|--add|
--enable|--disable|--rm|--export|--import|--reset]`; GUI: the editor in "Settings → Tags", the
"Recompute tags", "Export JSON", "Import JSON", "Restore the defaults" buttons.

**Consequences.** `SCHEMA_VERSION` 5 → 6; `TAG_LABELS`/`PROMPTS_*` removed from the code, `ocr.rs` and
`organize.rs` keep reading the labels from `tags` and change nothing. `--reset` ("Restore the
defaults") still returns the standard explicitly; the auto-update fires only on an untouched
dictionary. Exporting the dictionary is an explicit owner action (like `manifest --out`), it writes
`<cache>/photocat/tags.json` by default and does not touch the archive.

**Forbids:** hardcoding labels and prompts in the UI or in the core after the seeding; overwriting the
owner's dictionary on an update; recomputing photo embeddings when the prompts change; sending the
dictionary to the network; deleting/renaming archive files.

**Check (M15.5).** Prompts v1 (one per label) replaced by 2–3 common templates, the label weight is the
best of the templates, set version 2. Phone corpus (2229 files): confident 1285 → 1242, top-1 the same
for 83.2%, "document" in top-3 804 → 534, a false "document/screenshot" on photos with a camera
270/1655 → 225/1655 (16.3% → 13.6%), on snapshots with recognized text 123/145 — no losses. A second
folder "день рожденья" (110 files): confident 66 → 69, top-1 the same for 86.4%, "document" in top-3
38 → 21. `8.jpg`: document 0.274 / furniture 0.273 — "not sure". The average over templates lost to the
maximum on all four metrics (confident 1175, false 245, documents 120/145) — hence the maximum.
"Чертёж/схема" was not added to the default: it is a niche, not a common category; the owner will add
it through the dictionary.

**Check (M15.4).** A copy of the real DB (2230 files): `tags` seeded 14 tags, `--add "Шкаф"` with two
prompts → `embed --retag` 2229/2229 in 1.0–1.2 s; "Шкаф" 0.187–0.292, top-1 for 218 snapshots, at the
70th percentile the threshold 0.2454 and 669 snapshots — as many as the other tags. The edit survived
a restart (three separate processes), `--export` → `--reset` → `--import` returned 15 tags.
`cargo test` — JSON roundtrip, seeding once, label uniqueness, migration 5 → 6; `clippy`, `check.sh`
(41/0), the GUI on a copy of the DB — no crashes.

## 0037. An integrity check reminder: the deadline in the settings, the run only by a press

Checking a backup "when I remembered" does not work: a family archive has no administrator, and the
manifest already exists (M8/M9). A deadline is needed, but the app must not read 21 GiB in the
background by itself and must not set system timers.

**Decision.** `settings` holds `verify_root`, `verify_manifest`, `verify_interval_days` (30 by default,
0 — off), `verify_last_at` (RFC3339 UTC), `verify_last_ok` and `verify_last_report`.
`db::verify_due(settings, now)` is one function with an explicit `now`: the banner and the tests count
the same thing. "Due" means a manifest is set, the interval is above zero and at least the interval has
passed since the last pass; an empty or broken date is "due"; a date in the future (the clock was set
back) is not "due", without a panic. On start, when due, the GUI shows the banner "Time to check the
archive" with "Check" and "Remind later"; "Remind later" is for the current run only and is not written
to the DB. "Check" opens the "Integrity" screen and starts the same background check against the
manifest with progress and "Stop"; there is no automatic start.

**The result of a pass.** A written manifest and a finished check write the date, "ok" and the full
report into the settings and into `<cache>/photocat/verify-last.txt`; nothing into the archive folder.
A cancel or a failed pass (a broken manifest, a missing file) does not touch the previous successful
report and date: otherwise the reminder would "eat" the last good state. Writing a manifest also sets
the date: the archive has just been read in full, and there is no reason to check it in the same
minute.

**Forbids:** auto-starting a check by a timer in the app; writing the report into the archive folder;
counting a cancelled or failed pass as successful; losing the previous report because of an error.

**Check (M10).** `cargo test` — the "due/not due" calculation (no manifest, interval 0, exactly due,
overdue, too early, the clock went back), a settings roundtrip, the banner visible and starting no
pass, "remind later" hiding it until a restart, a broken manifest not wiping the previous report. The
GUI on a copy of the real DB — a short run without crashes. The CLI path `verify --manifest` is
unchanged (codes 0/1), a systemd timer example is in the README.

## 0038. Similar snapshots: an index of 8-bit blocks instead of a bare BK-tree

Pairwise dHash comparison is O(n²): on a corpus of 2230 files that is 0.025–0.03 s, but at 50k it is
already 1.25 billion checks. What is needed is an exact search of all pairs with a Hamming distance ≤
`max_dist`, without new dependencies and without changing the groups, union-find and the frame advice.

**What did not work.** A BK-tree (and any metric tree with the triangle inequality) degenerates on
64-bit hashes at radius 6: almost all random values lie at a distance of ~32, so for a query with
`d ≈ 32` the allowed child band `[d−6, d+6]` covers the bulk of the nodes. Release measurement: 20k
hashes — 2.58 s, 50k — 17.2 s (test peak RSS 45.7 MB); the goal is 50k in 5 s. The tree stays correct,
but not fast, so its role in the task is played by another exact index.

**Decision.** Above `INDEX_MIN = 20_000` hashes eight buckets of 8 dHash bits are built. If the distance
between two values is ≤ 7, at least one of the eight bytes matches entirely (pigeonhole); for
`max_dist ≤ 7` the candidates come from the eight buckets, duplicates are cut off by a per-query mark,
and the full Hamming distance over `u64` is checked before `union`. Below the threshold the previous
pairwise pass remains — it is cheaper in memory. For a radius of 8–64 there is no guarantee, so the
pairwise pass is used there too: such thresholds glue almost everything anyway. `find_groups_with`
takes the threshold explicitly, so that a test on the same random sets compares the index and the
pairwise pass.

**Forbids:** changing the group composition, the frame advice and `plan_csv`; storing the index in the
DB or recomputing it every GUI frame; adding a dependency for hashing or the index; using the index for
a radius > 7, where it gives no guarantee.

**Check (M11).** Corpus of 2230 files: `dupes --max-dist 6`, release — 0.017–0.025 s, the same 103
groups, 238 files, 214.1 MiB. Synthetic data (pairs "a hash and a hash with one bit changed"): 20k —
0.082 s, 50k — 0.446 s at `max_dist 6`; the test process peak RSS 27.1 and 48.2 MB. The test "the index
and the pairwise pass agree" — random sets 8/40/80/160, `max_dist` 0–10, plus empty, one element, all
identical, all far apart; exact copies by sha256 glue on both paths.

## 0039. The CLIP model catalogue: the dimension in the data, SigLIP 768 as a third model

`DIM = 512` was hardcoded in `embed.rs`: any new model with a different vector required a code edit,
and `tags` had the primary key `(file_id, tag)` — two models with the label "пляж" physically could not
live side by side. M12 requires a catalogue of data and no mixing.

**Decision.** `ModelEntry` holds everything that distinguishes a model: `dim`, the frame side, the
token maximum, the pad token, the normalisation (`derive::ImageNorm`), the number of frames and the
ONNX output names. `TextKind` is the way to read the text tower: `Projected` (hidden + dense), `Direct`
(CLIP `text_embeds`), `Pooler` (SigLIP `pooler_output`). The dimension is checked against the ONNX
metadata at load time and against the actual output on a run; an error names both numbers. Schema 7:
`tags` — `PRIMARY KEY(file_id,tag,source)`, the migration rebuilds the table and recreates the trigger
that removes the CLIP tags of **all** models when sha256 changes. The vectors were already separated by
the key `(file_id,model)`.

The third model is SigLIP base patch16-224 (`Xenova/siglip-base-patch16-224`, Apache-2.0, 203.5 MiB,
768 features). It is a different architecture and a different preprocessing: mean/std 0.5, bicubic, the
whole frame is stretched to 224×224 (without a centre crop), the text is padded to 64 `</s>` tokens.
That is why `norm`, `frames` and `pad_token` appeared in `ModelEntry`, instead of an "if SigLIP" branch
in three places. The CLIP models kept the previous ensemble of two frames, so as not to recompute 2229
vectors.

**Consequences and limitations.** SigLIP answers English queries; Russian search words do not work for
it. SigLIP's cosine scale is lower than CLIP's: a confident tag at 0.22 almost never fires, so tags are
visible through the percentile filter or after lowering the threshold; that is no reason to mix
vectors. `tag_hidden` is still about a label, not about a model — hiding "пляж" hides it in all models
for that snapshot. Switching the model does not delete the old vectors and dictionaries; `--retag`
recomputes only the active model.

**Check (M12).** Download: 36.7 s, 3 files, 203.5 MiB, sha256 matched. Corpus of 2227 files (2 in the
trash): `embed --refresh` SigLIP — 418.2 s (0.188 s/file, one frame), 0 errors, vectors 6.8 MiB;
`embed --retag` — 2.4 s; search over 2227 vectors — 35–40 ms; `clusters --k 30` — 1.5 s. Control red/blue
squares: the right order with a gap. `cargo test` — the catalogue, a dimension error, a refusal without
consent, the migration 6→7 and the preservation of the tags of two models; the old CLIP vectors and tags
stayed in place, `user_version` 7.

## 0040. Language of the project: English first, Russian second; UI translated by lookup

> **Superseded by ADR 0046** — the direction only: the source of the code and of the interface is
> English now and Russian is the translation. The mechanism decided here (one lookup table,
> `Lang` passed explicitly, `settings.lang`, CLI reports carrying `lang`) stands.

**Decision.** The repository speaks English: documentation, commit messages and new code comments.
The existing Russian documentation is kept as the second variant (`*.ru.md` next to the English main
file) and linked from it — nothing is deleted. Communication with the owner stays Russian.

The interface is bilingual: `src/i18n.rs` holds `Lang` (`En` default, `Ru`) and a table of
Russian→English pairs. The source text in the code stays Russian, and `i18n::t(lang, "…")` returns
the English translation when English is chosen; a string that is not in the table yet is shown in
Russian instead of disappearing. The choice lives in `settings.lang` (`en`/`ru`), is made in
"Settings → Interface" and is written to the DB immediately.

**Why a lookup table and not keys + resource files.** Keys (`grid.all_files`) make the code
unreadable for the owner, who reads Russian, and require every label to be renamed at once — a
mechanical rewrite of `app.rs` with no behaviour change. A table keyed by the Russian source means a
screen can be translated in one commit, the diff shows exactly which labels are covered, and a
missing translation degrades to a readable Russian label.

**Why the language is passed explicitly and not kept in a global.** Tests render frames headlessly
and assert text; a global would make those tests depend on each other's order. The `Lang` value
travels from `Settings` into the drawing functions.

**Why not gettext/fluent/rust-i18n.** A new dependency for a two-language personal app is a cost
with no benefit; the table is ~1 reference file, edited by hand. Revisit only if a third language
with plural rules appears.

**First launch.** There were no releases, so an old DB is not migrated: if the `lang` row is absent,
`db::ensure_lang` reads the locale: a set `LC_ALL` or `LC_MESSAGES` decides on its own (so
`LC_ALL=C` means English even when `LANG=ru_RU.UTF-8`), otherwise `LANGUAGE` (a preference list) and
`LANG`; an unknown language or a non-language locale (`C`, `POSIX`, `de_DE`) means English. The
result is stored once. The window and the CLI both call it before printing anything.

**CLI reports** follow the same language, but without a process-wide variable: the report structs of
`verify`, `organize`, `embed`, `faces`, `ocr` get a `lang` field filled from `settings.lang`, and
their `Display` impls go through `t(lang, …)`. Since M21.6 every report speaks that language.

**Consequences.** `t()` takes `&'static str`, so dynamic fragments are translated as parts, and
screens with numbers are translated piecewise. `db::human_in(lang, n)` carries the units
(`КиБ/МиБ` or `KiB/MiB`); `db::human()` stays for CLI output. `i18n::count` owns the plural rules
(Russian 1 / 2–4 / 5+, English 1 / N), so a counter is never glued together from a word and a number
in the wrong order. Coverage is tracked in `plan/I18N.md`; an untranslated
screen shows Russian — a visible state, not a bug. `db::human()` units (`КиБ/МиБ`) and month names
are content, not labels: they move to the table when the content panes are translated.

**Checks (M21).** `cargo test` — the English shell test renders the window headlessly and asserts
`All files`, `Settings`, `LIBRARY`, `FILTER`; the persistence test reopens the DB and finds
`settings.lang = ru`; the i18n table test rejects empty, duplicate or non-Cyrillic keys; the locale
test parses `ru_RU.UTF-8`, `de:ru`, `C`, `POSIX` and an empty tag; the counter test pins `1 файл`,
`2 файла`, `11 файлов`, `1 file`; the grid tests assert `May 2024`, `No date`, `Select all (4)`,
`2 selected · 40 B`; the month test checks both language tables and out-of-range numbers.
`clippy --all-targets` clean; `check.sh` — 44 checks, 0 violations.

## 0041. The tag weight is calibrated per model: SigLIP 2 in the catalogue, opt-in

M12 added SigLIP with a lower cosine scale and left the tail: "a confident tag of 0.22 almost never
fires". M15.6 needed a fourth model (SigLIP 2, multilingual) and hit the same wall — with one
threshold for all models, the meaning of `tag_show`/`tag_margin` depends on the model, and the owner
sees either "everything is confident" or "nothing is".

**Decision.** The raw cosine stays what is compared (percentiles, search), but the tag weight that
goes into `tags` is mapped by the model's own affine calibration
`score = cos * scale + shift` (`ModelEntry::score_calib`, `ModelEntry::score`). The coefficients are
chosen so that the **median of the best tag** and the **median of its lead over the second** land
where CLIP's already are — 0.263 and 0.0118 — so the owner's 0.22 / 0.01 keep their meaning when he
switches the model. The numbers come from 2340 files in two independent directories (the phone corpus
and "день рожденья"), measured once per model; they are a property of the model, not of a folder:
SigLIP 2 `(1.42, 0.14)`, SigLIP 1 `(0.55, 0.23)`, both CLIP models `(1.0, 0.0)` — identity, so no
stored tag changed for them.

`score_version` travels inside `tags.source` (`clip:siglip2-base-224-multi@v2`): rows scored under an
older calibration are not mixed into the new distribution, and `embed --retag` rewrites them from the
stored vectors in 2–4 s without reading a single photo. The percentile filter is unaffected by a
monotone map, so `tag_show_pct` and the per-tag percentiles keep working as before.

**Why an affine map and not "per-model thresholds".** A second set of settings (one slider per model)
would multiply settings the owner has to understand and would still leave two sliders meaning
different things across models. One map per model keeps one slider and one meaning; the map is data
in the catalogue, visible in `models list` and open to a re-measurement.

**Why SigLIP 2 is not the default.** It is a real 768-dimension multilingual model with an Apache-2.0
licence, but on the owner's archive it loses to the current CLIP on the document category at the same
confidence level: of the 142 shots with recognized text it confidently calls 69.0% documents (CLIP
85.9%), and on 1926 photos without text it confidently says "document/screenshot" in 9.7% of cases
(CLIP 3.3%). Its text tower is 270 MiB because of the 256k-token Gemma vocabulary (393.3 MiB in
total, over the 250 MiB default limit), and it is 2.7× slower per file (0.170 s against 0.063 s).
So it is added as an opt-in entry: the owner raises the limit to 400 MiB and clicks download if he
wants to try it; nothing is downloaded without that click. Its Russian search is weaker than the
multilingual CLIP text tower (the same label ranks the same photos in only 2 of 3 cases on the
sample), so the default stays `clip-vit-b32-multilingual`.

**What it forbids.** Comparing raw cosines of two models in the UI or in a report; a threshold that
silently changes meaning when the model is switched; adding a model to the catalogue without a
calibration pair and without a measurement on two folders.

**Consequences.** `siglip-base-224-en` becomes usable for the first time (0 → 55.6% confident tags at
the same threshold) at the price of a lower document recall (53.5%). Rows written by a calibration
expect the version in `source`; a DB that had SigLIP tags from before M15.6 shows none until
`embed --retag` — a documented state, not a silent mix. The licence of the new weights is Apache-2.0,
compatible with the project licence (AGENTS §1); no new dependency appeared — it is data in the
existing catalogue and the existing `tokenizers` crate reads the Gemma tokenizer.

**Checks (M15.6).** `cargo test` — the catalogue describes four models (unique ids and folders,
sha256, dimensions, licence, `score_calib` identity only for CLIP, `score_version` 2 for SigLIP),
the calibration maps both medians of both SigLIP models onto CLIP's within 0.006 / 0.002, the source
of a calibrated model carries `@v2`, and the padding test keeps the EOS last for left padding and
reproduces right padding for CLIP. `embed --retag` on the bench DB reproduces the measured numbers;
`models download` refuses SigLIP 2 at the 250 MiB limit and downloads 393.3 MiB at 400 with a sha256
match. Full numbers — `docs/ARCHITECTURE.md` § Measurements, `plan/STATUS.md` § M15.6.

## 0042. Tag quality is measured on a blind labelled sample, not on one folder's habits

Every tag change so far was argued with a proxy: the share of confident tags, the leading label on
`8.jpg`, the agreement of two models, OCR text as ground truth for documents. Those numbers can rise
while the tags get worse — "confident" says nothing about right, and a proxy measured on the phone
corpus invites tuning to it.

**Decision.** The tags are scored by the `photocat eval` command against a sample a human labelled.
The sample is taken from a folder by `sha256` order with a fixed stride — content, not names or
dates — and every shot gets one of the dictionary ids, `other` (no dictionary label fits) or `skip`;
up to two ids joined by `|` when two labels are equally fair. The predictions are written to a
separate file, so the labels are **blind**: the model's guess is never next to the tile being
labelled. Contact sheets with numbers, CSV for the labels, `--labels` to score. Two independent
folders are fixed **before** the experiment; a change is accepted only if it helps on both.

The report is: top-1, top-3 (over the shots a dictionary label fits), the coverage itself, the
accuracy of a *confident* tag (the same `tag_show` + `tag_margin` rule the card uses to say
"not sure"), and how often the model is confidently wrong on `other`. The percentile filter is
checked as recall of a labelled tag at 70 % and 100 %, with the 100 % row understood as a sanity
check (by construction it keeps one shot of the whole index).

**Why blind and why by content.** A label written while the prediction is visible measures
agreement with the model, not truth. A sample ordered by file name clusters by day and by device; one
ordered by `sha256` scatters content and is reproducible without a random seed.

**What it forbids.** Reporting "top-1 accuracy" from a sample with no `other` option (it would count a
wrong label as right); tuning prompts to the labelled sample; calling a metric "precision" when the
sample was chosen by the model's own tags (a top-N of a tag is not a random sample); comparing
numbers from different labelled sets between releases.

**Consequences.** A third of the sample lands in `other` (the dictionary has 14 common labels, and
the phone corpus is full of household details and adult portraits) — that is a fact about the
dictionary, not a defect, and it is reported first. The labels are one reviewer's reading of 320 px
previews, so the numbers are comparable between models and between runs of the same protocol, never
with someone else's labelled set. The first run (M15.7) gives top-1 85.4 % / 82.6 %, top-3 97.6 % /
97.7 %, confident-tag accuracy 79.7 % / 79.0 % on the two folders; the numbers are in
`docs/ARCHITECTURE.md` § Measurements and § "Labelled check of the tags".

## 0043. The fp16 towers of SigLIP 2 are not adopted: int8 costs no tag quality

M15.6 left one question open: SigLIP 2 caught 69.0 % of the shots with recognized text against
CLIP's 85.9 %, while its text tower is int8 (283.4 MiB) and `onnx-community` ships fp16 towers as
well (vision 186.0 MiB, text 564.9 MiB). Part of the loss could have been the quantisation, so the
answer had to be measured rather than guessed.

**Decision.** Both fp16 towers were swapped into a bench copy of the SigLIP 2 entry (a symlink in the
model folder and a temporary edit of the catalogue's file list during each run; nothing shipped) and
the same numbers were measured on the same 2231 files, with the same tag dictionary, the same
`tag_show` 0.22 / `tag_margin` 0.01 rule and the same calibration protocol as M15.6:

| towers (vision + text) | model | confident tags | documents with text caught | false “document” without text |
|---|---|---|---|---|
| int8 + int8 (the catalogue entry) | 393.3 MiB | 55.2 % | 69.0 % | 10.0 % |
| int8 + fp16 text | 661.6 MiB | 55.8 % | 64.8 % | 12.7 % |
| fp16 + fp16 | 748.9 MiB | 55.4 % | 49.0 % | 3.6 % |

int8 costs no tag quality: the confident share is the same in all three, the fp16 text tower is
*worse* on documents, and the fully fp16 model only trades document recall for precision (49.0 %
caught, 3.6 % false) — it is less document-biased, not better at finding documents. The document
weakness belongs to SigLIP 2 itself, not to the 8-bit files.

**Consequences.** SigLIP 2 stays a single int8 catalogue entry; no fp16 variant is added, the
default stays the multilingual CLIP, and `models list` still shows four entries. No dependency
remains either: ORT on CPU does run the fp16 graphs, and the only code needed was a 15-line reader of
an `f16` output tensor (`ort` feature `half` plus `half::f16`), reverted with the temporary catalogue
data once the numbers were in. The rejected variants would have cost 1.7–1.9× the weights on disk and
in RAM and a slower model load for no gain; the `embed --refresh` time was the same (528.7 s against
510.0 s). The fp16 weights are Apache-2.0, stay outside the repository and are named with their
sha256 in `plan/STATUS.md` § M15.6 tail, so the experiment can be repeated.

**What it forbids.** Adding an fp16 entry to the catalogue "because it is more precise" without the
same measurement; treating a lower false-positive rate as a win while the true-positive rate falls
further; keeping a dependency for a variant that lost.

## 0044. Two themes from the mock-up: one token palette, the mock-up's radii, amber as the only signal

The owner brought a mock-up set made outside the repository (Stitch): seven screens in HTML, two
design-system files (`functional_precision_desktop`, `functional_precision_light`) and the rule "for
screens without a layout, build by the same rules". The code has had one theme since M16: colours
came from `egui::Visuals`, the only own colour was the blue accent `rgb(90,170,255)`, the radius was
4 px, the caption under a tile was 26 px with the date, and `docs/UI.md` listed "theme settings" as
something the interface would not have.

**Decision.** The palette moves into one `app::Theme` struct with two constants, `DARK` (the default)
and `LIGHT`; `settings.theme` is `dark | light | system` and is stored in the key/value `settings`
table, so `SCHEMA_VERSION` does not move. `apply_theme` fills `egui::Visuals` from the tokens and sets
the shape for both palettes: the mock-up's radii (4 px for controls, chips and badges, 6 px for
navigation rows and menus, 8 px for tile cards, fields and dialogs), no shadows, the type scale
10/11/12/13/14/18,
monospace for numbers, uppercase caps labels. Screens read `theme(ui)` / `accent(ui)` / `dim(ui)` and
never write a colour by hand; the single exception is text over a darkened frame, where the
background is a photograph. `danger` is a second colour, allowed only on the path to the OS trash.

Where the sources disagree, the rendered HTML wins for dark (cool graphite `#0c0e12`/`#1a1c20`), and
the light `DESIGN.md` wins for light (slate `#f8fafc`/`#ffffff`/`#e2e8f0`, amber `#d97706`). The dark
front-matter of `DESIGN.md` is a warm Material scheme (`on-surface-variant #d8c3ad`) that contradicts
its own prose ("deep, cool industrial grays") and the rendered screens; a warm cast also tints the
judgement of a photograph, which is the one thing a photo cataloguer must not do.

**Why two themes and not "follow the system all the time" only.** The archive is looked at in the
evening and in daylight; the owner asked for both palettes. "System" is offered as a third option
but the default is dark, because the mock-up's dark screen is the reference and because a light
window at night is the brighter thing in the room.

**Why these radii.** The design-system prose claims a strict 0 px, but every rendered screen of the
mock-up uses Tailwind `rounded` / `rounded-md` / `rounded-lg` (4 / 6 / 8 px) on exactly three roles:
controls and chips, navigation rows and menus, cards and dialogs. The owner's verdict is the screen,
not the prose — the same rule that settled the palette — so the three radii are adopted and no fourth
is invented. A monospaced number does not shift its neighbours while indexing; 10 px is admitted only
for caps labels and numbers, where it is decoration around a value, never for a sentence.

**What it forbids.** A colour literal in a screen; a second accent; a third theme or an accent
picker; a fourth radius or a radius on a single screen; shadows/gradients; a 13 px body turning into
10 px prose;
bundled fonts (Inter, JetBrains Mono) until the owner decides — the mock-up's fonts are replaced by
the egui defaults for now, and no font file is added to the repository or to `NOTICE`.

**Consequences.** A screen that still uses `ui.visuals()` for a semantic colour now gets the theme
tokens through `Visuals`, so the light theme works without touching the screen; a screen that needs
a token the `Visuals` mapping does not cover calls `theme(ui)`. `docs/UI.md` § Layout, § Metrics and
§ "What the interface will not have" are updated in the same change, and the task list for the
remaining screens is `plan/M22-ui-spec.md` § 6.

## 0045. Previews follow the EXIF orientation; a stale preview is overwritten, not deleted

`derive::analyze_image` decoded a snapshot with `ImageReader::decode`, which ignores the EXIF
orientation. A phone writes the pixels sideways and records "rotate 90°" in the file; the preview,
the dHash, the sharpness and `files.width/height` were therefore all computed on the sideways frame,
and the grid, the card and the full screen showed such a portrait lying on its side. The owner
brought `IMG_20240822_124033.jpg` (orientation 6) as the evidence.

Fixing the decode alone was not enough: a preview is cached by `sha256[..24]` under
`<cache>/photocat/thumbs/` and `analyze_image` wrote it only when the file was missing. The file was
already there (wrong), so a corrected decoder would have kept showing the old sideways preview, and
`can_skip` would have skipped the file anyway, because path, size and mtime had not changed.

**Decision.** The preview is built from `load_oriented` (the decoder's orientation applied), and
`files.width`/`files.height` become the **displayed** dimensions. When the cached preview's own
dimensions do not match the expected ones, it is rewritten in the cache. `SCHEMA_VERSION` 7 → 8
clears `files.thumb` for images once, so the next `index` re-analyzes rows whose width/height were
stored unrotated; video covers are left alone (ffmpeg autorotates the frame, and `ffprobe` already
reports the rotated size).

**Why overwrite instead of a new key.** A new key (`<sha>-o`) would orphan every old preview (about
29 MB on the corpus) and regenerate them for nothing, and it would also have to regenerate video
covers. Overwriting a file in our own cache is not an archive operation: the archive is never
touched, and the cache is cleaned only by `storage.rs`, which already has that right. Deleting the
stale file instead is forbidden by the architecture check ("no deleting or renaming in `src/`") and
unnecessary.

**Cost.** `apply_orientation` allocates a rotated copy for the six non-trivial orientations, so a
worker holds one extra photo in memory at the peak. The one-time re-index after the upgrade decodes
every image: on the corpus 1972 images / 3.8 GiB — 252 s in the dev profile (release is about four
times faster, the documented 65 s order); every later pass skips them again as before. Thumbnails of
untouched files are not rewritten: the dimensions match and the write is skipped.

**What it forbids.** Building a preview without the orientation; storing raw `width`/`height` while
the preview is rotated; writing a preview "because the code changed" without checking its own
dimensions; deleting or renaming a cache file anywhere outside `src/storage.rs`; touching the
archive to "fix" a preview.

## 0046. English is the source of the code and of the interface; Russian is a translation

**Decision (the owner, 2026-11-24).** The repository speaks English everywhere: identifiers,
comments and doc comments, log and error text, the source text of the interface, and the defaults
the code writes into the DB. Cyrillic exists only as a translation — the Russian column of `PAIRS`
in `src/i18n.rs`, the `*.ru.md` documents, and the prompts of the multilingual model (`embed.rs`).
`tests/source_language.rs` enforces it.

This reverses ADR 0040, which kept the Russian source and translated to English. What 0040 decided
stays: one lookup table instead of keys and resource files, no new dependency, `Lang` passed
explicitly rather than kept in a global, `settings.lang` chosen in "Settings → Interface" and
detected from the locale on the first launch, CLI reports carrying `lang` in their structs.

**What the reversal changes.** `PAIRS` is `(English, Russian)`; `i18n::russian(en)` replaces
`english(ru)`; `t(lang, en)` returns the English source for `Lang::En` and the Russian text for
`Lang::Ru`, and a string missing from the table falls back to **English** — the source is always
readable. `i18n::count(lang, n, noun)` takes a noun key (`file`, `photo`, `shot`, `person`, `group`,
`tag`, `album`, `sub_album`) with both plural sets inside `i18n.rs`, so no screen holds a Russian
word. `i18n::USAGE` is `(English, Russian)`; `.0` is the English key.

**Why the owner reversed it.** The code is read and written in English; a Russian source made the
repository bilingual for no gain, and it hid the gaps: an untranslated label looked translated
because it was Russian either way. With an English source a missing translation is visible at once,
the code matches the documentation, the commit messages and the error text, and a checker can ban
Cyrillic outside the table instead of guessing which strings are labels.

**The price and the order.** 206 identifiers with Cyrillic (all of them test names, plus the album
helpers of the `db` tests) were renamed first (M23.1), then the table was inverted mechanically by
swapping the two literals of every pair, so the window did not change for a single commit and the
whole table stayed translated. The rest goes module by module: the 479 Russian literals that remain
in non-test code are error messages, report fragments and data, and `PENDING` in
`tests/source_language.rs` shrinks as each module is converted. The list only shrinks; adding a
module back is forbidden.

**Consequences for the user's data.** The owner's own words are never translated: album names, face
group names, user tags and the `labels.csv` stay in the language he typed. Defaults produced by the
code (tag labels, model titles and notes) are English in the source and reach the Russian interface
through the table. An old DB keeps its rows; a converted default written earlier stays Russian until
the dictionary is re-seeded by `tags --reset`, which is a documented, explicit action.

**Check (M23.1).** `cargo test` — 234 passed, 7 ignored; `cargo test --test source_language` — 2
passed; `cargo clippy --all-targets` clean; `rust-analyzer diagnostics .` — no errors (25 "inactive
code" weak warnings, all from `#[cfg]` branches); `check.sh` — 54 checks, 0 violations. A fresh
cache with `LC_ALL=C photocat settings` prints English; the owner's DB (`lang=ru`) still prints
Russian. Behaviour is unchanged: the inverted table reproduces the same two languages.

**What it forbids.** A Cyrillic identifier, comment or string outside `src/i18n.rs` (and the model
prompts in `embed.rs`); a Russian word in a screen or a report built by `format!` instead of `t()`
or `fill()`; a translated string in the code; growing `PENDING`; translating the owner's own
names, album titles or tags.

## 0047. The binary downloads the weights itself: `ureq` with `rustls`, not a `curl` process

**Decision (the owner, 2026-11-24).** "The program must be able to download data itself." A
cross-platform application is one file, so the weight downloader stops shelling out to `curl`
(ADR 0016) and becomes part of the binary: `ureq` 3.5 with `rustls` and the bundled WebPKI roots.

**Why this reverses ADR 0016.** 0016 chose a process over a dependency to keep TLS, root
certificates and network code out of the project. That reasoning holds for `ffmpeg` and
`tesseract`, which are large C programs doing real work (ADR 0010, 0019), and it does not hold for a
single GET: the consequence was a machine without `curl` that cannot fetch a 217 MiB model at all,
and a shell script as the manual fallback — which needed the same `curl` and existed only for Unix.
For a family archive on a machine the owner did not prepare, that is the wrong trade.

**What it costs.** `ureq` (MIT OR Apache-2.0) with `rustls` (Apache-2.0 OR ISC OR MIT),
`ring` (Apache-2.0 AND ISC), `rustls-webpki`/`untrusted` (ISC), `subtle` (BSD-3-Clause) and
`webpki-roots` (CDLA-Permissive-2.0, certificate data only) — every one of them permissive and
compatible with Apache-2.0, recorded here per AGENTS §1. The binary grew by 2.4 MB (85.1 → 87.6 MB).
`ureq` was already in the build graph as a build dependency of `ort-sys` (it fetches ONNX Runtime);
the rest of the list is new to the lock file but downloads nothing else at build time beyond the
normal registry fetch.

**What we would do without it.** Nothing else in the app needs TLS. Without this dependency the only
alternatives are the external `curl` (rejected: not one binary, not on every machine) or a vendored
download protocol (impossible: HTTPS is not hand-rollable safely). If `ureq` ever becomes
unmaintained, the replacement is `reqwest` (blocking, `rustls`) behind the same two functions.

**Boundary.** The client exists for one thing: `GET` of a catalogue file into the models folder.
`agent.get(...)` only — no POST/PUT/body, no headers carrying user data (`src/embed.rs`). The mirror
(hf-mirror) and three retries stay, the sha256 check stays, consent is still required. The boundary
check (`check.sh` §4) now enforces: no second HTTP client in `Cargo.toml`, `ureq` only in
`src/embed.rs`, no verbs other than `get` there.

**Consequences.** `curl` disappears from the external programs: `Tool::Curl`, `settings.tool_curl`,
`--curl`, the Settings field, the `doctor` line and the "without it" hint are gone. A machine with no
network still works, as before — the download is the only thing that fails, and it says so.
**Forbids:** an HTTP client other than the downloader's, a request carrying owner data, a background
download without a human action.
