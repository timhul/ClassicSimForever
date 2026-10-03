//! Where the loaders read the data files from: the disk ([`FsFiles`]), or memory ([`MemFiles`])
//! where there is no filesystem (the browser build), optionally with files of the user's on
//! top of the bundled ones ([`Overlay`]).
//!
//! Every loader has a `load_from(files, path)` next to its `load(path)`, which is
//! `load_from(&FsFiles, path)`. Paths keep their meaning: `include:` lines of a character
//! setup resolve relative to the including file whatever the source.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use crate::character_loader::normalize;

/// A source of data files.
pub trait Files {
    /// The contents of the file at `path`.
    fn read(&self, path: &Path) -> io::Result<String>;

    /// The files and directories directly in `dir`, sorted by path.
    fn entries(&self, dir: &Path) -> io::Result<Vec<Entry>>;

    /// Whether `path` is a directory.
    fn is_dir(&self, path: &Path) -> bool;

    /// One name per file, to recognise a file reached by two paths (include cycles); `path`
    /// itself when it cannot be resolved.
    fn canonical(&self, path: &Path) -> PathBuf {
        path.to_path_buf()
    }
}

/// An entry of a directory listing.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Entry {
    pub path: PathBuf,
    pub is_dir: bool,
}

/// The `*.yaml` / `*.yml` files directly in `dir`, sorted by name.
pub fn yaml_files(files: &dyn Files, dir: &Path) -> io::Result<Vec<PathBuf>> {
    Ok(files
        .entries(dir)?
        .into_iter()
        .filter(|entry| !entry.is_dir && is_yaml(&entry.path))
        .map(|entry| entry.path)
        .collect())
}

fn is_yaml(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext == "yaml" || ext == "yml")
}

/// The files on disk.
#[derive(Debug, Clone, Copy, Default)]
pub struct FsFiles;

impl Files for FsFiles {
    fn read(&self, path: &Path) -> io::Result<String> {
        std::fs::read_to_string(path)
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<Entry>> {
        let mut entries: Vec<Entry> = std::fs::read_dir(dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter_map(|path| {
                let is_dir = path.is_dir();
                (is_dir || path.is_file()).then_some(Entry { path, is_dir })
            })
            .collect();
        entries.sort();
        Ok(entries)
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
    }
}

/// Files held in memory, by path. Paths are normalized as on disk: `\` separates like `/`, and
/// `.` and `a/..` fold away (`characters/../rotations/a.yaml` is `rotations/a.yaml`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MemFiles {
    files: BTreeMap<PathBuf, String>,
}

impl MemFiles {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds the file `path`, replacing one already there.
    pub fn insert(&mut self, path: impl AsRef<Path>, text: impl Into<String>) {
        self.files.insert(key(path.as_ref()), text.into());
    }

    pub fn len(&self) -> usize {
        self.files.len()
    }

    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }
}

impl<P: AsRef<Path>, T: Into<String>> FromIterator<(P, T)> for MemFiles {
    fn from_iter<I: IntoIterator<Item = (P, T)>>(iter: I) -> Self {
        let mut files = Self::new();
        for (path, text) in iter {
            files.insert(path, text);
        }
        files
    }
}

/// `path` normalized: `\` is a separator on every platform, `.` and `name/..` fold away.
fn key(path: &Path) -> PathBuf {
    normalize(Path::new(&path.to_string_lossy().replace('\\', "/")))
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("{} is not among the files", path.display()),
    )
}

impl Files for MemFiles {
    fn read(&self, path: &Path) -> io::Result<String> {
        self.files
            .get(&key(path))
            .cloned()
            .ok_or_else(|| not_found(path))
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<Entry>> {
        let dir = key(dir);
        let mut entries = BTreeSet::new();
        for file in self.files.keys() {
            let Ok(rest) = file.strip_prefix(&dir) else {
                continue;
            };
            let mut components = rest.components();
            let (Some(first), more) = (components.next(), components.next().is_some()) else {
                continue;
            };
            entries.insert(Entry {
                path: dir.join(first),
                is_dir: more,
            });
        }
        if entries.is_empty() {
            return Err(not_found(&dir));
        }
        Ok(entries.into_iter().collect())
    }

    fn is_dir(&self, path: &Path) -> bool {
        let dir = key(path);
        self.files
            .keys()
            .any(|file| file != &dir && file.starts_with(&dir))
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        key(path)
    }
}

/// The files of `top`, then those of `base` it does not have: e.g. a setup the user pasted
/// (`top`) that includes the bundled `characters/common/` files (`base`).
#[derive(Clone, Copy)]
pub struct Overlay<'a> {
    pub top: &'a dyn Files,
    pub base: &'a dyn Files,
}

