# Human interaction with the photocat interface

*Русская версия: [vzaimodeystvie.ru.md](vzaimodeystvie.ru.md).*

The normative behaviour rules. The goal is an interface that is **predictable**: a person does not
study the application but does what he already knows. Everything written here is checked by eye and
by a test, not by "feeling".

## 1. Intuitiveness is three things

1. **One model.** A tile = a file, a group = a set of similar files, a face = a person in a snapshot.
   The model does not change from screen to screen: a tile is the same size everywhere, a click
   selects everywhere, the "keep" button means one thing everywhere.
2. **Recognition instead of recall.** The user must not remember where a function lives: it is
   visible on the screen in words. Puzzle icons, hidden menus and "hotkeys you have to know" do not
   count.
3. **Nothing jumps.** The position of navigation, buttons and fields does not change when the view
   changes, when previews load or when progress appears. A new element does not shift the old ones.

If a rule below contradicts "prettier", the rule wins.

## 2. Click and hover

- One click on a tile — selection. It is visible in the same frame (a 2 px outline) and in the status
  at the bottom.
- One click on a button — an action. There are no double clicks, drags or right clicks in the main
  scenario.
- Hover gives **only** a hint (a large preview, the full path, an explanation). No function is
  available only on hover.
- Buttons on a tile ("keep", "not him") are always visible.
- A click target: a secondary button — at least 20 px high, a primary one — 28 px; on a tile a button
  is at least 58×20.
- There are no destructive actions by design: no "delete", "move", "clear". The only confirmations
  are the two places where the disk state changes — "Empty the trash" (the OS trash) and cleaning the
  cache/index (see `docs/UI.md` §Interaction). Do not add others "just in case".

## 3. Response

| Time | What is mandatory |
|---|---|
| up to 100 ms | selection, switching the view, changing the filter, a text search answer (2–3 ms) |
| up to 1 s | duplicate group recomputation (0.03 s), face group rebuild (1.2 s), a semantic search answer (11–50 ms) — no indicator, but the result in the status |
| more than 1 s | indexing, CLIP, OCR, faces, model download — background, `done/total` progress, "Stop" |

- An action is never performed "silently": afterwards the status or a counter changes in the same
  frame.
- Heavy work does not block the window: computation is in a thread, reading the DB is not blocked by
  the pass transaction (see `SCHEMA_VERSION`).
- Progress does not steal focus, does not reset scrolling and does not close the selected file.

## 4. Long work: stop and resume

- Every long pass has a "Stop" next to the progress. A stop finishes the current file and commits
  what is done.
- After a stop the start button shows the remainder: "Resume (315)". A repeated run skips what is
  already done by `sha256`.
- Before a start the **plan** is visible, not "whatever happens": "Recognize documents (435)",
  "Find faces (1972)", "still without faces: 0 of 1972".
- Batch and single runs are separated by words: "Recognize text" (the selected snapshot) and
  "Recognize documents (435)" (a batch). A verb without a scale is forbidden.
- A second identical pass cannot be started: a repeated press is ignored, the button is disabled.

## 5. Errors and empty states

- An error lives in the status line at the bottom: what happened + what to do. One line, no stack.
- An error does not disappear by itself after a second: it stays until the next action.
- There are no modal error windows. Do not introduce exceptions.
- An empty state is text and, if appropriate, one button. Not an empty grid and not a spinner.
- The reason for being unavailable is written next to the button in words: "the model is not
  downloaded", "tesseract does not start — set the path in “Settings”".
- Partial success is shown honestly: "recognized 120 of 435", "errors 1"; the word "done" without
  numbers is not used.

## 6. Reversibility and trust

- Any state change can be undone: hidden tags — "restore the hidden tags (N)", a duplicate decision —
  press "keep" on another tile, a group name — clear the field and save.
- If it cannot be undone, the action is not offered.
- Original files never change; the interface does not promise that in words, it simply does not do
  it.
- A threshold (duplicates, tags, faces) is saved after the slider is released, not on every pixel;
  the result is visible next to it immediately ("groups found: 103"), otherwise the slider is a
  lottery.
- A change of the model, threshold or path explains the consequence: "after changing the model the
  tags must be computed".

## 7. Focus and keyboard

- Focus is always visible: a 2 px outline in the accent colour. Hiding focus is forbidden.
- Tab goes in visual order: top bar → left panel → grid → right panel → status.
- Enter = the main action (search, open); Esc = cancel, reset the search, clear the selection.
- The arrow keys move the selection across the grid, Home/End to the beginning and the end; the wheel
  scrolls.
- Hotkeys are only an addition to a visible button, not a replacement. Ctrl without fanaticism: Ctrl+I
  indexing, Ctrl+F search, F5 refresh.
- The Delete key is **not assigned** and will not be assigned: there is no deletion in the
  application.
- Typing in a field must not start heavy work: search on Enter or a button, not on every letter.

## 8. Switching views and keeping state

- The navigation items do not change place or order: "All files", "Albums", "Calendar", "Faces",
  "Duplicates and series", "Integrity", "Trash".
- When returning to a view, the following is kept: the selected file, the filter, the threshold, the
  selected face group, the calendar month, the search results.
- Switching the search mode (semantic ↔ by text) resets the results and says so in the status; the
  query line stays so it does not have to be typed again.
- A new view does not open with an already running heavy pass "by itself": only on a button.

## 9. Interface language

- One notion — one word. The vocabulary: **file** (an index entry), **snapshot** (a photo), **video**,
  **tile**, **group** (duplicates or faces), **face**, **tag**, **model**, **pass** (a long
  operation), **index** (the DB).
- A button is an infinitive: "Index", "Find groups", "Save the name".
- A status is a fact + a number + a unit: "indexed 2230/2230 files (21.2 GiB), errors 0, in 64.9 s".
- Forbidden: "file(s)", "pcs.", "Success!", exclamation marks, "Oops", "Please wait".
- Technical jargon is explained on the spot: not "FTS5" but "search by text on a photo"; not
  "embedding" but "tags" / "semantic search".

## 10. Feedback for ML decisions

- Confidence is shown as a number, not only as colour: `tag: children (0.24)`, `0.93` for a face.
- "Not sure" is a normal state: all tags with their weights, none highlighted, the file does not get
  into the filter.
- A human mistake is corrected in one click where it is visible: "not him" on a face tile, "remove"
  on a tag.
- After a correction the result is visible immediately: the face moved to a new group, the tag
  disappeared from the card and the filter, the counters were recomputed.
- A guess is not passed off as a fact: a date with its source (`exif` / `filename` / `mtime`), a tag
  with its weight, a face with the detection confidence.

## 11. What to check on a live person (5 minutes)

Give the owner five tasks without prompting, and record the time to the first correct click:

1. "Index this folder" → the folder field is found in ≤ 5 s.
2. "Find all duplicates" → the "Duplicates and series" view and "Find groups" in ≤ 5 s.
3. "Show the snapshots from 12 May 2024" → "Calendar" in ≤ 10 s.
4. "Find Lisa" → "Faces" and the group in ≤ 10 s.
5. "Find the receipt from the bank" → the search and the "by text on the photo" mode in ≤ 10 s.

A failure of any task is a findability defect, not "the user did not figure it out".
