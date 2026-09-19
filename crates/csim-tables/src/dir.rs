//! A directory of `<Table>.<build>.csv` files.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use crate::error::{Result, TableError};
use crate::row::{Header, Row, TableRow};

/// The table files of one client build inside a directory.
///
/// A dump directory may hold several builds side by side (`Spell.1.60.1.69893.csv`,
/// `Spell.1.61.0.70012.csv`); [`TableDir::open`] requires the build to be unambiguous and
/// [`TableDir::open_build`] picks one explicitly.
#[derive(Debug, Clone)]
pub struct TableDir {
    dir: PathBuf,
    build: String,
    files: BTreeMap<String, PathBuf>,
}

/// Splits `SpellEffect.1.60.1.69893.csv` into `("SpellEffect", "1.60.1.69893")`.
fn split_file_name(name: &str) -> Option<(&str, &str)> {
    let stem = name.strip_suffix(".csv")?;
    let (table, build) = stem.split_once('.')?;
    let valid_table =
        !table.is_empty() && table.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    let valid_build = !build.is_empty()
        && build
            .split('.')
            .all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()));
    (valid_table && valid_build).then_some((table, build))
}

fn scan(dir: &Path) -> Result<BTreeMap<String, BTreeMap<String, PathBuf>>> {
    let entries = std::fs::read_dir(dir).map_err(|source| TableError::Io {
        dir: dir.to_owned(),
        source,
    })?;
    let mut builds: BTreeMap<String, BTreeMap<String, PathBuf>> = BTreeMap::new();
    for entry in entries {
        let entry = entry.map_err(|source| TableError::Io {
            dir: dir.to_owned(),
            source,
        })?;
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if let Some((table, build)) = split_file_name(name) {
            builds
                .entry(build.to_owned())
                .or_default()
                .insert(table.to_owned(), path);
        }
    }
    if builds.is_empty() {
        return Err(TableError::NoTables {
            dir: dir.to_owned(),
        });
    }
    Ok(builds)
}

impl TableDir {
    /// Lists the builds that have files in `dir`.
    pub fn builds(dir: impl AsRef<Path>) -> Result<Vec<String>> {
        Ok(scan(dir.as_ref())?.into_keys().collect())
    }

    /// Opens the single build found in `dir`.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut builds = scan(dir)?;
        if builds.len() > 1 {
            return Err(TableError::AmbiguousBuild {
                dir: dir.to_owned(),
                builds: builds.into_keys().collect(),
            });
        }
        let (build, files) = builds.pop_first().expect("scan returns at least one build");
        Ok(Self {
            dir: dir.to_owned(),
            build,
            files,
        })
    }

    /// Opens the files of `build` in `dir`.
    pub fn open_build(dir: impl AsRef<Path>, build: &str) -> Result<Self> {
        let dir = dir.as_ref();
        let mut builds = scan(dir)?;
        let files = builds
            .remove(build)
            .ok_or_else(|| TableError::UnknownBuild {
                dir: dir.to_owned(),
                build: build.to_owned(),
            })?;
        Ok(Self {
            dir: dir.to_owned(),
            build: build.to_owned(),
            files,
        })
    }

    /// The directory the files live in.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The build the files belong to, e.g. `1.60.1.69893`.
    pub fn build(&self) -> &str {
        &self.build
    }

    /// Names of the tables that have a file, sorted.
    pub fn table_names(&self) -> impl Iterator<Item = &str> {
        self.files.keys().map(String::as_str)
    }

    /// Whether `table` has a file.
    pub fn has(&self, table: &str) -> bool {
        self.files.contains_key(table)
    }

    /// Path of `table`'s file.
    pub fn path(&self, table: &str) -> Result<&Path> {
        self.files
            .get(table)
            .map(PathBuf::as_path)
            .ok_or_else(|| TableError::MissingTable {
                table: table.to_owned(),
                build: self.build.clone(),
                dir: self.dir.clone(),
            })
    }

    /// Reads every row of `R`'s table.
    pub fn read<R: TableRow>(&self) -> Result<Vec<R>> {
        let mut rows = Vec::new();
        self.for_each_row(R::TABLE, |row| {
            rows.push(R::from_row(&row)?);
            Ok(())
        })?;
        Ok(rows)
    }

    /// Reads the rows of `R`'s table that `keep` accepts.
    pub fn read_where<R: TableRow>(&self, mut keep: impl FnMut(&R) -> bool) -> Result<Vec<R>> {
        let mut rows = Vec::new();
        self.for_each_row(R::TABLE, |row| {
            let parsed = R::from_row(&row)?;
            if keep(&parsed) {
                rows.push(parsed);
            }
            Ok(())
        })?;
        Ok(rows)
    }

    /// The column names of `table`'s file, in file order.
    pub fn columns(&self, table: &str) -> Result<Vec<String>> {
        let path = self.path(table)?;
        let mut reader = open_reader(path)?;
        let headers = reader.headers().map_err(|source| TableError::Csv {
            path: path.to_owned(),
            source,
        })?;
        Ok(headers.iter().map(str::to_owned).collect())
    }

    /// Number of data rows in `table`'s file.
    pub fn row_count(&self, table: &str) -> Result<usize> {
        let mut count = 0;
        self.for_each_row(table, |_| {
            count += 1;
            Ok(())
        })?;
        Ok(count)
    }

    /// Streams the records of `table` through `f` without building a typed row vector.
    pub fn for_each_row(
        &self,
        table: &str,
        mut f: impl FnMut(Row<'_>) -> Result<()>,
    ) -> Result<()> {
        let path = self.path(table)?;
        let csv_error = |source| TableError::Csv {
            path: path.to_owned(),
            source,
        };
        let mut reader = open_reader(path)?;
        let header = Header::new(table, reader.headers().map_err(csv_error)?.iter());
        let mut record = csv::StringRecord::new();
        while reader.read_record(&mut record).map_err(csv_error)? {
            let line = record.position().map_or(0, |p| p.line());
            f(Row::new(&header, &record, line))?;
        }
        Ok(())
    }
}

fn open_reader(path: &Path) -> Result<csv::Reader<std::fs::File>> {
    csv::ReaderBuilder::new()
        .has_headers(true)
        .flexible(false)
        .from_path(path)
        .map_err(|source| TableError::Csv {
            path: path.to_owned(),
            source,
        })
}

/// The set of table names a caller needs, for reporting which are missing from a directory.
pub fn missing_tables<'a>(
    dir: &TableDir,
    needed: impl IntoIterator<Item = &'a str>,
) -> Vec<String> {
    let have: BTreeSet<&str> = dir.table_names().collect();
    needed
        .into_iter()
        .filter(|t| !have.contains(t))
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_names_split_into_table_and_build() {
        assert_eq!(
            split_file_name("SpellEffect.1.60.1.69893.csv"),
            Some(("SpellEffect", "1.60.1.69893"))
        );
        assert_eq!(
            split_file_name("TraitNodeXTraitNodeEntry.2.0.csv"),
            Some(("TraitNodeXTraitNodeEntry", "2.0"))
        );
        assert_eq!(split_file_name("Spell.csv"), None);
        assert_eq!(split_file_name("Spell.1.60.1.69893.txt"), None);
        assert_eq!(split_file_name("notes.1.txt.csv"), None);
        assert_eq!(split_file_name(".1.2.csv"), None);
        assert_eq!(split_file_name("Spell..csv"), None);
    }
}
