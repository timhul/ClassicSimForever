//! `export-spells`: the spellbook of a class, or the racials, as `data/spells/*.yaml`.
//!
//! The walk follows `data/SPELL_INSTRUCTIONS.md` §1.3–1.6: the class's skill lines give the
//! trainable and talent-granted abilities, the Trait tree gives every talent spell (Forever-only
//! talents are not in any skill line), and the closure over `EffectTriggerSpell`,
//! `OVERRIDE_ACTIONBAR_SPELLS`, required auras and the overrides' references pulls in the hidden
//! payloads. NPC spells are never reached because the walk only starts from a class or race.
//! The result is pruned ([`crate::export::prune`]): effects the simulator has no use for and
//! spells left with nothing to do are dropped. The external buffs (`data/external_buffs.yaml`)
//! are walked the same way from the aura spells the registry names.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use csim_engine::buff::external::ExternalBuffDb;
use csim_engine::faction::PlayerClass;
use csim_engine::rulesets::Ruleset;
use csim_engine::spell::dbc::{
    AuraState, AuraType, DefenseType, ImplicitTarget, Mechanic, PowerType, ProcFlags,
    SpellEffectName, SpellSchoolMask,
};
use csim_engine::spell::overrides::Overrides;
use csim_engine::spell::record::{
    AuraOptions, AuraRestrictions, Categories, ClassOptions, Cooldown, EffectRecord, EquippedItems,
    Levels, PowerCost, SpellFile, SpellRecord,
};

use crate::db::Tables;
use crate::export::prune::{prune, PruneReport};
use crate::tables::SkillLineAbilityRow;

/// `SkillLine.CategoryID` of class skill lines.
const CLASS_SKILL_CATEGORY: u32 = 7;
/// `SkillLine.CategoryID` of racial and secondary skill lines.
const RACIAL_SKILL_CATEGORY: u32 = 9;

