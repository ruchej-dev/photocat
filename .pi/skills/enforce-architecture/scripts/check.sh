#!/usr/bin/env bash
# Check the architectural boundaries of photocat.
#   bash check.sh           — grep only, a few seconds
#   bash check.sh --build   — plus cargo check
set -uo pipefail

# scripts/ → enforce-architecture/ → skills/ → .pi/ → project root
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../.." && pwd)
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
bad=$(grep -nE '^(reqwest|hyper|ureq|curl|isahc|attohttpc)\s*=' Cargo.toml 2>/dev/null || true)
if [[ -n $bad ]]; then
  err "an HTTP client in Cargo.toml: the network only through a curl process (ADR 0016): $bad"
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
# curl is allowed only to the weight downloader; the launch itself lives in tools.rs.
bad=$(grep -nE 'command\(Tool::Curl\)' src/*.rs 2>/dev/null | grep -v -e '^src/embed.rs:' -e '^src/tools.rs:' || true)
if [[ -n $bad ]]; then
  err "curl is started outside src/embed.rs (ADR 0016):"
  echo "$bad" | sed 's/^/      /'
else
  pass
fi
# Upload flags are forbidden in the downloader: only GET requests for weights leave the machine.
bad=$(grep -nE '\s(-F|--data|-T|--upload-file)\s' src/embed.rs 2>/dev/null || true)
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

# ---------------------------------------------------------------- 6. build
if [[ ${1:-} == --build ]]; then
  echo "6. cargo check"
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
