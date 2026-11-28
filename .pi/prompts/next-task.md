---
description: "Take the next photocat task from the plan and drive it to verified numbers and an updated plan"
argument-hint: "[task ID — empty takes the one named in plan/STATUS.md]"
---

We are continuing the photocat project. Task: ${@:-the one recorded in `plan/STATUS.md` as `next:`}.

Order, do not reorder:

1. Read `plan/STATUS.md`, `plan/BACKLOG.md` (your block), `docs/ARCHITECTURE.md`, `docs/UI.md` — and
   only then the code the task names. If a document is still Russian (`plan/I18N.md`), the Russian
   original is the source of truth.
2. Restate the task in one paragraph: what changes in behaviour, for whom, how we verify it. If
   `plan/BACKLOG.md` has no acceptance criteria, write them there first.
3. If the task is about the interface — follow `.pi/skills/ui-design` and `.pi/skills/add-view`.
   If it is about layers or dependencies — `.pi/skills/enforce-architecture`.
4. Make the minimal change that meets the criteria without widening the scope.
5. Verify: `cargo clippy --all-targets`, `bash .pi/skills/enforce-architecture/scripts/check.sh`,
   then measure via `.pi/skills/corpus-bench` (release!), then, if the task touches the UI, a short
   GUI run **after warning the owner**.
6. Record the result: `docs/ARCHITECTURE.md` §Measurements, `docs/UI.md` if a screen changed,
   `plan/STATUS.md` (STATE + "Next"), `plan/BACKLOG.md` (move what is done to ✅), `plan/I18N.md` if
   the task translated a screen.
7. One commit per topic, message in English: what changed in behaviour. Push to `origin/main`.

Constraints that must not be broken on the way: originals are never deleted or renamed; `egui` only
in `src/app.rs`; no network; heavy work in a background thread with progress; UI labels go through
`i18n::t(lang, "…")` and the English text lands in `src/i18n/terms/`.

At the end of the answer: what was done, which numbers were obtained, what is left and what the next
step of the plan is.
