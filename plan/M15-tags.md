# M15 — tags that work on any archive

*Русская версия: [M15-tags.ru.md](M15-tags.ru.md).*

**Status:** M15.1 (the frame ensemble), M15.2 (the percentile threshold), M15.3 ("not sure" in the
card),
M15.4 (the dictionary of tags and prompts in the DB) and M15.5 (common prompt templates) are done;
next — M9,
then M15.6 (only after M12) and M15.7 (a labelled sample).
Read together with `plan/STATUS.md` (what is already done), `plan/BACKLOG.md` § M15
(the criteria) and `docs/ARCHITECTURE.md` § "Tags".

## 1. Why

The owner opened `8.jpg` (a vertical render of a wardrobe) and saw "document" on top, below —
"screenshot", "interior", and only then "furniture 0.25". The task is not to tweak
the prompts for this archive, but to make the tags work on **any** archive, for any owner,
from the first launch, without manual tuning for the contents.

The boundaries: CLIP tags are hints, not an object detector. The originals do not change, the network
— only for
the model weights, personal taste in the prompts must not get into the default.

## 2. What is already done (do not forget it before a start)

M14d was committed as a separate topic ("Do not stay silent, tag filter: the threshold scale and the
counters"):
the tag filter stopped silently showing zero.

- `src/app.rs`: the threshold slider next to the tag picker and in "Settings → Tags";
  the scale 0–100% **of the strongest weight in the whole index**; 100% = that weight;
  an old threshold above the maximum becomes 100% when the index is read, and the status says why;
- `src/app.rs`: each tag in the filter carries the number of shots at the current threshold; an
  empty filter
  explains the threshold and offers "Reset the filter";
- `src/embed.rs`: `max_score()` — the strongest weight among the tags of a file;
- `docs/UI.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`, `plan/STATUS.md` are updated;
- tests: `cargo test` — 108 tests, 104 passed, 4 `#[ignore]`; `clippy` — clean,
  `check.sh` — 39/0.

M15.1 is done too: `derive::clip_inputs` gives the centre crop and the whole frame (letterbox),
`embed`
averages the vectors, `embed --refresh` recomputes what was already computed. The result and the
honest
correction to the criterion — in § M15.1.

**After M15.2:** the filter is stored as a percentile (`tag_show_pct = 70`), and `tag_show = 0.22`
again means only the tag confidence. An old DB migrates when it is read: the confidence 0.22,
the filter 70% — see § M15.2.

## 3. The diagnosis: why `8.jpg` is "document"

`8.jpg` — 433×800, a white render of a wardrobe with shelves. In the DB:

| tag | weight |
|---|---|
| document | 0.276 |
| screenshot | 0.271 |
| interior | 0.266 |
| furniture | 0.255 |

The gap between the first and the second — 0.0047, that is, less than the confidence threshold 0.01.
The app
**does not consider "document" a confident tag**: there is no accent, the list is simply sorted by
weight.
The right answer on the frame is "not sure".

One of the causes is the **centre crop**. `derive::clip_input` took the square `min(w,h)` in the
centre
and stretched it into 224×224. A vertical frame of 433×800 (a preview of 173×320) keeps the white
shelves
and crossbars without a top, a bottom and a watermark — for CLIP that looks like a sheet/an
interface. But
the M15.1 measurement showed that the crop is not the only cause: the whole frame removes "document"
from
the leaders, but "interior" comes forward. The overstated baseline of the common prompts remains
(see § M15.1).

A diagnostic run on the same model (one frame, not a corpus measurement):

- the centre crop (as now): document 0.267, interior 0.261, screenshot 0.259, **furniture 0.254**;
- the whole frame with margins (letterbox): **furniture 0.254**, interior 0.254, screenshot 0.252,
  document 0.252.

Furniture comes first when the model sees the frame whole. The gaps are still tiny —
on such a render the model is unsure — but it proves: **the crop matters more than the prompts**. No
prompt helps if the model sees only white shelves.

The systematics over the corpus: of 717 shots with "furniture" ≥ 0.22, **337** have "document" or
"screenshot"
above furniture, and in all 337 furniture is second. This is not an accident, but an overstated
baseline of the common
prompts "document"/"screenshot" plus the crop. The centre crop and the common prompts are two
different causes,
both have to be treated.

## 4. The decisions (do not tune to the folder)

1. **The default set of tags is common.** No "wardrobe with sliding doors", "dresser", "drawing" from
   this
   archive in the standard prompts. The default: a label + 2–3 common templates, the same for any
   folder.
2. **The model input is an ensemble of frames.** A centre crop + the whole frame with margins
   (letterbox), the vectors
   are averaged and normalized. This treats vertical/wide frames and does not depend on the contents.
3. **The threshold is a percentile per tag**, not a raw cosine. 100% = the strongest shot
   **of this tag**, 0% = all. Then 100% always leaves something, and the scale transfers to any
   folder and any model. On a small folder (< 100 files) — the fallback raw threshold and
   the warning "little data".
4. **The confidence is by the lead.** If the first and the second tags are close (like 0.276 and
   0.271), the card
   explicitly says "tag: not sure", not a highlighted "document". Show the top-3 with the weights.
5. **The dictionary of tags is configurable.** The labels and the prompts are stored in the DB per
   model; the default
   is seeded once; the owner's edits are not overwritten. Own topics ("Wardrobe", "Dresser",
   "Drawing") are added as configuration, not as a hardcode for one archive.
