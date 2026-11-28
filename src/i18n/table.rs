//! The one table: the ordered list of the area tables.
//!
//! `pairs()` flattens them once, in the order a reader meets the screens, so the split costs no
//! second lookup and a key keeps the same translation wherever it is spoken.

use std::sync::OnceLock;

use super::terms;

/// The area tables in reading order: the name a duplicate is reported with, then its pairs.
pub const AREAS: &[(&str, &[(&str, &str)])] = &[
    ("shell", terms::shell::TERMS),
    ("grid", terms::grid::TERMS),
    ("inspector", terms::inspector::TERMS),
    ("dupes", terms::dupes::TERMS),
    ("faces", terms::faces::TERMS),
    ("albums", terms::albums::TERMS),
    ("calendar", terms::calendar::TERMS),
    ("settings", terms::settings::TERMS),
    ("reports", terms::reports::TERMS),
    ("dialogs", terms::dialogs::TERMS),
];

/// Every pair of every area, flattened once: the table `t()` looks a key up in.
pub fn pairs() -> &'static [(&'static str, &'static str)] {
    static ALL: OnceLock<Vec<(&'static str, &'static str)>> = OnceLock::new();
    ALL.get_or_init(|| {
        AREAS
            .iter()
            .flat_map(|(_, terms)| terms.iter().copied())
            .collect()
    })
}
