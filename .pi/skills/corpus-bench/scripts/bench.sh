#!/usr/bin/env bash
# Замер photocat на реальном корпусе. Архив не изменяется: только чтение + временная БД.
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
  if [[ ! -d $corpus ]]; then
    echo "быстрый корпус отсутствует: $corpus"
    echo "создать: 120 жёстких ссылок из основного корпуса в $corpus"
    exit 2
  fi
fi
[[ -d $corpus ]] || { echo "корпус не найден: $corpus"; exit 2; }

if [[ ! -x $BIN ]]; then
  echo "сборка релиза…"
  cargo build --release --quiet || exit 2
fi

mkdir -p -- "$WORK"
rm -f -- "$DB" "$DB-wal" "$DB-shm"

files=$(find "$corpus" -type f | wc -l)
bytes=$(du -sb --apparent-size "$corpus" | cut -f1)
echo "корпус: $corpus"
echo "файлов: $files, объём: $bytes байт"
echo

t0=$(date +%s.%N)
"$BIN" index "$corpus" --jobs 16 --db "$DB" 2>/dev/null | tail -1
t1=$(date +%s.%N)
printf 'время индексации: %.1f с\n' "$(echo "$t1 - $t0" | bc)"

echo
"$BIN" stats --db "$DB"

echo
t0=$(date +%s.%N)
"$BIN" dupes --db "$DB" --max-dist 6 --quiet 2>/dev/null | head -1
t1=$(date +%s.%N)
printf 'время кластеризации: %.2f с\n' "$(echo "$t1 - $t0" | bc)"

echo
echo "план раскладки по датам:"
"$BIN" organize --db "$DB" --out "$WORK/plan" 2>/dev/null | head -14

echo
printf 'БД: %s\n' "$(du -h --apparent-size "$DB" | cut -f1)"
if [[ -d $HOME/.cache/photocat/thumbs ]]; then
  printf 'превью в кэше: %s\n' "$(du -sh "$HOME/.cache/photocat/thumbs" | cut -f1)"
fi
echo
echo "готово. Записать строку в docs/ARCHITECTURE.md §Замеры (дата, корпус, операция, результат)."
