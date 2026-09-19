//! Exporters: turn the loaded tables into the YAML files under `data/`.

pub mod spells;

pub use spells::{export_class, export_racials, render, ExportError};
