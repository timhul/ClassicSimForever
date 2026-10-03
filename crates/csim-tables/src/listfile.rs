//! The icon texture names of the community listfile (https://github.com/wowdev/wow-listfile).
//!
//! The client tables name an icon by the `FileDataID` of its texture (`SpellMisc.
//! SpellIconFileDataID`, `Item.IconFileDataID`); Wowhead's CDN serves it by the texture's file
//! name. `tools/fetch_listfile.py` keeps the `interface/icons/` rows of the listfile in
//! [`LISTFILE_NAME`] next to the table dumps: `FileDataID;path` lines without a header.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::error::{Result, TableError};

/// The file `tools/fetch_listfile.py` writes in the table directory. It does not match
/// `<Table>.<build>.csv`, so [`TableDir`](crate::TableDir) ignores it.
pub const LISTFILE_NAME: &str = "listfile-icons.csv";

/// The directory of the icon textures.
const ICON_DIR: &str = "interface/icons/";

/// The icon texture names by `FileDataID`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IconNames {
    names: HashMap<u32, String>,
}

impl IconNames {
    /// Reads a listfile (see [`IconNames::parse`]).
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path).map_err(|source| TableError::ReadListfile {
            path: path.to_owned(),
            source,
        })?;
        Self::parse(&text, path)
    }

    /// Parses `FileDataID;path` lines, `path` being the file of the listfile for errors. Blank
    /// lines are skipped; paths outside `interface/icons/` and placeholder names (an `unk`,
    /// `unknown` or `autogen…` part, or the `FileDataID` itself) are ignored.
    pub fn parse(text: &str, path: &Path) -> Result<Self> {
        let mut names = HashMap::new();
        for (index, line) in text.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let malformed = || TableError::Listfile {
                path: path.to_owned(),
                line: index + 1,
                text: line.to_owned(),
            };
            let (id, file) = line.split_once(';').ok_or_else(malformed)?;
            let id: u32 = id.parse().map_err(|_| malformed())?;
            if let Some(name) = icon_name(id, file) {
                names.insert(id, name);
            }
        }
        Ok(Self { names })
    }

    /// The name of the icon texture `file_data_id` (`inv_sword_39`).
    pub fn get(&self, file_data_id: u32) -> Option<&str> {
        self.names.get(&file_data_id).map(String::as_str)
    }

    pub fn len(&self) -> usize {
        self.names.len()
    }

    pub fn is_empty(&self) -> bool {
        self.names.is_empty()
    }
}

/// The listfile in the table directory `dir`.
pub fn listfile_path(dir: &Path) -> PathBuf {
    dir.join(LISTFILE_NAME)
}

/// `interface/icons/INV_Sword_39.blp` → `inv_sword_39`; `None` outside the icon directory or
/// for a placeholder name.
fn icon_name(file_data_id: u32, file: &str) -> Option<String> {
    let file = file.trim().to_lowercase();
    let name = file.strip_prefix(ICON_DIR)?;
    let name = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
    if name.is_empty() || name.contains('/') || is_placeholder(file_data_id, name) {
        return None;
    }
    Some(name.to_owned())
}

/// Names the listfile maintainers give files they could not identify.
fn is_placeholder(file_data_id: u32, name: &str) -> bool {
    name.split(['_', '-', ' '])
        .any(|part| part == "unk" || part == "unknown" || part.starts_with("autogen"))
        || name.contains(&file_data_id.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> Result<IconNames> {
        IconNames::parse(text, Path::new("listfile-icons.csv"))
    }

    #[test]
    fn icon_rows_give_lowercase_names_without_directory_and_extension() {
        let names = parse(
            "132089;interface/icons/ability_ambush.blp\n\
             \n\
             135274;Interface/Icons/INV_Sword_39.BLP\r\n\
             136012;interface/icons/spell_nature_bloodlust.blp\n",
        )
        .unwrap();
        assert_eq!(names.len(), 3);
        assert_eq!(names.get(132089), Some("ability_ambush"));
        assert_eq!(names.get(135274), Some("inv_sword_39"));
        assert_eq!(names.get(136012), Some("spell_nature_bloodlust"));
        assert_eq!(names.get(1), None);
    }

    #[test]
    fn other_paths_and_placeholders_are_ignored() {
        let names = parse(
            "1;world/maps/azeroth/azeroth.wdt\n\
             2;interface/icons/sub/dir.blp\n\
             3;interface/icons/inv_misc_unk_01.blp\n\
             4;interface/icons/unknown.blp\n\
             5;interface/icons/autogen_icon.blp\n\
             4567890;interface/icons/icon_4567890.blp\n\
             6;interface/icons/inv_trunk_monk.blp\n",
        )
        .unwrap();
        assert_eq!(names.len(), 1, "{names:?}");
        assert_eq!(
            names.get(6),
            Some("inv_trunk_monk"),
            "`unk` inside a word is a name"
        );
    }

    #[test]
    fn malformed_lines_name_their_line() {
        let error = parse("1;interface/icons/a.blp\nnot a row\n").unwrap_err();
        assert_eq!(
            error.to_string(),
            "listfile-icons.csv line 2: not `FileDataID;path`: \"not a row\""
        );
        let error = parse("x;interface/icons/a.blp\n").unwrap_err();
        assert!(error.to_string().contains("line 1"), "{error}");
    }

    #[test]
    fn a_missing_file_is_an_error() {
        let error = IconNames::load(Path::new("does-not-exist/listfile-icons.csv")).unwrap_err();
        assert!(matches!(error, TableError::ReadListfile { .. }), "{error}");
    }
}
