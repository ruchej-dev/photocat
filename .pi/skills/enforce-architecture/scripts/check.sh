#!/usr/bin/env bash
# Check the architectural boundaries of photocat.
#   bash check.sh           — grep only, a few seconds
#   bash check.sh --build   — plus cargo check
set -uo pipefail

# scripts/ → enforce-architecture/ → skills/ → .pi/ → project root
HERE=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
ROOT=$(cd -- "$HERE/../../../.." && pwd)
cd -- "$ROOT" || exit 2

fail=0
ok=0
note() { printf '  %-6s %s\n' "$1" "$2"; }
err()  { printf '  ERROR %s\n' "$1"; fail=$((fail + 1)); }
pass() { ok=$((ok + 1)); }

echo "photocat: boundary check ($ROOT)"

# ---------------------------------------------------------------- 1. layers
declare -A ALLOWED=(
  [derive]="tools"
  [db]="tools i18n"
  [tools]="i18n"
  [i18n]=""
  [index]="derive db tools i18n"
  [dupes]="db derive i18n"
  [organize]="db derive i18n"
  [recycle]="db i18n"
  [storage]="db"
  [verify]="db derive i18n"
  [embed]="derive db tools i18n"
  [eval]="db embed"
  [ocr]="derive db tools i18n"
  [faces]="derive db embed i18n"
  [player]=""
  [app]="i18n derive db index dupes organize verify embed eval ocr faces player tools recycle storage"
  [main]="i18n derive db index dupes organize verify embed eval ocr faces player app tools"
)

