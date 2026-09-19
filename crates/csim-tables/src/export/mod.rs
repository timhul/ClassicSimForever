//! Exporters: turn the loaded tables into the YAML files under `data/`.

pub mod prune;
pub mod spells;

pub use prune::{prune, PruneReport};
pub use spells::{
    export_class, export_class_with_report, export_racials, export_racials_with_report, render,
    ExportError,
};
