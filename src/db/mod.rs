//! SQLite: the schema, inserts, reads, statistics.
//!
//! The module is split by subject (M27.3): `schema` is the only place the DDL lives, and a query
//! is found by the table it serves. `mod.rs` holds the module list and the re-exports only, so a
//! caller still writes `db::load_all` and never learns the folder.

mod albums;
mod faces;
mod files;
mod human;
mod progress;
mod schema;
mod settings;
mod tags;

#[cfg(test)]
mod tests;

pub use albums::*;
pub use faces::*;
pub use files::*;
pub use human::*;
pub use progress::*;
pub use schema::*;
pub use settings::*;
pub use tags::*;
