//! The repository speaks English (AGENTS.md §6): identifiers, comments and the source text of the
//! interface are English, and Cyrillic is allowed only as a translation.
//!
//! The check reads the sources, so it does not depend on the build: a Russian test name, a Russian
//! comment or a plain Russian label in the code fails before a reviewer has to spot it by eye.
//!
//! `PENDING` is the honest list of modules that still hold Russian comments or strings; it shrinks
//! with every step of the refactor and must end up empty. The list is checked in both directions:
//! a module that still has Russian must be named, a module named while already clean fails too.
//!
//! Test *messages* are English too (M23.5): a Russian assertion message, `panic!` text, `expect`
//! text or `#[ignore]` reason fails. What a test compares stays data — a Russian label the window
//! must show, an album name, a tag — so the message check looks only at the message positions of
//! the assertion macros.

use std::fs;
use std::path::Path;

/// Modules that still carry Russian comments or strings (the refactor works them off one by one).
const PENDING: &[&str] = &[
];

/// Files whose string literals are legitimate Russian *data*: the translation table itself and the
/// prompts for the multilingual model. Their comments are still English like everywhere else.
const RUSSIAN_DATA: &[&str] = &[
    // The language itself: month and weekday names, the plural forms of the counted nouns.
    "i18n/lang.rs",
    // The translation table: the Russian column of every area (M27.4).
    "i18n/terms/albums.rs",
    "i18n/terms/calendar.rs",
    "i18n/terms/dialogs.rs",
    "i18n/terms/dupes.rs",
    "i18n/terms/faces.rs",
    "i18n/terms/grid.rs",
    "i18n/terms/inspector.rs",
    "i18n/terms/reports.rs",
    "i18n/terms/settings.rs",
    "i18n/terms/shell.rs",
    // The prompts of the multilingual CLIP model and its default tag labels.
    "embed/tags.rs",
    // `labels.csv` is the owner's file: the parser keeps accepting the old Russian keywords
    // `пропуск` and `другое` next to `skip` and `other`.
    "eval.rs",
    // The CLI still accepts the Russian aliases `день`/`месяц`/`событие` for `--by`.
    "organize/plan.rs",
];

fn is_cyrillic(c: char) -> bool {
    matches!(c, '\u{0400}'..='\u{04FF}')
}

/// One token of a Rust source: an identifier, a string literal without its quotes, or one
/// character of punctuation. Comments and whitespace are dropped.
enum Tok {
    Ident(String),
    Str(String),
    Punct(char),
}

/// The assertion macros with the number of required arguments: everything after them is a message.
const MESSAGE_MACROS: &[(&str, usize)] = &[
    ("assert", 1),
    ("assert_eq", 2),
    ("assert_ne", 2),
    ("debug_assert", 1),
    ("debug_assert_eq", 2),
    ("debug_assert_ne", 2),
    ("panic", 0),
    ("anyhow", 0),
    ("bail", 0),
    ("format_err", 0),
];

/// The smallest tokenizer the message check needs. It knows strings, raw strings, char literals and
/// comments, so a comma or a bracket inside a format string cannot be mistaken for syntax.
fn tokens(src: &str) -> Vec<Tok> {
    let b: Vec<char> = src.chars().collect();
    let at = |i: usize| b.get(i).copied();
    let mut out = Vec::new();
    let mut i = 0usize;
    while i < b.len() {
        let c = b[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        if c == '/' && at(i + 1) == Some('/') {
            while i < b.len() && b[i] != '\n' {
                i += 1;
            }
            continue;
        }
        if c == '/' && at(i + 1) == Some('*') {
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
            continue;
        }
        // raw string: r"…", r#"…"#, br#"…"#
        let raw_prefix = {
            let rest: String = b[i..(i + 4).min(b.len())].iter().collect();
            ["br", "r", "cr"]
                .iter()
                .find(|p| rest.starts_with(*p))
                .map(|p| p.len())
        };
        if let Some(plen) = raw_prefix {
            let mut j = i + plen;
            let mut hashes = 0usize;
            while at(j) == Some('#') {
                hashes += 1;
                j += 1;
            }
            if at(j) == Some('"') {
                let start = j + 1;
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
                            let text: String = b[start..j].iter().collect();
                            out.push(Tok::Str(text));
                            j = k;
                            break;
                        }
                    }
                    j += 1;
                }
                i = j;
                continue;
            }
        }
        // plain, byte and C strings: "…", b"…", c"…"
        let quote_at = if c == '"' {
            Some(i)
        } else if matches!(c, 'b' | 'c') && at(i + 1) == Some('"') {
            Some(i + 1)
        } else {
            None
        };
        if let Some(q) = quote_at {
            let mut j = q + 1;
            let mut text = String::new();
            while j < b.len() {
                match b[j] {
                    '\\' => {
                        text.extend(&b[j..(j + 2).min(b.len())]);
                        j += 2;
                    }
                    '"' => break,
                    _ => {
                        text.push(b[j]);
                        j += 1;
                    }
                }
            }
            out.push(Tok::Str(text));
            i = (j + 1).min(b.len());
            continue;
        }
        // char literal or lifetime
        if c == '\'' {
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
            if b.get(j.wrapping_sub(1)) == Some(&'\'') && j > i + 1 {
                out.push(Tok::Punct('\''));
                i = j;
                continue;
            }
        }
        if c.is_alphanumeric() || c == '_' {
            let start = i;
            while i < b.len() && (b[i].is_alphanumeric() || b[i] == '_') {
                i += 1;
            }
            out.push(Tok::Ident(b[start..i].iter().collect()));
            continue;
        }
        out.push(Tok::Punct(c));
        i += 1;
    }
    out
}