echo "1. dependencies between modules"
for f in src/*.rs; do
  mod=$(basename "$f" .rs)
  [[ -v ALLOWED[$mod] ]] || { err "module src/$mod.rs is not registered in the layer map of check.sh"; continue; }
  refs=$(grep -oE 'crate::[a-z_][a-z0-9_]*' "$f" 2>/dev/null | cut -d: -f3 | sort -u | grep -vx "$mod" || true)
  for r in $refs; do
    if [[ " ${ALLOWED[$mod]} " == *" $r "* ]]; then
      pass
    else
      err "src/$mod.rs uses crate::$r, which the layers forbid"
    fi
  done
done
(( fail == 0 )) && note "ok" "all references stay inside the layers"

# ---------------------------------------------------------------- 2. egui
echo "2. egui only in app.rs"
bad=$(grep -lE '\b(egui|eframe)::|use eframe|use egui' src/*.rs 2>/dev/null | grep -v '^src/app.rs$' || true)
if [[ -n $bad ]]; then
  err "egui found outside src/app.rs: $(echo "$bad" | tr '\n' ' ')"
else
  pass
  note "ok" "the core does not know about the interface"
fi

# ---------------------------------------------------------------- 3. archive files
echo "3. no deleting or renaming files"
# The exception is src/storage.rs: it deletes only the contents of <cache>/photocat (the path is checked inside).
bad=$(grep -nE '\b(fs::remove_file|fs::remove_dir|fs::remove_dir_all|fs::rename|remove_file|remove_dir_all)' src/*.rs 2>/dev/null | grep -v '^src/storage.rs:' || true)
if [[ -n $bad ]]; then
  err "src/ deletes or renames files (ADR 0005):"
  echo "$bad" | sed 's/^/      /'
else
  pass
  note "ok" "the archive is not deleted or renamed; the cache — only storage.rs"
fi

# ---------------------------------------------------------------- 4. network
echo "4. the network only for models"
# One HTTP client, the built-in one (ADR 0047), and only in the downloader: a second client or a
# request anywhere else means some code the owner never consented to talks to the internet.
bad=$(grep -nE '^(reqwest|hyper|curl|isahc|attohttpc)\s*=' Cargo.toml 2>/dev/null || true)
if [[ -n $bad ]]; then
  err "a second HTTP client in Cargo.toml: the weights are fetched by ureq in src/embed.rs (ADR 0047): $bad"
else
  pass
fi
bad=$(grep -nE '^ureq\s*=' Cargo.toml >/dev/null 2>&1 || echo "ureq is not in Cargo.toml")
if [[ $bad == "ureq is not in Cargo.toml" ]]; then
  err "the downloader needs an HTTP client (ADR 0047)"
else
  pass
fi
bad=$(grep -nE 'Command::new\("(curl|ffmpeg|ffprobe|tesseract)"\)' src/*.rs 2>/dev/null \
  | grep -v '^src/tools.rs:' || true)
if [[ -n $bad ]]; then
  err "an external program is started outside src/tools.rs (ADR 0019):"
  echo "$bad" | sed 's/^/      /'
else
  pass
fi
# The HTTP client lives in the downloader only.
bad=$(grep -nE '\bureq::|\bureq\b' src/*.rs 2>/dev/null | grep -v '^src/embed.rs:' || true)
if [[ -n $bad ]]; then
  err "ureq is used outside src/embed.rs (ADR 0047):"
  echo "$bad" | sed 's/^/      /'
else
  pass
fi
# Only GET: a weight file is read, nothing about the owner is sent.
bad=$(grep -nE '\.(post|put|delete|patch)\s*\(' src/embed.rs 2>/dev/null || true)
if [[ -n $bad ]]; then
  err "the downloader can send data (ADR 0015):"
  echo "$bad" | sed 's/^/      /'
else
  pass
  note "ok" "the network is only for models, no data is sent"
fi

# ---------------------------------------------------------------- 5. dependencies
echo "5. every dependency is described in docs/DECISIONS.md"
missing=""
while read -r dep; do
  [[ -z $dep || $dep == "photocat" ]] && continue
  grep -qi "\b${dep}\b" docs/DECISIONS.md docs/ARCHITECTURE.md 2>/dev/null || missing+=" $dep"
done < <(sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml | grep -oE '^[a-z0-9_-]+' | sort -u)
if [[ -n $missing ]]; then
  printf '  WARNING not described: %s\n' "$missing"
else
  pass
  note "ok" "all dependencies are explained"
fi

# ---------------------------------------------------------------- 6. source language
# Identifiers, comments and the source text of the interface are English (AGENTS.md §6). The full
# scan lives in tests/source_language.rs; here it runs without a build, and a Cyrillic letter in a
# declaration is caught even before the test is compiled.
echo "6. the code speaks English"
bad=$(grep -nP '\b(fn|let|struct|enum|trait|const|static|mod|type)\s+(mut\s+)?[A-Za-z0-9_]*[\x{0400}-\x{04FF}]' src/*.rs 2>/dev/null || true)
if [[ -n $bad ]]; then
  err "a Cyrillic identifier in src/ (AGENTS.md §6):"
  echo "$bad" | sed 's/^/      /'
else
  pass
fi
if cargo test --quiet --test source_language >/tmp/photocat-source-language.txt 2>&1; then
  pass
  note "ok" "identifiers, comments, strings and test messages are English"
else
  err "tests/source_language.rs does not pass:"
  tail -12 /tmp/photocat-source-language.txt | sed 's/^/      /'
fi

# ---------------------------------------------------------------- 7. module budget
# ADR 0053: production code ≤ 700 lines per file, an `impl` block ≤ 800, a function ≤ 200; a module
# owns one screen or one pass. The list below holds today's offenders with the line count they had
# when they were written down. It only shrinks: an item that grew past its number fails the check,
# and so does an entry whose item is now under the budget (remove the entry instead of keeping it).
BUDGET_EXEMPT=(
  "src/app.rs|file|-|18381"
  "src/app.rs|impl|App|9061"
  "src/app.rs|impl|eframe::App for App|2935"
  "src/app.rs|fn|ui|2933"
  "src/app.rs|fn|viewer_modal|855"
  "src/app.rs|fn|file_card_content|656"
  "src/app.rs|fn|paint_icon|475"
  "src/app.rs|fn|roots_modal|374"
  "src/app.rs|fn|draw|305"
  "src/app.rs|fn|integrity_panel|305"
  "src/app.rs|fn|reload|282"
  "src/app.rs|fn|settings_model|236"
  "src/app.rs|fn|settings_tag_dictionary|202"
  "src/db.rs|file|-|3243"
  "src/db.rs|fn|init|271"
  "src/derive.rs|file|-|776"
  "src/embed.rs|file|-|1928"
  "src/faces.rs|file|-|1249"
  "src/i18n.rs|file|-|2352"
  "src/main.rs|file|-|1847"
  "src/main.rs|fn|main|1177"
  "src/main.rs|fn|albums_cli|294"
  "src/organize.rs|file|-|1484"
  "src/player.rs|file|-|1594"
  "src/verify.rs|file|-|1104"
)
echo "7. the module budget: ≤ 700 lines per file, ≤ 800 per impl, ≤ 200 per fn"
declare -A budget_lines=() budget_ceiling=() budget_seen=()
while IFS=$'\t' read -r path kind name lines; do
  key="$path|$kind|$name"
  (( ${budget_lines[$key]:-0} > lines )) || budget_lines[$key]=$lines
done < <(find src -name '*.rs' | sort | while IFS= read -r f; do awk -v file="$f" -f "$HERE/budget.awk" "$f"; done)
for entry in "${BUDGET_EXEMPT[@]}"; do
  budget_ceiling["${entry%|*}"]=${entry##*|}
done
over=0
budget_fail=0
for key in "${!budget_lines[@]}"; do
  lines=${budget_lines[$key]}
  kind=${key#*|}; kind=${kind%%|*}
  case $kind in
    file) budget=700 ;;
    impl) budget=800 ;;
    fn) budget=200 ;;
  esac
  (( lines > budget )) || continue
  over=$((over + 1))
  budget_seen[$key]=1
  if [[ ! -v budget_ceiling[$key] ]]; then
    budget_fail=$((budget_fail + 1))
    err "over the budget and not exempt: ${key//|/ } — $lines lines (limit $budget)"
  elif (( lines > budget_ceiling[$key] )); then
    budget_fail=$((budget_fail + 1))
    err "${key//|/ } grew: $lines lines, the exempt count is ${budget_ceiling[$key]}"
  fi
done
for key in "${!budget_ceiling[@]}"; do
  if [[ ! -v budget_seen[$key] ]]; then
    if [[ -v budget_lines[$key] ]]; then
      budget_fail=$((budget_fail + 1))
      err "${key//|/ } is under the budget now (${budget_lines[$key]} lines): remove the BUDGET_EXEMPT entry"
    else
      budget_fail=$((budget_fail + 1))
      err "BUDGET_EXEMPT names ${key//|/ }, which no longer exists: update or remove the entry"
    fi
  fi
done
if (( budget_fail == 0 )); then
  pass
  note "ok" "$over items over the budget, all written down in BUDGET_EXEMPT (the list only shrinks)"
fi

# ---------------------------------------------------------------- 8. build
if [[ ${1:-} == --build ]]; then
  echo "8. cargo check"
  if cargo check --all-targets --quiet 2>&1 | tail -5; then
    pass
    note "ok" "the build passes"
  else
    err "cargo check does not pass"
  fi
fi

echo
if (( fail == 0 )); then
  echo "RESULT: boundaries are in order ($ok checks)"
  exit 0
fi
echo "RESULT: $fail violations — the task is not closed"
exit 1
