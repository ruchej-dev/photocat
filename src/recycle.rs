//! The trash: a file goes to the OS trash only after an explicit confirmation.
//!
//! This is the only place where the archive leaves its folder. There is no permanent deletion
//! inside the application: the OS keeps the file in its trash until the owner empties it.
//! The DB row is removed only after a successful move.

use crate::db::{self, Progress};
use crate::i18n::{self, Lang};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub struct Report {
    pub lang: Lang,
    pub moved: usize,
    pub errors: usize,
    pub bytes: i64,
    pub cancelled: bool,
    pub details: Vec<String>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let stopped = if self.cancelled {
            i18n::t(self.lang, "stopped: ")
        } else {
            ""
        };
        let body = i18n::fill(
            self.lang,
            "to the OS trash: {}, errors {}, {}",
            &[
                self.moved.to_string(),
                self.errors.to_string(),
                db::human(self.bytes),
            ],
        );
        write!(f, "{stopped}{body}")?;
        if let Some(first) = self.details.first() {
            write!(f, " · {first}")?;
        }
        Ok(())
    }
}

/// Move the marked files into the OS trash and drop their rows from the index.
/// `items` is `(file_id, path, size)`.
pub fn empty(
    db_path: &Path,
    items: &[(i64, String, i64)],
    lang: Lang,
    progress: Option<Arc<Progress>>,
) -> Result<Report> {
    if let Some(p) = &progress {
        p.set_total(items.len());
        p.set_msg(i18n::t(lang, "trash: moving to the OS trash…"));
    }
    let mut report = Report {
        lang,
        moved: 0,
        errors: 0,
        bytes: 0,
        cancelled: false,
        details: Vec::new(),
    };
    for (id, path, size) in items {
        if progress.as_ref().is_some_and(|p| p.cancelled()) {
            report.cancelled = true;
            break;
        }
        match trash::delete(Path::new(path)) {
            Ok(()) => {
                // The file is in the OS trash already; only now is the index row dropped.
                if let Err(e) = db::purge_file(db_path, *id) {
                    report.errors += 1;
                    if report.details.len() < 3 {
                        report
                            .details
                            .push(format!("{path}: the index was not cleared: {e}"));
                    }
                } else {
                    report.moved += 1;
                    report.bytes += *size;
                }
            }
            Err(e) => {
                report.errors += 1;
                if report.details.len() < 3 {
                    report.details.push(format!("{path}: {e}"));
                }
            }
        }
        if let Some(p) = &progress {
            p.tick();
            if report.errors > 0 {
                p.error();
            }
        }
    }
    if let Some(p) = &progress {
        p.set_msg(i18n::t(lang, "trash: done"));
        p.finish();
    }
    if report.moved == 0 && report.errors > 0 && !report.cancelled {
        anyhow::bail!(
            "{}",
            i18n::fill(
                lang,
                "not a single file was moved: {}",
                &[report.details.join("; ")]
            )
        );
    }
    Ok(report)
}