#[derive(Debug, thiserror::Error)]
pub enum ExportError {
    #[error("no ChrClasses row has the token {0:?}")]
    UnknownClass(String),
    #[error("class {0:?} has no class skill lines (SkillRaceClassInfo / SkillLine category 7)")]
    NoSkillLines(String),
    #[error("no racial skill lines found (SkillLine category 9 named \"... Racial\")")]
    NoRacialLines,
    #[error("external buff spells not in the tables: {0:?}")]
    MissingSeeds(Vec<u32>),
    #[error("class {0:?} has no Trait tree (SkillLineXTraitTree) or no tab groups")]
    NoTraitTree(String),
    #[error("talent node {0} has several prerequisites {1:?}; the schema allows one")]
    SeveralPrerequisites(u32, Vec<u32>),
    #[error("the exported talent tree of {0} is inconsistent: {1}")]
    InvalidTalents(String, #[source] csim_engine::talent::TalentSpecError),
    #[error("cannot read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cannot parse {path}: {source}")]
    Yaml {
        path: PathBuf,
        #[source]
        source: serde_yaml::Error,
    },
}

/// The `ChrClasses` row of `class`, matched on the upper-case `Filename` token (`WARRIOR`).
pub fn chr_class_id(tables: &Tables, class: PlayerClass) -> Result<u32, ExportError> {
    let token = class.name().to_uppercase();
    tables
        .chr_classes()
        .find(|c| c.filename == token)
        .map(|c| c.id)
        .ok_or(ExportError::UnknownClass(token))
}

/// The class skill lines of `class` (Warrior: 26 Arms, 256 Fury, 257 Protection), sorted.
pub fn class_skill_lines(tables: &Tables, class: PlayerClass) -> Result<Vec<u32>, ExportError> {
    let class_bit = 1u32 << (chr_class_id(tables, class)? - 1);
    let mut lines: Vec<u32> = tables
        .skill_race_class_info()
        .iter()
        .filter(|r| r.class_mask == class_bit)
        .map(|r| r.skill_id)
        .filter(|id| {
            tables
                .skill_line(*id)
                .is_some_and(|l| l.category_id == CLASS_SKILL_CATEGORY)
        })
        .collect();
    lines.sort_unstable();
    lines.dedup();
    if lines.is_empty() {
        return Err(ExportError::NoSkillLines(class.name().to_owned()));
    }
    Ok(lines)
}

/// The racial skill lines ("<Race> Racial" / "Racial - <Race>"), sorted.
pub fn racial_skill_lines(tables: &Tables) -> Result<Vec<u32>, ExportError> {
    let mut lines: Vec<u32> = tables
        .skill_lines()
        .filter(|l| l.category_id == RACIAL_SKILL_CATEGORY && l.display_name.contains("Racial"))
        .map(|l| l.id)
        .collect();
    lines.sort_unstable();
    if lines.is_empty() {
        return Err(ExportError::NoRacialLines);
    }
    Ok(lines)
}

/// The talent spells of `class`'s Trait tree (`TraitDefinition.SpellID` plus the override /
/// visible spells a definition names), sorted.
pub fn talent_spells(tables: &Tables, class: PlayerClass) -> Result<Vec<u32>, ExportError> {
    let lines = class_skill_lines(tables, class)?;
    let mut ids = BTreeSet::new();
    for link in tables.skill_line_x_trait_tree() {
        if !lines.contains(&link.skill_line_id) {
            continue;
        }
        for &node in tables.trait_nodes_of_tree(link.trait_tree_id) {
            for &entry in tables.trait_node_entries_of_node(node) {
                let Some(entry) = tables.trait_node_entry(entry) else {
                    continue;
                };
                let Some(definition) = tables.trait_definition(entry.trait_definition_id) else {
                    continue;
                };
                for id in [
                    definition.spell_id,
                    definition.overrides_spell_id,
                    definition.visible_spell_id,
                ] {
                    if id != 0 && tables.spell_exists(id) {
                        ids.insert(id);
                    }
                }
            }
        }
    }
    Ok(ids.into_iter().collect())
}

/// Builds the spell walk for `class` and returns its pruned file.
pub fn export_class(
    tables: &Tables,
    class: PlayerClass,
    overrides: &Overrides,
) -> Result<SpellFile, ExportError> {
    export_class_with_report(tables, class, overrides).map(|(file, _)| file)
}

/// [`export_class`] plus what the pruning removed.
pub fn export_class_with_report(
    tables: &Tables,
    class: PlayerClass,
    overrides: &Overrides,
) -> Result<(SpellFile, PruneReport), ExportError> {
    let lines = class_skill_lines(tables, class)?;
    let mut ids: BTreeSet<u32> = abilities_in(tables, &lines).keys().copied().collect();
    ids.extend(talent_spells(tables, class)?);
    let mut file = build_file(tables, Some(class), &lines, ids, overrides);
    let report = prune(&mut file, overrides);
    Ok((file, report))
}

/// Builds the racial walk and returns its pruned file (`class` absent).
pub fn export_racials(tables: &Tables, overrides: &Overrides) -> Result<SpellFile, ExportError> {
    export_racials_with_report(tables, overrides).map(|(file, _)| file)
}

/// [`export_racials`] plus what the pruning removed.
pub fn export_racials_with_report(
    tables: &Tables,
    overrides: &Overrides,
) -> Result<(SpellFile, PruneReport), ExportError> {
    let lines = racial_skill_lines(tables)?;
    let ids: BTreeSet<u32> = abilities_in(tables, &lines).keys().copied().collect();
    let mut file = build_file(tables, None, &lines, ids, overrides);
    let report = prune(&mut file, overrides);
    Ok((file, report))
}

/// Builds the equipment walk (`data/spells/enchants.yaml`): the closure of the spells the
/// procs of `data/enchants.yaml` name, the same way as [`export_externals`]. The records are
/// never learned; a character registers the proc when it equips the enchant.
pub fn export_enchants(
    tables: &Tables,
    seeds: &BTreeSet<u32>,
    exclude: &BTreeSet<u32>,
    overrides: &Overrides,
) -> Result<(SpellFile, PruneReport), ExportError> {
    export_externals_with_report(tables, seeds, exclude, overrides)
}

/// The seeds of the external buff walk: the aura spells `data/external_buffs.yaml` names and
/// the rulesets' spells (Essence of the Red), which are external to the character too.
pub fn external_seeds(registry: &ExternalBuffDb) -> BTreeSet<u32> {
    let mut seeds = registry.spell_ids();
    seeds.extend(Ruleset::all_spells());
    seeds
}

/// Builds the external buff walk (`data/spells/externals.yaml`): the closure of `seeds`
/// ([`external_seeds`]), pruned, minus `exclude` (the ids another
/// file in `data/spells/` already carries, which the engine loads either way). The file has no
/// class and its records no skill line: they are never learned, only turned into buffs.
pub fn export_externals(
    tables: &Tables,
    seeds: &BTreeSet<u32>,
    exclude: &BTreeSet<u32>,
    overrides: &Overrides,
) -> Result<SpellFile, ExportError> {
    export_externals_with_report(tables, seeds, exclude, overrides).map(|(file, _)| file)
}

/// [`export_externals`] plus what the pruning removed.
pub fn export_externals_with_report(
    tables: &Tables,
    seeds: &BTreeSet<u32>,
    exclude: &BTreeSet<u32>,
    overrides: &Overrides,
) -> Result<(SpellFile, PruneReport), ExportError> {
    let missing: Vec<u32> = seeds
        .iter()
        .copied()
        .filter(|id| !tables.spell_exists(*id))
        .collect();
    if !missing.is_empty() {
        return Err(ExportError::MissingSeeds(missing));
    }
    let mut file = build_file(tables, None, &[], seeds.clone(), overrides);
    file.learnable = false;
    file.spells.retain(|record| !exclude.contains(&record.id));
    let report = prune(&mut file, overrides);
    Ok((file, report))
}

/// The spell ids of every `*.yaml` / `*.yml` file directly in `spells_dir` except `except`
/// (by file name): what an export must not repeat because `SpellDb::load` reads them all.
pub fn spell_ids_in_dir(spells_dir: &Path, except: &str) -> Result<BTreeSet<u32>, ExportError> {
    let mut ids = BTreeSet::new();
    let entries = std::fs::read_dir(spells_dir).map_err(|source| ExportError::Io {
        path: spells_dir.to_path_buf(),
        source,
    })?;
    for entry in entries.filter_map(Result::ok) {
        let path = entry.path();
        let is_yaml = path
            .extension()
            .is_some_and(|ext| ext == "yaml" || ext == "yml");
        if !path.is_file() || !is_yaml || path.file_name().is_some_and(|name| name == except) {
            continue;
        }
        let text = std::fs::read_to_string(&path).map_err(|source| ExportError::Io {
            path: path.clone(),
            source,
        })?;
        let file: SpellFile =
            serde_yaml::from_str(&text).map_err(|source| ExportError::Yaml { path, source })?;
        ids.extend(file.spells.iter().map(|record| record.id));
    }
    Ok(ids)
}

/// The `SkillLineAbility` row to describe each spell of `lines` with: a row of the lines whose
/// spell exists, preferring the trainable one (`ClassMask` ≠ 0) when a spell has several.
fn abilities_in<'a>(tables: &'a Tables, lines: &[u32]) -> BTreeMap<u32, &'a SkillLineAbilityRow> {
    let mut by_spell: BTreeMap<u32, &SkillLineAbilityRow> = BTreeMap::new();
    for &line in lines {
        for ability in tables.skill_line_abilities(line) {
            if !tables.spell_exists(ability.spell) {
                continue; // stale row (removed talent rank etc.)
            }
            by_spell
                .entry(ability.spell)
                .and_modify(|current| {
                    if current.class_mask == 0 && ability.class_mask != 0 {
                        *current = ability;
                    }
                })
                .or_insert(ability);
        }
    }
    by_spell
}