/// The index of the parenthesis that closes the one at `open`, counting all bracket kinds.
fn matching(toks: &[Tok], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (k, tok) in toks.iter().enumerate().skip(open) {
        if let Tok::Punct(c) = tok {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(k);
                    }
                }
                _ => {}
            }
        }
    }
    None
}

/// Top-level argument ranges between `open` and `close`.
fn argument_ranges(toks: &[Tok], open: usize, close: usize) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut start = open + 1;
    for (offset, tok) in toks[open + 1..close].iter().enumerate() {
        let k = open + 1 + offset;
        if let Tok::Punct(c) = tok {
            match c {
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    out.push((start, k));
                    start = k + 1;
                }
                _ => {}
            }
        }
    }
    if start < close {
        out.push((start, close));
    }
    out
}

/// Russian string literals that sit in a message position: an assertion message, `panic!` text,
/// `expect` text or the reason of `#[ignore = "…"]`.
fn russian_messages(src: &str) -> Vec<String> {
    let toks = tokens(src);
    let mut bad = Vec::new();
    for (i, tok) in toks.iter().enumerate() {
        let name = match tok {
            Tok::Ident(n) => n.as_str(),
            _ => continue,
        };
        // The argument ranges that hold a message, per macro: everything after the required ones.
        let args: Vec<(usize, usize)> = if let Some(&(_, required)) =
            MESSAGE_MACROS.iter().find(|(m, _)| *m == name)
        {
            let is_call = matches!(toks.get(i + 1), Some(Tok::Punct('!')))
                && matches!(toks.get(i + 2), Some(Tok::Punct('(')));
            if !is_call {
                continue;
            }
            match matching(&toks, i + 2) {
                Some(close) => argument_ranges(&toks, i + 2, close)
                    .into_iter()
                    .skip(required)
                    .collect(),
                None => continue,
            }
        } else if name == "expect" && matches!(toks.get(i + 1), Some(Tok::Punct('('))) {
            // `.expect("…")`
            vec![(i + 2, i + 3)]
        } else if name == "ignore" && matches!(toks.get(i + 1), Some(Tok::Punct('='))) {
            // `#[ignore = "…"]`
            vec![(i + 2, i + 3)]
        } else {
            continue;
        };
        for (start, end) in args {
            for tok in toks.get(start.min(toks.len())..end.min(toks.len())).unwrap_or(&[]) {
                if let Tok::Str(text) = tok
                    && text.chars().any(is_cyrillic)
                {
                    bad.push(text.clone());
                }
            }
        }
    }
    bad
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

fn test_part<'a>(name: &str, src: &'a str) -> &'a str {
    // A `tests.rs` module is test code from the first line: it has no `#[cfg(test)]` marker of
    // its own, the parent module carries it.
    if name.ends_with("tests.rs") {
        return src;
    }
    match src.split_once("#[cfg(test)]") {
        Some((_, tail)) => tail,
        None => "",
    }
}

fn sources() -> Vec<(String, String)> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut out = Vec::new();
    collect_sources(&dir, &dir, &mut out);
    out.sort();
    out
}

/// Every `.rs` under `src/`, the name relative to it (`cli/index.rs`), so a module in a folder is
/// checked like a top-level one.
fn collect_sources(dir: &Path, base: &Path, out: &mut Vec<(String, String)>) {
    for entry in fs::read_dir(dir).expect("src/ is readable") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            collect_sources(&path, base, out);
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("rs") {
            continue;
        }
        let name = path
            .strip_prefix(base)
            .expect("a source file is under src/")
            .to_string_lossy()
            .replace('\\', "/");
        out.push((name, fs::read_to_string(&path).expect("a source file is readable")));
    }
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
        let production = if name.ends_with("tests.rs") {
            ""
        } else {
            match src.split_once("#[cfg(test)]") {
                Some((head, _)) => head,
                None => src.as_str(),
            }
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
        "Russian belongs in src/i18n/ (AGENTS.md §6): {bad:?}"
    );
    assert!(
        pending_honest.is_empty(),
        "these modules are already clean — drop them from PENDING: {pending_honest:?}"
    );
}

/// A test message is read by a person when something breaks, so it is English like the rest of the
/// code. What a test compares stays data: `assert_eq!(t(Lang::Ru, "Settings"), "Настройки")` is a
/// translation the window must show, not a message.
#[test]
fn test_messages_are_english() {
    let mut bad = Vec::new();
    for (name, src) in sources() {
        for text in russian_messages(test_part(&name, &src)) {
            bad.push(format!("{name}: {text}"));
        }
    }
    assert!(
        bad.is_empty(),
        "test messages must be English (AGENTS.md §6): {bad:?}"
    );
}
