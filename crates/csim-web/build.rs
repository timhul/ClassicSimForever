//! Embeds the data the live viewer reads: every `*.yaml` / `*.yml` file under the entries of
//! [`DATA`] in the repository's `data/`, as `FILES` (`$OUT_DIR/files.rs`): `(path relative to
//! data/, include_str!(...))`, sorted by path. One wasm file always matches its data.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// What the live viewer loads: the data bundle, the setups and the keybinds. Not the table
/// dumps, icons, raids or sweeps.
const DATA: [&str; 11] = [
    "spells",
    "items",
    "item_sets.yaml",
    "enchants.yaml",
    "classes",
    "races.yaml",
    "talents",
    "external_buffs.yaml",
    "rotations",
    "characters",
    "keybinds",
];

fn main() {
    let manifest = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let data = manifest.join("../../data");
    let mut files = Vec::new();
    for entry in DATA {
        collect(&data.join(entry), &mut files);
    }
    files.sort();

    let mut out = String::from(
        "/// The embedded data files: (path relative to `data/`, contents), sorted by path.\n\
         pub static FILES: &[(&str, &str)] = &[\n",
    );
    for path in &files {
        let relative = path.strip_prefix(&data).expect("under data/");
        let key = relative.to_string_lossy().replace('\\', "/");
        let absolute = path.canonicalize().expect("an existing file");
        writeln!(
            out,
            "    ({key:?}, include_str!({:?})),",
            absolute.display()
        )
        .unwrap();
    }
    out.push_str("];\n");
    let out_dir = PathBuf::from(std::env::var("OUT_DIR").expect("set by cargo"));
    std::fs::write(out_dir.join("files.rs"), out).expect("OUT_DIR is writable");
}

/// The YAML files at or under `path`, telling cargo to rerun when any of them, or a directory
/// listing, changes.
fn collect(path: &Path, files: &mut Vec<PathBuf>) {
    println!("cargo:rerun-if-changed={}", path.display());
    if path.is_dir() {
        let mut entries: Vec<PathBuf> = std::fs::read_dir(path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()))
            .map(|entry| entry.expect("a directory entry").path())
            .collect();
        entries.sort();
        for entry in entries {
            collect(&entry, files);
        }
    } else if path
        .extension()
        .is_some_and(|ext| ext == "yaml" || ext == "yml")
    {
        files.push(path.to_path_buf());
    } else if !path.exists() {
        panic!("{} is missing", path.display());
    }
}
