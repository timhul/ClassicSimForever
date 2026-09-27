//! [`Tables`]: every table the exporters need, loaded and indexed.

use std::collections::HashMap;
use std::hash::Hash;
use std::path::Path;

use crate::dir::TableDir;
use crate::error::Result;
use crate::row::TableRow;
use crate::tables::*;

/// Indexes `rows` by `key`; when several rows share a key the first one in file order wins.
fn by_key<R, K: Eq + Hash>(rows: Vec<R>, key: impl Fn(&R) -> K) -> HashMap<K, R> {
    let mut map = HashMap::with_capacity(rows.len());
    for row in rows {
        map.entry(key(&row)).or_insert(row);
    }
    map
}

/// Groups `rows` by `key`, keeping file order inside each group.
fn group_by<R, K: Eq + Hash>(rows: Vec<R>, key: impl Fn(&R) -> K) -> HashMap<K, Vec<R>> {
    let mut map: HashMap<K, Vec<R>> = HashMap::new();
    for row in rows {
        map.entry(key(&row)).or_default().push(row);
    }
    map
}

/// The loaded, indexed dump of one build.
///
/// Single-row lookups (`spell_misc`, `spell_cooldowns`, ...) return `None` for spells without a
/// row, which the sparse tables use to mean "no cost / no cooldown / no proc / no level
/// requirement". Multi-row lookups return an empty slice in that case. Rows with a non-zero
/// `DifficultyID` (13 `SpellEffect` rows in the current dump, none of them player spells) are
/// dropped at load time.
#[derive(Debug)]
pub struct Tables {
    build: String,

    spell_names: HashMap<u32, SpellNameRow>,
    spells: HashMap<u32, SpellRow>,
    spell_misc: HashMap<u32, SpellMiscRow>,
    spell_effects: HashMap<u32, Vec<SpellEffectRow>>,
    spell_power: HashMap<u32, Vec<SpellPowerRow>>,
    spell_cooldowns: HashMap<u32, SpellCooldownsRow>,
    spell_categories: HashMap<u32, SpellCategoriesRow>,
    spell_category: HashMap<u32, SpellCategoryRow>,
    spell_duration: HashMap<u32, SpellDurationRow>,
    spell_cast_times: HashMap<u32, SpellCastTimesRow>,
    spell_range: HashMap<u32, SpellRangeRow>,
    spell_radius: HashMap<u32, SpellRadiusRow>,
    spell_levels: HashMap<u32, SpellLevelsRow>,
    spell_aura_options: HashMap<u32, SpellAuraOptionsRow>,
    spell_procs_per_minute: HashMap<u32, SpellProcsPerMinuteRow>,
    spell_class_options: HashMap<u32, SpellClassOptionsRow>,
    spell_shapeshift: HashMap<u32, SpellShapeshiftRow>,
    spell_shapeshift_forms: HashMap<u32, SpellShapeshiftFormRow>,
    spell_aura_restrictions: HashMap<u32, SpellAuraRestrictionsRow>,
    spell_equipped_items: HashMap<u32, SpellEquippedItemsRow>,
    spell_target_restrictions: HashMap<u32, SpellTargetRestrictionsRow>,
    spell_labels: HashMap<u32, Vec<u32>>,
    spell_item_enchantments: HashMap<u32, SpellItemEnchantmentRow>,

    skill_lines: HashMap<u32, SkillLineRow>,
    skill_line_abilities: Vec<SkillLineAbilityRow>,
    abilities_by_skill_line: HashMap<u32, Vec<usize>>,
    abilities_by_spell: HashMap<u32, Vec<usize>>,
    skill_race_class_info: Vec<SkillRaceClassInfoRow>,
    skill_line_x_trait_tree: Vec<SkillLineXTraitTreeRow>,

    trait_trees: HashMap<u32, TraitTreeRow>,
    trait_nodes: HashMap<u32, TraitNodeRow>,
    nodes_by_tree: HashMap<u32, Vec<u32>>,
    trait_node_entries: HashMap<u32, TraitNodeEntryRow>,
    entries_by_node: HashMap<u32, Vec<u32>>,
    trait_definitions: HashMap<u32, TraitDefinitionRow>,
    effect_points_by_definition: HashMap<u32, Vec<TraitDefinitionEffectPointsRow>>,
    trait_edges: Vec<TraitEdgeRow>,
    prerequisites_by_node: HashMap<u32, Vec<u32>>,
    trait_node_groups: HashMap<u32, TraitNodeGroupRow>,
    groups_by_node: HashMap<u32, Vec<u32>>,
    nodes_by_group: HashMap<u32, Vec<u32>>,
    conds_by_group: HashMap<u32, Vec<u32>>,
    group_display_info: HashMap<u32, TraitNodeGroupDisplayInfoRow>,
    trait_conds: HashMap<u32, TraitCondRow>,
    trait_currencies: HashMap<u32, TraitCurrencyRow>,
    trait_currency_sources: Vec<TraitCurrencySourceRow>,
    curves: HashMap<u32, CurveRow>,
    curve_points: HashMap<u32, Vec<CurvePointRow>>,