impl Files for Overlay<'_> {
    fn read(&self, path: &Path) -> io::Result<String> {
        match self.top.read(path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => self.base.read(path),
            result => result,
        }
    }

    fn entries(&self, dir: &Path) -> io::Result<Vec<Entry>> {
        let top = self.top.entries(dir);
        let base = self.base.entries(dir);
        let (top, base) = match (top, base) {
            (Err(error), Err(_)) => return Err(error),
            (top, base) => (top.unwrap_or_default(), base.unwrap_or_default()),
        };
        let merged: BTreeSet<Entry> = top.into_iter().chain(base).collect();
        Ok(merged.into_iter().collect())
    }

    fn is_dir(&self, path: &Path) -> bool {
        self.top.is_dir(path) || self.base.is_dir(path)
    }

    fn canonical(&self, path: &Path) -> PathBuf {
        self.base.canonical(path)
    }
}

/// The `*.yaml` files under `root` (recursively) as [`MemFiles`], by path relative to `root`.
#[cfg(test)]
pub(crate) fn yaml_tree(root: &Path) -> MemFiles {
    fn walk(root: &Path, dir: &Path, files: &mut MemFiles) {
        for entry in FsFiles.entries(dir).unwrap() {
            if entry.is_dir {
                walk(root, &entry.path, files);
            } else if is_yaml(&entry.path) {
                let text = FsFiles.read(&entry.path).unwrap();
                files.insert(entry.path.strip_prefix(root).unwrap(), text);
            }
        }
    }
    let mut files = MemFiles::new();
    walk(root, root, &mut files);
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    fn files() -> MemFiles {
        [
            ("characters/a.yaml", "a"),
            ("characters/common/b.yaml", "b"),
            ("characters/notes.txt", "n"),
            ("races.yaml", "r"),
        ]
        .into_iter()
        .collect()
    }

    #[test]
    fn paths_are_normalized_as_on_disk() {
        let files = files();
        assert_eq!(files.len(), 4);
        assert_eq!(files.read(Path::new("characters/a.yaml")).unwrap(), "a");
        assert_eq!(files.read(Path::new("./characters/a.yaml")).unwrap(), "a");
        assert_eq!(
            files.read(Path::new("characters\\common\\b.yaml")).unwrap(),
            "b"
        );
        assert_eq!(
            files
                .read(Path::new("characters/common/../../races.yaml"))
                .unwrap(),
            "r"
        );
        assert_eq!(
            files.canonical(Path::new("characters/common/../a.yaml")),
            Path::new("characters/a.yaml")
        );
        let error = files.read(Path::new("characters/c.yaml")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert!(error.to_string().contains("characters/c.yaml"), "{error}");
    }

    #[test]
    fn a_directory_lists_its_files_and_subdirectories() {
        let files = files();
        let entries = files.entries(Path::new("characters")).unwrap();
        let file = |path: &str| Entry {
            path: PathBuf::from(path),
            is_dir: false,
        };
        let dir = Entry {
            path: PathBuf::from("characters/common"),
            is_dir: true,
        };
        assert_eq!(
            entries,
            [file("characters/a.yaml"), dir, file("characters/notes.txt")]
        );
        assert_eq!(
            yaml_files(&files, Path::new("characters/")).unwrap(),
            [PathBuf::from("characters/a.yaml")]
        );
        assert_eq!(files.entries(Path::new("")).unwrap().len(), 2);
        assert!(files.is_dir(Path::new("characters/common")));
        assert!(!files.is_dir(Path::new("characters/a.yaml")));
        assert!(!files.is_dir(Path::new("talents")));
        let error = files.entries(Path::new("talents")).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
    }

    #[test]
    fn an_overlay_reads_the_top_files_first() {
        let base = files();
        let top: MemFiles = [("characters/a.yaml", "mine"), ("characters/z.yaml", "z")]
            .into_iter()
            .collect();
        let files = Overlay {
            top: &top,
            base: &base,
        };
        assert_eq!(files.read(Path::new("characters/a.yaml")).unwrap(), "mine");
        assert_eq!(files.read(Path::new("races.yaml")).unwrap(), "r");
        assert_eq!(
            yaml_files(&files, Path::new("characters")).unwrap(),
            [
                PathBuf::from("characters/a.yaml"),
                PathBuf::from("characters/z.yaml")
            ]
        );
        assert!(files.entries(Path::new("talents")).is_err());
    }

    #[test]
    fn the_disk_lists_sorted_entries() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../data/rotations");
        let entries = FsFiles.entries(&dir).unwrap();
        assert!(entries.windows(2).all(|pair| pair[0] < pair[1]));
        assert!(entries.iter().any(|entry| entry.is_dir));
        assert!(FsFiles.is_dir(&dir));
        assert!(FsFiles.entries(&dir.join("missing")).is_err());
    }
}
