#!/usr/bin/env bash
# Measure photocat on the real corpus. The archive does not change: reads only + a temporary DB.
#   bash bench.sh [--quick]
set -uo pipefail

ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../.." && pwd)
cd -- "$ROOT" || exit 2
BIN="$ROOT/target/release/photocat"
FULL="$HOME/Documents/nasty/mail_media/Фотографии с телефона"
QUICK="$HOME/.cache/pctest/imgs"
WORK="${TMPDIR:-/tmp}/pc-bench"
DB="$WORK/bench.db"

quick=0
[[ ${1:-} == --quick ]] && quick=1

corpus=$FULL
if (( quick )); then
  corpus=$QUICK
  if [[ ! -d $corpus || -z $(ls -A "$corpus" 2>/dev/null) ]]; then
    # a quick corpus: 120 hard links (they take no space), from the main corpus only
    [[ -d $FULL ]] || { echo "the main corpus is missing: $FULL"; exit 2; }
    mkdir -p -- "$corpus"
    n=0
    while IFS= read -r -d '' f; do
      ln -- "$f" "$corpus/$(basename -- "$f")" 2>/dev/null && n=$((n + 1))
      (( n >= 120 )) && break
    done < <(find "$FULL" -maxdepth 1 -type f -name '*.jpg' -print0)
    echo "quick corpus created: $corpus ($n files hard-linked)"
  fi
fi
[[ -d $corpus ]] || { echo "corpus not found: $corpus"; exit 2; }

if [[ ! -x $BIN ]]; then
  echo "building the release…"
  cargo build --release --quiet || exit 2
fi

mkdir -p -- "$WORK"
rm -f -- "$DB" "$DB-wal" "$DB-shm"

files=$(find "$corpus" -type f | wc -l)
bytes=$(du -sb --apparent-size "$corpus" | cut -f1)
echo "corpus: $corpus"
echo "files: $files, size: $bytes bytes"
echo

t0=$(date +%s.%N)
"$BIN" index "$corpus" --jobs 16 --db "$DB" 2>/dev/null | tail -1
t1=$(date +%s.%N)
printf 'indexing time: %.1f s\n' "$(awk -v a="$t0" -v b="$t1" 'BEGIN{print b-a}')"

echo
"$BIN" stats --db "$DB"

echo
t0=$(date +%s.%N)
"$BIN" dupes --db "$DB" --max-dist 6 --quiet 2>/dev/null | head -1
t1=$(date +%s.%N)
printf 'clustering time: %.2f s\n' "$(awk -v a="$t0" -v b="$t1" 'BEGIN{print b-a}')"

echo
echo "the date arrangement plan:"
"$BIN" organize --db "$DB" --out "$WORK/plan" 2>/dev/null | head -14

echo
printf 'DB: %s\n' "$(du -h --apparent-size "$DB" | cut -f1)"
if [[ -d $HOME/.cache/photocat/thumbs ]]; then
  printf 'previews in the cache: %s\n' "$(du -sh "$HOME/.cache/photocat/thumbs" | cut -f1)"
fi
echo
echo "done. Record a row in docs/ARCHITECTURE.md §Measurements (date, corpus, operation, result)."
