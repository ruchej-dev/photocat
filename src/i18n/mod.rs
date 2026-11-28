//! The language of the interface (M21, ADR 0040; ADR 0046): English is the main language,
//! Russian is the second one.
//!
//! The source text of the interface is written in English; `t()` returns the Russian translation
//! when Russian is chosen. The code therefore reads on its own, and a string that has no
//! translation yet is shown as written instead of disappearing.
//!
//! An L0 module: `std` only, no egui and no DB. The language is passed explicitly rather than kept
//! in a global variable, so a test that checks the text does not depend on its neighbours or on
//! the order they run in.
//!
//! Split by subject (M27.4): `lang.rs` holds the language and the lookup, `table.rs` the ordered
//! list of area tables, and `terms/` one table per screen area — the words of a screen are found
//! next to the screen's name.

mod lang;
mod table;
mod terms;

#[cfg(test)]
mod tests;

pub use lang::*;
pub use terms::reports::USAGE;