fn build_file(
    tables: &Tables,
    class: Option<PlayerClass>,
    lines: &[u32],
    seeds: BTreeSet<u32>,
    overrides: &Overrides,
) -> SpellFile {
    let abilities = abilities_in(tables, lines);
    let mut seeds = seeds;
    // Previous ranks are part of the walk even when their own row is stale.
    seeds.extend(abilities.values().map(|a| a.supercedes_spell));
    let ids = closure(tables, seeds, overrides);
    let spells = ids
        .iter()
        .map(|&id| {
            let mut record = record(tables, id, abilities.get(&id).copied());
            if !ids.contains(&record.supercedes) {
                record.supercedes = 0; // the previous rank no longer exists in this build
            }
            record
        })
        .collect();
    SpellFile {
        build: tables.build().to_owned(),
        class,
        learnable: true,
        spells,
    }
}

/// Every spell reachable from `seeds` through triggers, action-bar overrides, required auras
/// and the overrides' references, restricted to spells that exist.
pub fn closure(tables: &Tables, seeds: BTreeSet<u32>, overrides: &Overrides) -> BTreeSet<u32> {
    let mut done = BTreeSet::new();
    let mut pending: Vec<u32> = seeds.into_iter().collect();
    while let Some(id) = pending.pop() {
        if !tables.spell_exists(id) || !done.insert(id) {
            continue;
        }
        for effect in tables.spell_effects(id) {
            if effect.effect_trigger_spell != 0 {
                pending.push(effect.effect_trigger_spell);
            }
            if effect.effect_aura == AuraType::OverrideActionbarSpells.id() {
                pending.push(effect.effect_misc_value[0] as u32);
                pending.push(effect.effect_base_points as u32);
            }
        }
        if let Some(restrictions) = tables.spell_aura_restrictions(id) {
            pending.extend(
                [
                    restrictions.caster_aura_spell,
                    restrictions.target_aura_spell,
                    restrictions.exclude_caster_aura_spell,
                    restrictions.exclude_target_aura_spell,
                ]
                .into_iter()
                .filter(|&s| s != 0),
            );
        }
        if let Some(spell_override) = overrides.get(id) {
            pending.extend(spell_override.referenced_spells());
        }
    }
    done
}

