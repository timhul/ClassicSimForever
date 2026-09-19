//! Errors raised while locating and parsing table dumps.

use std::path::PathBuf;

/// Everything that can go wrong between a directory of `<Table>.<build>.csv` files and typed rows.
#[derive(Debug, thiserror::Error)]
pub enum TableError {
    /// The table directory could not be listed.
    #[error("cannot read table directory {dir}: {source}")]
    Io {
        dir: PathBuf,
        #[source]
        source: std::io::Error,
    },
    /// The directory holds no `<Table>.<build>.csv` file at all.
    #[error("no table files (`<Table>.<build>.csv`) found in {dir}")]
    NoTables { dir: PathBuf },
    /// The directory holds files of several builds and none was chosen.
    #[error("table directory {dir} contains several builds ({builds:?}); choose one")]
    AmbiguousBuild { dir: PathBuf, builds: Vec<String> },
    /// The chosen build has no files in the directory.
    #[error("no table files of build {build} in {dir}")]
    UnknownBuild { dir: PathBuf, build: String },
    /// A table the caller asked for has no file in the chosen build.
    #[error("table {table} (build {build}) not found in {dir}")]
    MissingTable {
        table: String,
        build: String,
        dir: PathBuf,
    },
    /// The CSV file itself is malformed or unreadable.
    #[error("{path}: {source}")]
    Csv {
        path: PathBuf,
        #[source]
        source: csv::Error,
    },
    /// A column the row type needs is absent from the file header.
    #[error("{table}: column {column} is missing from the header")]
    MissingColumn { table: String, column: String },
    /// A cell could not be parsed as the row type's field type.
    #[error("{table} line {line}, column {column}: cannot parse {value:?} as {ty}")]
    Parse {
        table: String,
        line: u64,
        column: String,
        value: String,
        ty: &'static str,
    },
}

/// Convenience alias used throughout the crate.
pub type Result<T> = std::result::Result<T, TableError>;
