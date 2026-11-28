# Measure the module budget of one Rust file (AGENTS.md §3, ADR 0053).
#
#   awk -v file=src/app.rs -f budget.awk src/app.rs
#
# Output, one record per line, tab separated:
#   file  -   <production lines>
#   impl  <name>  <lines>
#   fn    <name>  <lines>
#
# Production code is everything before the top-level `#[cfg(test)] mod tests` block, and a
# `tests.rs` module counts as nothing at all: the budget is about what a reader has to hold in
# mind, not about the tests.
# Strings, char literals and comments are stripped before braces are counted, so a
# `'{'` in a message cannot open a block.

function clean(s,   i, n, c, j, h, p, closer) {
  n = length(s); i = 1; out = ""
  if (in_block) {
    p = index(s, "*/")
    if (p == 0) return ""
    i = p + 2; in_block = 0
  }
  while (i <= n) {
    c = substr(s, i, 1)
    if (c == "/" && substr(s, i + 1, 1) == "/") break
    if (c == "/" && substr(s, i + 1, 1) == "*") {
      p = index(substr(s, i + 2), "*/")
      if (p == 0) { in_block = 1; break }
      i = i + 2 + p + 1; continue
    }
    if (c == "\"") {
      # a raw string r"…", r#"…"#; otherwise a normal one with escapes
      j = i - 1; h = 0
      while (j >= 1 && substr(s, j, 1) == "#") { h++; j-- }
      if (j >= 1 && substr(s, j, 1) == "r") {
        closer = "\""
        for (p = 0; p < h; p++) closer = closer "#"
        p = index(substr(s, i + 1), closer)
        if (p == 0) return out
        i = i + p + length(closer); continue
      }
      i++
      while (i <= n) {
        c = substr(s, i, 1)
        if (c == "\\") { i += 2; continue }
        if (c == "\"") { i++; break }
        i++
      }
      continue
    }
    if (c == "'") {
      # a char literal ('x', '\n', '{') is not a lifetime ('a)
      if (substr(s, i + 1, 1) != "\\" && substr(s, i + 2, 1) == "'") { i += 3; continue }
      if (substr(s, i + 1, 1) == "\\") {
        p = index(substr(s, i + 2), "'")
        if (p > 0 && p <= 12) { i = i + 2 + p; continue }
      }
      i++; continue
    }
    out = out c; i++
  }
  return out
}

function braces(s,   i, n, c, o, cl) {
  n = length(s); o = 0; cl = 0
  for (i = 1; i <= n; i++) {
    c = substr(s, i, 1)
    if (c == "{") o++
    else if (c == "}") cl++
  }
  return o - cl
}

{ L[NR] = $0 }
END {
  n = NR
  prod = n
  for (i = 1; i < n; i++)
    if (L[i] ~ /^#\[cfg\(test\)\]$/ && L[i + 1] ~ /^mod [A-Za-z0-9_]*tests?[A-Za-z0-9_]* \{$/)
      prod = i - 1
  # A `tests.rs` module is test code from the first line: the parent module carries the marker.
  if (file ~ /(^|\/)tests\.rs$/) prod = 0

  printf "%s\tfile\t-\t%d\n", file, prod

  depth = 0; top = 0; in_block = 0
  for (i = 1; i <= prod; i++) {
    line = clean(L[i])
    kind = ""
    if (line ~ /^[ \t]*((pub(\([^)]*\))?|default|unsafe)[ \t]+)*impl[ \t<{]/) {
      kind = "impl"
      rest = line
      sub(/^[ \t]*((pub(\([^)]*\))?|default|unsafe)[ \t]+)*impl/, "", rest)
      sub(/[ \t]*\{.*$/, "", rest)
      sub(/^[ \t]+/, "", rest)
      sub(/^<[^>]*>[ \t]*/, "", rest)
      gsub(/[ \t]+/, " ", rest)
      name = rest
    } else if (line ~ /^[ \t]*((pub(\([^)]*\))?|const|async|unsafe|extern[ \t]+"[^"]*")[ \t]+)*fn[ \t]+[A-Za-z0-9_]+/) {
      kind = "fn"
      match(line, /fn[ \t]+[A-Za-z0-9_]+/)
      name = substr(line, RSTART, RLENGTH)
      sub(/^fn[ \t]+/, "", name)
    }
    if (kind != "") {
      top++
      st_kind[top] = kind; st_name[top] = name; st_start[top] = i; st_base[top] = depth
    }
    depth += braces(line)
    while (top > 0 && depth <= st_base[top]) {
      printf "%s\t%s\t%s\t%d\n", file, st_kind[top], st_name[top], i - st_start[top] + 1
      top--
    }
  }
}
