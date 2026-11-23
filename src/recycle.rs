//! Корзина: файл уходит в системную корзину только после явного подтверждения.
//!
//! Это единственное место, где архив покидает свой каталог. Безвозвратного удаления
//! внутри приложения нет: ОС хранит файл в корзине, пока владелец сам её не очистит.
//! Строка БД удаляется только после успешного переноса.

use crate::db::{self, Progress};
use anyhow::Result;
use std::path::Path;
use std::sync::Arc;

pub struct Report {
    pub moved: usize,
    pub errors: usize,
    pub bytes: i64,
    pub cancelled: bool,
    pub details: Vec<String>,
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}в системную корзину: {}, ошибок {}, {}",
            if self.cancelled {
                "остановлено: "
            } else {
                ""
            },
            self.moved,
            self.errors,
            db::human(self.bytes)
        )?;
        if let Some(first) = self.details.first() {
            write!(f, " · {first}")?;
        }
        Ok(())
    }
}

/// Перенести помеченные файлы в системную корзину и убрать их строки из индекса.
/// `items` — `(file_id, путь, размер)`.
pub fn empty(
    db_path: &Path,
    items: &[(i64, String, i64)],
    progress: Option<Arc<Progress>>,
) -> Result<Report> {
    if let Some(p) = &progress {
        p.set_total(items.len());
        p.set_msg("корзина: переношу в системную корзину…");
    }
    let mut report = Report {
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
                // Файл уже в системной корзине; только теперь убираем строку индекса.
                if let Err(e) = db::purge_file(db_path, *id) {
                    report.errors += 1;
                    if report.details.len() < 3 {
                        report
                            .details
                            .push(format!("{path}: индекс не очищен: {e}"));
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
        p.set_msg("корзина: готово");
        p.finish();
    }
    if report.moved == 0 && report.errors > 0 && !report.cancelled {
        anyhow::bail!("ни один файл не перенесён: {}", report.details.join("; "));
    }
    Ok(report)
}
