//! Reader for the World of Warcraft Forever client table dumps.
//!
//! The game data used by ClassicSimForever comes from the client DB2 tables, dumped as one CSV
//! file per table named `<Table>.<build>.csv` (for example `SpellEffect.1.60.1.69893.csv`). The
//! files live outside git (`data/tables/`, see `data/SPELL_INSTRUCTIONS.md`,
//! `data/TALENT_INSTRUCTIONS.md` and `data/ITEM_INSTRUCTIONS.md` for what each table holds).
//!
//! This crate turns those files into typed rows and indexed lookups:
//!
//! - [`TableDir`] locates the files of one build in a directory and reads a table into typed rows
//!   ([`TableRow`]) by **header name**, since the column order differs between tables and dumps.
//! - [`tables`] declares one row struct per table the exporters need.
//! - [`Tables`] loads every one of them and builds the `ID` / `SpellID` indexes the exporters
//!   (spells, racials, talents — later tasks of Phase 3T in `TASKS.md`) join on.
//!
//! Numeric columns are parsed leniently in two ways that the dumps require: an empty cell reads
//! as `0`, and a negative value in an unsigned column is the two's-complement bit pattern (the
//! dumps write `-1` for "all races" masks and `-2147483648` for bit 31 of a class mask).

pub mod db;
pub mod dir;
pub mod error;
pub mod row;
pub mod tables;

pub use db::Tables;
pub use dir::TableDir;
pub use error::TableError;
pub use row::{Field, Row, TableRow};

/// Crate version.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
