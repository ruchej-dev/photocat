//! What a command prints while a long pass runs: one refreshed line, one spinner, a short version.

use crate::db::Progress;
use crate::i18n::{self, Lang};
use std::sync::Arc;

/// A short version label: "ffmpeg version 6.1.1" instead of the line with the copyright.
pub fn short_version(line: &str) -> String {
    line.split_whitespace().take(3).collect::<Vec<_>>().join(" ")
}

/// Refresh one line from a background thread while the pass runs. `template` is an `i18n::fill`
/// sentence with `{}`/`{}`/`{}` for done, total and errors and, when `with_message` is set, a fourth
/// `{}` for the pass's own note. Without the message the line stays empty until a total is known.
pub fn progress(lang: Lang, template: &'static str, with_message: bool, progress: Arc<Progress>) {
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(300));
        loop {
            let (done, total, errors) = (progress.done(), progress.total(), progress.errors());
            if total > 0 {
                let line = if with_message {
                    i18n::fill(
                        lang,
                        template,
                        &[
                            done.to_string(),
                            total.to_string(),
                            errors.to_string(),
                            progress.msg(),
                        ],
                    )
                } else {
                    i18n::fill(
                        lang,
                        template,
                        &[done.to_string(), total.to_string(), errors.to_string()],
                    )
                };
                eprint!("\r{line}");
            } else if with_message {
                eprint!("\r{}", progress.msg());
            }
            if progress.finished() {
                eprintln!();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    });
}

/// A spinner for a download: the pass's note on one line until it finishes.
pub fn spinner(progress: Arc<Progress>) {
    std::thread::spawn(move || {
        while !progress.finished() {
            eprintln!("\r{}", progress.msg());
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    });
}
