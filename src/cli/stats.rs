//! `photocat stats` — the index at a glance.

use crate::db;
use crate::flag_str;
use anyhow::Result;
use std::path::PathBuf;

use super::language;

pub fn run(args: &[String]) -> Result<()> {
    let db_path = db::resolve_db(flag_str(args, "--db").map(PathBuf::from).as_deref())?;
    db::print_stats(language(args)?, &db_path)
}