    chr_classes: HashMap<u32, ChrClassesRow>,
    chr_races: HashMap<u32, ChrRacesRow>,
    /// By class id, sorted by level.
    player_expected_stats: HashMap<u32, Vec<PlayerExpectedStatRow>>,
    char_base_info: Vec<CharBaseInfoRow>,
    power_types: HashMap<u32, PowerTypeRow>,

    items: HashMap<u32, ItemRow>,
    item_sparse: HashMap<u32, ItemSparseRow>,
    item_sub_classes: HashMap<(u32, u32), ItemSubClassRow>,
    rand_prop_points: HashMap<u32, RandPropPointsRow>,
    item_armor_total: HashMap<u32, ItemArmorTotalRow>,
    item_armor_quality: HashMap<u32, ItemArmorQualityRow>,
    item_armor_shield: HashMap<u32, ItemArmorShieldRow>,
    armor_locations: HashMap<u32, ArmorLocationRow>,
    item_damage: HashMap<(ItemDamageTable, u32), ItemDamageRow>,
    item_effects: HashMap<u32, ItemEffectRow>,
    /// Effect ids by item id, sorted by (legacy slot index, effect id).
    effects_by_item: HashMap<u32, Vec<u32>>,
    item_sets: HashMap<u32, ItemSetRow>,
    /// By set id, sorted by (threshold, spell id).
    item_set_spells: HashMap<u32, Vec<ItemSetSpellRow>>,
    item_limit_categories: HashMap<u32, ItemLimitCategoryRow>,
    item_name_descriptions: HashMap<u32, ItemNameDescriptionRow>,
    /// Bonus tree ids by item id, sorted.
    bonus_trees_by_item: HashMap<u32, Vec<u32>>,
    /// By parent tree id, sorted by node id.
    item_bonus_tree_nodes: HashMap<u32, Vec<ItemBonusTreeNodeRow>>,
    /// By bonus list id, sorted by (order index, id).
    item_bonuses: HashMap<u32, Vec<ItemBonusRow>>,
}

/// Reads one `ItemDamage*` table into the shared row layout.
fn read_damage<R: TableRow + Into<ItemDamageRow>>(dir: &TableDir) -> Result<Vec<ItemDamageRow>> {
    Ok(dir.read::<R>()?.into_iter().map(Into::into).collect())
}

fn slice<'a, K: Eq + Hash, R>(map: &'a HashMap<K, Vec<R>>, key: &K) -> &'a [R] {
    map.get(key).map_or(&[], Vec::as_slice)
}

