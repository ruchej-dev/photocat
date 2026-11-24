//! The repository speaks English (AGENTS.md §6): identifiers, comments and the source text of the
//! interface are English, and Cyrillic is allowed only as a translation.
//!
//! The check reads the sources, so it does not depend on the build: a Russian test name, a Russian
//! comment or a plain Russian label in the code fails before a reviewer has to spot it by eye.
//!
//! `PENDING` is the honest list of modules that still hold Russian comments or strings; it shrinks
//! with every step of the refactor and must end up empty. The list is checked in both directions:
//! a module that still has Russian must be named, a module named while already clean fails too.

use std::fs;
use std::path::Path;

/// Modules that still carry Russian comments or strings (the refactor works them off one by one).
const PENDING: &[&str] = &[
    "app.rs",
    "embed.rs",
    "main.rs",
    "player.rs",
];

/// Files whose string literals are legitimate Russian *data*: the translation table itself and the
/// prompts for the multilingual model. Their comments are still English like everywhere else.
const RUSSIAN_DATA: &[&str] = &[
    "i18n.rs",
    // The prompts of the multilingual CLIP model and its default tag labels.
    "embed.rs",
    // `labels.csv` is the owner's file: the parser keeps accepting the old Russian keywords
    // `пропуск` and `другое` next to `skip` and `other`.
    "eval.rs",
    // The CLI still accepts the Russian aliases `день`/`месяц`/`событие` for `--by`.
    "organize.rs",
];

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// Split a Rust source into code, comments and string literals. Raw strings (`r#"…"#`), byte and C
/// strings, char literals, lifetimes, line comments and nested block comments are handled; nothing
/// else in the language looks like a quote.
fn split(src: &str) -> (String, String, String) {
    let b: Vec<char> = src.chars().collect();
    let (mut code, mut comments, mut strings) = (String::new(), String::new(), String::new());
    let mut i = 0usize;
    let at = |i: usize| b.get(i).copied();
    while i < b.len() {
        // line comment
        if at(i) == Some('/') && at(i + 1) == Some('/') {
            let start = i;
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            comments.extend(&b[start..i]);
            code.push('\n');
            continue;
        }
        // block comment, nested
        if at(i) == Some('/') && at(i + 1) == Some('*') {
            let start = i;
            let mut depth = 0usize;
            while i < b.len() {
                if at(i) == Some('/') && at(i + 1) == Some('*') {
                    depth += 1;
                    i += 2;
                } else if at(i) == Some('*') && at(i + 1) == Some('/') {
                    depth -= 1;
                    i += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    i += 1;
                }
            }
            comments.extend(&b[start..i]);
            code.push('\n');
            continue;
        }
        // raw string: r"…", r#"…"#, br#"…"#, cr#"…"#
        let prefix = {
            let rest: String = b[i..(i + 4).min(b.len())].iter().collect();
            ["br", "r", "cr"]
                .iter()
                .find(|p| rest.starts_with(*p))
                .map(|p| p.len())
        };
        if let Some(plen) = prefix {
            let mut j = i + plen;
            let mut hashes = 0usize;
            while at(j) == Some('#') {
                hashes += 1;
                j += 1;
            }
            if at(j) == Some('"') {
                j += 1;
                loop {
                    if j >= b.len() {
                        break;
                    }
                    if at(j) == Some('"') {
                        let mut k = j + 1;
                        let mut seen = 0usize;
                        while seen < hashes && at(k) == Some('#') {
                            seen += 1;
                            k += 1;
                        }
                        if seen == hashes {
                            j = k;
                            break;
                        }
                    }
                    j += 1;
                }
                strings.extend(&b[i..j]);
                code.push(' ');
                i = j;
                continue;
            }
        }
        // byte/C string and plain string: "…", b"…", c"…"
        let quote_at = if at(i) == Some('"') {
            Some(i)
        } else if matches!(at(i), Some('b') | Some('c')) && at(i + 1) == Some('"') {
            Some(i + 1)
        } else {
            None
        };
        if let Some(q) = quote_at {
            let start = i;
            let mut j = q + 1;
            while j < b.len() {
                match b[j] {
                    '\\' => j += 2,
                    '"' => {
                        j += 1;
                        break;
                    }
                    _ => j += 1,
                }
            }
            strings.extend(&b[start..j.min(b.len())]);
            code.push(' ');
            i = j;
            continue;
        }
        // char literal `'x'`, `'\n'`, `'\u{1F600}'`; anything else is a lifetime and stays code
        if at(i) == Some('\'') {
            let mut j = i + 1;
            if at(j) == Some('\\') {
                j += 2;
                while j < b.len() && b[j] != '\'' {
                    j += 1;
                }
                j += 1;
            } else {
                j += 1;
                if at(j) == Some('\'') {
                    j += 1;
                }
            }
            if j <= b.len() && j > i + 1 && b.get(j - 1) == Some(&'\'') {
                strings.extend(&b[i..j]);
                code.push(' ');
                i = j;
                continue;
            }
        }
        code.push(b[i]);
        i += 1;
    }
    (code, comments, strings)
}

/// Words (identifier tokens) in a code chunk.
fn words(code: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start: Option<usize> = None;
    for (i, c) in code.char_indices() {
        let is_word = c.is_alphanumeric() || c == '_';
        match (is_word, start) {
            (true, None) => start = Some(i),
            (false, Some(s)) => {
                out.push(&code[s..i]);
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        out.push(&code[s..]);
    }
    out
}

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    for entry in fs::read_dir(&dir).expect("src/ is readable") {
        let path = entry.expect("a directory entry").path();
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .expect("a file name")
            .to_string();
        out.push((name, fs::read_to_string(&path).expect("a source file is readable")));
    }
    out.sort();
    out
}

#[test]
fn identifiers_are_english() {
    let mut bad = Vec::new();
    for (name, src) in sources() {
        let (code, _, _) = split(&src);
        for word in words(&code) {
            if word.chars().any(is_cyrillic) {
                bad.push(format!("{name}: `{word}`"));
            }
        }
    }
    assert!(
        bad.is_empty(),
        "identifiers must be English (AGENTS.md §6): {bad:?}"
    );
}

#[test]
fn converted_modules_hold_no_russian() {
    let mut bad = Vec::new();
    let mut pending_honest = Vec::new();
    for (name, src) in sources() {
        let pending = PENDING.contains(&name.as_str());
        // A test may quote the Russian translation or the owner's words: those literals are
        // expected values and fixtures, not interface text. Only the code before `#[cfg(test)]`
        // is held to the rule; comments are checked in the tests too.
        let production = match src.split_once("#[cfg(test)]") {
            Some((head, _)) => head,
            None => src.as_str(),
        };
        let (_, comments, _) = split(&src);
        let (_, _, strings) = split(production);
        let has_russian = comments.chars().any(is_cyrillic)
            || (strings.chars().any(is_cyrillic) && !RUSSIAN_DATA.contains(&name.as_str()));
        if pending {
            if !has_russian {
                pending_honest.push(name.clone());
            }
            continue;
        }
        if comments.chars().any(is_cyrillic) {
            bad.push(format!("{name}: a Russian comment"));
        }
        if strings.chars().any(is_cyrillic) && !RUSSIAN_DATA.contains(&name.as_str()) {
            bad.push(format!("{name}: a Russian string outside the translation table"));
        }
    }
    assert!(
        bad.is_empty(),
        "Russian belongs in src/i18n.rs (AGENTS.md §6): {bad:?}"
    );
    assert!(
        pending_honest.is_empty(),
        "these modules are already clean — drop them from PENDING: {pending_honest:?}"
    );
}
