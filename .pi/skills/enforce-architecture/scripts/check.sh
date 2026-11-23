#!/usr/bin/env bash
# Проверка архитектурных границ photocat.
#   bash check.sh           — только grep, секунды
#   bash check.sh --build   — плюс cargo check
set -uo pipefail

# scripts/ → enforce-architecture/ → skills/ → .pi/ → корень проекта
ROOT=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")/../../../.." && pwd)
cd -- "$ROOT" || exit 2

fail=0
ok=0
note() { printf '  %-6s %s\n' "$1" "$2"; }
err()  { printf '  ОШИБКА %s\n' "$1"; fail=$((fail + 1)); }
pass() { ok=$((ok + 1)); }

echo "photocat: проверка границ ($ROOT)"

# ---------------------------------------------------------------- 1. слои
declare -A ALLOWED=(
  [derive]="tools"
  [db]="tools"
  [tools]=""
  [index]="derive db tools"
  [dupes]="db derive"
  [organize]="db derive"
  [recycle]="db"
  [storage]="db"
  [verify]="db derive"
  [embed]="derive db tools"
  [ocr]="derive db tools"
  [faces]="derive db embed"
  [player]=""
  [app]="derive db index dupes organize embed ocr faces player tools recycle storage"
  [main]="derive db index dupes organize verify embed ocr faces player app tools"
)

echo "1. зависимости между модулями"
for f in src/*.rs; do
  mod=$(basename "$f" .rs)
  [[ -v ALLOWED[$mod] ]] || { err "модуль src/$mod.rs не зарегистрирован в карте слоёв check.sh"; continue; }
  refs=$(grep -oE 'crate::[a-z_][a-z0-9_]*' "$f" 2>/dev/null | cut -d: -f3 | sort -u | grep -vx "$mod" || true)
  for r in $refs; do
    if [[ " ${ALLOWED[$mod]} " == *" $r "* ]]; then
      pass
    else
      err "src/$mod.rs использует crate::$r, а слои это запрещают"
    fi
  done
done
(( fail == 0 )) && note "ок" "все ссылки внутри слоёв"

# ---------------------------------------------------------------- 2. egui
echo "2. egui только в app.rs"
bad=$(grep -lE '\b(egui|eframe)::|use eframe|use egui' src/*.rs 2>/dev/null | grep -v '^src/app.rs$' || true)
if [[ -n $bad ]]; then
  err "egui найден вне src/app.rs: $(echo "$bad" | tr '\n' ' ')"
else
  pass
  note "ок" "ядро не знает про интерфейс"
fi

# ---------------------------------------------------------------- 3. файлы архива
echo "3. запрет удаления и переименования файлов"
# Исключение — src/storage.rs: он удаляет только содержимое <кэш>/photocat (проверка пути внутри).
bad=$(grep -nE '\b(fs::remove_file|fs::remove_dir|fs::remove_dir_all|fs::rename|remove_file|remove_dir_all)' src/*.rs 2>/dev/null | grep -v '^src/storage.rs:' || true)
if [[ -n $bad ]]; then
  err "в src/ есть удаление/переименование (ADR 0005):"
  echo "$bad" | sed 's/^/      /'
else
  pass
  note "ок" "архив не удаляется и не переименовывается; кэш — только storage.rs"
fi

# ---------------------------------------------------------------- 4. сеть
echo "4. сеть только для моделей"
bad=$(grep -nE '^(reqwest|hyper|ureq|curl|isahc|attohttpc)\s*=' Cargo.toml 2>/dev/null || true)
if [[ -n $bad ]]; then
  err "HTTP-клиент в Cargo.toml: сеть только через curl-процесс (ADR 0016): $bad"
else
  pass
fi
bad=$(grep -nE 'Command::new\("(curl|ffmpeg|ffprobe|tesseract)"\)' src/*.rs 2>/dev/null \
  | grep -v '^src/tools.rs:' || true)
if [[ -n $bad ]]; then
  err "внешняя программа запускается мимо src/tools.rs (ADR 0019):"
  echo "$bad" | sed 's/^/      /'
else
  pass
fi
# curl разрешён только загрузчику весов; сам запуск живёт в tools.rs.
bad=$(grep -nE 'command\(Tool::Curl\)' src/*.rs 2>/dev/null | grep -v -e '^src/embed.rs:' -e '^src/tools.rs:' || true)
if [[ -n $bad ]]; then
  err "curl запускается вне src/embed.rs (ADR 0016):"
  echo "$bad" | sed 's/^/      /'
else
  pass
fi
# upload-флаги в загрузчике запрещены: наружу уходят только GET-запросы за весами
bad=$(grep -nE '\s(-F|--data|-T|--upload-file)\s' src/embed.rs 2>/dev/null || true)
if [[ -n $bad ]]; then
  err "загрузчик умеет отправлять данные (ADR 0015):"
  echo "$bad" | sed 's/^/      /'
else
  pass
  note "ок" "сеть только за моделями, без отправки данных"
fi

# ---------------------------------------------------------------- 5. зависимости
echo "5. каждая зависимость описана в docs/DECISIONS.md"
missing=""
while read -r dep; do
  [[ -z $dep || $dep == "photocat" ]] && continue
  grep -qi "\b${dep}\b" docs/DECISIONS.md docs/ARCHITECTURE.md 2>/dev/null || missing+=" $dep"
done < <(sed -n '/^\[dependencies\]/,/^\[/p' Cargo.toml | grep -oE '^[a-z0-9_-]+' | sort -u)
if [[ -n $missing ]]; then
  printf '  ВНИМАНИЕ не описаны: %s\n' "$missing"
else
  pass
  note "ок" "все зависимости объяснены"
fi

# ---------------------------------------------------------------- 6. сборка
if [[ ${1:-} == --build ]]; then
  echo "6. cargo check"
  if cargo check --all-targets --quiet 2>&1 | tail -5; then
    pass
    note "ок" "сборка проходит"
  else
    err "cargo check не проходит"
  fi
fi

echo
if (( fail == 0 )); then
  echo "ИТОГ: границы в порядке ($ok проверок)"
  exit 0
fi
echo "ИТОГ: нарушений $fail — задача не закрыта"
exit 1
