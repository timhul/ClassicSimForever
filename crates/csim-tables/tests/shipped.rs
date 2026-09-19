//! The shipped `data/spells/*.yaml` files are what `export-spells` produces from the table dump.
//! Needs `data/tables/` (gitignored); without it the test passes trivially with a note.

use std::path::Path;

use csim_engine::faction::PlayerClass;
use csim_engine::spell::overrides::Overrides;
use csim_engine::spell::record::OVERRIDES_DIR;
use csim_tables::export;
use csim_tables::{TableDir, Tables};

#[test]
fn shipped_spell_files_match_a_fresh_export() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let tables_dir = root.join("data/tables");
    let spells_dir = root.join("data/spells");
    let build = std::fs::read_to_string(spells_dir.join("warrior.yaml"))
        .unwrap()
        .lines()
        .find_map(|l| l.strip_prefix("build: "))
        .expect("the shipped file names its build")
        .trim()
        .to_owned();
    let Ok(dir) = TableDir::open_build(&tables_dir, &build) else {
        eprintln!("data/tables/*.{build}.csv not present, skipping the export parity check");
        return;
    };
    let tables = Tables::load(&dir).unwrap();
    let overrides = Overrides::load(&spells_dir.join(OVERRIDES_DIR)).unwrap();

    let warrior = export::export_class(&tables, PlayerClass::Warrior, &overrides).unwrap();
    let rendered = export::render(&warrior, "export-spells --class warrior").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("warrior.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/warrior.yaml is stale: re-run `csim-tables export-spells --class warrior`"
    );

    let racials = export::export_racials(&tables, &overrides).unwrap();
    let rendered = export::render(&racials, "export-spells --racials").unwrap();
    let shipped = std::fs::read_to_string(spells_dir.join("racials.yaml")).unwrap();
    assert!(
        rendered == shipped.replace("\r\n", "\n"),
        "data/spells/racials.yaml is stale: re-run `csim-tables export-spells --racials`"
    );
}