impl Tables {
    /// Loads and indexes every table of [`ALL_TABLES`] from `dir`.
    pub fn load(dir: &TableDir) -> Result<Self> {
        let spell_effects = {
            let mut effects = group_by(
                dir.read_where::<SpellEffectRow>(|e| e.difficulty_id == 0)?,
                |e| e.spell_id,
            );
            for rows in effects.values_mut() {
                rows.sort_by_key(|e| e.effect_index);
            }
            effects
        };
        let spell_power = {
            let mut power = group_by(dir.read::<SpellPowerRow>()?, |p| p.spell_id);
            for rows in power.values_mut() {
                rows.sort_by_key(|p| p.order_index);
            }
            power
        };
        let curve_points = {
            let mut points = group_by(dir.read::<CurvePointRow>()?, |p| p.curve_id);
            for rows in points.values_mut() {
                rows.sort_by(|a, b| a.pos[0].total_cmp(&b.pos[0]));
            }
            points
        };
        let player_expected_stats = {
            let mut stats = group_by(dir.read::<PlayerExpectedStatRow>()?, |r| r.class_id);
            for rows in stats.values_mut() {
                rows.sort_by_key(|r| r.level);
            }
            stats
        };

        let skill_line_abilities = dir.read::<SkillLineAbilityRow>()?;
        let mut abilities_by_skill_line: HashMap<u32, Vec<usize>> = HashMap::new();
        let mut abilities_by_spell: HashMap<u32, Vec<usize>> = HashMap::new();
        for (i, row) in skill_line_abilities.iter().enumerate() {
            abilities_by_skill_line
                .entry(row.skill_line)
                .or_default()
                .push(i);
            abilities_by_spell.entry(row.spell).or_default().push(i);
        }

        let trait_nodes = by_key(dir.read::<TraitNodeRow>()?, |n| n.id);
        let mut nodes_by_tree: HashMap<u32, Vec<u32>> = HashMap::new();
        for node in trait_nodes.values() {
            nodes_by_tree
                .entry(node.trait_tree_id)
                .or_default()
                .push(node.id);
        }
        for ids in nodes_by_tree.values_mut() {
            ids.sort_unstable();
        }

        let entries_by_node = {
            let mut links = dir.read::<TraitNodeXTraitNodeEntryRow>()?;
            links.sort_by_key(|l| (l.trait_node_id, l.index));
            let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
            for link in links {
                map.entry(link.trait_node_id)
                    .or_default()
                    .push(link.trait_node_entry_id);
            }
            map
        };

        let trait_edges = dir.read::<TraitEdgeRow>()?;
        let mut prerequisites_by_node: HashMap<u32, Vec<u32>> = HashMap::new();
        for edge in &trait_edges {
            prerequisites_by_node
                .entry(edge.right_trait_node_id)
                .or_default()
                .push(edge.left_trait_node_id);
        }

        let mut groups_by_node: HashMap<u32, Vec<u32>> = HashMap::new();
        let mut nodes_by_group: HashMap<u32, Vec<u32>> = HashMap::new();
        for link in dir.read::<TraitNodeGroupXTraitNodeRow>()? {
            groups_by_node
                .entry(link.trait_node_id)
                .or_default()
                .push(link.trait_node_group_id);
            nodes_by_group
                .entry(link.trait_node_group_id)
                .or_default()
                .push(link.trait_node_id);
        }

        let mut conds_by_group: HashMap<u32, Vec<u32>> = HashMap::new();
        for link in dir.read::<TraitNodeGroupXTraitCondRow>()? {
            conds_by_group
                .entry(link.trait_node_group_id)
                .or_default()
                .push(link.trait_cond_id);
        }

        let mut spell_labels: HashMap<u32, Vec<u32>> = HashMap::new();
        for label in dir.read::<SpellLabelRow>()? {
            spell_labels
                .entry(label.spell_id)
                .or_default()
                .push(label.label_id);
        }

        let item_damage = {
            let mut damage = HashMap::new();
            for (table, rows) in [
                (
                    ItemDamageTable::OneHand,
                    read_damage::<ItemDamageOneHandRow>(dir)?,
                ),
                (
                    ItemDamageTable::TwoHand,
                    read_damage::<ItemDamageTwoHandRow>(dir)?,
                ),
                (
                    ItemDamageTable::Ranged,
                    read_damage::<ItemDamageRangedRow>(dir)?,
                ),
                (
                    ItemDamageTable::Wand,
                    read_damage::<ItemDamageWandRow>(dir)?,
                ),
                (
                    ItemDamageTable::Thrown,
                    read_damage::<ItemDamageThrownRow>(dir)?,
                ),
            ] {
                for row in rows {
                    damage.entry((table, row.item_level)).or_insert(row);
                }
            }
            damage
        };
        let item_effects = by_key(dir.read::<ItemEffectRow>()?, |r| r.id);
        let effects_by_item = {
            let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
            for link in dir.read::<ItemXItemEffectRow>()? {
                map.entry(link.item_id)
                    .or_default()
                    .push(link.item_effect_id);
            }
            for ids in map.values_mut() {
                ids.sort_by_key(|id| (item_effects.get(id).map(|e| e.legacy_slot_index), *id));
                ids.dedup();
            }
            map
        };
        let item_set_spells = {
            let mut spells = group_by(dir.read::<ItemSetSpellRow>()?, |r| r.item_set_id);
            for rows in spells.values_mut() {
                rows.sort_by_key(|r| (r.threshold, r.spell_id));
            }
            spells
        };
        let bonus_trees_by_item = {
            let mut map: HashMap<u32, Vec<u32>> = HashMap::new();
            for link in dir.read::<ItemXBonusTreeRow>()? {
                map.entry(link.item_id)
                    .or_default()
                    .push(link.item_bonus_tree_id);
            }
            for ids in map.values_mut() {
                ids.sort_unstable();
                ids.dedup();
            }
            map
        };
        let item_bonus_tree_nodes = {
            let mut nodes = group_by(dir.read::<ItemBonusTreeNodeRow>()?, |r| {
                r.parent_item_bonus_tree_id
            });
            for rows in nodes.values_mut() {
                rows.sort_by_key(|r| r.id);
            }
            nodes
        };
        let item_bonuses = {
            let mut bonuses =
                group_by(dir.read::<ItemBonusRow>()?, |r| r.parent_item_bonus_list_id);
            for rows in bonuses.values_mut() {
                rows.sort_by_key(|r| (r.order_index, r.id));
            }
            bonuses
        };

        Ok(Self {
            build: dir.build().to_owned(),
            spell_names: by_key(dir.read::<SpellNameRow>()?, |r| r.id),
            spells: by_key(dir.read::<SpellRow>()?, |r| r.id),
            spell_misc: by_key(
                dir.read_where::<SpellMiscRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_effects,
            spell_power,
            spell_cooldowns: by_key(
                dir.read_where::<SpellCooldownsRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_categories: by_key(
                dir.read_where::<SpellCategoriesRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_category: by_key(dir.read::<SpellCategoryRow>()?, |r| r.id),
            spell_duration: by_key(dir.read::<SpellDurationRow>()?, |r| r.id),
            spell_cast_times: by_key(dir.read::<SpellCastTimesRow>()?, |r| r.id),
            spell_range: by_key(dir.read::<SpellRangeRow>()?, |r| r.id),
            spell_radius: by_key(dir.read::<SpellRadiusRow>()?, |r| r.id),
            spell_levels: by_key(
                dir.read_where::<SpellLevelsRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_aura_options: by_key(
                dir.read_where::<SpellAuraOptionsRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_procs_per_minute: by_key(dir.read::<SpellProcsPerMinuteRow>()?, |r| r.id),
            spell_class_options: by_key(dir.read::<SpellClassOptionsRow>()?, |r| r.spell_id),
            spell_shapeshift: by_key(dir.read::<SpellShapeshiftRow>()?, |r| r.spell_id),
            spell_shapeshift_forms: by_key(dir.read::<SpellShapeshiftFormRow>()?, |r| r.id),
            spell_aura_restrictions: by_key(
                dir.read_where::<SpellAuraRestrictionsRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_equipped_items: by_key(dir.read::<SpellEquippedItemsRow>()?, |r| r.spell_id),
            spell_target_restrictions: by_key(
                dir.read_where::<SpellTargetRestrictionsRow>(|r| r.difficulty_id == 0)?,
                |r| r.spell_id,
            ),
            spell_labels,
            spell_item_enchantments: by_key(dir.read::<SpellItemEnchantmentRow>()?, |r| r.id),

            skill_lines: by_key(dir.read::<SkillLineRow>()?, |r| r.id),
            skill_line_abilities,
            abilities_by_skill_line,
            abilities_by_spell,
            skill_race_class_info: dir.read()?,
            skill_line_x_trait_tree: dir.read()?,

            trait_trees: by_key(dir.read::<TraitTreeRow>()?, |r| r.id),
            trait_nodes,
            nodes_by_tree,
            trait_node_entries: by_key(dir.read::<TraitNodeEntryRow>()?, |r| r.id),
            entries_by_node,
            trait_definitions: by_key(dir.read::<TraitDefinitionRow>()?, |r| r.id),
            effect_points_by_definition: group_by(
                dir.read::<TraitDefinitionEffectPointsRow>()?,
                |r| r.trait_definition_id,
            ),
            trait_edges,
            prerequisites_by_node,
            trait_node_groups: by_key(dir.read::<TraitNodeGroupRow>()?, |r| r.id),
            groups_by_node,
            nodes_by_group,
            conds_by_group,
            group_display_info: by_key(dir.read::<TraitNodeGroupDisplayInfoRow>()?, |r| {
                r.trait_node_group_id
            }),
            trait_conds: by_key(dir.read::<TraitCondRow>()?, |r| r.id),
            trait_currencies: by_key(dir.read::<TraitCurrencyRow>()?, |r| r.id),
            trait_currency_sources: dir.read()?,
            curves: by_key(dir.read::<CurveRow>()?, |r| r.id),
            curve_points,

            chr_classes: by_key(dir.read::<ChrClassesRow>()?, |r| r.id),
            chr_races: by_key(dir.read::<ChrRacesRow>()?, |r| r.id),
            player_expected_stats,
            char_base_info: dir.read()?,
            power_types: by_key(dir.read::<PowerTypeRow>()?, |r| r.power_type_enum),

            items: by_key(dir.read::<ItemRow>()?, |r| r.id),
            item_sparse: by_key(dir.read::<ItemSparseRow>()?, |r| r.id),
            item_sub_classes: by_key(dir.read::<ItemSubClassRow>()?, |r| {
                (r.class_id, r.sub_class_id)
            }),
            rand_prop_points: by_key(dir.read::<RandPropPointsRow>()?, |r| r.id),
            item_armor_total: by_key(dir.read::<ItemArmorTotalRow>()?, |r| r.item_level),
            item_armor_quality: by_key(dir.read::<ItemArmorQualityRow>()?, |r| r.id),
            item_armor_shield: by_key(dir.read::<ItemArmorShieldRow>()?, |r| r.item_level),
            armor_locations: by_key(dir.read::<ArmorLocationRow>()?, |r| r.id),
            item_damage,
            item_effects,
            effects_by_item,
            item_sets: by_key(dir.read::<ItemSetRow>()?, |r| r.id),
            item_set_spells,
            item_limit_categories: by_key(dir.read::<ItemLimitCategoryRow>()?, |r| r.id),
            item_name_descriptions: by_key(dir.read::<ItemNameDescriptionRow>()?, |r| r.id),
            bonus_trees_by_item,
            item_bonus_tree_nodes,
            item_bonuses,
        })
    }

    /// Opens the single build in `dir` and loads it.
    pub fn load_dir(dir: impl AsRef<Path>) -> Result<Self> {
        Self::load(&TableDir::open(dir)?)
    }

    /// The build the tables belong to.
    pub fn build(&self) -> &str {
        &self.build
    }

    // ----- Spell* --------------------------------------------------------------------------

    /// Whether a spell with this id exists (has a `SpellName` row).
    pub fn spell_exists(&self, spell_id: u32) -> bool {
        self.spell_names.contains_key(&spell_id)
    }

    /// The spell's display name.
    pub fn spell_name(&self, spell_id: u32) -> Option<&str> {
        self.spell_names.get(&spell_id).map(|r| r.name.as_str())
    }

    /// Every spell id with a `SpellName` row, unsorted.
    pub fn spell_ids(&self) -> impl Iterator<Item = u32> + '_ {
        self.spell_names.keys().copied()
    }

    /// `Spell` (texts).
    pub fn spell(&self, spell_id: u32) -> Option<&SpellRow> {
        self.spells.get(&spell_id)
    }

    /// `SpellMisc`.
    pub fn spell_misc(&self, spell_id: u32) -> Option<&SpellMiscRow> {
        self.spell_misc.get(&spell_id)
    }

    /// `SpellEffect` rows of the spell, sorted by effect index.
    pub fn spell_effects(&self, spell_id: u32) -> &[SpellEffectRow] {
        slice(&self.spell_effects, &spell_id)
    }

    /// `SpellPower` rows of the spell, sorted by order index.
    pub fn spell_power(&self, spell_id: u32) -> &[SpellPowerRow] {
        slice(&self.spell_power, &spell_id)
    }

    /// `SpellCooldowns`.
    pub fn spell_cooldowns(&self, spell_id: u32) -> Option<&SpellCooldownsRow> {
        self.spell_cooldowns.get(&spell_id)
    }

    /// `SpellCategories`.
    pub fn spell_categories(&self, spell_id: u32) -> Option<&SpellCategoriesRow> {
        self.spell_categories.get(&spell_id)
    }

    /// `SpellCategory` definition by category id.
    pub fn spell_category(&self, category_id: u32) -> Option<&SpellCategoryRow> {
        self.spell_category.get(&category_id)
    }

    /// `SpellDuration` by duration index.
    pub fn spell_duration(&self, duration_index: u32) -> Option<&SpellDurationRow> {
        self.spell_duration.get(&duration_index)
    }

    /// `SpellCastTimes` by casting time index.
    pub fn spell_cast_times(&self, casting_time_index: u32) -> Option<&SpellCastTimesRow> {
        self.spell_cast_times.get(&casting_time_index)
    }

    /// `SpellRange` by range index.
    pub fn spell_range(&self, range_index: u32) -> Option<&SpellRangeRow> {
        self.spell_range.get(&range_index)
    }

    /// `SpellRadius` by radius index.
    pub fn spell_radius(&self, radius_index: u32) -> Option<&SpellRadiusRow> {
        self.spell_radius.get(&radius_index)
    }

    /// `SpellLevels`.
    pub fn spell_levels(&self, spell_id: u32) -> Option<&SpellLevelsRow> {
        self.spell_levels.get(&spell_id)
    }

    /// `SpellAuraOptions`.
    pub fn spell_aura_options(&self, spell_id: u32) -> Option<&SpellAuraOptionsRow> {
        self.spell_aura_options.get(&spell_id)
    }

    /// `SpellProcsPerMinute` by id.
    pub fn spell_procs_per_minute(&self, id: u32) -> Option<&SpellProcsPerMinuteRow> {
        self.spell_procs_per_minute.get(&id)
    }

    /// `SpellClassOptions`.
    pub fn spell_class_options(&self, spell_id: u32) -> Option<&SpellClassOptionsRow> {
        self.spell_class_options.get(&spell_id)
    }

    /// `SpellItemEnchantment` by id.
    pub fn spell_item_enchantment(&self, id: u32) -> Option<&SpellItemEnchantmentRow> {
        self.spell_item_enchantments.get(&id)
    }

    /// Every `SpellClassOptions` row, for family-wide queries.
    pub fn spell_class_options_iter(&self) -> impl Iterator<Item = &SpellClassOptionsRow> {
        self.spell_class_options.values()
    }

    /// `SpellShapeshift`.
    pub fn spell_shapeshift(&self, spell_id: u32) -> Option<&SpellShapeshiftRow> {
        self.spell_shapeshift.get(&spell_id)
    }

    /// `SpellShapeshiftForm` by form id.
    pub fn spell_shapeshift_form(&self, form_id: u32) -> Option<&SpellShapeshiftFormRow> {
        self.spell_shapeshift_forms.get(&form_id)
    }

    /// `SpellAuraRestrictions`.
    pub fn spell_aura_restrictions(&self, spell_id: u32) -> Option<&SpellAuraRestrictionsRow> {
        self.spell_aura_restrictions.get(&spell_id)
    }

    /// `SpellEquippedItems`.
    pub fn spell_equipped_items(&self, spell_id: u32) -> Option<&SpellEquippedItemsRow> {
        self.spell_equipped_items.get(&spell_id)
    }

    /// `SpellTargetRestrictions`.
    pub fn spell_target_restrictions(&self, spell_id: u32) -> Option<&SpellTargetRestrictionsRow> {
        self.spell_target_restrictions.get(&spell_id)
    }

    /// Label ids of the spell.
    pub fn spell_labels(&self, spell_id: u32) -> &[u32] {
        slice(&self.spell_labels, &spell_id)
    }

    // ----- SkillLine* ----------------------------------------------------------------------

    /// `SkillLine` by id.
    pub fn skill_line(&self, skill_line_id: u32) -> Option<&SkillLineRow> {
        self.skill_lines.get(&skill_line_id)
    }

    /// Every `SkillLine` row.
    pub fn skill_lines(&self) -> impl Iterator<Item = &SkillLineRow> {
        self.skill_lines.values()
    }

    /// `SkillLineAbility` rows of a skill line, in file order.
    pub fn skill_line_abilities(
        &self,
        skill_line_id: u32,
    ) -> impl Iterator<Item = &SkillLineAbilityRow> {
        slice(&self.abilities_by_skill_line, &skill_line_id)
            .iter()
            .map(|&i| &self.skill_line_abilities[i])
    }

    /// `SkillLineAbility` rows that grant a spell.
    pub fn skill_line_abilities_of_spell(
        &self,
        spell_id: u32,
    ) -> impl Iterator<Item = &SkillLineAbilityRow> {
        slice(&self.abilities_by_spell, &spell_id)
            .iter()
            .map(|&i| &self.skill_line_abilities[i])
    }

    /// Every `SkillRaceClassInfo` row.
    pub fn skill_race_class_info(&self) -> &[SkillRaceClassInfoRow] {
        &self.skill_race_class_info
    }

    /// Every `SkillLineXTraitTree` row.
    pub fn skill_line_x_trait_tree(&self) -> &[SkillLineXTraitTreeRow] {
        &self.skill_line_x_trait_tree
    }

    // ----- Trait* --------------------------------------------------------------------------

    /// `TraitTree` by id.
    pub fn trait_tree(&self, tree_id: u32) -> Option<&TraitTreeRow> {
        self.trait_trees.get(&tree_id)
    }

    /// `TraitNode` by id.
    pub fn trait_node(&self, node_id: u32) -> Option<&TraitNodeRow> {
        self.trait_nodes.get(&node_id)
    }

    /// Node ids of a tree, sorted.
    pub fn trait_nodes_of_tree(&self, tree_id: u32) -> &[u32] {
        slice(&self.nodes_by_tree, &tree_id)
    }

    /// `TraitNodeEntry` by id.
    pub fn trait_node_entry(&self, entry_id: u32) -> Option<&TraitNodeEntryRow> {
        self.trait_node_entries.get(&entry_id)
    }

    /// Entry ids of a node, in `_Index` order.
    pub fn trait_node_entries_of_node(&self, node_id: u32) -> &[u32] {
        slice(&self.entries_by_node, &node_id)
    }

    /// `TraitDefinition` by id.
    pub fn trait_definition(&self, definition_id: u32) -> Option<&TraitDefinitionRow> {
        self.trait_definitions.get(&definition_id)
    }

    /// `TraitDefinitionEffectPoints` rows of a definition.
    pub fn trait_definition_effect_points(
        &self,
        definition_id: u32,
    ) -> &[TraitDefinitionEffectPointsRow] {
        slice(&self.effect_points_by_definition, &definition_id)
    }

    /// Every `TraitEdge` row.
    pub fn trait_edges(&self) -> &[TraitEdgeRow] {
        &self.trait_edges
    }

    /// Nodes that must be maxed before `node_id` (left ends of the edges pointing at it).
    pub fn trait_prerequisites(&self, node_id: u32) -> &[u32] {
        slice(&self.prerequisites_by_node, &node_id)
    }

    /// `TraitNodeGroup` by id.
    pub fn trait_node_group(&self, group_id: u32) -> Option<&TraitNodeGroupRow> {
        self.trait_node_groups.get(&group_id)
    }

    /// Groups a node belongs to.
    pub fn trait_groups_of_node(&self, node_id: u32) -> &[u32] {
        slice(&self.groups_by_node, &node_id)
    }

    /// Nodes of a group.
    pub fn trait_nodes_of_group(&self, group_id: u32) -> &[u32] {
        slice(&self.nodes_by_group, &group_id)
    }

    /// Condition ids attached to a group.
    pub fn trait_conds_of_group(&self, group_id: u32) -> &[u32] {
        slice(&self.conds_by_group, &group_id)
    }

    /// `TraitNodeGroupDisplayInfo` of a group (tab groups only).
    pub fn trait_group_display_info(&self, group_id: u32) -> Option<&TraitNodeGroupDisplayInfoRow> {
        self.group_display_info.get(&group_id)
    }

    /// `TraitCond` by id.
    pub fn trait_cond(&self, cond_id: u32) -> Option<&TraitCondRow> {
        self.trait_conds.get(&cond_id)
    }

    /// `TraitCurrency` by id.
    pub fn trait_currency(&self, currency_id: u32) -> Option<&TraitCurrencyRow> {
        self.trait_currencies.get(&currency_id)
    }

    /// Every `TraitCurrencySource` row.
    pub fn trait_currency_sources(&self) -> &[TraitCurrencySourceRow] {
        &self.trait_currency_sources
    }

    /// `Curve` by id.
    pub fn curve(&self, curve_id: u32) -> Option<&CurveRow> {
        self.curves.get(&curve_id)
    }

    /// `CurvePoint` rows of a curve, sorted by `pos[0]` (rank).
    pub fn curve_points(&self, curve_id: u32) -> &[CurvePointRow] {
        slice(&self.curve_points, &curve_id)
    }

    // ----- Chr* / PowerType ----------------------------------------------------------------

    /// `ChrClasses` by class id.
    pub fn chr_class(&self, class_id: u32) -> Option<&ChrClassesRow> {
        self.chr_classes.get(&class_id)
    }

    /// Every `ChrClasses` row.
    pub fn chr_classes(&self) -> impl Iterator<Item = &ChrClassesRow> {
        self.chr_classes.values()
    }

    /// The `PlayerExpectedStat` row of a class at a level.
    pub fn player_expected_stat(
        &self,
        class_id: u32,
        level: u32,
    ) -> Option<&PlayerExpectedStatRow> {
        slice(&self.player_expected_stats, &class_id)
            .iter()
            .find(|row| row.level == level)
    }

    /// The race ids `CharBaseInfo` allows for a class, sorted.
    pub fn races_of_class(&self, class_id: u32) -> Vec<u32> {
        let mut races: Vec<u32> = self
            .char_base_info
            .iter()
            .filter(|row| row.class_id == class_id)
            .map(|row| row.race_id)
            .collect();
        races.sort_unstable();
        races.dedup();
        races
    }

    /// `ChrRaces` by race id.
    pub fn chr_race(&self, race_id: u32) -> Option<&ChrRacesRow> {
        self.chr_races.get(&race_id)
    }

    /// Every `ChrRaces` row.
    pub fn chr_races(&self) -> impl Iterator<Item = &ChrRacesRow> {
        self.chr_races.values()
    }

    /// `PowerType` by `PowerTypeEnum` (0 mana, 1 rage, ...).
    pub fn power_type(&self, power_type_enum: u32) -> Option<&PowerTypeRow> {
        self.power_types.get(&power_type_enum)
    }

    // ----- Item* ---------------------------------------------------------------------------

    /// `Item` by item id.
    pub fn item(&self, item_id: u32) -> Option<&ItemRow> {
        self.items.get(&item_id)
    }

    /// Every `Item` row, unsorted.
    pub fn items(&self) -> impl Iterator<Item = &ItemRow> {
        self.items.values()
    }

    /// `ItemSparse` by item id; `None` for the items the dump has no row for.
    pub fn item_sparse(&self, item_id: u32) -> Option<&ItemSparseRow> {
        self.item_sparse.get(&item_id)
    }

    /// Every `ItemSparse` row, unsorted.
    pub fn item_sparse_rows(&self) -> impl Iterator<Item = &ItemSparseRow> {
        self.item_sparse.values()
    }

    /// `ItemSubClass` by (`ClassID`, `SubClassID`).
    pub fn item_sub_class(&self, class_id: u32, sub_class_id: u32) -> Option<&ItemSubClassRow> {
        self.item_sub_classes.get(&(class_id, sub_class_id))
    }

    /// `RandPropPoints` of an item level.
    pub fn rand_prop_points(&self, item_level: u32) -> Option<&RandPropPointsRow> {
        self.rand_prop_points.get(&item_level)
    }

    /// `ItemArmorTotal` of an item level.
    pub fn item_armor_total(&self, item_level: u32) -> Option<&ItemArmorTotalRow> {
        self.item_armor_total.get(&item_level)
    }

    /// `ItemArmorQuality` of an item level.
    pub fn item_armor_quality(&self, item_level: u32) -> Option<&ItemArmorQualityRow> {
        self.item_armor_quality.get(&item_level)
    }

    /// `ItemArmorShield` of an item level.
    pub fn item_armor_shield(&self, item_level: u32) -> Option<&ItemArmorShieldRow> {
        self.item_armor_shield.get(&item_level)
    }

    /// `ArmorLocation` of an `InventoryType`.
    pub fn armor_location(&self, inventory_type: u32) -> Option<&ArmorLocationRow> {
        self.armor_locations.get(&inventory_type)
    }

    /// The row of an `ItemDamage*` table at an item level.
    pub fn item_damage(&self, table: ItemDamageTable, item_level: u32) -> Option<&ItemDamageRow> {
        self.item_damage.get(&(table, item_level))
    }

    /// `ItemEffect` by effect id.
    pub fn item_effect(&self, effect_id: u32) -> Option<&ItemEffectRow> {
        self.item_effects.get(&effect_id)
    }

    /// The effects of an item, by legacy slot index.
    pub fn item_effects_of_item(&self, item_id: u32) -> impl Iterator<Item = &ItemEffectRow> {
        slice(&self.effects_by_item, &item_id)
            .iter()
            .filter_map(|id| self.item_effects.get(id))
    }

    /// `ItemSet` by set id.
    pub fn item_set(&self, set_id: u32) -> Option<&ItemSetRow> {
        self.item_sets.get(&set_id)
    }

    /// Every `ItemSet` row, unsorted.
    pub fn item_sets(&self) -> impl Iterator<Item = &ItemSetRow> {
        self.item_sets.values()
    }

    /// The bonuses of a set, sorted by threshold.
    pub fn item_set_spells(&self, set_id: u32) -> &[ItemSetSpellRow] {
        slice(&self.item_set_spells, &set_id)
    }

    /// `ItemLimitCategory` by id.
    pub fn item_limit_category(&self, category_id: u32) -> Option<&ItemLimitCategoryRow> {
        self.item_limit_categories.get(&category_id)
    }

    /// `ItemNameDescription` by id.
    pub fn item_name_description(&self, id: u32) -> Option<&ItemNameDescriptionRow> {
        self.item_name_descriptions.get(&id)
    }

    /// The bonus tree ids of an item, sorted.
    pub fn item_bonus_trees(&self, item_id: u32) -> &[u32] {
        slice(&self.bonus_trees_by_item, &item_id)
    }

    /// The nodes of a bonus tree, sorted by id.
    pub fn item_bonus_tree_nodes(&self, tree_id: u32) -> &[ItemBonusTreeNodeRow] {
        slice(&self.item_bonus_tree_nodes, &tree_id)
    }

    /// The bonuses of a bonus list, by order index.
    pub fn item_bonuses(&self, bonus_list_id: u32) -> &[ItemBonusRow] {
        slice(&self.item_bonuses, &bonus_list_id)
    }
}