6. **A recompute without a repeated CLIP over the photos.** The image vectors do not depend on the
   prompts. An edit
   of the dictionary recomputes only the text vectors and the weights from the already stored
   `embeddings` —
   that is seconds/a minute, not a full `embed`.
7. **The model is a separate lever.** SigLIP 2 multilingual / CLIP ViT-L/14 give a better quality,
   but that is another size, ONNX, tokenizer and calibration. First the crop and the dictionary, then
   the model.

## 5. The tasks in order

### M15.1 — an ensemble of frames instead of one centre crop ✅ (2026-11-23)

**Done.** `derive::clip_inputs` decodes the preview once and gives two frames: the centre crop and
the whole frame with margins (letterbox, the margins — the mean colour). `embed_thumbs` computes two
vectors per shot,
averages them and normalizes again; the DB holds one vector. `photocat embed --refresh` resets the
vectors of the
active model, otherwise the skip by sha256 does not let it recompute. Tests: the letterbox keeps
the proportions and fills the margins with the mean colour; `clip_inputs` gives two frames; the mean
of two
orthonormal vectors has unit length again.

**Measurements.** The corpus of 2229 files: 176.6 s against 77.3 s (2.3×, within 2.5×), 0 errors.
"furniture" ≥0.22: 717 → 1029; top-1 "document" 266 → 248; a confident tag 1284 → 1285; in 98.4%
of the old confident ones top-1 is the same; the vectors 4.35 MiB, the DB did not grow.

**A correction to the acceptance.** `8.jpg` with the whole frame: the centre — screenshot 0.278 /
document 0.275 /
interior 0.271 / furniture 0.257; the whole frame — interior 0.274 / furniture 0.262 / screenshot
0.261 /
document 0.257; the ensemble — interior 0.278 / screenshot 0.275 / document 0.272 / furniture 0.265.
"Furniture" does not become first. The criterion "furniture first" from one crop is unreachable
without tuning
the prompts, so it is moved: top-1 and "not sure" — M15.3, the prompts — M15.5. A labelled
sample of ≥ 100 shots does not exist yet — that is M15.7; instead the stability of top-1 was
measured on the corpus.

### M15.2 — a percentile per tag instead of a global maximum ✅ (2026-11-23)

**Done.** `settings.tag_show_pct` (0–100, 70 by default) — the filter threshold; the percentile
is counted separately per tag from the distribution of the weights (`embed::tag_distributions`),
"all
tags" — by the best weight of the file. The distributions are built when the tags are loaded, not
every frame.
The tag confidence stayed separate (`tag_show` = 0.22 + a lead of 0.01) and keeps working in
the card, OCR and `{tag}` in the showcase. The migration: if the `tag_show_pct` key is absent,
`tag_show`
returns to 0.22, the filter is set to 70%. Fewer than 100 files — the raw threshold and a warning.

**Measurements** (2229 files): 0% — 2229 shots for each tag; 70% — 669 for each (the thresholds
0.229–0.248), at least one tag passes for 2129 of 2229; 100% — exactly 1 shot for each tag
("furniture" 0.298, "document" 0.327, "children" 0.280, "interior" 0.306). The counters at the tags
match
the filter: both go through `tag_threshold`.

**Acceptance.** Met: at 100% any tag has ≥ 1 shot, at 0% — all, the counters match,
a small folder warns and takes the raw threshold, the tests for the percentiles and the migration are
green.