/// The record of spell `id`, described by `ability` when it sits in one of the walked skill
/// lines.
pub fn record(tables: &Tables, id: u32, ability: Option<&SkillLineAbilityRow>) -> SpellRecord {
    let mut record = SpellRecord::new(id, tables.spell_name(id).unwrap_or(""));
    if let Some(spell) = tables.spell(id) {
        record.rank_text = spell.name_subtext.clone();
        record.description = spell.description.clone();
    }
    if let Some(ability) = ability {
        record.skill_line = Some(ability.skill_line);
        record.class_mask = ability.class_mask;
        record.race_mask = ability.race_masks[0];
        record.supercedes = ability.supercedes_spell;
        record.acquire_method = ability.acquire_method;
    }
    if let Some(misc) = tables.spell_misc(id) {
        record.attributes = misc.attributes;
        record.school_mask = SpellSchoolMask::from_bits(misc.school_mask);
        record.cast_time_ms = tables
            .spell_cast_times(misc.casting_time_index)
            .map_or(0, |c| c.base.max(0) as u32);
        record.duration_ms = (misc.duration_index != 0)
            .then(|| tables.spell_duration(misc.duration_index))
            .flatten()
            .map(|d| d.duration);
        record.range_yd = tables
            .spell_range(misc.range_index)
            .map_or(0.0, |r| r.range_max[0]);
    } else {
        record.school_mask = SpellSchoolMask::empty();
    }
    record.power = tables
        .spell_power(id)
        .iter()
        .map(|p| PowerCost {
            power_type: PowerType::from_id(p.power_type),
            cost: p.mana_cost,
            cost_pct: p.power_cost_pct,
            per_second: p.mana_per_second,
        })
        .collect();
    if let Some(cd) = tables.spell_cooldowns(id) {
        record.cooldown = Cooldown {
            recovery_ms: cd.recovery_time,
            category_recovery_ms: cd.category_recovery_time,
            start_recovery_ms: cd.start_recovery_time,
        };
    }
    if let Some(cat) = tables.spell_categories(id) {
        record.categories = Categories {
            category: cat.category,
            start_recovery_category: cat.start_recovery_category,
            defense_type: DefenseType::from_id(cat.defense_type),
            mechanic: Mechanic::from_id(cat.mechanic),
            dispel_type: cat.dispel_type,
        };
    }
    if let Some(shape) = tables.spell_shapeshift(id) {
        record.shapeshift_mask = shape.shapeshift_mask[0];
        record.shapeshift_exclude = shape.shapeshift_exclude[0];
    }
    if let Some(levels) = tables.spell_levels(id) {
        record.levels = Levels {
            base: levels.base_level,
            spell: levels.spell_level,
            max: levels.max_level,
        };
    }
    if let Some(aura) = tables.spell_aura_options(id) {
        record.aura_options = AuraOptions {
            proc_chance: aura.proc_chance,
            proc_charges: aura.proc_charges,
            proc_category_recovery_ms: aura.proc_category_recovery,
            proc_type_mask: ProcFlags::from_bits(aura.proc_type_mask[0]),
            ppm: tables
                .spell_procs_per_minute(aura.spell_procs_per_minute_id)
                .map_or(0.0, |p| p.base_proc_rate),
            max_stacks: aura.cumulative_aura,
        };
    }
    if let Some(class) = tables.spell_class_options(id) {
        record.class_options = Some(ClassOptions {
            set: class.spell_class_set,
            mask: class.spell_class_mask,
        });
    }
    if let Some(equip) = tables.spell_equipped_items(id) {
        if equip.equipped_item_class >= 0 {
            record.equipped_items = Some(EquippedItems {
                class: equip.equipped_item_class,
                subclass_mask: equip.equipped_item_subclass,
                inv_type_mask: equip.equipped_item_inv_types,
            });
        }
    }
    if let Some(aura) = tables.spell_aura_restrictions(id) {
        record.aura_restrictions = AuraRestrictions {
            caster_aura_state: AuraState::from_id(aura.caster_aura_state),
            target_aura_state: AuraState::from_id(aura.target_aura_state),
            exclude_caster_aura_state: AuraState::from_id(aura.exclude_caster_aura_state),
            exclude_target_aura_state: AuraState::from_id(aura.exclude_target_aura_state),
            caster_aura_spell: aura.caster_aura_spell,
            target_aura_spell: aura.target_aura_spell,
            exclude_caster_aura_spell: aura.exclude_caster_aura_spell,
            exclude_target_aura_spell: aura.exclude_target_aura_spell,
        };
    }
    if let Some(target) = tables.spell_target_restrictions(id) {
        record.max_targets = target.max_targets;
    }
    let mut labels = tables.spell_labels(id).to_vec();
    labels.sort_unstable();
    labels.dedup();
    record.labels = labels;
    record.effects = tables
        .spell_effects(id)
        .iter()
        .map(|e| EffectRecord {
            index: e.effect_index,
            effect: SpellEffectName::from_id(e.effect),
            aura: AuraType::from_id(e.effect_aura),
            base_points: e.effect_base_points,
            real_points_per_level: e.effect_real_points_per_level,
            variance: e.variance,
            points_per_resource: e.effect_points_per_resource,
            aura_period_ms: e.effect_aura_period,
            amplitude: e.effect_amplitude,
            chain_amplitude: e.effect_chain_amplitude,
            chain_targets: e.effect_chain_targets,
            trigger_spell: e.effect_trigger_spell,
            bonus_coefficient: e.effect_bonus_coefficient,
            bonus_coefficient_from_ap: e.bonus_coefficient_from_ap,
            mechanic: Mechanic::from_id(e.effect_mechanic),
            misc_value: e.effect_misc_value,
            radius_yd: [
                tables
                    .spell_radius(e.effect_radius_index[0])
                    .map_or(0.0, |r| r.radius),
                tables
                    .spell_radius(e.effect_radius_index[1])
                    .map_or(0.0, |r| r.radius),
            ],
            spell_class_mask: e.effect_spell_class_mask,
            implicit_target: [
                ImplicitTarget::from_id(e.implicit_target[0]),
                ImplicitTarget::from_id(e.implicit_target[1]),
            ],
            attributes: e.effect_attributes,
        })
        .collect();
    record
}