### M15.3 — an explicit "not sure" in the card ✅ (2026-11-23)

**Done.** If `confident_tag_with` returned `None`, the card writes "tag: not sure — the first weights
are close", does not highlight the first tag, shows the top-3 with the weight and the percentile (the
share of shots with a lower
weight of this tag), the rest — under "all tags (N)". A confident tag has the line "tag: <name>" and
an accent. On a small folder the percentile is not shown (noisy), only the weight remains.

**Acceptance.** `8.jpg` (document 0.271 / interior 0.270, a lead of 0.001) shows "not sure", not
"document"; on an obvious shot one tag is highlighted. Checked by tests without a window through
`egui::Context::run_ui`, `clippy`, `check.sh` and a GUI smoke test.

### M15.4 — a dictionary of tags and prompts in the DB ✅ (2026-11-23)

**Done.** The table `tag_defs(model, tag_id, label, prompts, enabled, position, min_pct)`
(schema 6); `DEFAULT_TAGS` of 14 common labels; the seeding only into an empty table, the owner's
edits
are not overwritten; `embed --retag` recomputes the weights over the stored vectors (2229 files in
1.0–1.2 s); the editor in "Settings → Tags", `photocat tags` (list/add/enable/disable/rm/export/
import/reset); JSON without paths and photos; the old labels that are not in the dictionary stay in
the filter.
The measurement and the acceptance — `plan/STATUS.md` § M15.4, the decision — ADR 0036.

The original criteria:

- The table `tag_defs(model, tag_id, label, prompts_json, enabled, order, min_percentile)` or
  JSON in `settings`; bump `db::SCHEMA_VERSION`, catch the old DBs up.
- The default set in the code; seed only if the model has nothing; do not overwrite the owner's
  edits. A version of the set — so that a default update does not break a custom one.
- Replace `TAG_LABELS`/`PROMPTS_RU`/`PROMPTS_EN` in `app.rs`, `ocr.rs`, `organize.rs` with
  the loaded set. `tags` and `tag_hidden` stay as they are (the key file_id+tag).
- A recompute from the stored `embeddings` without reading the photos again; the "Recompute tags"
  button
  and/or `embed --retag`.
- Add/remove/rename a tag, enable/disable, change the prompts; JSON import/export.
- **Acceptance:** on a test DB add "Wardrobe" with a prompt, recompute, filter;
  the edits survive a restart; the default contains no words from this archive; the roundtrip
  tests.

### M15.5 — a common ensemble of prompts and optional tags ✅ (2026-11-23)

**Done.** All 14 labels have 2–3 common templates in two languages; the tag weight is the best of the
templates
(the average lost on all the metrics). `DEFAULT_TAG_VERSION` = 2, the dictionary fingerprint
(`settings.tag_defs_sig`) updates the default only in an untouched dictionary. "Drawing/scheme" is
not added to the default:
it is a niche, the owner adds it through the dictionary. The measurement on two folders and the
honest
losses of confidence — `plan/STATUS.md` § M15.5; ADR 0036 is updated.

The original criteria:

- 2–3 common templates per label: "a photo of {label}", "the picture shows {label}",
  "a close-up: {label}"; for complex labels keep one precise descriptive prompt.
- Do not add "wardrobe with sliding doors" and the like to the default. "Drawing/scheme" — a common
  category, it can be
  enabled by default or left disabled; decide by the owner.
- Evaluate on two folders, not on one; compare with M15.1/M15.2.
- **Acceptance:** the default is the same for any folder; measurements of top-1/top-3
  before/after; docs.

### M15.6 — a better model and the calibration of the weight ✅ (2026-11-24)

**Done.** SigLIP 2 base 224 multilingual is the fourth catalogue entry
(`onnx-community/siglip2-base-patch16-224-ONNX`, Apache-2.0, 393.3 MiB, 768 features), opt-in: it does
not fit the 250 MiB limit, the owner raises it to 400. Two things had to become data first:

- **the padding side.** SigLIP 2's tokenizer pads to 64 on the right with `<pad>` itself, and the
  pooler reads the last token: with right padding every text turned into one vector (cosine "a
  dog"/"a cat" 0.976) and the tags stopped differing. `pad_left` strips the tokenizer's tail and pads
  on the left, the EOS stays last; the four test frames are then matched 4/4.
- **the calibration.** The stored weight is mapped by the model's own affine map
  `cos * scale + shift` into the CLIP scale, so 0.22 and 0.01 mean the same thing on every model:
  SigLIP 2 `(1.42, 0.14)`, SigLIP 1 `(0.55, 0.23)`, CLIP identity. `tags.source` carries `@v2`, so an
  older calibration is never mixed in; `embed --retag` rewrites the rows in 2–4 s.

**Measured on two folders** (2340 files; the phone corpus + "день рожденья"): confident tags 55.6 %
CLIP / 55.3 % SigLIP 2 / 55.6 % SigLIP 1; documents with text caught 85.9 / 69.0 / 53.5 %; false
"document" on photos without text 3.3 / 9.7 / 1.0 %; 0.063 / 0.170 / 0.173 s per file. SigLIP 1 went
from 0 % confident tags to 55.6 % — the M12 tail is closed. SigLIP 2 is **worse** than CLIP on the
document category and 2.7× slower, so the default stays `clip-vit-b32-multilingual`. Numbers and the
files — `plan/STATUS.md` § M15.6, ADR 0041.

**Left open.** CLIP ViT-L/14 (over 500 MiB) was not tried; neither was the fp16 SigLIP 2 text tower
(538.7 MiB), so it is unknown how much of the loss is int8 quantisation. The old models and vectors
are not mixed: `embeddings` is keyed by `(file_id, model)`, `tags` by `(file_id, tag, source)`, and
the calibration version is part of the source.

### M15.7 — a check on two independent folders

- Do not tune the prompts to `Фотографии с телефона`. The second folder — any other (another
  folder of the owner, a public set), fixed before the experiments.
- The metrics: top-1/top-3 on a labelled sample (≥ 100 shots, including renders/screenshots),
  how many "furniture" shots are found at the 100% / 70% percentile, the embedding time, the DB
  size.
- **Acceptance:** the numbers in `docs/ARCHITECTURE.md` § Measurements with the date, the folder and
  the model;
  an improvement on both, not only on ours.

### M15.8 — documents and the interface

- `docs/UI.md`: the percentile scale, "not sure", the dictionary editor.
- `docs/ARCHITECTURE.md`: the preprocessing, the `tag_defs` schema, the recompute from the vectors.
- `docs/DECISIONS.md`: ADR 0032.
- `plan/STATUS.md`: M15 with the numbers.

## 6. What was already tried and did not work

- **A direct percentage of the cosine (0–100% = 0.00–1.00)** — 100% = 1.00, at 100% it is always
  empty;
  rejected.
- **The model limit 0.50** — 100% = 0.50, on this archive it is empty too (the maximum 0.32);
  rejected by the owner.
- **A global maximum** — the current state: 100% = 0.32, something remains; but for a separate
  tag ("furniture" 0.29) at 100% there may be zero. That is why the next step is a percentile per
  tag.
- **Z-scores per tag** — on `8.jpg` document z≈2.9, furniture z≈1.4; document is still on top.
  The normalization is useful, but alone it does not save; the crop matters more.
- **Prompts for this archive** — forbidden: they do not transfer to other folders.

## 7. Open questions to the owner

1. ~~"Drawing/scheme" — a common default tag or only in the dictionary editor?~~ Decided in M15.5:
   it is not in the default (a niche, not a common category), it is added through the dictionary.
2. How many crops: 2 (the centre + the whole frame) or 3 (also 0.7×)? The price — the embedding
   time.
3. Which second folder for the check? (an independent one is needed, not `Фотографии с телефона`).
4. Recompute the existing vectors automatically on an update or by a button?
5. ~~A bigger model (SigLIP 2 / CLIP-L): how many minutes per corpus and how much space is
   allowed?~~ Answered in M15.6: SigLIP 2 base 224 is 393.3 MiB and 0.170 s per file; it is in the
   catalogue as an opt-in entry (the limit has to be raised to 400 MiB), the default stays the
   multilingual CLIP, CLIP ViT-L/14 was not tried.
6. The current `tag_show = 0.25` — keep it as a percentile or return 0.22/70%?

## 8. Not doing

- Do not hardcode the prompts and the thresholds for `Фотографии с телефона`.
- Do not send photos/paths/prompts to the network; the network — only for the weights with consent.
- Do not delete or rename the archive files.
- Do not promise an "object detector": CLIP tags stay hints, an exact object — a free
  search or a separate model/detector (that is a different class already).
- Do not change the default set of tags without an entry in `docs/UI.md` and `docs/DECISIONS.md`.