/// Renders a file as YAML with a header naming its origin; the output is deterministic for a
/// given dump so re-exports diff cleanly. Lists of plain numbers and enum names are written in
/// flow style (`[1, 2, 3]`) so a record reads like a table row.
pub fn render(file: &SpellFile, command: &str) -> Result<String, serde_yaml::Error> {
    let body = flow_scalar_sequences(&serde_yaml::to_string(file)?);
    Ok(format!(
        "# Generated by `csim-tables {command}` from client build {}.\n\
         # Do not edit: re-export from a new table dump instead. Hand-written additions go in\n\
         # data/spells/overrides/ (see crates/csim-engine/src/spell/overrides.rs).\n\
         {body}",
        file.build
    ))
}

/// Whether a block-sequence item is a bare number or an upper-case name, i.e. safe to put in a
/// flow sequence unquoted.
fn is_plain_scalar(item: &str) -> bool {
    !item.is_empty()
        && (item.parse::<f64>().is_ok()
            || item
                .chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit() || c == '_'))
}

/// Rewrites a block sequence of plain scalars (serde_yaml's style, items at the key's indent)
/// as a flow sequence: `key: [a, b]`. Other sequences are left alone.
pub fn flow_scalar_sequences(yaml: &str) -> String {
    let lines: Vec<&str> = yaml.lines().collect();
    let mut out = String::with_capacity(yaml.len());
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let indent = line.len() - line.trim_start().len();
        let key = line.trim_start();
        let is_key_only = key.ends_with(':') && !key.starts_with('-') && !key.starts_with('#');
        if is_key_only {
            let mut items = Vec::new();
            let mut j = i + 1;
            while j < lines.len() {
                let candidate = lines[j];
                let candidate_indent = candidate.len() - candidate.trim_start().len();
                let Some(item) = candidate.trim_start().strip_prefix("- ") else {
                    break;
                };
                if candidate_indent != indent || !is_plain_scalar(item) {
                    break;
                }
                items.push(item);
                j += 1;
            }
            if !items.is_empty() {
                out.push_str(&line[..indent]);
                out.push_str(key);
                out.push_str(" [");
                out.push_str(&items.join(", "));
                out.push_str("]\n");
                i = j;
                continue;
            }
        }
        out.push_str(line);
        out.push('\n');
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalar_sequences_become_flow_style() {
        let block = "spells:\n- id: 1\n  attributes:\n  - 5\n  - 0\n  implicit_target:\n  - UNIT_CASTER\n  - NONE\n  power:\n  - type: RAGE\n    cost: 300\n  labels:\n  - 25\n  description: 'a: b'\n";
        let flow = flow_scalar_sequences(block);
        assert_eq!(
            flow,
            "spells:\n- id: 1\n  attributes: [5, 0]\n  implicit_target: [UNIT_CASTER, NONE]\n  power:\n  - type: RAGE\n    cost: 300\n  labels: [25]\n  description: 'a: b'\n"
        );
        let parsed: serde_yaml::Value = serde_yaml::from_str(&flow).unwrap();
        let original: serde_yaml::Value = serde_yaml::from_str(block).unwrap();
        assert_eq!(parsed, original);
    }

    #[test]
    fn plain_scalars_are_numbers_and_names() {
        assert!(is_plain_scalar("-2147483648"));
        assert!(is_plain_scalar("0.5"));
        assert!(is_plain_scalar("UNIT_TARGET_ENEMY"));
        assert!(!is_plain_scalar("Mortal Strike"));
        assert!(!is_plain_scalar("'quoted'"));
        assert!(!is_plain_scalar("type: RAGE"));
        assert!(!is_plain_scalar(""));
    }
}
